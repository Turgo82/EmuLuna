//! Review file disposal separately from removing library records.
use eframe::egui;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::mpsc,
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct Request {
    pub root: PathBuf,
    pub native_progress: PathBuf,
    pub cache: PathBuf,
    pub ids: Vec<String>,
    // Covers may be displayed from either the original file or a converted copy.
    pub covers: Vec<(String, PathBuf)>,
}
struct Game {
    id: String,
    title: String,
    system: String,
    rom: PathBuf,
}
#[derive(Default, PartialEq, Eq)]
struct Files {
    roms: BTreeSet<PathBuf>,
    states: BTreeSet<PathBuf>,
    screenshots: BTreeSet<PathBuf>,
    saves: BTreeSet<PathBuf>,
    cache: BTreeSet<PathBuf>,
}
#[derive(Clone, Copy, Default)]
struct Choices {
    roms: bool,
    states: bool,
    screenshots: bool,
    saves: bool,
    cache: bool,
}
impl Files {
    fn chosen(&self, choices: Choices) -> BTreeSet<&PathBuf> {
        [
            (choices.roms, &self.roms),
            (choices.states, &self.states),
            (choices.screenshots, &self.screenshots),
            (choices.saves, &self.saves),
            (choices.cache, &self.cache),
        ]
        .into_iter()
        .filter(|(chosen, _)| *chosen)
        .flat_map(|(_, files)| files)
        .collect()
    }
}
struct Plan {
    games: Vec<Game>,
    files: Files,
    shared_roms: usize,
    rom_error: Option<String>,
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn open(root: &Path, flags: OpenFlags) -> Result<Connection, String> {
    let db = Connection::open_with_flags(root.join("library.sqlite3"), flags).map_err(err)?;
    db.busy_timeout(Duration::from_secs(2)).map_err(err)?;
    Ok(db)
}
fn component(value: &str) -> bool {
    let mut parts = Path::new(value).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}
fn collect_files(
    folder: &Path,
    filter: fn(&Path) -> bool,
    output: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    // The callers supply canonical storage roots. Reject linked game folders and parents.
    if !folder.canonicalize().is_ok_and(|path| path == folder) {
        return Ok(());
    }
    let mut pending = vec![(folder.to_owned(), 0)];
    let mut scanned = 0;
    while let Some((folder, depth)) = pending.pop() {
        for entry in fs::read_dir(folder).map_err(err)? {
            let entry = entry.map_err(err)?;
            scanned += 1;
            if scanned > 100_000 || depth > 12 {
                return Err("The selected game's saved media exceeds the scan limit.".into());
            }
            let kind = entry.file_type().map_err(err)?;
            if kind.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file() && filter(&entry.path()) {
                output.insert(entry.path());
            }
        }
    }
    Ok(())
}
fn ext(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|part| part.to_str())
        .is_some_and(|part| part.eq_ignore_ascii_case(expected))
}
fn state_file(path: &Path) -> bool {
    emuluna_state_file::has_state_extension(path) || ext(path, "png")
}
fn screenshot(path: &Path) -> bool {
    ext(path, "png")
}
fn save_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|part| part.to_str())
        .is_some_and(|name| !name.starts_with('.') && !name.ends_with(".lock"))
}

fn collect_media(
    request: &Request,
    game: &Game,
    consoles: Option<&Path>,
    native: Option<&Path>,
    files: &mut Files,
) -> Result<(), String> {
    type MediaFiles<'a> = (&'static str, fn(&Path) -> bool, &'a mut BTreeSet<PathBuf>);
    let kinds: [MediaFiles<'_>; 3] = [
        ("states", state_file, &mut files.states),
        ("screenshots", screenshot, &mut files.screenshots),
        ("saves", save_file, &mut files.saves),
    ];
    for (kind, filter, target) in kinds {
        collect_files(&request.root.join(kind).join(&game.id), filter, target)?;
        if let Some(consoles) = consoles {
            collect_files(
                &consoles.join(&game.system).join(kind).join(&game.id),
                filter,
                target,
            )?;
        }
        if let Some(native) = native {
            let game_root = native.join(&game.id);
            if game_root.canonicalize().is_ok_and(|path| path == game_root) {
                for core in fs::read_dir(game_root).map_err(err)? {
                    let core = core.map_err(err)?;
                    if core.file_type().map_err(err)?.is_dir() {
                        collect_files(&core.path().join(kind), filter, target)?;
                    }
                }
            }
        }
    }
    Ok(())
}
fn rom_files(
    db: &Connection,
    games: &[Game],
    ids: &HashSet<&str>,
) -> Result<(BTreeSet<PathBuf>, usize), String> {
    let mut inventory = db
        .prepare("SELECT path FROM game_files WHERE game_id=?1")
        .map_err(err)?;
    let mut selected = BTreeSet::new();
    let mut protected = HashSet::new();
    for game in games {
        let entries = inventory
            .query_map([&game.id], |row| row.get::<_, String>(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let parent = game
            .rom
            .parent()
            .ok_or("A ROM path has no parent folder.")?;
        let base = parent.canonicalize().ok();
        let mut paths = vec![game.rom.clone()];
        if entries.len() > 1 {
            for entry in entries {
                let entry = Path::new(&entry);
                if !entry
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                {
                    return Err("A disc inventory contains a path outside its game folder.".into());
                }
                let path = parent.join(entry);
                if let (Ok(path), Some(base)) = (path.canonicalize(), &base) {
                    if !path.starts_with(base) {
                        return Err("A disc file points outside its game folder.".into());
                    }
                }
                paths.push(path);
            }
        }
        for path in paths {
            let canonical = match path.canonicalize() {
                Ok(path) => path,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(err(error)),
            };
            if ids.contains(game.id.as_str()) {
                if !canonical.is_file() || fs::symlink_metadata(&path).is_ok_and(|m| m.is_symlink())
                {
                    return Err("A selected ROM path is a folder or a symbolic link.".into());
                }
                selected.insert(canonical);
            } else {
                protected.insert(canonical);
            }
        }
    }
    let before = selected.len();
    selected.retain(|path| !protected.contains(path));
    let shared = before - selected.len();
    Ok((selected, shared))
}

fn plan(db: &Connection, request: &Request) -> Result<Plan, String> {
    let ids: HashSet<_> = request.ids.iter().map(String::as_str).collect();
    let mut statement = db
        .prepare("SELECT id,title,system,rom_path FROM games ORDER BY title COLLATE NOCASE")
        .map_err(err)?;
    let games = statement
        .query_map([], |row| {
            Ok(Game {
                id: row.get(0)?,
                title: row.get(1)?,
                system: row.get(2)?,
                rom: request.root.join(row.get::<_, String>(3)?),
            })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut files = Files::default();
    let (shared_roms, rom_error) = match rom_files(db, &games, &ids) {
        Ok((roms, shared)) => {
            files.roms = roms;
            (shared, None)
        }
        Err(error) => (0, Some(error)),
    };
    let consoles = crate::media::authorized_console(&request.root);
    let native = request
        .native_progress
        .canonicalize()
        .ok()
        .filter(|root| root == &request.native_progress);
    let retained_prefixes: HashSet<_> = games
        .iter()
        .filter(|game| !ids.contains(game.id.as_str()))
        .filter_map(|game| game.id.get(..12))
        .collect();
    let wanted_prefixes: HashSet<_> = games
        .iter()
        .filter(|game| ids.contains(game.id.as_str()))
        .filter_map(|game| game.id.get(..12))
        .filter(|prefix| !retained_prefixes.contains(prefix))
        .collect();
    let screenshot_root = request.root.join("screenshots");
    if screenshot_root
        .canonicalize()
        .is_ok_and(|path| path == screenshot_root)
    {
        for entry in fs::read_dir(screenshot_root).map_err(err)? {
            let entry = entry.map_err(err)?;
            if entry.file_type().map_err(err)?.is_file()
                && screenshot(&entry.path())
                && entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.split_once('-'))
                    .is_some_and(|(prefix, _)| wanted_prefixes.contains(prefix))
            {
                files.screenshots.insert(entry.path());
            }
        }
    }
    if request
        .cache
        .canonicalize()
        .is_ok_and(|path| path == request.cache)
    {
        let other_covers: HashSet<_> = request
            .covers
            .iter()
            .filter(|(id, _)| !ids.contains(id.as_str()))
            .map(|(_, path)| path)
            .collect();
        for (id, cover) in &request.covers {
            if ids.contains(id.as_str()) && !other_covers.contains(cover) {
                if let Some(path) = crate::artwork::preview_path(cover, &request.cache) {
                    if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_file()) {
                        files.cache.insert(path);
                    }
                }
            }
        }
    }
    let games: Vec<_> = games
        .into_iter()
        .filter(|game| ids.contains(game.id.as_str()))
        .collect();
    if games.len() != ids.len() || games.is_empty() {
        return Err("The selection changed. Select the games again before removing them.".into());
    }
    for game in &games {
        if !component(&game.id) || !component(&game.system) {
            return Err("A selected game has an invalid library identifier.".into());
        }
        collect_media(
            request,
            game,
            consoles.as_deref(),
            native.as_deref(),
            &mut files,
        )?;
    }
    Ok(Plan {
        games,
        files,
        shared_roms,
        rom_error,
    })
}

pub(crate) struct Removed {
    pub ids: Vec<String>,
    pub warning: Option<String>,
}
fn remove(
    request: Request,
    reviewed: &Plan,
    choices: Choices,
    mut trash: impl FnMut(&Path) -> Result<(), String>,
) -> Result<Removed, String> {
    let mut db = open(&request.root, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    db.pragma_update(None, "foreign_keys", true).map_err(err)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    let current = plan(&tx, &request)?;
    let files = current.files.chosen(choices);
    if (choices.roms && current.rom_error.is_some()) || files != reviewed.files.chosen(choices) {
        return Err("The files changed since you reviewed them. Reopen the removal dialog to review the new list.".into());
    }
    // The legacy artwork table has no foreign key. Other related rows cascade.
    for game in &current.games {
        tx.execute("DELETE FROM artwork WHERE game_id=?1", [&game.id])
            .map_err(err)?;
        tx.execute(
            "DELETE FROM settings WHERE instr(key, ?1)=1 OR instr(key, ?2)=1",
            [
                format!("core_option.{}.", game.id),
                format!("core_option.game.{}.", game.id),
            ],
        )
        .map_err(err)?;
        tx.execute("DELETE FROM games WHERE id=?1", [&game.id])
            .map_err(err)?;
    }
    for (moved, path) in files.into_iter().enumerate() {
        if let Err(error) = trash(path) {
            return Err(format!("{error} {moved} file(s) were already moved to Trash. The library entries were kept."));
        }
    }
    tx.commit().map_err(err)?;
    let consoles = crate::media::authorized_console(&request.root);
    if choices.roms {
        for game in &current.games {
            let legacy = request.root.join("roms");
            let mut roots = vec![
                legacy.join(&game.id),
                legacy.join(&game.system).join(&game.id),
            ];
            if let Some(consoles) = &consoles {
                let roms = consoles.join(&game.system).join("roms");
                if let Ok(rom) = game.rom.parent().unwrap_or(Path::new("")).canonicalize() {
                    if let Ok(relative) = rom.strip_prefix(&roms) {
                        if let Some(Component::Normal(folder)) = relative.components().next() {
                            roots.push(roms.join(folder));
                        }
                    }
                }
            }
            for root in roots {
                if !root.canonicalize().is_ok_and(|path| path == root) {
                    continue;
                }
                for path in &current.files.roms {
                    if !path.starts_with(&root) {
                        continue;
                    }
                    let mut parent = path.parent();
                    while let Some(folder) = parent.filter(|folder| folder.starts_with(&root)) {
                        // remove_dir cannot remove unrelated files or nonempty folders.
                        let _ = fs::remove_dir(folder);
                        parent = folder.parent();
                    }
                }
            }
        }
    }
    let mut warning = None;
    if let Some(consoles) = consoles {
        let systems: BTreeSet<_> = current.games.iter().map(|game| &game.system).collect();
        for system in systems {
            let folder = consoles.join(system);
            if folder.canonicalize().is_ok_and(|path| path == folder) {
                if let Err(error) = crate::importing::export_inventory(&request.root, &db, system) {
                    warning = Some(format!("Could not refresh the {system} inventory: {error}"));
                }
            }
        }
    }
    Ok(Removed {
        ids: request.ids,
        warning,
    })
}
pub(crate) fn desktop_trash(path: &Path) -> Result<(), String> {
    let output = Command::new("gio")
        .args(["trash", "--"])
        .arg(path)
        .output()
        .map_err(err)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Could not move “{}” to Trash. {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

enum Event {
    Reviewed(Request, Result<Plan, String>),
    Finished(Result<Removed, String>),
}
struct Dialog {
    request: Request,
    plan: Plan,
    choices: Choices,
}
#[derive(Default)]
pub(crate) struct Removal {
    dialog: Option<Dialog>,
    work: Option<mpsc::Receiver<Event>>,
    working: &'static str,
    pub message: String,
}
impl Removal {
    pub fn busy(&self) -> bool {
        self.work.is_some()
    }

    pub fn request(&mut self, ctx: &egui::Context, request: Request) {
        if self.work.is_some() {
            return;
        }
        self.message.clear();
        let (send, receive) = mpsc::channel();
        self.work = Some(receive);
        self.working = "Checking selected files…";
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = open(&request.root, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .and_then(|db| plan(&db, &request));
            let _ = send.send(Event::Reviewed(request, result));
            ctx.request_repaint();
        });
    }

    pub fn show(&mut self, ctx: &egui::Context, blocked: bool) -> Option<Removed> {
        if let Some(result) = self.work.as_ref().and_then(|work| work.try_recv().ok()) {
            self.work = None;
            match result {
                Event::Reviewed(request, Ok(plan)) => {
                    self.dialog = Some(Dialog {
                        request,
                        plan,
                        choices: Choices::default(),
                    });
                }
                Event::Finished(Ok(removed)) => return Some(removed),
                Event::Reviewed(_, Err(error)) | Event::Finished(Err(error)) => {
                    self.message = format!("Could not finish removal. {error}");
                }
            }
        }
        let mut close = false;
        let mut confirm = false;
        if self.work.is_some() {
            egui::Modal::new(egui::Id::new("removal-working"))
                .frame(egui::Frame::window(&ctx.style()).inner_margin(18))
                .show(ctx, |ui| {
                    ui.heading("Remove games");
                    ui.label(self.working);
                    crate::activity::progress(ui, 250., 9., 0, 0, true);
                });
        }
        if let Some(dialog) = &mut self.dialog {
            let result = egui::Modal::new(egui::Id::new("remove-library-games"))
                .frame(egui::Frame::window(&ctx.style()).inner_margin(18))
                .show(ctx, |ui| {
                    ui.set_width((ctx.screen_rect().width() - 60.).min(530.));
                    ui.spacing_mut().interact_size.y = 28.;
                    ui.heading("Remove games");
                    ui.label(if dialog.plan.games.len() == 1 {
                        format!("Remove “{}” from the library?", dialog.plan.games[0].title)
                    } else {
                        format!("Remove {} games from the library?", dialog.plan.games.len())
                    });
                    ui.radio_value(&mut dialog.choices.roms, false, "Keep ROM files");
                    ui.add_enabled_ui(
                        dialog.plan.rom_error.is_none() && !dialog.plan.files.roms.is_empty(),
                        |ui| {
                            ui.radio_value(
                                &mut dialog.choices.roms,
                                true,
                                "Move ROM files to Trash",
                            );
                        },
                    );
                    if let Some(error) = &dialog.plan.rom_error {
                        ui.label(format!("ROM files must be kept. {error}"));
                    }
                    if dialog.plan.shared_roms > 0 {
                        ui.label(format!(
                            "{} shared ROM file(s) will be kept for other library games.",
                            dialog.plan.shared_roms
                        ));
                    }
                    for (label, chosen, files) in [
                        (
                            "Also move save states and their previews to Trash",
                            &mut dialog.choices.states,
                            &dialog.plan.files.states,
                        ),
                        (
                            "Also move in-game saves to Trash",
                            &mut dialog.choices.saves,
                            &dialog.plan.files.saves,
                        ),
                        (
                            "Also move screenshots to Trash",
                            &mut dialog.choices.screenshots,
                            &dialog.plan.files.screenshots,
                        ),
                        (
                            "Also move cached box-art thumbnails to Trash",
                            &mut dialog.choices.cache,
                            &dialog.plan.files.cache,
                        ),
                    ] {
                        ui.add_enabled(!files.is_empty(), egui::Checkbox::new(chosen, label));
                    }
                    let files = dialog.plan.files.chosen(dialog.choices);
                    egui::Frame::new()
                        .fill(ui.visuals().extreme_bg_color)
                        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                        .corner_radius(6.)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            let height = (ctx.screen_rect().height() - 464.).clamp(64., 116.);
                            egui::ScrollArea::vertical()
                                .max_height(height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if files.is_empty() {
                                        ui.weak("No files will be moved or deleted.");
                                    } else {
                                        ui.label(format!(
                                            "{} file(s) to move to Trash",
                                            files.len()
                                        ));
                                        for path in &files {
                                            ui.label(path.display().to_string());
                                        }
                                    }
                                });
                        });
                    if blocked {
                        ui.weak("Wait for the current game or library task to finish.");
                    }
                    ui.add_space(8.);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if files.is_empty() {
                            "Remove from library"
                        } else {
                            "Remove and move files to Trash"
                        };
                        confirm = ui
                            .add_enabled(
                                !blocked,
                                egui::Button::new(label)
                                    .min_size(egui::vec2(148., 34.))
                                    .corner_radius(7.),
                            )
                            .clicked();
                        close = ui
                            .add_sized([84., 34.], egui::Button::new("Cancel").corner_radius(7.))
                            .clicked();
                    });
                });
            close |= result.should_close();
        }
        if confirm {
            let dialog = self.dialog.take().expect("confirmation requires a dialog");
            let (send, receive) = mpsc::channel();
            self.work = Some(receive);
            self.working = "Removing selected games…";
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = remove(dialog.request, &dialog.plan, dialog.choices, desktop_trash);
                let _ = send.send(Event::Finished(result));
                ctx.request_repaint();
            });
        } else if close {
            self.dialog = None;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Request) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let db = Connection::open(root.join("library.sqlite3")).unwrap();
        db.execute_batch(
            "
            CREATE TABLE games(id TEXT PRIMARY KEY, title TEXT, system TEXT, rom_path TEXT);
            CREATE TABLE artwork(game_id TEXT PRIMARY KEY);
            CREATE TABLE settings(key TEXT PRIMARY KEY);
            CREATE TABLE game_files(game_id TEXT REFERENCES games(id) ON DELETE CASCADE, path TEXT);
            INSERT INTO games VALUES
                ('a','Game A','nes','roms/nes/a/a.nes'),
                ('b','Game B','nes','roms/nes/b/b.nes'),
                ('c','Shared Game','nes','roms/nes/b/b.nes');
            INSERT INTO game_files VALUES ('a','a.nes'), ('b','b.nes'), ('c','b.nes');
            INSERT INTO artwork VALUES ('a'), ('b'), ('c');
        ",
        )
        .unwrap();
        for name in [
            "roms/nes/a/a.nes",
            "roms/nes/b/b.nes",
            "states/a/auto.oesavestate",
            "states/a/auto.png",
            "saves/a/battery.srm",
            "screenshots/a/screen.png",
            "native/a/core/states/slot-1.oesavestate",
            "native/a/core/saves/game.srm",
        ] {
            let file = root.join(name);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"fixture").unwrap();
        }
        let request = Request {
            root: root.clone(),
            native_progress: root.join("native"),
            cache: root.join("cache"),
            ids: vec!["a".into(), "b".into()],
            covers: Vec::new(),
        };
        (temp, request)
    }

    #[test]
    fn multi_remove_keeps_shared_roms_and_unchecked_saved_media() {
        let (_temp, request) = fixture();
        let db = open(&request.root, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let reviewed = plan(&db, &request).unwrap();
        assert_eq!(reviewed.shared_roms, 1);
        assert_eq!(reviewed.files.states.len(), 3);
        assert_eq!(reviewed.files.saves.len(), 2);
        let root = request.root.clone();
        let outcome = remove(
            request,
            &reviewed,
            Choices {
                roms: true,
                states: true,
                ..Default::default()
            },
            |path| {
                // Stand in for the desktop Trash on disposable fixtures only.
                fs::remove_file(path).map_err(err)
            },
        )
        .unwrap();
        assert_eq!(outcome.ids, ["a", "b"]);
        assert!(!root.join("roms/nes/a").exists());
        assert!(root.join("roms/nes/b/b.nes").is_file());
        assert!(!root.join("states/a/auto.png").exists());
        assert!(!root
            .join("native/a/core/states/slot-1.oesavestate")
            .exists());
        assert!(root.join("screenshots/a/screen.png").is_file());
        assert!(root.join("saves/a/battery.srm").is_file());
        assert_eq!(
            db.query_row("SELECT id FROM games", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "c"
        );
        assert_eq!(
            db.query_row("SELECT game_id FROM game_files", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "c"
        );
    }

    #[test]
    fn failed_or_changed_trash_plan_keeps_library_entries() {
        let (_temp, request) = fixture();
        let db = open(&request.root, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let reviewed = plan(&db, &request).unwrap();
        let choices = Choices {
            states: true,
            ..Default::default()
        };
        let failure = remove(request.clone(), &reviewed, choices, |_| {
            Err("Trash unavailable.".into())
        });
        assert!(failure.err().unwrap().contains("library entries were kept"));
        fs::write(request.root.join("states/a/new.png"), b"late file").unwrap();
        let changed = remove(request, &reviewed, choices, |_| {
            panic!("changed plans must not move files")
        });
        assert!(changed.err().unwrap().contains("files changed"));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM games", [], |row| row
                .get::<_, usize>(0))
                .unwrap(),
            3
        );
    }
}
