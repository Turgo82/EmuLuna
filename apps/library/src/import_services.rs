//! Background follow-up work after imports, independent of emulation and UI frames.
use crate::{disc_backup, settings};
use eframe::egui;
use emuluna_library_model::Game;
use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    path::PathBuf,
    sync::mpsc,
};
#[derive(Default)]
pub struct Services {
    receiver: Option<mpsc::Receiver<Event>>,
    pending: VecDeque<Job>,
    pub busy: bool,
    pub message: String,
}
struct Job {
    root: PathBuf,
    games: Vec<Game>,
    settings: BTreeMap<String, String>,
}
enum Event {
    Message(String),
    Finished(String),
}
impl Services {
    pub fn queue(&mut self, root: PathBuf, games: Vec<Game>, settings: BTreeMap<String, String>) {
        if !games.is_empty() {
            self.pending.push_back(Job {
                root,
                games,
                settings,
            });
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let mut refresh = false;
        if let Some(rx) = &self.receiver {
            while let Ok(event) = rx.try_recv() {
                match event {
                    Event::Message(s) => self.message = s,
                    Event::Finished(s) => {
                        self.message = s;
                        refresh = true;
                        self.busy = false;
                    }
                }
            }
        }
        if !self.busy {
            self.receiver = None;
            if let Some(job) = self.pending.pop_front() {
                let (tx, rx) = mpsc::channel();
                self.receiver = Some(rx);
                self.busy = true;
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = follow_up(job, &tx, &ctx);
                    let _ = tx.send(Event::Finished(result.unwrap_or_else(|e| {
                        format!("Games imported; background lookup needs attention: {e}")
                    })));
                    ctx.request_repaint();
                });
            }
        }
        refresh
    }
}
fn follow_up(job: Job, tx: &mpsc::Sender<Event>, ctx: &egui::Context) -> Result<String, String> {
    let catalog: Value = serde_json::from_str(include_str!("../../../data/systems.json"))
        .map_err(|e| e.to_string())?;
    let mut checked = HashSet::new();
    let mut messages = Vec::new();
    if job
        .settings
        .get("core_auto_install")
        .is_none_or(|v| v == "1")
    {
        for game in &job.games {
            if !checked.insert(game.system.clone()) {
                continue;
            }
            let selected = job
                .settings
                .get(&format!("core.{}", game.system))
                .map_or("auto", String::as_str);
            if !["auto", "builtin", ""].contains(&selected) {
                continue;
            }
            if emuluna_launch_config::installed_choices(&job.root, &game.system)
                .is_ok_and(|v| !v.is_empty())
            {
                continue;
            }
            let Some(core) = catalog[&game.system]["default_core"].as_str() else {
                continue;
            };
            if job
                .settings
                .get(&format!("core.removed.{core}"))
                .is_some_and(|v| v == "1")
            {
                continue;
            }
            let _ = tx.send(Event::Message(format!(
                "Downloading recommended core for {}…",
                game.system
            )));
            ctx.request_repaint();
            if let Err(e) = settings::tools::download_core(&job.root, core) {
                messages.push(e);
            }
        }
    }
    let artwork = job.settings.get("artwork_auto").is_none_or(|v| v == "1");
    let metadata = job.settings.get("metadata_auto").is_none_or(|v| v == "1");
    let quality = job
        .settings
        .get("artwork.webp_quality")
        .and_then(|v| v.parse::<u8>().ok())
        .filter(|q| [75, 85, 95, 100].contains(q))
        .unwrap_or(85);
    let region = job
        .settings
        .get(crate::region::KEY)
        .map_or(crate::region::DEFAULT, String::as_str);
    let mut covers = 0;
    for game in &job.games {
        let db = Connection::open_with_flags(
            job.root.join("library.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(|e| e.to_string())?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let original: Option<String> = db
            .query_row(
                "SELECT original_filename FROM games WHERE id=?",
                [&game.id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        let (title, url) = cached_identity(&job.root, game, original.as_deref(), region)
            .unwrap_or_else(|| {
                (
                    crate::region::title_from_filename(&game.title).to_owned(),
                    String::new(),
                )
            });
        if metadata {
            let overridden:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM metadata_overrides WHERE game_id=? AND field='title')",[&game.id],|r|r.get(0)).unwrap_or(false);
            if !overridden {
                db.execute(
                    "UPDATE games SET title=? WHERE id=? AND title<>?1",
                    params![title, game.id],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        if artwork && game.cover.is_none() {
            let _ = tx.send(Event::Message(format!("Finding cover art for {title}…")));
            ctx.request_repaint();
            if let Some(bytes) = disc_backup::cover(&job.root, &game.system, &title, &url, region) {
                let root = job.root.join("covers");
                std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                if let Ok(file) = settings::conversion::encode(
                    &root,
                    &bytes,
                    quality,
                    &std::sync::atomic::AtomicBool::new(false),
                ) {
                    db.execute(
                        "UPDATE games SET cover=? WHERE id=? AND (cover IS NULL OR cover='')",
                        params![format!("covers/{file}"), game.id],
                    )
                    .map_err(|e| e.to_string())?;
                    covers += 1;
                }
            }
        }
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(|e| e.to_string())?;
        crate::importing::export_inventory(&job.root, &db, &game.system)?;
    }
    Ok(if messages.is_empty() {
        format!("Import complete · {covers} covers downloaded")
    } else {
        format!(
            "Games imported · {covers} covers downloaded. {}",
            messages.join("\n")
        )
    })
}
fn cached_identity(
    root: &std::path::Path,
    game: &Game,
    original: Option<&str>,
    region: &str,
) -> Option<(String, String)> {
    let db = Connection::open_with_flags(
        crate::storage::metadata(root).join("openvgdb.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .ok()?;
    let filename = original.or_else(|| game.rom.file_name().and_then(|name| name.to_str()))?;
    catalog_identity(&db, &game.system, filename, region)
}

fn catalog_identity(
    db: &Connection,
    system: &str,
    filename: &str,
    region: &str,
) -> Option<(String, String)> {
    let catalog: Value = serde_json::from_str(include_str!("../../../data/systems.json")).ok()?;
    let names = crate::openvgdb::system_names(&catalog, system)?;
    db.query_row(
        "SELECT releaseTitleName, releaseCoverFront FROM ROMs JOIN SYSTEMS USING(systemID)
         JOIN RELEASES USING(romID) LEFT JOIN REGIONS ON regionLocalizedID=REGIONS.regionID
         WHERE systemShortName IN (SELECT value FROM json_each(?1))
           AND romFileName=?2 COLLATE NOCASE AND trim(releaseTitleName) <> ''
         ORDER BY regionName=?3 COLLATE NOCASE DESC, regionName='World' DESC, releaseID LIMIT 1",
        [names.as_str(), filename, region],
        |row| {
            Ok((
                row.get::<_, String>(0)?.trim().to_owned(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        },
    )
    .ok()
}

// Existing filename-style entries use the same catalog title as a new import.
pub fn refresh_display_titles(library: &mut emuluna_library_model::Library, region: &str) {
    let Ok(index) = Connection::open_with_flags(
        library.root.join("library.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        return;
    };
    let Ok(mut originals) = index.prepare("SELECT original_filename FROM games WHERE id=?") else {
        return;
    };
    let mut overrides = index
        .prepare("SELECT 1 FROM metadata_overrides WHERE game_id=? AND field='title'")
        .ok();
    let catalog = Connection::open_with_flags(
        crate::storage::metadata(&library.root).join("openvgdb.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .ok();
    let mut pending = BTreeMap::<String, Vec<(usize, String)>>::new();
    for (i, game) in library.games.iter_mut().enumerate() {
        let cleaned = crate::region::title_from_filename(&game.title);
        if cleaned == game.title
            || overrides
                .as_mut()
                .is_some_and(|query| query.exists([&game.id]).unwrap_or(false))
        {
            continue;
        }
        let original = originals
            .query_row([&game.id], |row| row.get::<_, Option<String>>(0))
            .ok()
            .flatten();
        if let Some(name) = original.or_else(|| {
            game.rom
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        }) {
            pending
                .entry(game.system.clone())
                .or_default()
                .push((i, name.to_ascii_lowercase()));
        }
        game.set_display_title(cleaned.to_owned());
    }
    let (Some(db), Ok(systems)) = (
        catalog,
        serde_json::from_str::<Value>(include_str!("../../../data/systems.json")),
    ) else {
        return;
    };
    let Ok(mut statement) = db.prepare(
        "SELECT lower(romFileName), trim(releaseTitleName) FROM ROMs JOIN SYSTEMS USING(systemID)
         JOIN RELEASES USING(romID) LEFT JOIN REGIONS ON regionLocalizedID=REGIONS.regionID
         WHERE systemShortName IN (SELECT value FROM json_each(?1))
           AND lower(romFileName) IN (SELECT value FROM json_each(?2))
           AND trim(releaseTitleName) <> ''
         ORDER BY regionName=?3 COLLATE NOCASE DESC, regionName='World' DESC, releaseID",
    ) else {
        return;
    };
    for (system, games) in pending {
        let Some(names) = crate::openvgdb::system_names(&systems, &system) else {
            continue;
        };
        let Ok(files) = serde_json::to_string(
            &games
                .iter()
                .map(|(_, filename)| filename)
                .collect::<Vec<_>>(),
        ) else {
            continue;
        };
        let Ok(rows) = statement.query_map([names.as_str(), &files, region], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }) else {
            continue;
        };
        let mut titles = BTreeMap::new();
        for (filename, title) in rows.flatten() {
            titles.entry(filename).or_insert(title);
        }
        for (index, name) in games {
            if let Some(title) = titles.get(&name) {
                library.games[index].set_display_title(title.clone());
            }
        }
    }
}
