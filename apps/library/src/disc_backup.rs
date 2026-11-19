//! Physical-CD backup wizard. Read-only MMC helper; staged output is never imported on failure.
pub(crate) mod artwork;
mod progress;
use crate::{importing, preferences::Preferences};
use eframe::egui::{self, Color32};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    #[default]
    Insert,
    Identifying,
    Ready,
    Reading,
    Success,
    Attention,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leave {
    Close,
    Another,
}
const DISC_POLL_INTERVAL: Duration = Duration::from_secs(2);

struct DriveProbe {
    drive: String,
    result: mpsc::Receiver<Result<bool, String>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for DriveProbe {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct Backup {
    pub open: bool,
    pub(crate) busy: bool,
    drives: Vec<(String, String)>,
    drive: String,
    title: String,
    system: String,
    identified_system: String,
    token: String,
    serial: String,
    stage: Stage,
    cover_missing: bool,
    read_status: String,
    webp_quality: u8,
    committing: bool,
    close_after_cancel: bool,
    confirm_leave: Option<Leave>,
    success_since: Option<Instant>,
    artwork_rect: Option<egui::Rect>,
    estimate: progress::Estimate,
    scanning: bool,
    sectors: u64,
    tracks: u64,
    chd: bool,
    compatibility: bool,
    audio_check: Option<u8>,
    layout: Vec<Value>,
    accepted: bool,
    another: bool,
    next_probe: Option<Instant>,
    drive_probe: Option<DriveProbe>,
    message: String,
    progress: (u64, u64),
    receiver: Option<mpsc::Receiver<Event>>,
    cancel: Arc<AtomicBool>,
    prepared: Option<Prepared>,
    texture: Option<egui::TextureHandle>,
    cover_bytes: Option<Vec<u8>>,
}
struct Prepared {
    _folder: tempfile::TempDir,
    source: PathBuf,
}
enum Event {
    Disc(Value),
    Progress(u64, u64),
    Message(String),
    Cover(Vec<u8>),
    LookupDone,
    Waiting(String),
    Prepared(Prepared),
    Imported(String, bool),
    ImportFailed(Prepared, String),
    Error(String),
}
fn backup_menu_choice<T: PartialEq>(
    ui: &mut egui::Ui,
    value: &mut T,
    option: T,
    label: &str,
) -> bool {
    ui.spacing_mut().item_spacing.y = 0.;
    let selected = *value == option;
    if crate::actions::menu_row(ui, label, if selected { "✓" } else { "" }, true).clicked() {
        *value = option;
        !selected
    } else {
        false
    }
}

impl Backup {
    pub fn request(&mut self) {
        self.open = true;
        self.accepted = false;
        if !self.busy && self.prepared.is_none() {
            self.reset_form();
        }
    }
    fn reset_form(&mut self) {
        self.stage = Stage::Insert;
        self.token.clear();
        self.title.clear();
        self.system.clear();
        self.identified_system.clear();
        self.serial.clear();
        self.layout.clear();
        self.sectors = 0;
        self.tracks = 0;
        self.texture = None;
        self.cover_bytes = None;
        self.cover_missing = false;
        self.read_status.clear();
        self.message.clear();
        self.success_since = None;
        self.progress = (0, 0);
        self.estimate = Default::default();
        self.refresh_drives();
        self.drive_probe = None;
        self.next_probe = Some(Instant::now());
    }
    fn refresh_drives(&mut self) {
        self.drives = drives();
        if !self.drives.iter().any(|(p, _)| p == &self.drive) {
            self.drive = self
                .drives
                .first()
                .map_or(String::new(), |(p, _)| p.clone());
        }
        self.token.clear();
    }
    fn apply_leave(&mut self, action: Leave) {
        match action {
            Leave::Close => {
                self.drive_probe = None;
                self.open = false;
            }
            Leave::Another => self.reset_form(),
        }
    }
    fn request_leave(&mut self, action: Leave) {
        if self.busy {
            if !self.committing {
                self.close_after_cancel = true;
                self.cancel.store(true, Ordering::Relaxed);
                self.read_status = "Stopping safely…".into();
            }
            return;
        }
        if self.prepared.is_some() {
            self.confirm_leave = Some(action);
        } else {
            self.apply_leave(action);
        }
    }
    pub(crate) fn sidebar(&mut self, ui: &mut egui::Ui) {
        let can_cancel = !self.committing && !self.cancel.load(Ordering::Relaxed);
        let detail = if self.committing {
            "Finishing…"
        } else {
            &self.read_status
        };
        if crate::activity::sidebar(
            ui,
            if self.stage == Stage::Identifying {
                "Identifying disc"
            } else {
                "Disc backup"
            },
            detail,
            self.progress,
            (!self.committing).then_some(can_cancel),
        ) {
            self.request_leave(Leave::Close);
        }
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        root: Option<&Path>,
        prefs: &mut Preferences,
    ) -> bool {
        if let Some(q) = prefs
            .get("artwork.webp_quality")
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|q| [75, 85, 95, 100].contains(q))
        {
            self.webp_quality = q;
        } else if self.webp_quality == 0 {
            self.webp_quality = root
                .and_then(|root| {
                    rusqlite::Connection::open_with_flags(
                        root.join("library.sqlite3"),
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    )
                    .ok()
                })
                .and_then(|db| {
                    db.query_row(
                        "SELECT value FROM settings WHERE key='artwork.webp_quality'",
                        [],
                        |r| r.get::<_, String>(0),
                    )
                    .ok()
                })
                .and_then(|v| v.parse::<u8>().ok())
                .filter(|q| [75, 85, 95, 100].contains(q))
                .unwrap_or(85);
        }
        let mut changed = false;
        if prefs.get("disc_backup.agreement_version").is_none() {
            if let Some(root) = root {
                if let Ok(db) = rusqlite::Connection::open_with_flags(
                    root.join("library.sqlite3"),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                ) {
                    if db
                        .query_row(
                            "SELECT value FROM settings WHERE key='disc_backup.agreement_version'",
                            [],
                            |r| r.get::<_, String>(0),
                        )
                        .ok()
                        .as_deref()
                        == Some("1")
                    {
                        prefs.set("disc_backup.agreement_version", "1");
                    }
                }
            }
        }
        let events = self
            .receiver
            .as_ref()
            .map(|rx| rx.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            let report=match &event {
                Event::Prepared(_)=>Some(("prepared",None,None)),
                Event::Imported(id,duplicate)=>Some(("complete",Some(id.clone()),Some(*duplicate))),
                Event::ImportFailed(_,_)=>Some(("error",None,None)),
                Event::Error(_) if self.stage==Stage::Reading||self.committing=>Some((if self.cancel.load(Ordering::Relaxed){"cancelled"}else{"error"},None,None)),
                _=>None,
            }.map(|(status,id,duplicate)|{let message=match &event{Event::Error(e)|Event::ImportFailed(_,e)=>Some(e.as_str()),_=>None};serde_json::json!({"status":status,"game_id":id,"duplicate":duplicate,"message":message,"title":&self.title,"system":&self.system,"device":&self.drive,"serial":&self.serial,"paranoia_mode":self.audio_check.unwrap_or(2),"reader":if self.compatibility{"cdrdao"}else{"mmc"},"output_format":if self.chd{"chd"}else{"cue"}})});
            match event {
                Event::Disc(v) => {
                    self.title = v["title"].as_str().unwrap_or("").into();
                    self.system = v["system"].as_str().unwrap_or("").into();
                    self.identified_system = self.system.clone();
                    self.serial = v["serial"].as_str().unwrap_or("").into();
                    self.token = v["token"].as_str().unwrap_or("").into();
                    self.sectors = v["sectors"].as_u64().unwrap_or(0);
                    self.tracks = v["tracks"].as_u64().unwrap_or(0);
                    self.layout = v["layout"].as_array().cloned().unwrap_or_default();
                    self.stage = Stage::Ready;
                    self.message.clear();
                }
                Event::LookupDone => {
                    self.busy = false;
                    self.stage = Stage::Ready;
                    self.cover_missing = self.cover_bytes.is_none()
                        && !self.title.trim().is_empty()
                        && !self.system.is_empty();
                }
                Event::Progress(n, t) => {
                    if !self.scanning {
                        self.progress = (n, t);
                    }
                    if self.stage == Stage::Reading
                        && !self.scanning
                        && !self.compatibility
                        && t > 0
                    {
                        let lba = n.saturating_sub(1) / 2352;
                        if let Some(track) = self
                            .layout
                            .iter()
                            .rev()
                            .find(|v| v["lba"].as_u64().is_some_and(|start| start <= lba))
                        {
                            self.read_status = format!(
                                "Reading track {} of {} · {}",
                                track["number"].as_u64().unwrap_or(1),
                                self.tracks,
                                if track["data"].as_bool() == Some(true) {
                                    "Data"
                                } else {
                                    "Audio"
                                }
                            );
                        }
                    }
                }
                Event::Message(s) => {
                    let scanning = s.starts_with("Scanning track boundaries");
                    if scanning && !self.scanning {
                        self.estimate = Default::default();
                    }
                    self.scanning = scanning;
                    if scanning
                        || s.contains("Compressing")
                        || s.contains("Verifying")
                        || s.starts_with("Preparing CD audio")
                    {
                        self.progress = (0, 0);
                    }
                    self.read_status = s;
                }
                Event::Cover(bytes) => {
                    if let Ok(image) = image::load_from_memory(&bytes) {
                        if image.width() <= 4096 && image.height() <= 4096 {
                            let image = image.to_rgba8();
                            self.texture = Some(ctx.load_texture(
                                "disc-cover",
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [image.width() as usize, image.height() as usize],
                                    image.as_raw(),
                                ),
                                Default::default(),
                            ));
                            self.cover_bytes = Some(bytes);
                        }
                    }
                }
                Event::Prepared(p) => {
                    self.prepared = Some(p);
                    self.busy = false;
                    self.stage = Stage::Success;
                    self.success_since = Some(Instant::now());
                    self.read_status.clear();
                    self.message = "Choose Done to add the backup to your library.".into();
                    self.progress = (0, 0);
                }
                Event::Imported(_, _) => {
                    self.busy = false;
                    self.committing = false;
                    self.prepared = None;
                    changed = true;
                    self.apply_leave(if self.another {
                        Leave::Another
                    } else {
                        Leave::Close
                    });
                }
                Event::ImportFailed(p, s) => {
                    self.prepared = Some(p);
                    self.busy = false;
                    self.committing = false;
                    self.stage = Stage::Success;
                    self.message = s;
                }
                Event::Waiting(s) => {
                    self.busy = false;
                    self.stage = Stage::Insert;
                    self.message = s;
                    self.next_probe = Some(Instant::now() + DISC_POLL_INTERVAL);
                }
                Event::Error(s) => {
                    self.busy = false;
                    self.committing = false;
                    self.stage = Stage::Attention;
                    self.message = s;
                    self.token.clear();
                    self.read_status.clear();
                }
            }
            if let (Some(root), Some(report)) = (root, report) {
                let _ = crate::settings::tools::atomic_json(
                    &crate::storage::cache(root).join("disc-backups/last-backup.json"),
                    &report,
                );
            }
        }

        if self.close_after_cancel && !self.busy {
            self.close_after_cancel = false;
            self.prepared = None;
            self.open = false;
        }
        if !self.open {
            return changed;
        }
        if prefs.get("disc_backup.agreement_version") == Some("1")
            && self.stage == Stage::Insert
            && !self.busy
        {
            self.poll_drive(ctx, root);
        }
        let size = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(egui::vec2(1280., 900.));
        let size = egui::vec2(
            960_f32.min((size.x - 32.).max(400.)),
            800_f32.min((size.y - 64.).max(420.)),
        );
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("native-disc-backup"),
            backup_viewport(size),
            |child, class| {
                if child.input(|i| i.viewport().close_requested()) {
                    child.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.request_leave(Leave::Close);
                    ctx.request_repaint();
                }
                if class == egui::ViewportClass::Embedded {
                    let mut open = true;
                    egui::Window::new(emuluna_theme::fonts::window_title("Back up a game disc — EmuLuna"))
                        .open(&mut open)
                        .collapsible(false)
                        .resizable(false)
                        .fixed_size(size)
                        .show(child, |ui| self.contents(ui, ctx, root, prefs));
                    if !open {
                        close = true;
                    }
                } else {
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::NONE
                                .fill(child.style().visuals.window_fill)
                                .inner_margin(24.),
                        )
                        .show(child, |ui| self.contents(ui, ctx, root, prefs));
                }
                if let Some(action) = self.confirm_leave {
                    egui::Modal::new(egui::Id::new("keep-disc-backup")).show(child, |ui| {
                        ui.set_max_width(420.);
                        ui.heading("Keep this disc backup?");
                        ui.label("Add the backup to your library or discard it.");
                        ui.horizontal(|ui| {
                            if ui.button("Add to library").clicked() {
                                self.confirm_leave = None;
                                self.another = action == Leave::Another;
                                if let Some(root) = root {
                                    self.finish(ctx, root.to_owned());
                                }
                            }
                            if ui.button("Discard backup").clicked() {
                                self.confirm_leave = None;
                                self.prepared = None;
                                self.apply_leave(action);
                            }
                            if ui.button("Cancel").clicked() {
                                self.confirm_leave = None;
                            }
                        });
                    });
                }
            },
        );
        if close {
            self.request_leave(Leave::Close);
        }
        changed
    }
    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        root: Option<&Path>,
        prefs: &mut Preferences,
    ) {
        if prefs.get("disc_backup.agreement_version") != Some("1") {
            ui.heading("Before you back up a disc");
            ui.label("Use this feature only for backups you are legally entitled to make. Section 117 concerns owners of copies of computer programs and archival copies. It does not automatically authorize copying other works, including music and films.");
            ui.label("Archival copies must be destroyed if continued possession of the program ceases to be rightful. Transfers are subject to the conditions in Section 117(b).");
            ui.hyperlink_to(
                "Read Section 117 — U.S. Copyright Office",
                "https://www.copyright.gov/title17/92chap1.html#117",
            );
            ui.hyperlink_to(
                "Software backup FAQ",
                "https://www.copyright.gov/help/faq/faq-digital.html",
            );
            ui.checkbox(
                &mut self.accepted,
                "I own the discs I back up and agree to these conditions.",
            );
            ui.add_space(16.);
            if ui
                .add_enabled(self.accepted, egui::Button::new("I accept — Continue"))
                .clicked()
            {
                prefs.set("disc_backup.agreement_version", "1");
                ctx.request_repaint();
            }
            return;
        }
        if self.stage == Stage::Success {
            self.success_contents(ui, ctx, root);
            return;
        }
        let footer_height = if self.busy { 160. } else { 64. };
        let form_height = (ui.available_height() - footer_height).max(200.);
        egui::ScrollArea::vertical()
            .id_salt("disc-backup-form")
            .auto_shrink([false, false])
            .max_height(form_height)
            .show(ui, |ui| self.form_contents(ui, ctx, root, form_height));
        ui.add_space(12.);
        if self.busy {
            self.progress_footer(ui);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = !self.busy
                && self.stage == Stage::Ready
                && !self.token.is_empty()
                && !self.title.trim().is_empty()
                && ["segacd", "saturn", "psx", "pcecd"].contains(&self.system.as_str());
            if ui
                .add_enabled(
                    ready && root.is_some(),
                    egui::Button::new("Back up disc").min_size(egui::vec2(135., 32.)),
                )
                .clicked()
            {
                if let Some(root) = root {
                    self.backup(ctx, root.to_owned());
                }
            }
            if ui
                .add_enabled(
                    !self.committing && !self.close_after_cancel,
                    egui::Button::new(if self.busy { "Cancel" } else { "Close" })
                        .min_size(egui::vec2(88., 32.)),
                )
                .clicked()
            {
                self.request_leave(Leave::Close);
            }
        });
        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }
    fn channel(&mut self) -> mpsc::Sender<Event> {
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.busy = true;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.progress = (0, 0);
        self.estimate = Default::default();
        self.scanning = false;
        tx
    }
    fn poll_drive(&mut self, ctx: &egui::Context, root: Option<&Path>) {
        if let Some(probe) = &self.drive_probe {
            match probe.result.try_recv() {
                Ok(result) => {
                    let matches_drive = probe.drive == self.drive;
                    self.drive_probe = None;
                    if matches_drive {
                        match result {
                            Ok(true) => {
                                self.identify(ctx, root.map(Path::to_owned));
                                return;
                            }
                            Ok(false) => {}
                            Err(error) => {
                                let failed_drive = self.drive.clone();
                                self.refresh_drives();
                                if self.drive == failed_drive {
                                    self.stage = Stage::Attention;
                                    self.message = error;
                                    return;
                                }
                            }
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => self.drive_probe = None,
            }
        }
        let now = Instant::now();
        if self.next_probe.is_none_or(|next| now >= next) {
            self.refresh_drives();
            self.next_probe = Some(now + DISC_POLL_INTERVAL);
            if !self.drive.is_empty() {
                let drive = self.drive.clone();
                let worker_drive = drive.clone();
                let (sender, result) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.drive_probe = Some(DriveProbe {
                    drive,
                    result,
                    cancel: cancel.clone(),
                });
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = run_capture(
                        Command::new(helper()).args(["status", &worker_drive]),
                        &cancel,
                    )
                    .and_then(|output| {
                        match String::from_utf8_lossy(&output).trim() {
                            "ready" => Ok(true),
                            "waiting" => Ok(false),
                            _ => Err("Unexpected response from the optical drive helper.".into()),
                        }
                    });
                    let _ = sender.send(result);
                    ctx.request_repaint();
                });
            }
        }
        if let Some(next) = self.next_probe {
            ctx.request_repaint_after(next.saturating_duration_since(Instant::now()));
        }
    }

    fn identify(&mut self, ctx: &egui::Context, root: Option<PathBuf>) {
        self.token.clear();
        self.texture = None;
        self.cover_bytes = None;
        self.stage = Stage::Identifying;
        self.message.clear();
        self.cover_missing = false;
        self.read_status.clear();
        let tx = self.channel();
        let ctx = ctx.clone();
        let drive = self.drive.clone();
        let cancel = self.cancel.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let output =
                    run_capture(Command::new(helper()).args(["inspect", &drive]), &cancel)?;
                let mut disc: Value = serde_json::from_slice(&output).map_err(err)?;
                let cover_url = root
                    .as_deref()
                    .and_then(|root| lookup(root, &mut disc))
                    .unwrap_or_default();
                let title = disc["title"].as_str().unwrap_or("").to_owned();
                let system = disc["system"].as_str().unwrap_or("").to_owned();
                tx.send(Event::Disc(disc)).map_err(err)?;
                ctx.request_repaint();
                if let Some(bytes) =
                    cover_with_cancel(root.as_deref(), &system, &title, &cover_url, artwork::preferred_region(), &cancel)
                {
                    let _ = tx.send(Event::Cover(bytes));
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err("The operation was cancelled.".into());
                }
                let _ = tx.send(Event::LookupDone);
                Ok::<_, String>(())
            })();
            if let Err(e) = result {
                let waiting = e.contains("Insert a game CD") || drives().is_empty();
                let _ = tx.send(if waiting {
                    Event::Waiting(e)
                } else {
                    Event::Error(e)
                });
            }
            ctx.request_repaint();
        });
    }
    fn backup(&mut self, ctx: &egui::Context, root: PathBuf) {
        for name in if self.compatibility {
            vec!["cdrdao", "toc2cue"]
        } else {
            vec![]
        }
        .into_iter()
        .chain(if self.chd { Some("chdman") } else { None })
        {
            if !tool_available(&tool(name)) {
                self.message =
                    format!("{name} is not installed. Install it before backing up this disc.");
                return;
            }
        }
        let tx = self.channel();
        self.stage = Stage::Reading;
        self.progress = (0, self.sectors * 2352);
        self.message = "Keep the disc in the drive.".into();
        self.read_status = "Reading the track layout…".into();
        let ctx = ctx.clone();
        let drive = self.drive.clone();
        let token = self.token.clone();
        let chd = self.chd;
        let compatibility = self.compatibility;
        let audio_check = self.audio_check.unwrap_or(2);
        let layout = self.layout.clone();
        let system = self.system.clone();
        let sectors = self.sectors;
        let cancel = self.cancel.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let cache = crate::storage::cache(&root).join("disc-backups");
                fs::create_dir_all(&cache).map_err(err)?;
                let folder = tempfile::Builder::new()
                    .prefix(".native-backup-")
                    .tempdir_in(&cache)
                    .map_err(err)?;
                let expected = sectors * 2352;
                for (location, needed) in [
                    (
                        cache.clone(),
                        expected * if chd { 3 } else { 2 } + 64 * 1024 * 1024,
                    ),
                    (
                        root.join("consoles").join(&system).join("roms"),
                        expected + 64 * 1024 * 1024,
                    ),
                ] {
                    if available_space(&location).is_some_and(|free| free < needed) {
                        return Err("Not enough free space for the disc image and its verified library copy.".into());
                    }
                }
                if compatibility {
                    compatibility_read(
                        &drive,
                        &token,
                        folder.path(),
                        &cache.join("last-reader.log"),
                        audio_check,
                        sectors,
                        &layout,
                        &cancel,
                        &tx,
                        &ctx,
                    )?;
                } else {
                    let log_path = cache.join("last-reader.log");
                    fs::write(&log_path, b"").map_err(err)?;
                    let log = fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                        .map_err(err)?;
                    let mut child = Command::new(helper())
                        .arg("backup")
                        .arg(&drive)
                        .arg(folder.path())
                        .arg(&token)
                        .stdout(Stdio::piped())
                        .stderr(log)
                        .spawn()
                        .map(ManagedChild)
                        .map_err(err)?;
                    let stdout = child.stdout.take().ok_or("No reader output")?;
                    let sender = tx.clone();
                    let repaint = ctx.clone();
                    let reader = std::thread::spawn(move || {
                        use std::io::Write;
                        let mut log = fs::OpenOptions::new().append(true).open(log_path).ok();
                        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                            if let Some(log) = &mut log {
                                let _ = writeln!(log, "{line}");
                            }
                            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                                if v["kind"] == "progress" {
                                    let _ = sender.send(Event::Progress(
                                        v["done"].as_u64().unwrap_or(0),
                                        v["total"].as_u64().unwrap_or(0),
                                    ));
                                    repaint.request_repaint();
                                }
                            }
                        }
                    });
                    let status = loop {
                        if cancel.load(Ordering::Relaxed) {
                            let _ = child.kill();
                            let _ = child.wait();
                            let _ = reader.join();
                            return Err("Backup cancelled. No game was added.".into());
                        }
                        if let Some(status) = child.try_wait().map_err(err)? {
                            break status;
                        }
                        std::thread::sleep(Duration::from_millis(75));
                    };
                    let _ = reader.join();
                    if !status.success() {
                        return Err(fs::read_to_string(cache.join("last-reader.log"))
                            .unwrap_or_else(|_| "Disc reader failed; no game was added.".into()));
                    }
                }
                let mut source = folder.path().join("disc.cue");
                let bin = folder.path().join("disc.bin");
                if !source.is_file() || !bin.is_file() {
                    return Err("Incomplete disc backup. No game was added.".into());
                }
                if chd {
                    let _ = tx.send(Event::Message("Compressing to CHD…".into()));
                    ctx.request_repaint();
                    let tool = tool("chdman");
                    let out = folder.path().join("disc.chd");
                    run_capture_logged(
                        Command::new(&tool)
                            .arg("createcd")
                            .arg("-i")
                            .arg(&source)
                            .arg("-o")
                            .arg(&out)
                            .args(["-np", "2"]),
                        &cancel,
                        &cache.join("last-compression.log"),
                    )?;
                    let _ = tx.send(Event::Message("Verifying CHD…".into()));
                    ctx.request_repaint();
                    run_capture_logged(
                        Command::new(tool).arg("verify").arg("-i").arg(&out),
                        &cancel,
                        &cache.join("last-verification.log"),
                    )?;
                    source = out;
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err("Backup cancelled. No game was added.".into());
                }
                Ok(Prepared {
                    _folder: folder,
                    source,
                })
            })();
            let _ = tx.send(match result {
                Ok(p) => Event::Prepared(p),
                Err(e) => Event::Error(e),
            });
            ctx.request_repaint();
        });
    }
    fn finish(&mut self, ctx: &egui::Context, root: PathBuf) {
        if let Some(p) = self.prepared.take() {
            let tx = self.channel();
            self.committing = true;
            self.message = "Adding backup to the library…".into();
            let ctx = ctx.clone();
            let system = self.system.clone();
            let title = self.title.clone();
            let cover = self.cover_bytes.clone();
            let quality = if self.webp_quality == 0 {
                85
            } else {
                self.webp_quality
            };
            let provenance = if self.serial.is_empty() {
                self.drive.clone()
            } else {
                self.serial.clone()
            };
            std::thread::spawn(move || {
                let result = commit_backup(
                    &root,
                    &p.source,
                    &system,
                    &title,
                    cover.as_deref(),
                    &provenance,
                    quality,
                );
                let event = match result {
                    Ok(result) => Event::Imported(result.id, result.duplicate),
                    Err(e) => Event::ImportFailed(
                        p,
                        format!("Backup is safe in staging, but import failed: {e}. Retry Done."),
                    ),
                };
                let _ = tx.send(event);
                ctx.request_repaint();
            });
        }
    }
}
impl Backup {
    fn form_contents(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        root: Option<&Path>,
        form_height: f32,
    ) {
        let step = match self.stage {
            Stage::Insert | Stage::Attention => 0,
            Stage::Identifying | Stage::Ready => 1,
            _ => 2,
        };
        ui.horizontal(|ui| {
            for (i, name) in ["1  Insert disc", "2  Identify game", "3  Back up"]
                .iter()
                .enumerate()
            {
                if i > 0 {
                    ui.label("›");
                }
                ui.label(if i == step {
                    egui::RichText::new(*name).strong()
                } else {
                    egui::RichText::new(*name)
                });
            }
        });
        ui.add_space(12.);
        let mut refresh = false;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!self.busy, |ui| {
                egui::ComboBox::from_id_salt("backup-drive")
                    .width((ui.available_width() - 110.).max(200.))
                    .selected_text(
                        self.drives
                            .iter()
                            .find(|(p, _)| p == &self.drive)
                            .map_or("No optical drive connected", |(_, n)| n.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        for (p, n) in &self.drives {
                            if backup_menu_choice(ui, &mut self.drive, p.clone(), n) {
                                refresh = true;
                            }
                        }
                    });
                if ui.button("Refresh").clicked() {
                    refresh = true;
                }
            });
        });
        if refresh {
            self.reset_form();
            ctx.request_repaint();
        }
        ui.add_space(18.);
        ui.columns(2, |columns| {
            let art_height = (form_height - 100.).clamp(280., 460.);
            let rect = columns[0]
                .allocate_exact_size(
                    egui::vec2(columns[0].available_width(), art_height),
                    egui::Sense::hover(),
                )
                .0;
            self.draw_artwork(&columns[0], rect, self.busy);
            self.artwork_rect = Some(rect);
            let ui = &mut columns[1];
            let heading = match self.stage {
                Stage::Insert if self.drives.is_empty() => "Connect an optical drive",
                Stage::Insert => "Insert a game CD",
                Stage::Identifying => "Identifying disc…",
                Stage::Ready => "Disc details",
                Stage::Reading => "Backing up disc…",
                Stage::Attention => "Disc backup error",
                Stage::Success => "Success",
            };
            ui.label(egui::RichText::new(heading).strong().size(22.));
            if !self.message.is_empty() {
                ui.add_space(10.);
                ui.label(&self.message);
            }
            if matches!(self.stage, Stage::Ready | Stage::Reading) {
                ui.add_space(12.);
                let labels = [
                    "Game",
                    "Console",
                    "Disc reader",
                    "Save as",
                    "Audio checking",
                ];
                let label_font = egui::TextStyle::Body.resolve(ui.style());
                let label_width = labels
                    .iter()
                    .map(|label| {
                        ui.painter()
                            .layout_no_wrap(
                                (*label).into(),
                                label_font.clone(),
                                ui.visuals().text_color(),
                            )
                            .size()
                            .x
                    })
                    .fold(0., f32::max)
                    .ceil();
                let field_gap = 12.;
                let field_end_padding = 6.;
                let field_width =
                    (ui.available_width() - label_width - field_gap - field_end_padding)
                        .floor()
                        .max(40.);
                let text_margin = egui::Margin::symmetric(4, 2);
                ui.add_enabled_ui(!self.busy, |ui| {
                    egui::Grid::new("disc-review-form")
                        .spacing([field_gap, 12.])
                        .show(ui, |ui| {
                            ui.label("Game");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.title)
                                    .margin(text_margin)
                                    .desired_width((field_width - text_margin.sum().x).max(1.))
                                    .char_limit(160),
                            );
                            ui.end_row();
                            ui.label("Console");
                            ui.add_enabled_ui(self.identified_system.is_empty(), |ui| {
                                egui::ComboBox::from_id_salt("backup-system")
                                    .width(field_width)
                                    .truncate()
                                    .selected_text(console_name(&self.system))
                                    .show_ui(ui, |ui| {
                                        for id in ["segacd", "saturn", "psx", "pcecd"] {
                                            backup_menu_choice(
                                                ui,
                                                &mut self.system,
                                                id.into(),
                                                console_name(id),
                                            );
                                        }
                                    });
                            });
                            ui.end_row();
                            ui.label("Disc reader");
                            let reader = egui::ComboBox::from_id_salt("disc-reader")
                                .width(field_width)
                                .truncate()
                                .selected_text(if self.compatibility {
                                    "cdrdao compatibility"
                                } else {
                                    "Direct MMC (default)"
                                })
                                .show_ui(ui, |ui| {
                                    backup_menu_choice(
                                        ui,
                                        &mut self.compatibility,
                                        false,
                                        "Direct MMC (default)",
                                    );
                                    backup_menu_choice(
                                        ui,
                                        &mut self.compatibility,
                                        true,
                                        "cdrdao compatibility",
                                    );
                                });
                            if !self.compatibility {
                                reader.response.on_hover_text(correction_note(false, 0));
                            }
                            ui.end_row();
                            ui.label("Save as");
                            egui::ComboBox::from_id_salt("backup-format")
                                .width(field_width)
                                .truncate()
                                .selected_text(if self.chd {
                                    "CHD (lossless compression)"
                                } else {
                                    "BIN / CUE"
                                })
                                .show_ui(ui, |ui| {
                                    backup_menu_choice(ui, &mut self.chd, false, "BIN / CUE");
                                    backup_menu_choice(
                                        ui,
                                        &mut self.chd,
                                        true,
                                        "CHD (lossless compression)",
                                    );
                                });
                            ui.end_row();
                            ui.label("Audio checking");
                            let mut level = self.audio_check.unwrap_or(2);
                            ui.add_enabled_ui(self.compatibility, |ui| {
                                egui::ComboBox::from_id_salt("disc-audio-check")
                                    .width(field_width)
                                    .truncate()
                                    .selected_text(correction_label(level))
                                    .show_ui(ui, |ui| {
                                        for v in 0..=3 {
                                            backup_menu_choice(
                                                ui,
                                                &mut level,
                                                v,
                                                correction_label(v),
                                            );
                                        }
                                    })
                                    .response
                                    .on_hover_text(correction_note(true, level));
                            });
                            self.audio_check = Some(level);
                            ui.end_row();
                        });
                });
                ui.add_space(12.);
                let audio = self
                    .layout
                    .iter()
                    .filter(|v| v["data"].as_bool() == Some(false))
                    .count();
                ui.label(format!(
                    "{} tracks · {audio} audio · {:.0} MiB",
                    self.tracks,
                    self.sectors as f64 * 2352. / 1048576.
                ));
                if !self.serial.is_empty() {
                    ui.label(format!("Disc ID: {}", self.serial));
                }
                ui.add_space(12.);
                if !self.read_status.is_empty() {
                    ui.label(&self.read_status);
                }
                if ui
                    .add_enabled(
                        !self.busy && !self.title.trim().is_empty() && !self.system.is_empty(),
                        egui::Button::new("Find cover"),
                    )
                    .clicked()
                {
                    self.find_cover(ctx, root.map(Path::to_owned));
                }
            }
            if self.stage == Stage::Attention {
                if let Some(root) = root {
                    if ui.link("Open backup logs").clicked() {
                        let _ = crate::actions::open_path(
                            crate::storage::cache(root).join("disc-backups"),
                        );
                    }
                }
            }
        });
    }
    fn success_contents(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, root: Option<&Path>) {
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("Success")
                    .strong()
                    .size(36.)
                    .color(Color32::from_rgb(70, 205, 125)),
            );
            ui.add_space(12.);
            let height = (ui.available_height() - 170.).clamp(120., 560.);
            let (slot, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().min(600.), height),
                egui::Sense::hover(),
            );
            let elapsed = self
                .success_since
                .map_or(1., |t| (t.elapsed().as_secs_f32() / 0.55).min(1.));
            let t = 1. - (1. - elapsed).powi(3);
            let rect = if let Some(start) = self.artwork_rect {
                egui::Rect::from_min_max(start.min.lerp(slot.min, t), start.max.lerp(slot.max, t))
            } else {
                slot
            };
            self.draw_artwork(ui, rect, false);
            if elapsed < 1. {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
            ui.add_space(10.);
            ui.label(egui::RichText::new(&self.title).strong().size(20.));
            ui.label(&self.message);
            ui.add_space(12.);
            if self.busy {
                ui.spinner();
            }
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - 265.) * 0.5).max(0.));
                if ui
                    .add_enabled(
                        !self.busy && root.is_some(),
                        egui::Button::new("Done").min_size(egui::vec2(105., 34.)),
                    )
                    .clicked()
                {
                    self.another = false;
                    if let Some(root) = root {
                        self.finish(ctx, root.to_owned());
                    }
                }
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new("Back up another").min_size(egui::vec2(145., 34.)),
                    )
                    .clicked()
                {
                    self.request_leave(Leave::Another);
                }
            });
        });
    }
    fn draw_artwork(&self, ui: &egui::Ui, rect: egui::Rect, reading: bool) {
        let p = ui.painter();
        if let Some(texture) = &self.texture {
            let available = rect.shrink(12.);
            let size = texture.size_vec2();
            let scale = (available.width() / size.x).min(available.height() / size.y);
            p.image(
                texture.id(),
                egui::Rect::from_center_size(available.center(), size * scale),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
            return;
        }
        let center = rect.center();
        let radius = ((rect.width() - 30.).min(rect.height() - 40.).min(240.) * 0.5).max(25.);
        if self.cover_missing && matches!(self.stage, Stage::Ready | Stage::Reading) {
            p.text(
                center - egui::vec2(0., radius + 4.),
                egui::Align2::CENTER_BOTTOM,
                "Cover not found",
                egui::FontId::proportional(13.),
                ui.visuals().weak_text_color(),
            );
        }
        for i in (1..=40).rev() {
            let t = i as f32 / 40.;
            let color = Color32::from_rgb(
                (245. - t * 103.) as u8,
                (247. - t * 91.) as u8,
                (251. - t * 78.) as u8,
            );
            p.circle_filled(center, radius * t, color);
        }
        p.circle_stroke(
            center,
            radius,
            egui::Stroke::new(1_f32, Color32::from_rgb(137, 149, 166)),
        );
        for fraction in [0.72, 0.76, 0.8, 0.84, 0.88, 0.92] {
            p.circle_stroke(
                center,
                radius * fraction,
                egui::Stroke::new(0.6_f32, Color32::from_white_alpha(35)),
            );
        }
        p.circle_filled(center, radius * 0.26, Color32::from_rgb(213, 219, 227));
        p.circle_stroke(
            center,
            radius * 0.26,
            egui::Stroke::new(1_f32, Color32::from_rgb(167, 179, 193)),
        );
        p.circle_filled(center, radius * 0.11, ui.visuals().window_fill);
        p.circle_stroke(
            center,
            radius * 0.11,
            egui::Stroke::new(1_f32, Color32::from_rgb(142, 153, 170)),
        );
        if reading {
            let angle = ui.input(|i| i.time as f32) * 1.745;
            let points = (0..=24)
                .map(|n| {
                    let a = angle + n as f32 / 24. * 1.134;
                    center + egui::vec2(a.cos(), a.sin()) * (radius + 6.)
                })
                .collect::<Vec<_>>();
            p.add(egui::Shape::line(
                points,
                egui::Stroke::new(3_f32, ui.visuals().selection.bg_fill),
            ));
        }
    }
    fn progress_footer(&mut self, ui: &mut egui::Ui) {
        let (done, total) = self.progress;
        let now = ui.input(|i| i.time);
        let state = self.estimate.update(done, total, now);
        ui.horizontal(|ui| {
            if total > 0 {
                ui.label(format!(
                    "{:.1} / {:.0} MiB · {:.0}%",
                    done as f64 / 1048576.,
                    total as f64 / 1048576.,
                    done as f64 / total as f64 * 100.
                ));
                if state.rate > 0. {
                    ui.label(format!("· {:.2} MiB/s", state.rate / 1048576.));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(if self.scanning {
                    "Audio copying follows".into()
                } else if total == 0 {
                    if self.committing { "Finishing…" } else { "" }.into()
                } else if state.waiting {
                    "ETA paused".into()
                } else if let Some(eta) = state.eta {
                    format!("About {} left", progress::duration(eta))
                } else {
                    "Calculating ETA…".into()
                });
            });
        });
        if state.waiting {
            ui.small(format!(
                "Waiting for drive · no new data for {}",
                progress::duration(state.idle)
            ));
        }
        if self.scanning {
            ui.small("Checking track boundaries and pregaps. This scan can take several minutes without writing new bytes.");
        }
        crate::activity::progress(ui, ui.available_width(), 18., done, total, !state.waiting);
    }
    fn find_cover(&mut self, ctx: &egui::Context, root: Option<PathBuf>) {
        self.texture = None;
        self.cover_bytes = None;
        self.cover_missing = false;
        let tx = self.channel();
        let title = self.title.trim().to_owned();
        let system = self.system.clone();
        let cancel = self.cancel.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(bytes) = cover_with_cancel(root.as_deref(), &system, &title, "", artwork::preferred_region(), &cancel) {
                let _ = tx.send(Event::Cover(bytes));
            }
            let _ = tx.send(if cancel.load(Ordering::Relaxed) {
                Event::Error("The operation was cancelled.".into())
            } else {
                Event::LookupDone
            });
            ctx.request_repaint();
        });
    }
}
fn console_name(id: &str) -> &str {
    match id {
        "segacd" => "Sega Mega-CD",
        "saturn" => "Sega Saturn",
        "psx" => "Sony PlayStation",
        "pcecd" => "PC Engine / TurboGrafx-CD",
        _ => "Choose console…",
    }
}
fn correction_label(level: u8) -> &'static str {
    match level {
        0 => "0 — Direct (no checking)",
        1 => "1 — Overlap reads",
        3 => "3 — Full scratch repair",
        _ => "2 — Verify audio (recommended)",
    }
}
fn correction_note(compatibility: bool, level: u8) -> &'static str {
    if !compatibility {
        return "Reads raw sectors directly. Failed batches retry individual sectors; unreadable sectors stop the backup. Detailed sub-index scanning is not performed.";
    }
    match level{0=>"Fastest: copies audio without checking; clicks or other audio errors may remain.",1=>"Overlapping audio reads help prevent jitter; fewer checks than verification mode.",3=>"Most thorough: adds scratch detection and repair; difficult discs can take much longer.",_=>"Overlapping reads plus audio verification, without additional scratch repair."}
}

fn tool_available(path: &Path) -> bool {
    if path.components().count() > 1 {
        return path.is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(path).is_file()))
}

#[derive(Default)]
struct ReaderStatus {
    track: u32,
    audio: bool,
    analyzing: bool,
}
impl ReaderStatus {
    fn update(&mut self, text: &str, total: usize) -> String {
        for line in text.lines() {
            let lower = line.to_ascii_lowercase();
            let marker = if let Some((_, rest)) = lower.split_once("copying data track ") {
                self.audio = false;
                Some(rest)
            } else if let Some((_, rest)) = lower.split_once("copying audio tracks ") {
                self.audio = true;
                Some(rest)
            } else {
                lower.trim().strip_prefix("track ")
            };
            if let Some(rest) = marker {
                if let Some(n) = rest
                    .split(|c: char| !c.is_ascii_digit())
                    .find(|s| !s.is_empty())
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    self.track = n;
                }
            }
        }
        let analyzed = text.rfind("Analyzing...");
        let reading = text.rfind("Reading...");
        if analyzed.is_some() || reading.is_some() {
            self.analyzing = analyzed > reading;
        }
        if self.track == 0 {
            return "Preparing the drive and reading the track layout…".into();
        }
        if self.analyzing {
            format!(
                "Scanning track boundaries · Track {} of {total}",
                self.track
            )
        } else {
            format!(
                "Reading track {} of {total} · {}",
                self.track,
                if self.audio { "Audio" } else { "Data" }
            )
        }
    }
}

struct Committed {
    id: String,
    duplicate: bool,
}
fn commit_backup(
    root: &Path,
    source: &Path,
    system: &str,
    title: &str,
    cover: Option<&[u8]>,
    provenance: &str,
    quality: u8,
) -> Result<Committed, String> {
    let imported = importing::import_disc(root, source, system)?;
    let mut db = rusqlite::Connection::open(root.join("library.sqlite3")).map_err(err)?;
    db.busy_timeout(Duration::from_secs(5)).map_err(err)?;
    let original_source: String = db
        .query_row("SELECT source FROM games WHERE id=?", [&imported.id], |r| {
            r.get(0)
        })
        .map_err(err)?;
    name_managed_backup(root, &mut db, &imported.id, title)?;
    if !imported.duplicate || original_source == source.to_string_lossy() {
        db.execute(
            "UPDATE games SET title=?,source=? WHERE id=?",
            rusqlite::params![
                title.trim(),
                format!("Optical disc: {provenance}"),
                imported.id
            ],
        )
        .map_err(err)?;
        if db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='metadata_overrides')",[],|r|r.get::<_,bool>(0)).unwrap_or(false){db.execute("INSERT OR IGNORE INTO metadata_overrides(game_id,field) VALUES(?,'title')",[&imported.id]).map_err(err)?;}
        if let Some(bytes) = cover {
            let covers = root.join("covers");
            if fs::create_dir_all(&covers).is_ok() {
                if let Ok(filename) = crate::settings::conversion::encode(
                    &covers,
                    bytes,
                    quality,
                    &AtomicBool::new(false),
                ) {
                    let _ = db.execute(
                        "UPDATE games SET cover=? WHERE id=?",
                        rusqlite::params![format!("covers/{filename}"), imported.id],
                    );
                }
            }
        }
    }
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .map_err(err)?;
    importing::export_inventory(root, &db, system)?;
    Ok(Committed {
        id: imported.id,
        duplicate: imported.duplicate,
    })
}
fn name_managed_backup(
    root: &Path,
    db: &mut rusqlite::Connection,
    id: &str,
    title: &str,
) -> Result<(), String> {
    use std::io::Write;
    let (stored, original): (String, Option<String>) = db
        .query_row(
            "SELECT rom_path,original_filename FROM games WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(err)?;
    if Path::new(&stored).is_absolute() {
        return Err("Disc backup must be inside the managed console library".into());
    }
    let source = root.join(&stored);
    let base = source.parent().ok_or("Invalid disc path")?;
    let name = safe_name(title);
    if source.file_stem().is_some_and(|s| s == name.as_str()) {
        return Ok(());
    }
    let extension = source
        .extension()
        .and_then(|s| s.to_str())
        .ok_or("Invalid disc format")?;
    let mut destination = base.join(format!("{name}.{extension}"));
    for suffix in [&id[..6.min(id.len())], &id[..12.min(id.len())], id] {
        if !destination.exists() {
            break;
        }
        destination = base.join(format!("{name} [{suffix}].{extension}"));
    }
    if destination.exists() {
        return Err("The named backup already exists. Choose another game name.".into());
    }
    let mut created = Vec::new();
    let mut old = vec![source.clone()];
    let result = (|| {
        let mut rows = Vec::new();
        if extension.eq_ignore_ascii_case("cue") {
            let (binary_path,sha,size):(String,String,u64)=db.query_row("SELECT path,sha256,size FROM game_files WHERE game_id=? AND lower(path) LIKE '%.bin' LIMIT 1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
            let relative = Path::new(&binary_path);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err("Invalid managed audio track path".into());
            }
            let binary = base.join(relative);
            let renamed_binary = destination.with_extension("bin");
            fs::hard_link(&binary, &renamed_binary).map_err(err)?;
            created.push(renamed_binary.clone());
            old.push(binary);
            let text = fs::read_to_string(&source).map_err(err)?;
            let mut replaced = 0;
            let mut lines = Vec::new();
            for line in text.lines() {
                if line
                    .split_whitespace()
                    .next()
                    .is_some_and(|s| s.eq_ignore_ascii_case("FILE"))
                {
                    replaced += 1;
                    lines.push(format!(
                        "FILE \"{}\" BINARY",
                        renamed_binary.file_name().unwrap().to_string_lossy()
                    ));
                } else {
                    lines.push(line.into());
                }
            }
            if replaced != 1 {
                return Err("The backup CUE could not be renamed safely".into());
            }
            let text = lines.join("\n") + "\n";
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(err)?;
            created.push(destination.clone());
            output.write_all(text.as_bytes()).map_err(err)?;
            output.sync_all().map_err(err)?;
            rows.push((
                destination
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                importing::hash(&destination)?,
                text.len() as u64,
            ));
            rows.push((
                renamed_binary
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                sha,
                size,
            ));
        } else {
            fs::hard_link(&source, &destination).map_err(err)?;
            created.push(destination.clone());
            let (sha, size): (String, u64) = db
                .query_row(
                    "SELECT sha256,size FROM game_files WHERE game_id=? LIMIT 1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(err)?;
            rows.push((
                destination
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                sha,
                size,
            ));
        }
        let previous = Path::new(original.as_deref().unwrap_or(&stored))
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let has_previous = {
            let mut statement = db.prepare("PRAGMA table_info(games)").map_err(err)?;
            let fields = statement
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err)?;
            fields.iter().any(|s| s == "previous_rom_stem")
        };
        let tx = db.transaction().map_err(err)?;
        tx.execute(
            "UPDATE games SET rom_path=?,original_filename=? WHERE id=?",
            rusqlite::params![
                destination
                    .strip_prefix(root)
                    .map_err(err)?
                    .to_string_lossy(),
                destination.file_name().unwrap().to_string_lossy(),
                id
            ],
        )
        .map_err(err)?;
        if has_previous {
            tx.execute(
                "UPDATE games SET previous_rom_stem=? WHERE id=?",
                rusqlite::params![previous, id],
            )
            .map_err(err)?;
        }
        tx.execute("DELETE FROM game_files WHERE game_id=?", [id])
            .map_err(err)?;
        for (path, sha, size) in rows {
            tx.execute(
                "INSERT INTO game_files(game_id,path,sha256,size) VALUES(?,?,?,?)",
                rusqlite::params![id, path, sha, size],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        for p in created {
            let _ = fs::remove_file(p);
        }
        return Err(error);
    }
    for p in old {
        fs::remove_file(p).map_err(err)?;
    }
    Ok(())
}

fn helper() -> PathBuf {
    crate::resources::path("build/cores/emuluna-disc-backup")
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn tool(name: &str) -> PathBuf {
    let local = crate::resources::path("build/tools/disc-reader/bin")
        .join(name);
    if local.is_file() {
        local
    } else {
        PathBuf::from(name)
    }
}
pub(crate) fn run_capture(cmd: &mut Command, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    run_capture_inner(cmd, cancel, None)
}
fn run_capture_logged(
    cmd: &mut Command,
    cancel: &AtomicBool,
    path: &Path,
) -> Result<Vec<u8>, String> {
    run_capture_inner(cmd, cancel, Some(path))
}
fn run_capture_inner(
    cmd: &mut Command,
    cancel: &AtomicBool,
    diagnostic: Option<&Path>,
) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    helper_environment(cmd)?;
    let mut out = tempfile::tempfile().map_err(err)?;
    let mut errors = tempfile::tempfile().map_err(err)?;
    let command = format!("{cmd:?}");
    let started = Instant::now();
    let result = (|| {
        let mut child = cmd
            .stdout(out.try_clone().map_err(err)?)
            .stderr(errors.try_clone().map_err(err)?)
            .spawn()
            .map(ManagedChild)
            .map_err(err)?;
        let status = loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Operation cancelled. No game was added.".into());
            }
            if let Some(status) = child.try_wait().map_err(err)? {
                break status;
            }
            std::thread::sleep(Duration::from_millis(75));
        };
        let file = if status.success() {
            &mut out
        } else {
            &mut errors
        };
        file.seek(SeekFrom::Start(0)).map_err(err)?;
        let mut bytes = Vec::new();
        file.take(1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if status.success() {
            Ok(bytes)
        } else {
            if bytes.is_empty() {
                out.seek(SeekFrom::Start(0)).map_err(err)?;
                (&mut out)
                    .take(1024 * 1024)
                    .read_to_end(&mut bytes)
                    .map_err(err)?;
            }
            let text = String::from_utf8_lossy(&bytes).into_owned();
            Err(if text.is_empty() {
                format!("Disc tool exited with {status}")
            } else {
                text
            })
        }
    })();
    if let Some(path) = diagnostic {
        let save = (|| {
            let mut log = tempfile::NamedTempFile::new_in(path.parent().ok_or("Invalid log path")?)
                .map_err(err)?;
            let metadata = serde_json::json!({"command":command,"outcome":result.as_ref().err().map_or("completed",String::as_str),"elapsed_seconds":started.elapsed().as_secs_f64()});
            writeln!(log, "{metadata}").map_err(err)?;
            let mut remaining = 32 * 1024 * 1024;
            for file in [&mut out, &mut errors] {
                file.seek(SeekFrom::Start(0)).map_err(err)?;
                let copied = std::io::copy(&mut file.take(remaining), &mut log).map_err(err)?;
                remaining -= copied;
            }
            log.persist(path).map_err(err)?;
            Ok::<_, String>(())
        })();
        if let Err(e) = save {
            eprintln!("Could not save disc tool diagnostics: {e}");
        }
    }
    result
}
fn drives() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/block") {
        for entry in entries.flatten() {
            let p = entry.path();
            if fs::read_to_string(p.join("device/type")).is_ok_and(|s| s.trim() == "5") {
                let dev = format!("/dev/{}", entry.file_name().to_string_lossy());
                let vendor = fs::read_to_string(p.join("device/vendor")).unwrap_or_default();
                let model = fs::read_to_string(p.join("device/model")).unwrap_or_default();
                out.push((
                    dev.clone(),
                    format!("{} {} ({dev})", vendor.trim(), model.trim()),
                ));
            }
        }
    }
    out.sort();
    out
}
fn safe_name(title: &str) -> String {
    let mut cleaned = String::new();
    let mut invalid_run = false;
    for c in title.chars() {
        let invalid = c < ' ' || c == '\u{7f}' || "<>:\"/\\|?*".contains(c);
        if invalid {
            if !invalid_run {
                cleaned.push_str(" - ");
            }
            invalid_run = true;
        } else {
            cleaned.push(c);
            invalid_run = false;
        }
    }
    let mut name = cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches([' ', '.'])
        .to_owned();
    while name.len() > 180 {
        name.pop();
    }
    name = name.trim_end_matches([' ', '.']).to_owned();
    if name.is_empty() {
        name = "Game disc".into();
    }
    let first = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&first.as_str())
        || first
            .strip_prefix("COM")
            .or_else(|| first.strip_prefix("LPT"))
            .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"));
    if reserved {
        name = format!("Game - {name}");
    }
    name
}
pub(crate) fn cover(root: &Path, system: &str, title: &str, cover_url: &str, region: &str) -> Option<Vec<u8>> {
    cover_with_cancel(
        Some(root),
        system,
        title,
        cover_url,
        region,
        &AtomicBool::new(false),
    )
}
fn cover_with_cancel(
    root: Option<&Path>,
    system: &str,
    title: &str,
    cover_url: &str,
    region: &str,
    cancel: &AtomicBool,
) -> Option<Vec<u8>> {
    if title.is_empty() {
        return None;
    }
    let catalog: Value = serde_json::from_str(include_str!("../../../data/systems.json")).ok()?;
    let console = catalog[system]["thumbnail"].as_str()?;
    let encode = |s: &str| {
        s.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect::<String>()
    };
    let dir = tempfile::tempdir().ok()?;
    let target = dir.path().join("cover.png");
    let mut urls = Vec::new();
    if cover_url.starts_with("https://") {
        urls.push(cover_url.to_owned());
    }
    urls.extend(artwork::urls(root, system, console, title, region, cancel));
    for suffix in [format!(" ({region})"), " (USA, Europe)".into(), String::new()] {
        urls.push(format!("https://raw.githubusercontent.com/libretro-thumbnails/{console}/master/Named_Boxarts/{}.png",encode(&format!("{title}{suffix}"))));
    }
    for url in urls {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
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
                .arg(&target)
                .arg(url),
            cancel,
        )
        .is_ok()
        {
            let bytes = fs::read(&target).ok()?;
            if image::load_from_memory(&bytes).is_ok() {
                return Some(bytes);
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_names_are_safe_and_preserve_titles() {
        assert_eq!(safe_name("Ecco the Dolphin"), "Ecco the Dolphin");
        assert_eq!(safe_name("../bad/\"name"), "- bad - name");
        assert_eq!(safe_name("..."), "Game disc");
    }
}
#[cfg(test)]
mod process_tests {
    use super::*;
    #[test]
    fn cancellation_reaps_reader_process_without_importing() {
        let cancel = AtomicBool::new(true);
        let start = Instant::now();
        let result = run_capture(Command::new("sleep").arg("30"), &cancel);
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn reader_failures_return_error_text() {
        let result = run_capture(
            Command::new("sh").args(["-c", "printf 'read failed' >&2; exit 1"]),
            &AtomicBool::new(false),
        );
        assert_eq!(result.unwrap_err(), "read failed");
    }
}

fn lookup(root: &Path, disc: &mut Value) -> Option<String> {
    let serial = disc["serial"].as_str()?;
    if serial.is_empty() {
        return None;
    }
    let normalize = |s: &str| {
        let s = s
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(char::to_uppercase)
            .collect::<String>();
        s.strip_prefix("GM").unwrap_or(&s).to_owned()
    };
    let wanted = normalize(serial);
    let catalog: Value = serde_json::from_str(include_str!("../../../data/systems.json")).ok()?;
    let names = crate::openvgdb::system_names(&catalog, disc["system"].as_str()?)?;
    let db = rusqlite::Connection::open_with_flags(
        crate::storage::metadata(root).join("openvgdb.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .ok()?;
    let mut statement=db.prepare("SELECT ROMs.romSerial,RELEASES.releaseTitleName,RELEASES.releaseCoverFront FROM ROMs JOIN SYSTEMS USING(systemID) JOIN RELEASES USING(romID) WHERE SYSTEMS.systemShortName IN (SELECT value FROM json_each(?1)) AND ROMs.romSerial IS NOT NULL LIMIT 200000").ok()?;
    let rows = statement
        .query_map([names], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            ))
        })
        .ok()?;
    let matches = rows
        .filter_map(Result::ok)
        .filter(|(s, t, _)| !t.is_empty() && normalize(s) == wanted)
        .collect::<Vec<_>>();
    let (_, title, url) = matches.first()?;
    if matches.iter().any(|(_, t, _)| t != title) {
        return None;
    }
    disc["title"] = Value::String(title.clone());
    Some(url.clone())
}
#[allow(clippy::too_many_arguments)]
fn compatibility_read(
    drive: &str,
    token: &str,
    folder: &Path,
    log: &Path,
    level: u8,
    sectors: u64,
    layout: &[Value],
    cancel: &AtomicBool,
    sender: &mpsc::Sender<Event>,
    ctx: &egui::Context,
) -> Result<(), String> {
    let probe = |cancel: &AtomicBool| -> Result<(), String> {
        let bytes = run_capture(Command::new(helper()).args(["inspect", drive]), cancel)?;
        let v: Value = serde_json::from_slice(&bytes).map_err(err)?;
        if v["token"] != token {
            return Err("Disc changed. Refresh before backing up.".into());
        }
        Ok(())
    };
    probe(cancel)?;
    let program = tool("cdrdao");
    let output = fs::File::create(log).map_err(err)?;
    let mut command = Command::new(&program);
    command.current_dir(folder).args([
        "read-cd",
        "--read-raw",
        "--paranoia-mode",
        &level.to_string(),
    ]);
    if drives().iter().any(|(p, name)| {
        p == drive
            && name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .contains("CW8123")
    }) {
        command.args(["--driver", "generic-mmc:0x20"]);
    }
    command
        .args(["--device", drive, "--datafile", "disc.bin", "disc.toc"])
        .env("LC_ALL", "C")
        .stdout(output.try_clone().map_err(err)?)
        .stderr(output);
    let libdir = program
        .parent()
        .and_then(Path::parent)
        .map(|p| p.join("lib"));
    if let Some(libdir) = libdir.filter(|p| p.is_dir()) {
        let mut paths = vec![libdir];
        if let Some(old) = std::env::var_os("LD_LIBRARY_PATH") {
            paths.extend(std::env::split_paths(&old));
        }
        command.env("LD_LIBRARY_PATH", std::env::join_paths(paths).map_err(err)?);
    }
    let mut child = command.spawn().map(ManagedChild).map_err(err)?;
    let started = Instant::now();
    let binary = folder.join("disc.bin");
    let mut log_size = 0;
    let mut reader_status = ReaderStatus::default();
    let status = loop {
        let error = if cancel.load(Ordering::Relaxed) {
            Some("Backup cancelled. No game was added.".into())
        } else if started.elapsed() > Duration::from_secs(7200)
            || fs::metadata(log).is_ok_and(|m| m.len() > 32 * 1024 * 1024)
        {
            Some("Disc reader exceeded its time/log limit. No game was added.".into())
        } else if reader_failed(log)? {
            Some("The drive reported a failed READ CD command. No game was added. Check its connection and retry.".into())
        } else {
            None
        };
        if let Some(error) = error {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let len = fs::metadata(log).map_err(err)?.len();
        if len != log_size {
            log_size = len;
            let text = read_tail(log)?;
            let detail = reader_status.update(&text, layout.len());
            let _ = sender.send(Event::Message(detail));
            ctx.request_repaint();
        }
        if let Some(status) = child.try_wait().map_err(err)? {
            break status;
        }
        let done = fs::metadata(&binary)
            .map_or(0, |m| m.len())
            .min(sectors * 2352);
        let _ = sender.send(Event::Progress(done, sectors * 2352));
        ctx.request_repaint();
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() || reader_failed(log)? {
        return Err(format!(
            "Disc reader failed. No game was added. {}",
            read_tail(log)?
                .chars()
                .rev()
                .take(700)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        ));
    }
    run_capture(
        Command::new(tool("toc2cue"))
            .current_dir(folder)
            .args(["disc.toc", "disc.cue"]),
        cancel,
    )?;
    let size = fs::metadata(&binary).map_err(err)?.len();
    let cue = fs::read_to_string(folder.join("disc.cue")).map_err(err)?;
    let audio = validate_cue(&cue, size, sectors, layout)?;
    let _ = sender.send(Event::Message("Preparing CD audio…".into()));
    ctx.request_repaint();
    swap_audio(&binary, &audio, cancel)?;
    probe(cancel)?;
    Ok(())
}
fn read_tail(path: &Path) -> Result<String, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path).map_err(err)?;
    let len = f.metadata().map_err(err)?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(65536)))
        .map_err(err)?;
    let mut bytes = Vec::new();
    f.take(65536).read_to_end(&mut bytes).map_err(err)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
fn reader_failed(log: &Path) -> Result<bool, String> {
    Ok(read_tail(log)?.lines().any(|l| {
        let l = l.to_ascii_uppercase();
        l.contains("ERROR:") && l.contains("SCSI COMMAND READ CD") && l.contains("FAILED:")
    }))
}
fn cue_time(s: &str) -> Result<u64, String> {
    let parts = s
        .split(':')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    if parts.len() != 3 || parts[0] > 100 || parts[1] >= 60 || parts[2] >= 75 {
        return Err("Invalid CUE timing".into());
    }
    Ok((parts[0] * 60 + parts[1]) * 75 + parts[2])
}
fn validate_cue(
    text: &str,
    size: u64,
    sectors: u64,
    original: &[Value],
) -> Result<Vec<(u64, u64)>, String> {
    #[derive(Default)]
    struct Track {
        audio: bool,
        index: Option<u64>,
        start: Option<u64>,
        gap: Option<u64>,
    }
    let mut tracks: Vec<Track> = Vec::new();
    let mut files = 0;
    for line in text.lines() {
        let line = line.trim();
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.is_empty() {
            continue;
        }
        match parts[0].to_ascii_uppercase().as_str() {
            "FILE" => {
                files += 1;
                if line != "FILE \"disc.bin\" BINARY" {
                    return Err("Unsupported CUE file layout".into());
                }
            }
            "TRACK" => {
                if parts.len() != 3
                    || parts[1].parse::<usize>().ok() != Some(tracks.len() + 1)
                    || !matches!(parts[2], "AUDIO" | "MODE1/2352" | "MODE2/2352")
                {
                    return Err("Unsupported raw-sector CUE track".into());
                }
                tracks.push(Track {
                    audio: parts[2] == "AUDIO",
                    ..Default::default()
                });
            }
            "INDEX" => {
                if parts.len() != 3 || tracks.is_empty() {
                    return Err("Invalid CUE index".into());
                }
                let t = tracks.last_mut().unwrap();
                let value = cue_time(parts[2])?;
                let field = match parts[1] {
                    "00" => &mut t.start,
                    "01" => &mut t.index,
                    _ => continue,
                };
                if field.replace(value).is_some() {
                    return Err("Duplicate CUE index".into());
                }
            }
            "PREGAP" => {
                if parts.len() != 2 || tracks.is_empty() {
                    return Err("Invalid CUE pregap".into());
                }
                if tracks
                    .last_mut()
                    .unwrap()
                    .gap
                    .replace(cue_time(parts[1])?)
                    .is_some()
                {
                    return Err("Duplicate CUE pregap".into());
                }
            }
            _ => {}
        }
    }
    if files != 1
        || tracks.is_empty()
        || tracks.len() != original.len()
        || tracks[0].index != Some(0)
    {
        return Err("Copied track list does not match the physical CD".into());
    }
    let mut gap = 0;
    let mut previous = None;
    for (i, t) in tracks.iter().enumerate() {
        let index = t.index.ok_or("Missing CUE index 01")?;
        let start = t.start.unwrap_or(index);
        if let Some(g) = t.gap {
            if i == 0 || t.start.is_some() || !t.audio || g == 0 || g >= sectors {
                return Err("Unsupported pregap layout".into());
            }
            gap += g;
        }
        if start > index
            || index >= sectors
            || previous.is_some_and(|p| start <= p)
            || original[i]["lba"].as_u64() != Some(index + gap)
            || original[i]["data"].as_bool() != Some(!t.audio)
        {
            return Err("Copied track timing differs from the physical CD".into());
        }
        previous = Some(index);
    }
    if gap >= sectors || size != (sectors - gap) * 2352 {
        return Err("Disc image is incomplete; missing sectors are never guessed".into());
    }
    let stored = size / 2352;
    let mut audio = Vec::new();
    for (i, t) in tracks.iter().enumerate() {
        let index = t.index.unwrap();
        let start = t.start.unwrap_or(index);
        let end = tracks
            .get(i + 1)
            .map_or(stored, |t| t.start.unwrap_or(t.index.unwrap()));
        if index >= stored || end <= start || end > stored {
            return Err("CUE points outside the stored image".into());
        }
        if t.audio {
            audio.push((start * 2352, end * 2352));
        }
    }
    Ok(audio)
}
fn swap_audio(path: &Path, ranges: &[(u64, u64)], cancel: &AtomicBool) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(err)?;
    let mut buf = vec![0; 2 * 1024 * 1024];
    for (start, end) in ranges {
        let mut offset = *start;
        while offset < *end {
            if cancel.load(Ordering::Relaxed) {
                return Err("Backup cancelled. No game was added.".into());
            }
            let n = buf.len().min((end - offset) as usize);
            if !n.is_multiple_of(2) {
                return Err("Incomplete audio samples".into());
            }
            f.seek(SeekFrom::Start(offset)).map_err(err)?;
            f.read_exact(&mut buf[..n]).map_err(err)?;
            for sample in buf[..n].as_chunks_mut::<2>().0 {
                sample.swap(0, 1);
            }
            f.seek(SeekFrom::Start(offset)).map_err(err)?;
            f.write_all(&buf[..n]).map_err(err)?;
            offset += n as u64;
        }
    }
    f.sync_all().map_err(err)
}

struct ManagedChild(std::process::Child);
impl std::ops::Deref for ManagedChild {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ManagedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(test)]
mod compatibility_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn validates_real_track_timeline_and_swaps_only_audio_samples() {
        let cue="FILE \"disc.bin\" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\n TRACK 02 AUDIO\n INDEX 00 00:00:01\n INDEX 01 00:00:02\n";
        let layout = vec![
            json!({"number":1,"lba":0,"data":true}),
            json!({"number":2,"lba":2,"data":false}),
        ];
        let ranges = validate_cue(cue, 3 * 2352, 3, &layout).unwrap();
        assert_eq!(ranges, vec![(2352, 3 * 2352)]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.bin");
        let mut data = vec![0; 3 * 2352];
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        fs::write(&path, &data).unwrap();
        swap_audio(&path, &ranges, &AtomicBool::new(false)).unwrap();
        let after = fs::read(path).unwrap();
        assert_eq!(&after[..2352], &data[..2352]);
        for i in (2352..after.len()).step_by(2) {
            assert_eq!(after[i], data[i + 1]);
            assert_eq!(after[i + 1], data[i]);
        }
    }
    #[test]
    fn represents_unstored_pregaps_without_fabricating_sectors() {
        let cue="FILE \"disc.bin\" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\n TRACK 02 AUDIO\n PREGAP 00:00:01\n INDEX 01 00:00:01\n";
        let layout = vec![json!({"lba":0,"data":true}), json!({"lba":2,"data":false})];
        assert!(validate_cue(cue, 2 * 2352, 3, &layout).is_ok());
        assert!(validate_cue(cue, 3 * 2352, 3, &layout).is_err());
        assert!(validate_cue(&cue.replace("00:00:01", "00:00:02"), 2 * 2352, 3, &layout).is_err());
    }
    #[test]
    fn recognizes_read_failure_even_when_reader_reports_success() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("reader.log");
        fs::write(
            &log,
            "Reading...\nERROR: SCSI command Read CD failed: drive disconnected\n",
        )
        .unwrap();
        assert!(reader_failed(&log).unwrap());
    }
}

fn available_space(path: &Path) -> Option<u64> {
    let mut path = path;
    while !path.exists() {
        path = path.parent()?;
    }
    let output = Command::new("df")
        .args(["-B1", "--output=avail"])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .split_whitespace()
        .last()?
        .parse()
        .ok()
}

fn helper_environment(cmd: &mut Command) -> Result<(), String> {
    let lib = Path::new(cmd.get_program())
        .parent()
        .and_then(Path::parent)
        .map(|p| p.join("lib"));
    if let Some(lib) = lib.filter(|p| p.is_dir()) {
        let mut paths = vec![lib];
        if let Some(old) = std::env::var_os("LD_LIBRARY_PATH") {
            paths.extend(std::env::split_paths(&old));
        }
        cmd.env("LD_LIBRARY_PATH", std::env::join_paths(paths).map_err(err)?);
    }
    Ok(())
}
#[cfg(test)]
mod helper_tests {
    use super::*;
    #[test]
    #[ignore = "requires bundled chdman and native CHD probe"]
    fn bundled_chd_round_trip_preserves_data_and_audio_layout() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = vec![0; 16 * 2352];
        for i in 0..8 {
            let start = i * 2352;
            bytes[start + 1..start + 11].fill(255);
            bytes[start + 15] = 1;
        }
        bytes[16..30].copy_from_slice(b"SEGADISCSYSTEM");
        for b in &mut bytes[8 * 2352..] {
            *b = 42;
        }
        fs::write(dir.path().join("disc.bin"), bytes).unwrap();
        fs::write(dir.path().join("disc.cue"),"FILE \"disc.bin\" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\n TRACK 02 AUDIO\n INDEX 01 00:00:08\n").unwrap();
        let chd = dir.path().join("disc.chd");
        let cancel = AtomicBool::new(false);
        run_capture(
            Command::new(tool("chdman"))
                .arg("createcd")
                .arg("-i")
                .arg(dir.path().join("disc.cue"))
                .arg("-o")
                .arg(&chd)
                .args(["-np", "1"]),
            &cancel,
        )
        .unwrap();
        run_capture(
            Command::new(tool("chdman"))
                .args(["verify", "-i"])
                .arg(&chd),
            &cancel,
        )
        .unwrap();
        let probe =
            crate::resources::path("build/cores/emuluna-chd-probe");
        let sample = run_capture(Command::new(probe).arg(chd), &cancel).unwrap();
        assert!(sample.windows(14).any(|s| s == b"SEGADISCSYSTEM"));
    }
}

fn backup_viewport(size: egui::Vec2) -> egui::ViewportBuilder {
    crate::window_state::app_viewport()
        .with_title("Back up a game disc — EmuLuna")
        .with_inner_size(size)
        .with_min_inner_size(size)
        .with_max_inner_size(size)
        .with_resizable(false)
        .with_decorations(true)
        .with_maximize_button(false)
}
#[cfg(test)]
mod parity_tests {
    use super::*;
    use serde_json::json;
    fn prepared() -> Prepared {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("disc.cue");
        fs::write(&source, b"verified staging fixture").unwrap();
        Prepared {
            _folder: folder,
            source,
        }
    }
    fn library() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let db = rusqlite::Connection::open(root.path().join("library.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE games(id TEXT PRIMARY KEY,title TEXT,system TEXT,rom_path TEXT,source TEXT,added REAL,original_filename TEXT,previous_rom_stem TEXT,cover TEXT);CREATE TABLE game_files(game_id TEXT,path TEXT,sha256 TEXT,size INTEGER,PRIMARY KEY(game_id,path));CREATE TABLE metadata_overrides(game_id TEXT,field TEXT,PRIMARY KEY(game_id,field));").unwrap();
        root
    }
    fn disc() -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        fs::write(
            folder.path().join("disc.cue"),
            "FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        let mut bytes = vec![0; 2352];
        bytes[16..30].copy_from_slice(b"SEGADISCSYSTEM");
        fs::write(folder.path().join("disc.bin"), bytes).unwrap();
        folder
    }
    #[test]
    fn leaving_completed_backup_requires_explicit_choice() {
        let p = prepared();
        let path = p.source.clone();
        let mut b = Backup {
            open: true,
            stage: Stage::Success,
            prepared: Some(p),
            ..Default::default()
        };
        b.request_leave(Leave::Another);
        assert_eq!(b.confirm_leave, Some(Leave::Another));
        assert!(path.exists());
        assert!(b.open);
        b.confirm_leave = None;
        b.request_leave(Leave::Close);
        assert_eq!(b.confirm_leave, Some(Leave::Close));
        assert!(path.exists());
    }
    #[test]
    fn close_cancels_reading_but_never_interrupts_committing() {
        let mut b = Backup {
            open: true,
            busy: true,
            ..Default::default()
        };
        b.request_leave(Leave::Close);
        assert!(b.cancel.load(Ordering::Relaxed));
        assert!(b.close_after_cancel);
        assert!(b.open);
        let mut committing = Backup {
            open: true,
            busy: true,
            committing: true,
            ..Default::default()
        };
        committing.request_leave(Leave::Close);
        assert!(!committing.cancel.load(Ordering::Relaxed));
        assert!(!committing.close_after_cancel);
        assert!(committing.open);
    }
    #[test]
    fn named_backups_reimport_with_original_identity_and_restore_missing_tracks() {
        let root = library();
        let source = disc();
        let cue = source.path().join("disc.cue");
        commit_backup(
            root.path(),
            &cue,
            "segacd",
            "Ecco the Dolphin",
            None,
            "GM MK-4408 -00",
            85,
        )
        .unwrap();
        let db = rusqlite::Connection::open(root.path().join("library.sqlite3")).unwrap();
        let (id, rom, origin): (String, String, String) = db
            .query_row("SELECT id,rom_path,source FROM games", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(
            id,
            "d64a491d69570475a2a3d6b03e67f015fa0c138de6eabacff2049cb4edb22aca"
        );
        assert!(rom.ends_with("Ecco the Dolphin.cue"));
        assert_eq!(origin, "Optical disc: GM MK-4408 -00");
        let bin = root.path().join(&rom).with_extension("bin");
        assert!(bin.is_file());
        assert!(fs::read_to_string(root.path().join(&rom))
            .unwrap()
            .contains("Ecco the Dolphin.bin"));
        assert_eq!(
            fs::read_to_string(&cue).unwrap(),
            "FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n"
        );
        commit_backup(
            root.path(),
            &cue,
            "segacd",
            "Another reviewed title",
            None,
            "same disc",
            85,
        )
        .unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM games", [], |r| r.get::<_, usize>(0))
                .unwrap(),
            1
        );
        let rom: String = db
            .query_row("SELECT rom_path FROM games WHERE id=?", [&id], |r| r.get(0))
            .unwrap();
        assert!(rom.ends_with("Another reviewed title.cue"));
        fs::remove_file(root.path().join(&rom).with_extension("bin")).unwrap();
        commit_backup(
            root.path(),
            &cue,
            "segacd",
            "Recovered disc",
            None,
            "same disc",
            85,
        )
        .unwrap();
        let restored: String = db
            .query_row("SELECT rom_path FROM games WHERE id=?", [&id], |r| r.get(0))
            .unwrap();
        assert!(root.path().join(restored).with_extension("bin").is_file());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM games", [], |r| r.get::<_, usize>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn failed_naming_keeps_the_prior_playable_copy() {
        let root = library();
        let source = disc();
        let imported =
            importing::import_disc(root.path(), &source.path().join("disc.cue"), "segacd").unwrap();
        let mut db = rusqlite::Connection::open(root.path().join("library.sqlite3")).unwrap();
        let stored: String = db
            .query_row("SELECT rom_path FROM games", [], |r| r.get(0))
            .unwrap();
        db.execute_batch("CREATE TRIGGER reject_rename BEFORE UPDATE ON games BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(name_managed_backup(root.path(), &mut db, &imported.id, "Name").is_err());
        assert!(root.path().join(&stored).is_file());
        assert!(root.path().join(&stored).with_extension("bin").is_file());
        assert!(!root
            .path()
            .join(&stored)
            .with_file_name("Name.cue")
            .exists());
        assert!(!root
            .path()
            .join(&stored)
            .with_file_name("Name.bin")
            .exists());
    }
    #[test]
    fn correction_levels_and_status_messages_match_old_reader() {
        assert!(correction_note(false, 3).contains("Failed batches retry individual sectors"));
        assert!(correction_note(true, 0).contains("without checking"));
        assert!(correction_note(true, 2).contains("without additional scratch repair"));
        let mut status = ReaderStatus::default();
        assert!(status
            .update("Copying audio tracks 2-16\nAnalyzing...", 16)
            .starts_with("Scanning track boundaries"));
        assert!(status.update("Reading...", 16).contains("Audio"));
        assert_eq!(safe_name("CON"), "Game - CON");
        assert!(safe_name(&"界".repeat(160)).len() <= 180);
    }
    #[test]
    fn all_steps_keep_actions_inside_fixed_window() {
        for height in [656., 800.] {
            for (stage, busy, scanning) in [
                (Stage::Insert, false, false),
                (Stage::Ready, false, false),
                (Stage::Reading, true, false),
                (Stage::Reading, true, true),
                (Stage::Success, false, false),
            ] {
                let ctx = egui::Context::default();
                let theme = emuluna_theme::Theme::builtin("luna").unwrap();
                theme.apply(&ctx);
                let mut b = Backup {
                    stage,
                    busy,
                    scanning,
                    title: "Ecco the Dolphin".into(),
                    system: "segacd".into(),
                    tracks: 2,
                    sectors: 1000,
                    token: "fixture".into(),
                    layout: vec![
                        json!({"number":1,"lba":0,"data":true}),
                        json!({"number":2,"lba":500,"data":false}),
                    ],
                    progress: (100, 1000),
                    message: "A status message.".into(),
                    ..Default::default()
                };
                let mut prefs = Preferences::default();
                prefs.set("disc_backup.agreement_version", "1");
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(960., height),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE.inner_margin(24.))
                            .show(ctx, |ui| {
                                b.contents(ui, ctx, None, &mut prefs);
                                assert!(
                                    ui.min_rect().bottom() <= height - 20.
                                        && ui.min_rect().right() <= 936.5,
                                    "{stage:?} overflow at {height}: {:?}",
                                    ui.min_rect()
                                );
                            });
                    },
                );
            }
        }
    }
}
#[cfg(test)]
mod lookup_state_tests {
    use super::*;

    #[test]
    fn empty_drive_probe_keeps_the_wizard_idle() {
        let ctx = egui::Context::default();
        let (sender, result) = mpsc::channel();
        let mut backup = Backup {
            drive: "/dev/sr0".into(),
            next_probe: Some(Instant::now() + DISC_POLL_INTERVAL),
            drive_probe: Some(DriveProbe {
                drive: "/dev/sr0".into(),
                result,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            ..Default::default()
        };
        backup.poll_drive(&ctx, None);
        assert!(!backup.busy);
        assert!(backup.stage == Stage::Insert);
        sender.send(Ok(false)).unwrap();
        backup.poll_drive(&ctx, None);
        assert!(backup.drive_probe.is_none());
        assert!(backup.stage == Stage::Insert);
        assert!(!backup.busy && backup.receiver.is_none());
    }

    use serde_json::json;
    #[test]
    fn identified_form_waits_for_cover_job_before_enabling_backup() {
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        let mut b = Backup {
            open: true,
            busy: true,
            stage: Stage::Identifying,
            receiver: Some(rx),
            ..Default::default()
        };
        let mut prefs = Preferences::default();
        prefs.set("disc_backup.agreement_version", "1");
        tx.send(Event::Disc(json!({"title":"Disc","system":"segacd","serial":"GM TEST","token":"token","sectors":1000,"tracks":1,"layout":[{"number":1,"lba":0,"data":true}]}))).unwrap();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            b.show(ctx, None, &mut prefs);
        });
        assert_eq!(b.stage, Stage::Ready);
        assert!(b.busy);
        assert!(!b.cover_missing);
        tx.send(Event::LookupDone).unwrap();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            b.show(ctx, None, &mut prefs);
        });
        assert!(!b.busy);
        assert_eq!(b.stage, Stage::Ready);
        assert!(b.cover_missing);

        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        tx.send(Event::Cover(png.into_inner())).unwrap();
        tx.send(Event::LookupDone).unwrap();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            b.show(ctx, None, &mut prefs);
        });
        assert!(b.texture.is_some());
        assert!(!b.cover_missing);
    }
    #[test]
    fn cancelled_close_waits_for_worker_result() {
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        let mut b = Backup {
            open: true,
            busy: true,
            stage: Stage::Reading,
            receiver: Some(rx),
            ..Default::default()
        };
        let mut prefs = Preferences::default();
        prefs.set("disc_backup.agreement_version", "1");
        b.request_leave(Leave::Close);
        assert!(b.open);
        tx.send(Event::Error("Cancelled".into())).unwrap();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            b.show(ctx, None, &mut prefs);
        });
        assert!(!b.open);
        assert!(!b.busy);
    }
}
#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn tool_errors_preserve_stdout_stderr_and_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last-compression.log");
        assert!(run_capture_logged(
            Command::new("sh").args([
                "-c",
                "printf 'tool detail'; printf 'read failed' >&2; exit 1"
            ]),
            &AtomicBool::new(false),
            &path
        )
        .is_err());
        let log = fs::read_to_string(path).unwrap();
        assert!(log.contains("tool detail"));
        assert!(log.contains("read failed"));
        let metadata: Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(metadata["outcome"], "read failed");
    }
}
