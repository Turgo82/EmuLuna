//! Browse and manage saved media without blocking the UI thread.
use crate::artwork::Artwork;
use eframe::egui;
use emuluna_library_model::{Game, Library};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime},
};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    States,
    Screenshots,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub preview: Option<PathBuf>,
    pub game: String,
    pub title: String,
    pub system: String,
    pub label: String,
    pub modified: SystemTime,
    screenshot_folder: Option<PathBuf>,
}
#[derive(Clone)]
pub enum Action {
    Resume {
        game: String,
        path: PathBuf,
        autosave: bool,
    },
    Open(PathBuf),
    Delete(Kind, Entry),
}
type IndexResult = (Kind, Result<Vec<Entry>, String>);
type DeleteResult = (Kind, PathBuf, Result<Option<String>, String>);
pub struct Browser {
    entries: Vec<Entry>,
    visible: Vec<usize>,
    games: HashSet<String>,
    kind: Kind,
    receiver: Option<mpsc::Receiver<IndexResult>>,
    pub message: String,
    pub busy: bool,
    pub refresh: bool,
    indexed: bool,
    last_scan: Option<Instant>,
    pending_delete: Option<(Kind, Entry)>,
    deletion: Option<mpsc::Receiver<DeleteResult>>,
}
impl Default for Browser {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            visible: Vec::new(),
            games: HashSet::new(),
            kind: Kind::States,
            receiver: None,
            message: String::new(),
            busy: false,
            refresh: false,
            indexed: false,
            last_scan: None,
            pending_delete: None,
            deletion: None,
        }
    }
}
pub(crate) fn authorized_console(root: &Path) -> Option<PathBuf> {
    let folder = root.join("consoles");
    let linked = fs::symlink_metadata(&folder).is_ok_and(|m| m.file_type().is_symlink());
    let approved = match fs::read(root.join("console-location.json")) {
        Ok(bytes) if bytes.len() <= 16384 => {
            let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            if value["format"].as_u64() != Some(1) {
                return None;
            }
            let path = PathBuf::from(value["path"].as_str()?);
            if !path.is_absolute() {
                return None;
            }
            path
        }
        Ok(_) => return None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !linked => folder.clone(),
        Err(_) => return None,
    };
    let actual = folder.canonicalize().ok()?;
    if actual != approved.canonicalize().ok()? {
        return None;
    }
    Some(actual)
}
fn scan_folder(folder: &Path, game: &Game, kind: Kind, budget: &mut usize, out: &mut Vec<Entry>) {
    let Ok(base) = folder.canonicalize() else {
        return;
    };
    let mut pending = vec![(base.clone(), 0)];
    while let Some((path, depth)) = pending.pop() {
        if depth > 8 || *budget == 0 {
            continue;
        }
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        for file in entries.flatten() {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let Ok(kind_file) = file.file_type() else {
                continue;
            };
            if kind_file.is_symlink() {
                continue;
            }
            if kind_file.is_dir() {
                pending.push((file.path(), depth + 1));
                continue;
            }
            if !kind_file.is_file() {
                continue;
            }
            let path = file.path();
            let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if !match kind {
                Kind::States => {
                    emuluna_state_file::has_state_extension(&path)
                        && (extension == emuluna_state_file::STATE_EXTENSION
                            || !fs::symlink_metadata(
                                path.with_extension(emuluna_state_file::STATE_EXTENSION),
                            )
                            .is_ok_and(|metadata| metadata.is_file()))
                }
                Kind::Screenshots => matches!(extension, "png" | "jpg" | "jpeg" | "webp"),
            } {
                continue;
            }
            let Ok(metadata) = file.metadata() else {
                continue;
            };
            if metadata.len()
                > if kind == Kind::States {
                    129 * 1024 * 1024
                } else {
                    32 * 1024 * 1024
                }
            {
                continue;
            }
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let preview = if kind == Kind::Screenshots {
                Some(path.clone())
            } else {
                let candidate = path.with_extension("png");
                fs::symlink_metadata(&candidate)
                    .ok()
                    .filter(|m| {
                        m.file_type().is_file() && m.modified().is_ok_and(|time| time >= modified)
                    })
                    .map(|_| candidate)
                    .or_else(|| game.cover.clone())
            };
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Saved state");
            let label = if kind == Kind::Screenshots {
                "Screenshot".into()
            } else if stem == "auto" {
                if path
                    .ancestors()
                    .skip(1)
                    .take(2)
                    .any(|parent| parent.file_name().is_some_and(|name| name == "autosave"))
                {
                    "Autosave".into()
                } else {
                    "Legacy autosave".into()
                }
            } else if let Some(slot) = stem.strip_prefix("slot-") {
                format!("Slot {slot}")
            } else {
                stem.to_owned()
            };
            out.push(Entry {
                path,
                preview,
                game: game.id.clone(),
                title: game.title.clone(),
                system: game.system.clone(),
                label,
                modified,
                screenshot_folder: (kind == Kind::Screenshots).then(|| base.clone()),
            });
        }
    }
}
fn index(root: &Path, native: &Path, games: &[Game], kind: Kind) -> Result<Vec<Entry>, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let consoles = authorized_console(&root);
    let folder_kind = if kind == Kind::States {
        "states"
    } else {
        "screenshots"
    };
    let mut result = Vec::new();
    let mut budget = 100_000;
    let native_root = native.canonicalize().ok();
    for game in games {
        if game.id.len() != 64
            || !game.id.bytes().all(|b| b.is_ascii_hexdigit())
            || game.system.is_empty()
            || !game
                .system
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            continue;
        }
        let mut folders = vec![(root.join(folder_kind).join(&game.id), root.clone())];
        if let Some(consoles) = &consoles {
            folders.push((
                consoles.join(&game.system).join(folder_kind).join(&game.id),
                consoles.clone(),
            ));
        }
        if let Some(native_root) = &native_root {
            if let Ok(cores) = fs::read_dir(native.join(&game.id)) {
                for core in cores.flatten().take(256) {
                    if core.file_type().is_ok_and(|m| m.is_dir()) {
                        folders.push((core.path().join(folder_kind), native_root.clone()));
                    }
                }
            }
        }
        for (folder, allowed) in folders {
            if fs::symlink_metadata(&folder).is_ok_and(|m| m.file_type().is_dir())
                && folder.canonicalize().is_ok_and(|p| p.starts_with(allowed))
            {
                scan_folder(&folder, game, kind, &mut budget, &mut result);
            }
        }
    }
    if kind == Kind::Screenshots {
        let mut prefixes = std::collections::HashMap::<String, Vec<&Game>>::new();
        for game in games {
            prefixes
                .entry(game.id.chars().take(12).collect())
                .or_default()
                .push(game);
        }
        if let Ok(files) = fs::symlink_metadata(root.join("screenshots")).and_then(|m| {
            if m.file_type().is_dir() {
                fs::read_dir(root.join("screenshots"))
            } else {
                Err(std::io::Error::other("Linked screenshots folder"))
            }
        }) {
            for file in files.flatten().take(100_000) {
                if !file.file_type().is_ok_and(|m| m.is_file()) {
                    continue;
                }
                let name = file.file_name();
                let name = name.to_string_lossy();
                let prefix = name.split('-').next().unwrap_or("");
                if file.path().extension().is_none_or(|s| s != "png") {
                    continue;
                }
                if let Some(games) = prefixes.get(prefix).filter(|g| g.len() == 1) {
                    let game = games[0];
                    result.push(Entry {
                        path: file.path(),
                        preview: Some(file.path()),
                        game: game.id.clone(),
                        title: game.title.clone(),
                        system: game.system.clone(),
                        label: "Screenshot".into(),
                        modified: file
                            .metadata()
                            .and_then(|m| m.modified())
                            .unwrap_or(SystemTime::UNIX_EPOCH),
                        screenshot_folder: None,
                    });
                }
            }
        }
    }
    let mut seen = HashSet::new();
    result.retain(|entry| seen.insert(entry.path.clone()));
    result.sort_by_key(|entry| std::cmp::Reverse(entry.modified));
    Ok(result)
}
fn trash_entry(
    kind: Kind,
    path: &Path,
    screenshot_folder: Option<&Path>,
    mut trash: impl FnMut(&Path) -> Result<(), String>,
) -> Result<Option<String>, String> {
    let parent = path.parent().ok_or("The media file has no parent folder")?;
    if parent.canonicalize().ok().as_deref() != Some(parent)
        || !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
    {
        return Err("The media file moved or changed. Refresh the list and try again.".into());
    }
    trash(path)?;
    if kind == Kind::Screenshots {
        if let Some(folder) = screenshot_folder {
            for directory in parent
                .ancestors()
                .take_while(|path| path.starts_with(folder))
            {
                match fs::remove_dir(directory) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Ok(Some(format!(
                        "Screenshot moved to Trash, but its folder could not be removed: {error}"
                    )))
                    }
                }
            }
        }
        return Ok(None);
    }
    let legacy = emuluna_state_file::readable_path(path);
    let preview = path.with_extension("png");
    let warnings: Vec<_> = [legacy, preview]
        .into_iter()
        .filter(|file| file != path && fs::symlink_metadata(file).is_ok_and(|m| m.is_file()))
        .filter_map(|file| trash(&file).err())
        .collect();
    Ok((!warnings.is_empty()).then(|| warnings.join("\n")))
}

fn media_menu(
    ui: &mut egui::Ui,
    kind: Kind,
    entry: &Entry,
    busy: bool,
    action: &mut Option<Action>,
) {
    ui.set_width(220.);
    ui.spacing_mut().item_spacing.y = 0.;
    if kind == Kind::States
        && crate::actions::menu_row(ui, "Resume from this state", "", !busy).clicked()
    {
        *action = Some(Action::Resume {
            game: entry.game.clone(),
            path: entry.path.clone(),
            autosave: entry.label == "Autosave",
        });
        ui.close_menu();
    }
    if crate::actions::menu_row(ui, "Show in file manager", "", true).clicked() {
        if let Some(parent) = entry.path.parent() {
            *action = Some(Action::Open(parent.to_owned()));
        }
        ui.close_menu();
    }
    ui.separator();
    if crate::actions::menu_row(ui, "Delete…", "", !busy || kind == Kind::Screenshots).clicked() {
        *action = Some(Action::Delete(kind, entry.clone()));
        ui.close_menu();
    }
}

impl Browser {
    pub fn request_delete(&mut self, kind: Kind, entry: Entry) {
        if self.deletion.is_none() {
            self.pending_delete = Some((kind, entry));
        }
    }

    pub fn show_delete(&mut self, ctx: &egui::Context, playing: bool) {
        let Some((kind, entry)) = self.pending_delete.as_ref() else {
            return;
        };
        let mut confirmed = false;
        let mut canceled = false;
        let modal = egui::Modal::new(egui::Id::new("delete-library-media")).show(ctx, |ui| {
            ui.set_max_width(360.);
            ui.heading(if *kind == Kind::States {
                "Delete save state?"
            } else {
                "Delete screenshot?"
            });
            ui.label(format!(
                "Move {} for “{}” to Trash?",
                entry.label.to_lowercase(),
                entry.title
            ));
            if *kind == Kind::States && playing {
                ui.label("Close the game before deleting a save state.");
            }
            ui.horizontal(|ui| {
                canceled = ui.button("Cancel").clicked();
                confirmed = ui
                    .add_enabled(
                        !playing || *kind == Kind::Screenshots,
                        egui::Button::new("Delete"),
                    )
                    .clicked();
            });
        });
        if confirmed {
            let (kind, entry) = self.pending_delete.take().unwrap();
            let (sender, receiver) = mpsc::channel();
            self.deletion = Some(receiver);
            let ctx = ctx.clone();
            thread::spawn(move || {
                let result = trash_entry(
                    kind,
                    &entry.path,
                    entry.screenshot_folder.as_deref(),
                    crate::removal::desktop_trash,
                );
                let _ = sender.send((kind, entry.path, result));
                ctx.request_repaint();
            });
        } else if canceled || modal.should_close() {
            self.pending_delete = None;
        }
    }

    pub fn poll_deletion(&mut self, artwork: &mut Artwork) -> bool {
        let Some((kind, path, result)) = self.deletion.as_ref().and_then(|r| r.try_recv().ok())
        else {
            return false;
        };
        self.deletion = None;
        match result {
            Ok(warning) => {
                artwork.invalidate(&path);
                if kind == Kind::States {
                    artwork.invalidate(&path.with_extension("png"));
                }
                self.entries.retain(|entry| entry.path != path);
                self.rebuild();
                self.receiver = None;
                self.busy = false;
                self.refresh = true;
                if let Some(warning) = warning {
                    self.message = warning;
                }
                kind == Kind::States
            }
            Err(error) => {
                self.message = error;
                false
            }
        }
    }

    pub fn set_games(&mut self, games: HashSet<String>) {
        self.games = games;
        self.rebuild();
    }
    fn rebuild(&mut self) {
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| self.games.contains(&e.game).then_some(i))
            .collect();
        if self.kind == Kind::States {
            self.visible.sort_by_cached_key(|index| {
                let entry = &self.entries[*index];
                let slot = |entry: &Entry| {
                    if entry.label == "Autosave" {
                        0
                    } else {
                        entry
                            .label
                            .strip_prefix("Slot ")
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(u32::MAX)
                    }
                };
                (entry.title.to_lowercase(), entry.game.as_str(), slot(entry))
            });
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context, artwork: &mut Artwork) {
        if let Some((kind, result)) = self.receiver.as_ref().and_then(|r| r.try_recv().ok()) {
            self.busy = false;
            self.receiver = None;
            self.indexed = kind == self.kind;
            if kind != self.kind {
                return;
            }
            match result {
                Ok(entries) => {
                    let previous: std::collections::HashMap<_, _> = self
                        .entries
                        .iter()
                        .map(|entry| (entry.path.as_path(), entry))
                        .collect();
                    for entry in &entries {
                        if previous.get(entry.path.as_path()).is_some_and(|old| {
                            old.modified != entry.modified || old.preview != entry.preview
                        }) {
                            if let Some(preview) = &entry.preview {
                                artwork.invalidate(preview);
                            }
                        }
                    }
                    self.entries = entries;
                    self.rebuild();
                    self.message = String::new();
                }
                Err(error) => self.message = error,
            }
            ctx.request_repaint();
        }
    }
    pub fn select(&mut self, kind: Kind) {
        if self.kind != kind {
            self.kind = kind;
            self.indexed = false;
            self.entries.clear();
            self.visible.clear();
        }
    }
    pub fn ensure(&mut self, library: &Library, native: PathBuf, ctx: &egui::Context) {
        let interval = Duration::from_secs(2);
        ctx.request_repaint_after(interval);
        if self.busy
            || (self.indexed
                && !self.refresh
                && self.last_scan.is_some_and(|scan| scan.elapsed() < interval))
        {
            return;
        }
        self.last_scan = Some(Instant::now());
        self.refresh = false;
        self.busy = true;
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        let root = library.root.clone();
        let games = library.games.clone();
        let kind = self.kind;
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send((kind, index(&root, &native, &games, kind)));
            ctx.request_repaint();
        });
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        artwork: &mut Artwork,
        list: bool,
        busy: bool,
        preview_radius: u8,
    ) -> Option<Action> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.add_space(24.);
            ui.label(format!(
                "{} {}",
                self.visible.len(),
                if self.kind == Kind::States {
                    "save states"
                } else {
                    "screenshots"
                }
            ));
        });
        if self.busy && !self.indexed {
            ui.spinner();
            ui.label("Finding your saved media…");
            return None;
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if self.visible.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.weak("No saved media for this selection yet.");
            });
            return None;
        }
        if busy && self.kind == Kind::States {
            ui.weak("Close the running game before resuming a different state.");
        }
        let width = ui.available_width();
        let columns = if list {
            1
        } else {
            ((width - 32.) / 240.).floor().max(1.) as usize
        };
        let mut layout = Vec::new();
        for group in self.visible.chunk_by(|a, b| {
            self.kind == Kind::Screenshots || self.entries[*a].game == self.entries[*b].game
        }) {
            if self.kind == Kind::States {
                layout.push((Some(group[0]), &[][..]));
            }
            layout.extend(group.chunks(columns).map(|row| (None, row)));
        }
        let heading = |ui: &mut egui::Ui, index: usize| {
            let entry = &self.entries[index];
            ui.label(egui::RichText::new(&entry.title).strong());
            ui.separator();
        };
        if list {
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt(("media-list", format!("{:?}", self.kind)))
                .show_rows(ui, 50., layout.len(), |ui, rows| {
                    for row in rows {
                        let (header, entries) = layout[row];
                        if let Some(index) = header {
                            ui.allocate_ui(egui::vec2(width, 50.), |ui| heading(ui, index));
                            continue;
                        }
                        let entry = &self.entries[entries[0]];
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 50.),
                            egui::Sense::click(),
                        );
                        if response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                4.,
                                ui.visuals().widgets.hovered.weak_bg_fill,
                            );
                        }
                        let thumb = egui::Rect::from_min_size(
                            rect.min + egui::vec2(16., 4.),
                            egui::vec2(56., 42.),
                        );
                        if let Some(texture) =
                            entry.preview.as_deref().and_then(|p| artwork.texture(p))
                        {
                            let size = entry
                                .preview
                                .as_deref()
                                .and_then(|p| artwork.size(p))
                                .unwrap_or(egui::vec2(56., 42.));
                            let scale = (thumb.width() / size.x).min(thumb.height() / size.y);
                            egui::Image::new((texture, size))
                                .corner_radius(preview_radius / 2)
                                .paint_at(
                                    ui,
                                    egui::Rect::from_center_size(thumb.center(), size * scale),
                                );
                        }
                        let text_rect = egui::Rect::from_min_max(
                            rect.min + egui::vec2(84., 0.),
                            egui::pos2(
                                (rect.right() - 180.).max(rect.left() + 140.),
                                rect.bottom(),
                            ),
                        );
                        ui.painter().with_clip_rect(text_rect).text(
                            egui::pos2(text_rect.left(), rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            if self.kind == Kind::States {
                                &entry.label
                            } else {
                                &entry.title
                            },
                            crate::theme::fonts::bold(14.),
                            ui.visuals().text_color(),
                        );
                        ui.painter().text(
                            egui::pos2(rect.right() - 16., rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            if self.kind == Kind::States {
                                entry.system.clone()
                            } else {
                                format!("{} · {}", entry.label, entry.system)
                            },
                            egui::FontId::proportional(13.),
                            ui.visuals().weak_text_color(),
                        );
                        if response.double_clicked() && (self.kind == Kind::Screenshots || !busy) {
                            action = Some(if self.kind == Kind::States {
                                Action::Resume {
                                    game: entry.game.clone(),
                                    path: entry.path.clone(),
                                    autosave: entry.label == "Autosave",
                                }
                            } else {
                                Action::Open(entry.path.clone())
                            });
                        }
                        response
                            .context_menu(|ui| media_menu(ui, self.kind, entry, busy, &mut action));
                    }
                });
            return action;
        }
        egui::ScrollArea::vertical()
            .id_salt(("media", format!("{:?}", self.kind)))
            .show(ui, |ui| {
                for (header, entries) in layout {
                    let height = if header.is_some() { 46. } else { 206. };
                    let (row_rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                    if !ui.is_rect_visible(row_rect) {
                        continue;
                    }
                    if let Some(index) = header {
                        let mut child = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt(("media-heading", index))
                                .max_rect(row_rect.shrink2(egui::vec2(20., 0.))),
                        );
                        heading(&mut child, index);
                        continue;
                    }
                    for (col, index) in entries.iter().enumerate() {
                        let entry = &self.entries[*index];
                        let cell = egui::Rect::from_min_size(
                            row_rect.min + egui::vec2(20. + col as f32 * 240., 0.),
                            egui::vec2(220., height),
                        );
                        let mut card = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt(("media-card", &entry.path))
                                .max_rect(cell)
                                .layout(egui::Layout::top_down(egui::Align::Center)),
                        );
                        card.set_clip_rect(ui.clip_rect().intersect(cell));
                        let ui = &mut card;
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(208., 156.), egui::Sense::click());
                        ui.painter().rect_filled(
                            rect,
                            preview_radius,
                            ui.visuals().extreme_bg_color,
                        );
                        if let Some(texture) =
                            entry.preview.as_deref().and_then(|p| artwork.texture(p))
                        {
                            let size = entry
                                .preview
                                .as_deref()
                                .and_then(|p| artwork.size(p))
                                .unwrap_or(egui::vec2(208., 156.));
                            let scale = (rect.width() / size.x).min(rect.height() / size.y);
                            let target = egui::Rect::from_center_size(rect.center(), size * scale);
                            egui::Image::new((texture, size))
                                .corner_radius(preview_radius)
                                .paint_at(ui, target);
                        }
                        if self.kind == Kind::States {
                            ui.label(egui::RichText::new(&entry.label).strong());
                        } else {
                            ui.label(egui::RichText::new(&entry.title).strong());
                            ui.weak(&entry.system);
                        }
                        if response.double_clicked() && (self.kind == Kind::Screenshots || !busy) {
                            action = Some(if self.kind == Kind::States {
                                Action::Resume {
                                    game: entry.game.clone(),
                                    path: entry.path.clone(),
                                    autosave: entry.label == "Autosave",
                                }
                            } else {
                                Action::Open(entry.path.clone())
                            });
                        }
                        response
                            .context_menu(|ui| media_menu(ui, self.kind, entry, busy, &mut action));
                    }
                }
            });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deleting_saved_media_moves_only_the_selected_files_and_state_companions() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder
            .path()
            .canonicalize()
            .unwrap()
            .join("auto.emuluna-state");
        let legacy = path.with_extension("oesavestate");
        let preview = path.with_extension("png");
        let unrelated = path.with_file_name("slot-1.png");
        for file in [&path, &legacy, &preview, &unrelated] {
            fs::write(file, b"media").unwrap();
        }
        let trash = |file: &Path| fs::remove_file(file).map_err(|e| e.to_string());
        assert_eq!(trash_entry(Kind::States, &path, None, trash).unwrap(), None);
        assert!(!path.exists() && !legacy.exists() && !preview.exists());
        assert!(unrelated.exists());
        assert_eq!(
            trash_entry(
                Kind::Screenshots,
                &unrelated,
                Some(path.parent().unwrap()),
                trash
            )
            .unwrap(),
            None
        );
        assert!(!unrelated.exists());
        assert!(!path.parent().unwrap().exists());
    }

    fn library(root: &Path) -> Library {
        let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE games(id TEXT,title TEXT,system TEXT,rom_path TEXT,cover TEXT,rating INTEGER,favorite INTEGER,added REAL,last_played REAL,play_count INTEGER,original_filename TEXT); CREATE TABLE settings(key TEXT,value TEXT); CREATE TABLE collections(id INTEGER,name TEXT); CREATE TABLE collection_games(collection_id INTEGER,game_id TEXT);").unwrap();
        db.execute("INSERT INTO games VALUES(?,'Original fixture','nes','original.nes',NULL,0,0,1,NULL,0,NULL)",["a".repeat(64)]).unwrap();
        drop(db);
        Library::load(root).unwrap()
    }
    #[test]
    fn media_index_reads_console_legacy_and_native_without_following_links() {
        let root = tempfile::tempdir().unwrap();
        let native = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let library = library(root.path());
        let id = &library.games[0].id;
        let legacy = root
            .path()
            .join("states")
            .join(id)
            .join("libretro/nestopia/hash");
        let console = root.path().join("consoles/nes/states").join(id);
        let own = native.path().join(id).join("nestopia/states/hash");
        for folder in [&legacy, &console, &own] {
            fs::create_dir_all(folder).unwrap();
            fs::write(
                folder.join("slot-1.oesavestate"),
                b"original synthetic state",
            )
            .unwrap();
        }
        fs::write(outside.path().join("secret.oesavestate"), b"not selected").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), console.join("escape")).unwrap();
        let before = fs::read(root.path().join("library.sqlite3")).unwrap();
        let entries = index(root.path(), native.path(), &library.games, Kind::States).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|e| e.label == "Slot 1"));
        assert_eq!(
            fs::read(root.path().join("library.sqlite3")).unwrap(),
            before
        );
        let screenshots = root.path().join("screenshots");
        fs::create_dir_all(&screenshots).unwrap();
        fs::write(
            screenshots.join(format!("{}-old.png", &id[..12])),
            b"preview",
        )
        .unwrap();
        // A state's PNG sidecar is not an ordinary screenshot.
        fs::write(own.join("slot-1.png"), b"state preview").unwrap();
        assert_eq!(
            index(
                root.path(),
                native.path(),
                &library.games,
                Kind::Screenshots
            )
            .unwrap()
            .len(),
            1
        );
        #[cfg(unix)]
        {
            fs::remove_dir_all(root.path().join("states")).unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("states")).unwrap();
            fs::create_dir_all(outside.path().join(id)).unwrap();
            fs::write(
                outside.path().join(id).join("foreign.oesavestate"),
                b"foreign",
            )
            .unwrap();
            assert_eq!(
                index(root.path(), native.path(), &library.games, Kind::States)
                    .unwrap()
                    .len(),
                2
            );
            // Only an explicitly approved external console root may be read.
            fs::remove_dir_all(root.path().join("consoles")).unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("consoles")).unwrap();
            let ext = outside.path().join("nes/states").join(id);
            fs::create_dir_all(&ext).unwrap();
            fs::write(ext.join("auto.oesavestate"), b"approved").unwrap();
            assert_eq!(
                index(root.path(), native.path(), &library.games, Kind::States)
                    .unwrap()
                    .len(),
                1
            );
            fs::write(
                root.path().join("console-location.json"),
                serde_json::to_vec(&serde_json::json!({"format":1,"path":outside.path()})).unwrap(),
            )
            .unwrap();
            assert_eq!(
                index(root.path(), native.path(), &library.games, Kind::States)
                    .unwrap()
                    .len(),
                2
            );
        }
    }
}
