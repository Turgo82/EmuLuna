//! Native imports: bounded scanning, verified managed copies, transactional index writes.
use eframe::egui;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub struct Importer {
    pub open: bool,
    pub busy: bool,
    folder: bool,
    dropped: Vec<PathBuf>,
    retry_paths: Vec<PathBuf>,
    auto_start: bool,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    system: String,
    receiver: Option<mpsc::Receiver<Event>>,
    pub message: String,
    pub progress: (usize, usize),
}
enum Event {
    Selection(Vec<PathBuf>),
    Progress(usize, usize, String),
    Done(Result<usize, String>),
}
impl Importer {
    pub fn show(&mut self, ctx: &egui::Context, root: Option<&Path>) -> bool {
        let mut changed = false;
        if self.auto_start && !self.busy {
            if let Some(root) = root {
                self.auto_start = false;
                self.start(ctx, root.to_owned());
            }
        }
        if let Some(rx) = &self.receiver {
            while let Ok(event) = rx.try_recv() {
                match event {
                    Event::Selection(paths) => self.retry_paths = paths,
                    Event::Progress(n, t, name) => {
                        self.progress = (n, t);
                        self.message = name;
                    }
                    Event::Done(result) => {
                        self.busy = false;
                        changed = true;
                        self.message = match result {
                            Ok(n) => {
                                self.open = false;
                                self.retry_paths.clear();
                                format!("Imported {n} new games. Existing games were skipped.")
                            }
                            Err(e) => {
                                self.open = true;
                                e
                            }
                        };
                    }
                }
            }
        }
        if !self.busy {
            self.receiver = None;
        }
        if self.open && !self.busy && !self.auto_start {
            let mut open = true;
            egui::Window::new(emuluna_theme::fonts::window_title("Import games"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.set_width(440.);
                    ui.label("Import follows your library copy setting. Your originals are kept.");
                    ui.horizontal(|ui| {
                        ui.label("Console");
                        egui::ComboBox::from_id_salt("import-console")
                            .selected_text(if self.system.is_empty() {
                                "Detect automatically"
                            } else {
                                &self.system
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.system,
                                    String::new(),
                                    "Detect automatically",
                                );
                                for (id, spec) in catalog().as_object().expect("catalog") {
                                    ui.selectable_value(
                                        &mut self.system,
                                        id.clone(),
                                        spec["name"].as_str().unwrap_or(id),
                                    );
                                }
                            });
                    });
                    ui.label("Choose a console for ambiguous disc images such as CHD or CUE.");
                    if ui
                        .add_enabled(
                            !self.busy && root.is_some(),
                            egui::Button::new(if !self.retry_paths.is_empty() {
                                "Retry import"
                            } else if !self.dropped.is_empty() {
                                "Import dropped games"
                            } else if self.folder {
                                "Choose folder…"
                            } else {
                                "Choose games…"
                            }),
                        )
                        .clicked()
                    {
                        self.start(ctx, root.expect("enabled").to_owned());
                    }
                    if self.busy {
                        let (n, t) = self.progress;
                        ui.add(
                            egui::ProgressBar::new(if t == 0 { 0. } else { n as f32 / t as f32 })
                                .show_percentage(),
                        );
                        ui.spinner();
                        if ui.button("Cancel import").clicked() {
                            self.cancel
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    if !self.busy && !self.retry_paths.is_empty() {
                        egui::ScrollArea::vertical()
                            .max_height(150.)
                            .show(ui, |ui| {
                                let mut remove = None;
                                for (i, path) in self.retry_paths.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            path.file_name().unwrap_or_default().to_string_lossy(),
                                        );
                                        if ui.small_button("Skip").clicked() {
                                            remove = Some(i);
                                        }
                                    });
                                }
                                if let Some(i) = remove {
                                    self.retry_paths.remove(i);
                                }
                            });
                    }
                    if !self.message.is_empty() {
                        ui.label(&self.message);
                    }
                });
            self.open = open;
        }
        changed
    }
    pub fn show_issues(&mut self, root: &Path) {
        self.auto_start = false;
        self.open = true;
        self.system.clear();
        self.message = "Choose a console and retry the files that need attention.".into();
        self.retry_paths = Connection::open_with_flags(
            root.join("library.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()
        .and_then(|db| {
            db.prepare("SELECT path FROM import_issues WHERE status='open'")
                .ok()
                .and_then(|mut q| {
                    q.query_map([], |r| r.get::<_, String>(0))
                        .ok()
                        .map(|rows| rows.filter_map(Result::ok).map(PathBuf::from).collect())
                })
        })
        .unwrap_or_default();
        if self.retry_paths.is_empty() {
            self.message = "No unresolved import issues.".into();
        }
    }
    pub(crate) fn sidebar(&mut self, ui: &mut egui::Ui) {
        use std::sync::atomic::Ordering;
        let cancelling = self.cancel.load(Ordering::Relaxed);
        let (done, total) = self.progress;
        if crate::activity::sidebar(
            ui,
            "Importing games",
            if cancelling {
                "Cancelling…"
            } else {
                &self.message
            },
            (done as u64, total as u64),
            Some(!cancelling),
        ) {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }
    pub fn request(&mut self, folder: bool) {
        if self.busy {
            return;
        }
        self.folder = folder;
        self.retry_paths.clear();
        self.dropped.clear();
        self.system.clear();
        self.auto_start = true;
        self.open = true;
    }
    pub fn drop_paths(&mut self, paths: Vec<PathBuf>) {
        if !self.busy {
            self.dropped = paths;
            self.retry_paths.clear();
            self.system.clear();
            self.auto_start = true;
            self.open = true;
            self.message = "Dropped files ready. Choose a console if needed, then import.".into();
        }
    }
    fn start(&mut self, ctx: &egui::Context, root: PathBuf) {
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.busy = true;
        self.message = "Choosing games…".into();
        self.progress = (0, 0);
        let folder = self.folder;
        let dropped = if self.dropped.is_empty() {
            self.retry_paths.clone()
        } else {
            std::mem::take(&mut self.dropped)
        };
        self.cancel = Default::default();
        let cancel = self.cancel.clone();
        let system = self.system.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let paths = if dropped.is_empty() {
                    pick(folder)?
                } else {
                    dropped
                };
                if paths.is_empty() {
                    return Ok(0);
                }
                let _ = tx.send(Event::Selection(paths.clone()));
                let initial_count = game_count(&root)?;
                let mut files = Vec::new();
                let mut seen = HashSet::new();
                for path in paths {
                    scan(&path, &root, &mut files, &mut seen)?;
                }
                // Never import an audio/data track separately when a descriptor references it.
                let mut tracks = HashSet::new();
                for file in &files {
                    if !matches!(ext(file).as_str(), "cue" | "gdi" | "m3u" | "ccd") {
                        continue;
                    }
                    if let Ok(rows) = content(file) {
                        if rows.len() > 1 {
                            tracks.extend(rows.into_iter().skip(1).map(|r| r.0));
                        }
                    }
                }
                files.retain(|p| !tracks.contains(p));
                let total = files.len();

                let mut errors = Vec::new();
                let mut failed_paths = Vec::new();
                for (i, path) in files.into_iter().enumerate() {
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        return Err(format!(
                            "Import cancelled. {} new games were kept.",
                            game_count(&root)?.saturating_sub(initial_count)
                        ));
                    }
                    let _ = tx.send(Event::Progress(
                        i,
                        total,
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into(),
                    ));
                    ctx.request_repaint();
                    match import_selected(&root, &path, &system) {
                        Ok(_) => {
                            let _ = save_issue(&root, &path, None, &system);
                        }
                        Err(e) => {
                            let _ = save_issue(&root, &path, Some(&e), &system);
                            failed_paths.push(path.clone());
                            errors.push(format!("{}: {e}", path.display()));
                        }
                    }
                }
                let _ = tx.send(Event::Selection(failed_paths));
                let count = game_count(&root)?.saturating_sub(initial_count);
                if errors.is_empty() {
                    Ok(count)
                } else {
                    Err(format!(
                        "Imported {count} new games. {} file(s) need attention:\n{}",
                        errors.len(),
                        errors.into_iter().take(12).collect::<Vec<_>>().join("\n")
                    ))
                }
            })();
            let _ = tx.send(Event::Done(result));
            ctx.request_repaint();
        });
    }
}
fn catalog() -> Value {
    serde_json::from_str(include_str!("../../../data/systems.json")).expect("systems")
}
fn ext(p: &Path) -> String {
    p.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}
fn recognized(p: &Path) -> bool {
    let e = ext(p);
    catalog().as_object().unwrap().values().any(|s| {
        s["extensions"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(&e)))
    })
}
fn scan(
    path: &Path,
    root: &Path,
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    if fs::symlink_metadata(path)
        .map_err(err)?
        .file_type()
        .is_symlink()
    {
        return Ok(());
    }
    let path = path.canonicalize().map_err(err)?;
    if path.starts_with(root)
        || root
            .join("consoles")
            .canonicalize()
            .is_ok_and(|p| path.starts_with(p))
    {
        return Ok(());
    }
    if !seen.insert(path.clone()) {
        return Ok(());
    }
    if seen.len() > 100_000 {
        return Err("The folder exceeds 100,000 entries. Choose a smaller folder.".into());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(err)? {
            let e = entry.map_err(err)?;
            if !e.file_name().to_string_lossy().starts_with('.') {
                scan(&e.path(), root, out, seen)?;
            }
        }
    } else if recognized(&path) || ext(&path) == "zip" {
        out.push(path)
    }
    Ok(())
}
fn pick(folder: bool) -> Result<Vec<PathBuf>, String> {
    let mut cmd = Command::new("kdialog");
    cmd.args([
        "--title",
        "Import games",
        if folder {
            "--getexistingdirectory"
        } else {
            "--getopenfilename"
        },
    ]);
    if !folder {
        cmd.args(["--multiple", "--separate-output"]);
    }
    let output = cmd
        .output()
        .or_else(|_| {
            let mut c = Command::new("zenity");
            c.args(["--file-selection", "--title=Import games"]);
            if folder {
                c.arg("--directory");
            } else {
                c.args(["--multiple", "--separator=\n"]);
            }
            c.output()
        })
        .map_err(err)?;
    if !output.status.success() {
        return Ok(vec![]);
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(err)?
        .lines()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect())
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub(crate) fn hash(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(err)?;
    let mut h = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b).map_err(err)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
// Descriptor parser preserves relative paths and rejects references outside the disc set.
fn content(path: &Path) -> Result<Vec<(PathBuf, String, u64)>, String> {
    fn visit(
        p: &Path,
        base: &Path,
        rows: &mut Vec<(PathBuf, String, u64)>,
        seen: &mut HashSet<PathBuf>,
        visiting: &mut HashSet<PathBuf>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 8 || seen.len() >= 256 {
            return Err("Disc set exceeds nesting/file limits".into());
        }
        let p = p
            .canonicalize()
            .map_err(|e| format!("Missing disc file: {e}"))?;
        if !p.starts_with(base) {
            return Err("Disc references must stay inside the disc folder".into());
        }
        if visiting.contains(&p) {
            return Err("Disc playlist contains a reference cycle".into());
        }
        if !seen.insert(p.clone()) {
            return Ok(());
        }
        visiting.insert(p.clone());
        let size = fs::metadata(&p).map_err(err)?.len();
        if size == 0 || size > 8 * 1024 * 1024 * 1024 {
            return Err("Empty file or file exceeds 8 GiB".into());
        }
        rows.push((p.clone(), hash(&p)?, size));
        let extension = ext(&p);
        let mut names = Vec::new();
        if matches!(extension.as_str(), "cue" | "gdi" | "m3u" | "ccd") {
            if size > 1024 * 1024 {
                return Err("Disc descriptor exceeds 1 MiB".into());
            }
            let text = fs::read_to_string(&p).map_err(err)?;
            if extension == "cue" {
                let upper = text.to_ascii_uppercase();
                if !upper
                    .lines()
                    .any(|l| l.split_whitespace().next() == Some("TRACK"))
                    || !upper.lines().any(|l| {
                        l.split_whitespace()
                            .collect::<Vec<_>>()
                            .starts_with(&["INDEX", "01"])
                    })
                {
                    return Err("CUE needs FILE, TRACK and INDEX 01 entries".into());
                }
            }
            for line in text.trim_start_matches('\u{feff}').lines() {
                let line = line.trim();
                if extension == "cue"
                    && line
                        .split_whitespace()
                        .next()
                        .is_some_and(|s| s.eq_ignore_ascii_case("FILE"))
                {
                    names.push(filename(
                        line.split_once(char::is_whitespace)
                            .ok_or("Missing CUE filename")?
                            .1
                            .trim(),
                    )?);
                }
                if extension == "m3u" && !line.is_empty() && !line.starts_with('#') {
                    names.push(line.into());
                }
                if extension == "gdi" {
                    let mut rest = line;
                    let mut fields = 0;
                    for _ in 0..4 {
                        rest = rest.trim_start();
                        if let Some(i) = rest.find(char::is_whitespace) {
                            rest = &rest[i..];
                            fields += 1;
                        } else {
                            break;
                        }
                    }
                    if fields == 4 {
                        names.push(filename(rest.trim())?);
                    }
                }
            }
            if extension == "ccd" {
                names.push(
                    p.with_extension("img")
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into(),
                );
                if p.with_extension("sub").is_file() {
                    names.push(
                        p.with_extension("sub")
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .into(),
                    );
                }
            }
            if names.is_empty() {
                return Err("Disc descriptor contains no track references".into());
            }
        }
        for name in names {
            let name = name.replace('\\', "/");
            let rel = Path::new(&name);
            if rel.is_absolute()
                || rel.components().any(|c| {
                    !matches!(
                        c,
                        std::path::Component::Normal(_) | std::path::Component::CurDir
                    )
                })
            {
                return Err("Unsafe disc reference".into());
            }
            visit(
                &p.parent().unwrap().join(rel),
                base,
                rows,
                seen,
                visiting,
                depth + 1,
            )?;
        }
        visiting.remove(&p);
        Ok(())
    }
    let p = path.canonicalize().map_err(err)?;
    let mut rows = Vec::new();
    visit(
        &p,
        p.parent().unwrap(),
        &mut rows,
        &mut HashSet::new(),
        &mut HashSet::new(),
        0,
    )?;
    if rows.iter().map(|(_, _, n)| n).sum::<u64>() > 32 * 1024 * 1024 * 1024 {
        return Err("Disc set exceeds 32 GiB".into());
    }
    Ok(rows)
}
fn filename(s: &str) -> Result<String, String> {
    if let Some(s) = s.strip_prefix('"') {
        Ok(s.split_once('"').ok_or("Unclosed track filename")?.0.into())
    } else {
        Ok(s.split_whitespace()
            .next()
            .ok_or("Missing track filename")?
            .into())
    }
}
fn detected_system(sample: &[u8]) -> Option<&'static str> {
    for (signature, id) in [
        (b"SEGADISCSYSTEM".as_slice(), "segacd"),
        (b"SEGA SEGASATURN".as_slice(), "saturn"),
        (b"SEGA SEGAKATANA".as_slice(), "dreamcast"),
        (b"PLAYSTATION".as_slice(), "psx"),
    ] {
        if sample.windows(signature.len()).any(|s| s == signature) {
            return Some(id);
        }
    }
    None
}
pub(crate) fn import(root: &Path, source: &Path, selected: &str) -> Result<bool, String> {
    import_mode(root, source, selected, false).map(|r| r.changed)
}
pub(crate) fn import_managed(root: &Path, source: &Path, selected: &str) -> Result<bool, String> {
    import_mode(root, source, selected, true).map(|r| r.changed)
}
pub(crate) struct ManagedImport {
    pub id: String,
    pub duplicate: bool,
    pub changed: bool,
}
pub(crate) fn import_disc(
    root: &Path,
    source: &Path,
    selected: &str,
) -> Result<ManagedImport, String> {
    import_mode(root, source, selected, true)
}
fn import_mode(
    root: &Path,
    source: &Path,
    selected: &str,
    force_copy: bool,
) -> Result<ManagedImport, String> {
    if source.to_str().is_none() {
        return Err("ROM paths must use valid Unicode filenames".into());
    }
    let systems = catalog();
    let mut choices = Vec::new();
    let extension = ext(source);
    for (id, s) in systems.as_object().unwrap() {
        if s["extensions"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(&extension)))
        {
            choices.push(id.as_str());
        }
    }
    let rows = content(source)?;
    let sample = if extension == "chd" {
        let helper =
            crate::resources::path("build/cores/emuluna-chd-probe");
        bounded_command(Command::new(helper).arg(source), 16 * 1024 * 1024).map_err(|_| {
            "CHD image could not be validated. Check the file and native CHD helper.".to_owned()
        })?
    } else {
        let mut data = Vec::new();
        let mut read = 0;
        for (path, _, _) in &rows {
            if read >= 2 {
                break;
            }
            if !matches!(ext(path).as_str(), "cue" | "gdi" | "m3u" | "ccd") {
                fs::File::open(path)
                    .map_err(err)?
                    .take(2 * 1024 * 1024)
                    .read_to_end(&mut data)
                    .map_err(err)?;
                read += 1;
            }
        }
        data
    };
    let detected = detected_system(&sample);
    let mut system = if selected.is_empty() {
        if let Some(id) = detected {
            if !choices.contains(&id) {
                return Err("This disc format is not supported by the detected console".into());
            }
            id
        } else {
            if choices.len() != 1 {
                return Err(
                    "Choose the console in the import window, then retry this file.".into(),
                );
            }
            choices[0]
        }
    } else {
        if !choices.contains(&selected) {
            return Err("File extension is not supported by the selected console".into());
        }
        if detected.is_some_and(|id| id != selected) {
            return Err("The disc identifies as another console. Use automatic detection.".into());
        }
        selected
    };
    if matches!(system, "gb" | "gbc") && sample.get(0x143).is_some_and(|b| b & 0x80 != 0) {
        system = "gbc";
    }
    let minimum = match system {
        "snes" => 32768,
        "gba" => 192,
        "gb" | "gbc" => 0x150,
        _ => 1,
    };
    if rows[0].2 < minimum {
        return Err("ROM size is invalid for the selected console".into());
    }
    let disc = systems[system]["media"].as_str() == Some("disc");
    if !disc && rows[0].2 > 512 * 1024 * 1024 {
        return Err("Cartridge ROM exceeds 512 MiB".into());
    }
    if disc && extension == "bin" {
        return Err("Import the matching CUE sheet, not an individual disc track".into());
    }
    let base = source.parent().unwrap();
    let mut inventory=rows.iter().map(|(p,h,n)|json!({"path":p.strip_prefix(base).unwrap().to_string_lossy().replace('\\',"/"),"sha256":h,"size":n})).collect::<Vec<_>>();
    inventory.sort_by_key(|r| r["path"].as_str().unwrap().to_owned());
    let id = if rows.len() == 1 {
        rows[0].1.clone()
    } else {
        let canonical = python_json(&Value::Array(inventory.clone()));
        format!("{:x}", Sha256::digest(canonical.as_bytes()))
    };
    let mut db = Connection::open_with_flags(
        root.join("library.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .map_err(err)?;
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(err)?;
    let existing: Option<String> = db
        .query_row("SELECT rom_path FROM games WHERE id=?", [&id], |r| r.get(0))
        .optional()
        .map_err(err)?;
    let mut consolidate = force_copy
        && existing
            .as_ref()
            .is_some_and(|p| Path::new(p).is_absolute());
    if force_copy {
        if let Some(p) = existing.as_ref().filter(|p| !Path::new(p).is_absolute()) {
            let current = root.join(p);
            if !current.is_file() {
                consolidate = true;
            } else {
                match content(&current) {
                    Ok(actual) => {
                        for (path, sha, _) in actual {
                            let relative = path
                                .strip_prefix(current.parent().ok_or("Invalid managed disc path")?)
                                .map_err(err)?
                                .to_string_lossy();
                            let expected: Option<String> = db
                                .query_row(
                                    "SELECT sha256 FROM game_files WHERE game_id=? AND path=?",
                                    params![id, relative],
                                    |r| r.get(0),
                                )
                                .optional()
                                .map_err(err)?;
                            if expected.as_deref().is_some_and(|value| value != sha) {
                                return Err("An existing managed disc copy differs. Restore that copy or choose another library location.".into());
                            }
                        }
                    }
                    Err(e) if e.contains("Missing disc file") => consolidate = true,
                    Err(e) => return Err(e),
                }
            }
        }
    }
    if existing.is_some() && !consolidate {
        return Ok(ManagedImport {
            id,
            duplicate: true,
            changed: false,
        });
    }
    let storage = root.join("consoles");
    if storage.is_symlink() {
        let location: Value =
            serde_json::from_slice(&fs::read(root.join("console-location.json")).map_err(err)?)
                .map_err(err)?;
        let approved = PathBuf::from(
            location["path"]
                .as_str()
                .ok_or("Invalid console storage location")?,
        );
        if location["format"] != 1
            || !approved.is_absolute()
            || approved.canonicalize().map_err(err)? != storage.canonicalize().map_err(err)?
        {
            return Err("Console folder does not match the selected library location".into());
        }
    }
    let console = storage.join(system);
    if console.is_symlink() {
        return Err("Console subfolders cannot be symbolic links".into());
    }
    let folder = console.join("roms");
    fs::create_dir_all(&folder).map_err(err)?;
    for kind in ["saves", "states", "screenshots"] {
        fs::create_dir_all(console.join(kind)).map_err(err)?;
    }
    if !force_copy
        && db
            .query_row(
                "SELECT value FROM settings WHERE key='copy_games'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .as_deref()
            == Some("0")
    {
        let name = source.file_name().ok_or("Invalid ROM filename")?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(err)?
            .as_secs_f64();
        let tx = db.transaction().map_err(err)?;
        tx.execute("INSERT INTO games(id,title,system,rom_path,source,added,original_filename) VALUES(?,?,?,?,?,?,?)",params![id,crate::region::title_from_filename(&source.file_stem().unwrap().to_string_lossy()),system,source.to_string_lossy(),source.to_string_lossy(),now,name.to_string_lossy()]).map_err(err)?;
        for row in &inventory {
            tx.execute(
                "INSERT OR IGNORE INTO game_files(game_id,path,sha256,size) VALUES(?,?,?,?)",
                params![
                    id,
                    row["path"].as_str(),
                    row["sha256"].as_str(),
                    row["size"].as_u64()
                ],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(err)?;
        export_inventory(root, &db, system)?;
        return Ok(ManagedImport {
            id,
            duplicate: false,
            changed: true,
        });
    }
    let staging = tempfile::tempdir_in(&folder).map_err(err)?;
    for (p, h, _) in &rows {
        let to = staging.path().join(p.strip_prefix(base).unwrap());
        fs::create_dir_all(to.parent().unwrap()).map_err(err)?;
        fs::copy(p, &to).map_err(err)?;
        if hash(&to)? != *h {
            return Err("A source file changed during copying".into());
        }
    }
    let name = source.file_name().ok_or("Invalid filename")?;
    let destination = if rows.len() > 1 {
        folder.join(format!(
            "{}-{}",
            source.file_stem().unwrap().to_string_lossy(),
            &id[..12]
        ))
    } else {
        folder.join(name)
    };
    let destination = if destination.exists() {
        folder.join(format!(
            "{}-{id}.{}",
            source.file_stem().unwrap().to_string_lossy(),
            extension
        ))
    } else {
        destination
    };
    let managed = if rows.len() > 1 {
        destination.join(name)
    } else {
        destination.clone()
    };
    // Publish without overwriting existing library content. Roll back published files on index failure.
    if rows.len() > 1 {
        fs::create_dir(&destination).map_err(err)?;
        for entry in fs::read_dir(staging.path()).map_err(err)? {
            let entry = entry.map_err(err)?;
            fs::rename(entry.path(), destination.join(entry.file_name())).map_err(err)?;
        }
    } else {
        fs::hard_link(staging.path().join(name), &destination).map_err(err)?;
    }
    let result = (|| {
        let tx = db.transaction().map_err(err)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(err)?
            .as_secs_f64();
        if consolidate {
            tx.execute(
                "UPDATE games SET rom_path=? WHERE id=?",
                params![managed.strip_prefix(root).unwrap().to_string_lossy(), id],
            )
            .map_err(err)?;
        } else {
            tx.execute("INSERT INTO games(id,title,system,rom_path,source,added,original_filename) VALUES(?,?,?,?,?,?,?)",params![id,crate::region::title_from_filename(&source.file_stem().unwrap().to_string_lossy()),system,managed.strip_prefix(root).unwrap().to_string_lossy(),source.to_string_lossy(),now,name.to_string_lossy()]).map_err(err)?;
        }

        if consolidate {
            tx.execute("DELETE FROM game_files WHERE game_id=?", [&id])
                .map_err(err)?;
        }
        for row in &inventory {
            tx.execute(
                "INSERT OR IGNORE INTO game_files(game_id,path,sha256,size) VALUES(?,?,?,?)",
                params![
                    id,
                    row["path"].as_str(),
                    row["sha256"].as_str(),
                    row["size"].as_u64()
                ],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok::<_, String>(())
    })();
    if let Err(e) = result {
        if rows.len() > 1 {
            let _ = fs::remove_dir_all(&destination);
        } else {
            let _ = fs::remove_file(&destination);
        }
        return Err(e);
    }
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .map_err(err)?;
    export_inventory(root, &db, system)?;
    Ok(ManagedImport {
        id,
        duplicate: existing.is_some(),
        changed: true,
    })
}
fn python_json(v: &Value) -> String {
    match v {
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .map(|(k, v)| format!("{}: {}", python_json(&json!(k)), python_json(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(python_json).collect::<Vec<_>>().join(", ")
        ),
        Value::String(s) => {
            let mut out = String::from("\"");
            for c in s.chars() {
                if c.is_ascii() {
                    out.push_str(
                        &serde_json::to_string(&c.to_string()).unwrap()
                            [1..serde_json::to_string(&c.to_string()).unwrap().len() - 1],
                    );
                } else {
                    let mut buf = [0; 2];
                    for u in c.encode_utf16(&mut buf) {
                        out.push_str(&format!("\\u{u:04x}"));
                    }
                }
            }
            out.push('"');
            out
        }
        _ => v.to_string(),
    }
}
pub(crate) fn export_inventory(root: &Path, db: &Connection, system: &str) -> Result<(), String> {
    let mut stmt = db
        .prepare("SELECT id,title,rom_path FROM games WHERE system=? ORDER BY title")
        .map_err(err)?;
    let records = stmt
        .query_map([system], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut games = Vec::new();
    for (id, title, rom) in records {
        let mut s = db
            .prepare("SELECT path,sha256,size FROM game_files WHERE game_id=?")
            .map_err(err)?;
        let files=s.query_map([&id],|r|Ok(json!({"path":r.get::<_,String>(0)?,"sha256":r.get::<_,String>(1)?,"size":r.get::<_,u64>(2)?}))).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
        games.push(json!({"id":id,"title":title,"rom":rom,"external":Path::new(&rom).is_absolute(),"files":files,"saves":format!("consoles/{system}/saves/{id}"),"states":format!("consoles/{system}/states/{id}"),"screenshots":format!("consoles/{system}/screenshots/{id}")}));
    }
    let target = root.join("consoles").join(system).join("games.json");
    let mut temp = tempfile::NamedTempFile::new_in(target.parent().unwrap()).map_err(err)?;
    temp.write_all(
        &serde_json::to_vec_pretty(&json!({"format":1,"system":system,"games":games}))
            .map_err(err)?,
    )
    .map_err(err)?;
    temp.persist(target).map_err(err)?;
    Ok(())
}

fn bounded_command(cmd: &mut Command, limit: u64) -> Result<Vec<u8>, String> {
    use std::process::Stdio;
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(err)?;
    let result = (|| {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or("No archive output")?
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() as u64 > limit {
            return Err("ZIP output exceeds the import size limit".into());
        }
        Ok(bytes)
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(err)?;
    if !status.success() {
        return Err(
            "Could not read ZIP archive. Check it is complete and unzip is installed.".into(),
        );
    }
    result
}
fn import_selected(root: &Path, path: &Path, system: &str) -> Result<usize, String> {
    if ext(path) == "bin" && (system.is_empty() || catalog()[system]["media"] == "disc") {
        if let Some(cue) = companion_cue(path) {
            return import(root, &cue, system).map(usize::from);
        }
    }
    if ext(path) != "zip" {
        return import(root, path, system).map(usize::from);
    }
    let list = bounded_command(Command::new("unzip").arg("-Z1").arg(path), 1024 * 1024)?;
    let list = String::from_utf8(list).map_err(err)?;
    let names = list
        .lines()
        .filter(|name| !name.ends_with('/'))
        .collect::<Vec<_>>();
    if names.iter().any(|name| {
        matches!(
            ext(Path::new(name)).as_str(),
            "cue" | "chd" | "gdi" | "ccd" | "iso" | "cdi" | "m3u"
        )
    }) {
        return Err(
            "Extract disc archives first, then import the CUE/CHD or playlist with its tracks."
                .into(),
        );
    }
    let names = names
        .into_iter()
        .filter(|name| recognized(Path::new(name)))
        .collect::<Vec<_>>();
    if names.is_empty() || names.len() > 100 {
        return Err("ZIP must contain between 1 and 100 cartridge ROMs".into());
    }
    let staging = tempfile::tempdir().map_err(err)?;
    let mut total = 0u64;
    let mut count = 0;
    for (i, name) in names.into_iter().enumerate() {
        if name.contains(['*', '?', '[', ']', '\n', '\r']) {
            return Err("ZIP contains unsupported wildcard filenames; extract it manually.".into());
        }
        let bytes = bounded_command(
            Command::new("unzip").arg("-p").arg(path).arg(name),
            128 * 1024 * 1024 - total,
        )?;
        total += bytes.len() as u64;
        let folder = staging.path().join(i.to_string());
        fs::create_dir(&folder).map_err(err)?;
        let to = folder.join(Path::new(name).file_name().ok_or("Invalid ZIP filename")?);
        fs::write(&to, &bytes).map_err(err)?;
        if import_managed(root, &to, system)? {
            count += 1;
            let id = format!("{:x}", Sha256::digest(&bytes));
            let db = Connection::open(root.join("library.sqlite3")).map_err(err)?;
            db.execute(
                "UPDATE games SET source=? WHERE id=?",
                params![path.to_string_lossy(), id],
            )
            .map_err(err)?;
            db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                .map_err(err)?;
        }
    }
    Ok(count)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn library() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("library.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE games(id TEXT PRIMARY KEY,title TEXT NOT NULL,system TEXT NOT NULL,rom_path TEXT NOT NULL,source TEXT NOT NULL,added REAL,original_filename TEXT);CREATE TABLE game_files(game_id TEXT,path TEXT,sha256 TEXT,size INTEGER,PRIMARY KEY(game_id,path));").unwrap();
        dir
    }
    #[test]
    fn copies_deduplicates_and_handles_filename_collisions_without_touching_sources() {
        let root = library();
        let source = tempfile::tempdir().unwrap();
        let a = source.path().join("Demo.nes");
        fs::write(&a, b"first rom").unwrap();
        assert!(import(root.path(), &a, "").unwrap());
        assert!(!import(root.path(), &a, "").unwrap());
        fs::write(&a, b"second rom").unwrap();
        assert!(import(root.path(), &a, "").unwrap());
        assert_eq!(
            fs::read(root.path().join("consoles/nes/roms/Demo.nes")).unwrap(),
            b"first rom"
        );
        assert_eq!(fs::read(&a).unwrap(), b"second rom");
        let v: Value =
            serde_json::from_slice(&fs::read(root.path().join("consoles/nes/games.json")).unwrap())
                .unwrap();
        assert_eq!(v["games"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn cue_tracks_stay_together_and_missing_tracks_never_create_entries() {
        let root = library();
        let source = tempfile::tempdir().unwrap();
        let cue = source.path().join("Game.cue");
        fs::write(
            &cue,
            "FILE \"Game.bin\" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\n",
        )
        .unwrap();
        assert!(import(root.path(), &cue, "segacd").is_err());
        fs::write(source.path().join("Game.bin"), [0; 2352]).unwrap();
        assert!(import(root.path(), &cue, "segacd").unwrap());
        assert!(!import(root.path(), &cue, "segacd").unwrap());
        let db = Connection::open(root.path().join("library.sqlite3")).unwrap();
        let rom: String = db
            .query_row("SELECT rom_path FROM games", [], |r| r.get(0))
            .unwrap();
        assert!(root.path().join(&rom).with_file_name("Game.bin").is_file());
        assert!(import(root.path(), &source.path().join("Game.bin"), "segacd").is_err());
    }
    #[test]
    fn descriptor_traversal_is_rejected_and_ambiguous_images_require_console() {
        let root = library();
        let source = tempfile::tempdir().unwrap();
        let cue = source.path().join("Bad.cue");
        fs::write(&cue, "FILE \"../outside.bin\" BINARY\n").unwrap();
        assert!(import(root.path(), &cue, "segacd").is_err());
        let chd = source.path().join("Unknown.chd");
        fs::write(&chd, b"not a usable disc").unwrap();
        assert!(import(root.path(), &chd, "").is_err());
    }
    #[test]
    fn failed_database_insert_removes_the_published_copy() {
        let root = library();
        let db = Connection::open(root.path().join("library.sqlite3")).unwrap();
        db.execute_batch("DROP TABLE game_files;").unwrap();
        let source = tempfile::tempdir().unwrap();
        let rom = source.path().join("Test.nes");
        fs::write(&rom, b"valid fixture").unwrap();
        assert!(import(root.path(), &rom, "nes").is_err());
        assert!(!root.path().join("consoles/nes/roms/Test.nes").exists());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM games", [], |r| r.get::<_, usize>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn json_identity_matches_legacy_spacing_and_unicode_escaping() {
        assert_eq!(
            python_json(&json!({"size":2352,"path":"é.bin","sha256":"abc"})),
            "{\"path\": \"\\u00e9.bin\", \"sha256\": \"abc\", \"size\": 2352}"
        );
    }
    #[test]
    fn dropped_games_wait_for_user_confirmation() {
        let mut importer = Importer::default();
        importer.drop_paths(vec![PathBuf::from("Example.nes")]);
        assert!(importer.open);
        assert!(!importer.busy);
        assert_eq!(importer.dropped.len(), 1);
    }
}
#[cfg(test)]
mod more_tests {
    use super::*;
    #[test]
    fn detects_cd_consoles_from_headers_instead_of_names() {
        for (s, id) in [
            ("SEGADISCSYSTEM", "segacd"),
            ("SEGA SEGASATURN", "saturn"),
            ("SEGA SEGAKATANA", "dreamcast"),
            ("PLAYSTATION", "psx"),
        ] {
            assert_eq!(
                detected_system(format!("padding {s} bytes").as_bytes()),
                Some(id)
            );
        }
        assert_eq!(detected_system(b"No game header"), None);
    }
    #[test]
    fn gdi_filenames_with_spaces_and_multiple_whitespace_keep_all_tracks() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("track 01.bin"), [1; 2352]).unwrap();
        fs::write(
            dir.path().join("Game.gdi"),
            "1\n1   0   4   2352   \"track 01.bin\" 0\n",
        )
        .unwrap();
        assert_eq!(content(&dir.path().join("Game.gdi")).unwrap().len(), 2);
    }
    #[test]
    fn native_drop_worker_imports_without_opening_a_file_picker() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open(root.path().join("library.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE games(id TEXT PRIMARY KEY,title TEXT,system TEXT,rom_path TEXT,source TEXT,added REAL,original_filename TEXT);CREATE TABLE game_files(game_id TEXT,path TEXT,sha256 TEXT,size INTEGER);").unwrap();
        drop(db);
        let roms = tempfile::tempdir().unwrap();
        let path = roms.path().join("Dropped.nes");
        fs::write(&path, b"native drop fixture").unwrap();
        let mut importer = Importer::default();
        let ctx = egui::Context::default();
        importer.drop_paths(vec![path]);
        importer.start(&ctx, root.path().to_owned());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut result = None;
        while std::time::Instant::now() < until {
            if let Ok(Event::Done(done)) = importer
                .receiver
                .as_ref()
                .unwrap()
                .recv_timeout(std::time::Duration::from_millis(20))
            {
                result = Some(done);
                break;
            }
        }
        assert_eq!(result, Some(Ok(1)));
        assert!(root.path().join("consoles/nes/roms/Dropped.nes").is_file());
    }
}
#[cfg(test)]
mod archive_tests {
    use super::*;
    #[test]
    fn zip_cartridge_import_keeps_the_archive_and_original_filename() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open(root.path().join("library.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE games(id TEXT PRIMARY KEY,title TEXT,system TEXT,rom_path TEXT,source TEXT,added REAL,original_filename TEXT);CREATE TABLE game_files(game_id TEXT,path TEXT,sha256 TEXT,size INTEGER);").unwrap();
        drop(db);
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("Zipped.nes"), b"zip fixture").unwrap();
        let status = Command::new("zip")
            .current_dir(source.path())
            .args(["-q", "Game.zip", "Zipped.nes"])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            import_selected(root.path(), &source.path().join("Game.zip"), "").unwrap(),
            1
        );
        assert!(source.path().join("Game.zip").is_file());
        assert_eq!(
            fs::read(root.path().join("consoles/nes/roms/Zipped.nes")).unwrap(),
            b"zip fixture"
        );
    }
}

fn game_count(root: &Path) -> Result<usize, String> {
    let db = Connection::open_with_flags(
        root.join("library.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(err)?;
    db.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0))
        .map_err(err)
}

fn save_issue(root: &Path, path: &Path, message: Option<&str>, system: &str) -> Result<(), String> {
    let db = Connection::open_with_flags(
        root.join("library.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .map_err(err)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS import_issues(id INTEGER PRIMARY KEY,path TEXT NOT NULL UNIQUE,code TEXT NOT NULL,message TEXT NOT NULL,system TEXT,status TEXT NOT NULL DEFAULT 'open',created REAL NOT NULL,updated REAL NOT NULL)").map_err(err)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(err)?
        .as_secs_f64();
    if let Some(message) = message {
        db.execute("INSERT INTO import_issues(path,code,message,system,status,created,updated) VALUES(?,'native_import',?,?,'open',?,?) ON CONFLICT(path) DO UPDATE SET message=excluded.message,system=excluded.system,status='open',updated=excluded.updated",params![path.to_string_lossy(),message,system,now,now]).map_err(err)?;
    } else {
        db.execute(
            "UPDATE import_issues SET status='resolved',updated=? WHERE path=?",
            params![now, path.to_string_lossy()],
        )
        .map_err(err)?;
    }
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .map_err(err)?;
    Ok(())
}
#[cfg(test)]
mod parity_tests {
    use super::*;
    fn root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("library.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE games(id TEXT PRIMARY KEY,title TEXT,system TEXT,rom_path TEXT,source TEXT,added REAL,original_filename TEXT);CREATE TABLE game_files(game_id TEXT,path TEXT,sha256 TEXT,size INTEGER,PRIMARY KEY(game_id,path));CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT);INSERT INTO settings VALUES('copy_games','0');").unwrap();
        dir
    }
    #[test]
    fn external_import_preference_is_respected_and_backups_consolidate_external_duplicates() {
        let root = root();
        let source = tempfile::tempdir().unwrap();
        let file = source.path().join("External.nes");
        fs::write(&file, b"external fixture").unwrap();
        assert!(import(root.path(), &file, "").unwrap());
        let db = Connection::open(root.path().join("library.sqlite3")).unwrap();
        let path: String = db
            .query_row("SELECT rom_path FROM games", [], |r| r.get(0))
            .unwrap();
        assert_eq!(Path::new(&path), file);
        assert!(!root.path().join("consoles/nes/roms/External.nes").exists());
        assert!(import_managed(root.path(), &file, "").unwrap());
        let path: String = db
            .query_row("SELECT rom_path FROM games", [], |r| r.get(0))
            .unwrap();
        assert!(!Path::new(&path).is_absolute());
        assert_eq!(
            fs::read(root.path().join(path)).unwrap(),
            b"external fixture"
        );
        assert_eq!(fs::read(file).unwrap(), b"external fixture");
    }
    #[test]
    fn zipped_cartridges_stay_managed_even_when_external_import_is_selected() {
        let root = root();
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("Archive.nes"), b"zip external fixture").unwrap();
        assert!(Command::new("zip")
            .current_dir(source.path())
            .args(["-q", "Archive.zip", "Archive.nes"])
            .status()
            .unwrap()
            .success());
        assert_eq!(
            import_selected(root.path(), &source.path().join("Archive.zip"), "").unwrap(),
            1
        );
        let db = Connection::open(root.path().join("library.sqlite3")).unwrap();
        let path: String = db
            .query_row("SELECT rom_path FROM games", [], |r| r.get(0))
            .unwrap();
        assert!(!Path::new(&path).is_absolute());
        assert!(root.path().join(path).is_file());
    }
    #[test]
    fn unresolved_issues_are_kept_and_resolved_on_retry() {
        let root = root();
        let path = Path::new("/unavailable/Test.chd");
        save_issue(root.path(), path, Some("Choose the console"), "").unwrap();
        let mut importer = Importer::default();
        importer.show_issues(root.path());
        assert_eq!(importer.retry_paths, vec![path]);
        save_issue(root.path(), path, None, "segacd").unwrap();
        importer.show_issues(root.path());
        assert!(importer.retry_paths.is_empty());
    }
    #[test]
    fn folder_import_queues_picker_immediately_and_keeps_automatic_detection() {
        let mut importer = Importer {
            system: "psx".into(),
            ..Default::default()
        };
        importer.request(true);
        assert!(importer.auto_start);
        assert!(importer.folder);
        assert!(importer.system.is_empty());
    }
}

fn companion_cue(path: &Path) -> Option<PathBuf> {
    let target = path.canonicalize().ok()?;
    let parent = path.parent()?;
    for entry in fs::read_dir(parent).ok()?.take(4096).flatten() {
        let cue = entry.path();
        if ext(&cue) != "cue" || fs::metadata(&cue).ok()?.len() > 1024 * 1024 {
            continue;
        }
        let text = fs::read_to_string(&cue).ok()?;
        for line in text.lines() {
            let line = line.trim();
            if line
                .split_whitespace()
                .next()
                .is_some_and(|s| s.eq_ignore_ascii_case("FILE"))
            {
                let name = filename(line.split_once(char::is_whitespace)?.1.trim()).ok()?;
                if parent
                    .join(name.replace('\\', "/"))
                    .canonicalize()
                    .ok()
                    .as_ref()
                    == Some(&target)
                {
                    return Some(cue);
                }
            }
        }
    }
    None
}
#[cfg(test)]
mod folder_tests {
    use super::*;
    #[test]
    fn recursive_folder_import_skips_hidden_files_and_disc_tracks_and_deduplicates() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open(root.path().join("library.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE games(id TEXT PRIMARY KEY,title TEXT,system TEXT,rom_path TEXT,source TEXT,added REAL,original_filename TEXT);CREATE TABLE game_files(game_id TEXT,path TEXT,sha256 TEXT,size INTEGER,PRIMARY KEY(game_id,path));").unwrap();
        drop(db);
        let source = tempfile::tempdir().unwrap();
        let nested = source.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("One.nes"), b"nested ROM").unwrap();
        fs::write(source.path().join(".Hidden.nes"), b"must not import").unwrap();
        let mut gb = vec![0; 0x150];
        gb[0x143] = 0x80;
        fs::write(source.path().join("Color.gb"), gb).unwrap();
        let mut track = vec![0; 2352];
        track[16..30].copy_from_slice(b"SEGADISCSYSTEM");
        fs::write(source.path().join("track.bin"), track).unwrap();
        fs::write(
            source.path().join("Game.cue"),
            "file\t\"track.bin\" BINARY\n track\t01 MODE1/2352\n index 01 00:00:00\n",
        )
        .unwrap();
        assert!(Command::new("zip")
            .current_dir(&nested)
            .args(["-q", "Duplicate.zip", "One.nes"])
            .status()
            .unwrap()
            .success());
        let mut importer = Importer::default();
        importer.drop_paths(vec![source.path().to_owned()]);
        importer.start(&egui::Context::default(), root.path().to_owned());
        let result = loop {
            if let Event::Done(result) = importer
                .receiver
                .as_ref()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap()
            {
                break result;
            }
        };
        assert_eq!(result, Ok(3));
        let db = Connection::open(root.path().join("library.sqlite3")).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM games WHERE system='gbc'", [], |r| {
                r.get::<_, usize>(0)
            })
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM games WHERE system='segacd'",
                [],
                |r| r.get::<_, usize>(0)
            )
            .unwrap(),
            1
        );
    }
    #[test]
    fn playlist_cycles_are_rejected() {
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("Loop.m3u");
        fs::write(&path, "Loop.m3u\n").unwrap();
        assert!(content(&path).unwrap_err().contains("cycle"));
    }
}
