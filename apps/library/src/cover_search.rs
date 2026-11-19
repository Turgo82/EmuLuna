//! Reviewable manual search of the existing Libretro artwork catalog.
use crate::disc_backup::{artwork, run_capture};
use eframe::egui;
use emuluna_library_model::Game;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

struct Candidate {
    title: String,
    bytes: Vec<u8>,
    texture: egui::TextureHandle,
    metadata: BTreeMap<String, String>,
}
enum Event {
    Cover(String, Vec<u8>, BTreeMap<String, String>),
    Finished(String),
}
#[derive(Default)]
pub struct Picker {
    game: Option<Game>,
    query: String,
    results: Vec<Candidate>,
    selected: Option<usize>,
    receiver: Option<mpsc::Receiver<Event>>,
    cancel: Option<Arc<AtomicBool>>,
    status: String,
    replace_metadata: bool,
}
impl Picker {
    pub fn open(&mut self, game: Game, root: &Path, ctx: &egui::Context) {
        self.stop();
        self.query = game.title.clone();
        self.replace_metadata = false;
        self.game = Some(game);
        self.search(root, ctx);
    }
    fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.receiver = None;
    }
    fn search(&mut self, root: &Path, ctx: &egui::Context) {
        self.stop();
        self.results.clear();
        self.selected = None;
        let Some(game) = &self.game else {
            return;
        };
        let system = game.system.clone();
        let query = self.query.clone();
        let root = root.to_owned();
        let ctx = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.status = "Searching the cover catalog…".into();
        std::thread::spawn(move || {
            let catalog: serde_json::Value =
                match serde_json::from_str(include_str!("../../../data/systems.json")) {
                    Ok(v) => v,
                    Err(_) => return,
                };
            let Some(repository) = catalog[&system]["thumbnail"].as_str() else {
                let _ = tx.send(Event::Finished(
                    "No artwork catalog is available for this console.".into(),
                ));
                ctx.request_repaint();
                return;
            };
            let Some(names) = artwork::index(Some(&root), &system, repository, &cancel) else {
                let _ = tx.send(Event::Finished(
                    "The artwork catalog is unavailable. Check your connection and try again."
                        .into(),
                ));
                ctx.request_repaint();
                return;
            };
            let query = artwork::normalized_title(&query);
            let metadata = catalog_metadata(&root, &system);
            let mut ranked: Vec<_> = names
                .into_iter()
                .filter_map(|name| {
                    let stem = Path::new(&name).file_stem()?.to_string_lossy();
                    let normalized = artwork::normalized_title(&stem);
                    let score = score(&query, &normalized);
                    (score > 0).then_some((score, name))
                })
                .collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            ranked.truncate(30);
            let dir = match tempfile::tempdir() {
                Ok(d) => d,
                Err(_) => return,
            };
            let path = dir.path().join("cover");
            let mut found = 0;
            for (_, name) in ranked {
                if cancel.load(Ordering::Relaxed) || found >= 18 {
                    break;
                }
                let encoded = name
                    .bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                            (b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect::<String>();
                let url = format!("https://raw.githubusercontent.com/libretro-thumbnails/{repository}/master/Named_Boxarts/{encoded}");
                if run_capture(
                    Command::new("curl")
                        .args([
                            "--fail",
                            "--silent",
                            "--location",
                            "--proto",
                            "=https",
                            "--max-time",
                            "8",
                            "--max-filesize",
                            "12582912",
                            "--output",
                        ])
                        .arg(&path)
                        .arg(url),
                    &cancel,
                )
                .is_ok()
                {
                    if let Ok(bytes) = fs::read(&path) {
                        let title = Path::new(&name)
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let fields = matching_metadata(&metadata, &title);
                        if tx.send(Event::Cover(title, bytes, fields)).is_err() {
                            return;
                        }
                        found += 1;
                        ctx.request_repaint();
                    }
                }
            }
            let _ = tx.send(Event::Finished(if found == 0 {
                "No matching covers found. Try another search.".into()
            } else {
                format!("{found} covers found. Choose one to use.")
            }));
            ctx.request_repaint();
        });
    }
    pub fn show(&mut self, ctx: &egui::Context, root: &Path, quality: u8) -> bool {
        if let Some(rx) = &self.receiver {
            while let Ok(event) = rx.try_recv() {
                match event {
                    Event::Cover(title, bytes, metadata) => {
                        if let Ok(image) = image::load_from_memory(&bytes) {
                            let image = image.thumbnail(160, 180).to_rgba8();
                            let size = [image.width() as usize, image.height() as usize];
                            let texture = ctx.load_texture(
                                format!("cover-search-{}", self.results.len()),
                                egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                                egui::TextureOptions::LINEAR,
                            );
                            self.results.push(Candidate {
                                title,
                                bytes,
                                texture,
                                metadata,
                            });
                        }
                    }
                    Event::Finished(status) => {
                        self.status = status;
                        self.cancel = None;
                    }
                }
            }
        }
        let Some(game) = self.game.clone() else {
            return false;
        };
        let mut close = false;
        let mut open = true;
        let mut search = false;
        let mut save = false;
        let contents = |ui: &mut egui::Ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12., 10.);
            ui.heading(format!("Find a cover for {}", game.title));
            if let Some(filename) = game.rom.file_name() {
                ui.label(format!("Filename: {}", filename.to_string_lossy()));
            }
            ui.label("Search the artwork catalog and choose a cover.");
            ui.horizontal(|ui| {
                let field_width =
                    (ui.available_width() - 88. - ui.spacing().item_spacing.x).max(100.);
                let response = egui::Frame::new()
                    .fill(ui.visuals().widgets.inactive.bg_fill)
                    .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                    .corner_radius(5.)
                    .inner_margin(egui::Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .frame(false)
                                .desired_width((field_width - 30.).max(70.)),
                        )
                    })
                    .inner;
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    search = true;
                }
                if ui
                    .add_sized([88., 34.], egui::Button::new("Search"))
                    .clicked()
                {
                    search = true;
                }
            });
            ui.label(&self.status);
            ui.separator();
            let metadata_note = "No matching game information was found; the cover's title will be used for the game name.";
            let note_height = ui
                .painter()
                .layout(
                    metadata_note.into(),
                    egui::TextStyle::Body.resolve(ui.style()),
                    ui.visuals().weak_text_color(),
                    ui.available_width(),
                )
                .size()
                .y;
            let footer_height = 36.
                + ui.spacing().interact_size.y
                + note_height
                + 4. * ui.spacing().item_spacing.y
                + 6.;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height((ui.available_height() - footer_height).max(0.))
                .show(ui, |ui| {
                    let columns = ((ui.available_width() / 200.).floor() as usize).max(1);
                    egui::Grid::new("cover-search-grid")
                        .num_columns(columns)
                        .spacing([20., 22.])
                        .show(ui, |ui| {
                            for (i, item) in self.results.iter().enumerate() {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(180., 215.),
                                    egui::Layout::top_down(egui::Align::Center),
                                    |ui| {
                                        ui.set_max_width(180.);
                                        let img = egui::Image::new(&item.texture)
                                            .fit_to_exact_size(egui::vec2(160., 170.))
                                            .maintain_aspect_ratio(true);
                                        if ui
                                            .add(
                                                egui::Button::image(img)
                                                    .selected(self.selected == Some(i)),
                                            )
                                            .clicked()
                                        {
                                            self.selected = Some(i);
                                        }
                                        let (title, variant) = display_title(&item.title);
                                        ui.add(egui::Label::new(title).truncate())
                                            .on_hover_text(&item.title);
                                        if !variant.is_empty() {
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(variant).weak().small(),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(&item.title);
                                        }
                                    },
                                );
                                if (i + 1) % columns == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                });
            ui.separator();
            ui.checkbox(
                &mut self.replace_metadata,
                "Replace game name and available metadata",
            );
            if self.replace_metadata {
                if let Some(item) = self.selected.and_then(|i| self.results.get(i)) {
                    if item.metadata.is_empty() {
                        ui.weak(metadata_note);
                    }
                }
            }
            ui.horizontal(|ui| {
                if ui
                    .add_sized([90., 36.], egui::Button::new("Cancel"))
                    .clicked()
                {
                    close = true;
                }
                if ui
                    .add_enabled_ui(self.selected.is_some(), |ui| {
                        ui.add_sized([170., 36.], egui::Button::new("Use selected cover"))
                    })
                    .inner
                    .clicked()
                {
                    save = true;
                }
            });
        };
        let size = (ctx.screen_rect().size() - egui::vec2(48., 64.))
            .clamp(egui::vec2(320., 240.), egui::vec2(830., 650.));
        egui::Window::new(emuluna_theme::fonts::window_title("Find cover art"))
            .id(egui::Id::new("cover-search"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .movable(false)
            .open(&mut open)
            .collapsible(false)
            .fixed_size(size)
            .resizable(false)
            .show(ctx, contents);
        if search && !self.query.trim().is_empty() {
            self.search(root, ctx);
        }
        if save {
            if let Some(item) = self.selected.and_then(|i| self.results.get(i)) {
                let fields = self.replace_metadata.then(|| {
                    let mut fields = item.metadata.clone();
                    fields
                        .entry("title".into())
                        .or_insert_with(|| cover_title(&item.title));
                    fields
                });
                match save_cover(root, &game.id, &item.bytes, quality, fields.as_ref()) {
                    Ok(()) => {
                        self.stop();
                        self.game = None;
                        return true;
                    }
                    Err(e) => self.status = format!("Could not save cover: {e}"),
                }
            }
        }
        if close || !open {
            self.stop();
            self.game = None;
        }
        false
    }
}
fn display_title(title: &str) -> (&str, &str) {
    match title.find(['(', '[']) {
        Some(index) => (title[..index].trim(), title[index..].trim()),
        None => (title.trim(), ""),
    }
}
fn cover_title(title: &str) -> String {
    let title = title.split(['(', '[']).next().unwrap_or(title).trim();
    if let Some(rest) = title.strip_suffix(", The") {
        format!("The {rest}")
    } else {
        title.to_owned()
    }
}

fn catalog_metadata(root: &Path, system: &str) -> Vec<BTreeMap<String, String>> {
    let result = (|| -> Result<Vec<BTreeMap<String, String>>, Box<dyn std::error::Error>> {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../../data/systems.json"))?;
        let systems = crate::openvgdb::system_names(&catalog, system).ok_or("Unknown console")?;
        let db = rusqlite::Connection::open_with_flags(
            crate::storage::metadata(root).join("openvgdb.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let columns = {
            let mut s = db.prepare("PRAGMA table_info(RELEASES)")?;
            let rows = s
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let mappings = [
            ("releaseDeveloper", "developer"),
            ("releasePublisher", "publisher"),
            ("releaseDate", "release_date"),
            ("releaseGenre", "genre"),
        ];
        let extra = mappings
            .iter()
            .filter(|(column, _)| columns.iter().any(|c| c.eq_ignore_ascii_case(column)))
            .map(|(column, alias)| format!(", {column} AS {alias}"))
            .collect::<String>();
        let sql = format!("SELECT DISTINCT releaseTitleName, regionName{extra} FROM RELEASES JOIN ROMs USING(romID) JOIN SYSTEMS USING(systemID) LEFT JOIN REGIONS ON regionLocalizedID=REGIONS.regionID WHERE systemShortName IN (SELECT value FROM json_each(?1)) AND releaseTitleName IS NOT NULL");
        let mut statement = db.prepare(&sql)?;
        let names = std::iter::once("title")
            .chain(std::iter::once("region"))
            .chain(
                mappings
                    .iter()
                    .filter(|(column, _)| columns.iter().any(|c| c.eq_ignore_ascii_case(column)))
                    .map(|(_, alias)| *alias),
            )
            .collect::<Vec<_>>();
        let rows = statement
            .query_map([systems], |row| {
                let mut fields = BTreeMap::new();
                for (i, name) in names.iter().enumerate() {
                    if let Ok(Some(value)) = row.get::<_, Option<String>>(i) {
                        let value = value.trim();
                        if !value.is_empty() {
                            fields.insert((*name).to_owned(), value.to_owned());
                        }
                    }
                }
                Ok(fields)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })();
    result.unwrap_or_default()
}
fn matching_metadata(rows: &[BTreeMap<String, String>], cover: &str) -> BTreeMap<String, String> {
    let title = artwork::normalized_title(cover);
    let region = if cover.contains("(Japan") {
        "Japan"
    } else if cover.contains("(Europe") {
        "Europe"
    } else if cover.contains("(USA") {
        "USA"
    } else {
        ""
    };
    rows.iter()
        .filter(|row| {
            row.get("title")
                .is_some_and(|s| artwork::normalized_title(s) == title)
        })
        .min_by_key(|row| {
            (
                row.get("region")
                    .is_none_or(|v| !v.eq_ignore_ascii_case(region)),
                row.get("region")
                    .is_none_or(|v| !v.eq_ignore_ascii_case("USA")),
            )
        })
        .cloned()
        .unwrap_or_default()
}
fn apply_metadata(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    fields: &BTreeMap<String, String>,
) -> Result<(), String> {
    let exists = |table: &str| {
        tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
            [table],
            |r| r.get::<_, bool>(0),
        )
        .map_err(|e| e.to_string())
    };
    let has_metadata = exists("metadata")?;
    let has_overrides = exists("metadata_overrides")?;
    let columns = if has_metadata {
        let mut s = tx
            .prepare("PRAGMA table_info(metadata)")
            .map_err(|e| e.to_string())?;
        let rows = s
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    } else {
        vec![]
    };
    if has_metadata {
        tx.execute("INSERT OR IGNORE INTO metadata(game_id) VALUES(?)", [id])
            .map_err(|e| e.to_string())?;
    }
    for key in [
        "title",
        "region",
        "developer",
        "publisher",
        "release_date",
        "genre",
    ] {
        let Some(value) = fields.get(key).map(|s| s.trim()).filter(|s| !s.is_empty()) else {
            continue;
        };
        if key == "title" {
            tx.execute(
                "UPDATE games SET title=? WHERE id=?",
                rusqlite::params![value, id],
            )
            .map_err(|e| e.to_string())?;
        } else if has_metadata && columns.iter().any(|c| c == key) {
            tx.execute(
                &format!("UPDATE metadata SET {key}=? WHERE game_id=?"),
                rusqlite::params![value, id],
            )
            .map_err(|e| e.to_string())?;
        } else {
            continue;
        }
        if has_overrides {
            tx.execute(
                "INSERT OR IGNORE INTO metadata_overrides(game_id,field) VALUES(?,?)",
                rusqlite::params![id, key],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn score(query: &str, title: &str) -> usize {
    if query.is_empty() {
        return 0;
    }
    if query == title {
        return 1000;
    }
    if title.contains(query) || query.contains(title) && !title.is_empty() {
        return 500;
    }
    let q: Vec<_> = query.split_whitespace().collect();
    let matching = q
        .iter()
        .filter(|w| title.split_whitespace().any(|v| v == **w))
        .count();
    if matching * 2 >= q.len() {
        matching * 100 / q.len().max(1)
    } else {
        0
    }
}
fn save_cover(
    root: &Path,
    id: &str,
    bytes: &[u8],
    quality: u8,
    metadata: Option<&BTreeMap<String, String>>,
) -> Result<(), String> {
    let covers = root.join("covers");
    fs::create_dir_all(&covers).map_err(|e| e.to_string())?;
    let name =
        crate::settings::conversion::encode(&covers, bytes, quality, &AtomicBool::new(false))?;
    let mut db =
        rusqlite::Connection::open(root.join("library.sqlite3")).map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let has_revision = {
        let mut s = tx
            .prepare("PRAGMA table_info(games)")
            .map_err(|e| e.to_string())?;
        let columns = s
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        columns.iter().any(|s| s == "cover_revision")
    };
    let sql = if has_revision {
        "UPDATE games SET cover=?, cover_revision=cover_revision+1 WHERE id=?"
    } else {
        "UPDATE games SET cover=? WHERE id=?"
    };
    if tx
        .execute(sql, rusqlite::params![format!("covers/{name}"), id])
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("Game no longer exists".into());
    }
    if let Some(fields) = metadata {
        apply_metadata(&tx, id, fields)?;
    }
    if tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='artwork')",
            [],
            |r| r.get::<_, bool>(0),
        )
        .map_err(|e| e.to_string())?
    {
        tx.execute("DELETE FROM artwork WHERE game_id=?", [id])
            .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranks_relevant_names() {
        assert!(
            score("ecco the dolphin", "ecco the dolphin")
                > score("ecco the dolphin", "ecco the dolphin 2")
        );
        assert_eq!(score("ecco the dolphin", "sonic"), 0);
        assert_eq!(
            display_title("10-Yard Fight (USA) [Rev A]"),
            ("10-Yard Fight", "(USA) [Rev A]")
        );
    }
    #[test]
    fn chosen_cover_updates_library_and_invalidates_download_record() {
        let root = tempfile::tempdir().unwrap();
        let db = rusqlite::Connection::open(root.path().join("library.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE games(id TEXT PRIMARY KEY, cover TEXT, cover_revision INTEGER NOT NULL DEFAULT 0); CREATE TABLE artwork(game_id TEXT); INSERT INTO games(id) VALUES('game'); INSERT INTO artwork VALUES('game');").unwrap();
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(4, 4)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        save_cover(root.path(), "game", bytes.get_ref(), 85, None).unwrap();
        let (cover, revision): (String, i64) = db
            .query_row(
                "SELECT cover,cover_revision FROM games WHERE id='game'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(root.path().join(cover).is_file());
        assert_eq!(revision, 1);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM artwork", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn selected_metadata_replaces_only_available_fields_and_respects_region() {
        let rows = vec![
            BTreeMap::from([
                ("title".into(), "Ecco the Dolphin".into()),
                ("region".into(), "Europe".into()),
                ("developer".into(), "Developer EU".into()),
            ]),
            BTreeMap::from([
                ("title".into(), "Ecco the Dolphin".into()),
                ("region".into(), "USA".into()),
                ("developer".into(), "Developer US".into()),
            ]),
        ];
        let fields = matching_metadata(&rows, "Ecco the Dolphin (USA)");
        assert_eq!(fields.get("developer").unwrap(), "Developer US");
        assert!(matching_metadata(&rows, "Ecco the Dolphin 2 (USA)").is_empty());
        assert_eq!(
            cover_title("Legend of Zelda, The (USA)"),
            "The Legend of Zelda"
        );
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE games(id TEXT PRIMARY KEY,title TEXT); CREATE TABLE metadata(game_id TEXT PRIMARY KEY,region TEXT DEFAULT '',developer TEXT DEFAULT '',publisher TEXT DEFAULT '',release_date TEXT DEFAULT '',genre TEXT DEFAULT ''); CREATE TABLE metadata_overrides(game_id TEXT,field TEXT,PRIMARY KEY(game_id,field)); INSERT INTO games VALUES('game','Old title'); INSERT INTO metadata(game_id,publisher) VALUES('game','Existing publisher');").unwrap();
        let tx = db.transaction().unwrap();
        apply_metadata(&tx, "game", &fields).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("SELECT title FROM games", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "Ecco the Dolphin"
        );
        assert_eq!(
            db.query_row("SELECT publisher FROM metadata", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "Existing publisher"
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM metadata_overrides WHERE field='title'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
}
