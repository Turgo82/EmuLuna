//! Isolated test sessions: never reuse the user's save or writable BIOS folders.
use eframe::egui;
use emuluna_library_model::Game;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;
const BIOS_BUDGET: u64 = 256 * 1024 * 1024;
const CATALOG: &str = include_str!("../../../data/cores.json");
pub(crate) fn autosave_enabled(settings: &BTreeMap<String, String>, game: &str) -> bool {
    settings
        .get(&format!("game.{game}.autosave"))
        .or_else(|| settings.get("native.auto_resume"))
        .is_some_and(|value| value == "1")
}

pub(crate) fn autosave_path(root: &Path, progress: &Path, system: &str, game: &str) -> PathBuf {
    let directory =
        crate::storage::native_media(root, progress, system, game, "autosave", "states");
    emuluna_state_file::readable_path(&emuluna_state_file::automatic_path(&directory))
}

pub(crate) fn state_core(
    root: &Path,
    system: &str,
    game: &str,
    identity: &emuluna_state_file::Identity,
) -> Result<emuluna_launch_config::InstalledCore, String> {
    use emuluna_state_file::Renderer;
    if identity.rom != game {
        return Err("This save state belongs to a different game".into());
    }
    if identity.renderer == Renderer::Vulkan {
        return Err("This state needs Vulkan playback, which is still being ported".into());
    }
    let parts: Vec<_> = identity.core.split(':').collect();
    let ["libretro", id, digest] = parts.as_slice() else {
        return Err(
            "This state needs a legacy core that the native player does not support".into(),
        );
    };
    emuluna_launch_config::resolve_state_core(root, system, id, digest)
}

pub struct Request {
    pub root: PathBuf,
    pub game: Game,
    pub settings: BTreeMap<String, String>,
    pub theme: String,
    pub theme_root: PathBuf,
    pub preset: String,
    pub frames: Option<u32>,
    pub progress_root: PathBuf,
    pub resume: Option<PathBuf>,
    pub start_from_boot: bool,
}
struct Plan {
    work: TempDir,
    command: Command,
}
enum Update {
    Started(PathBuf, Option<String>),
    Ready,
    Controls(String),
    Finished(TempDir, Result<(), String>),
    Failed(String),
}
pub struct Launcher {
    ctx: egui::Context,
    sender: mpsc::Sender<Update>,
    receiver: mpsc::Receiver<Update>,
    stop: Arc<AtomicBool>,
    pub busy: bool,
    pub ready: bool,
    pub completed_filter: Option<String>,
    pub completed_options: Option<BTreeMap<String, String>>,
    pub controls_request: Option<String>,
    pub restore_on_exit: Arc<AtomicBool>,
    pub message: String,
    pub log: Option<PathBuf>,
    pub last_success: Option<bool>,
    pub started_game: Option<String>,
    last_session: Option<TempDir>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Launcher {
    pub fn new(ctx: egui::Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            ctx,
            sender,
            receiver,
            stop: Arc::new(AtomicBool::new(false)),
            busy: false,
            ready: false,
            completed_filter: None,
            completed_options: None,
            controls_request: None,
            restore_on_exit: Arc::new(AtomicBool::new(false)),
            message: String::new(),
            log: None,
            last_success: None,
            started_game: None,
            last_session: None,
            worker: None,
        }
    }
    pub fn start(&mut self, request: Request) {
        if self.busy {
            return;
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.busy = true;
        self.ready = false;
        self.last_success = None;
        self.message = format!("Opening {}…", request.game.title);
        self.log = None;
        self.last_session = None;
        let sender = self.sender.clone();
        let ctx = self.ctx.clone();
        let stop = self.stop.clone();
        let restore_on_exit = self.restore_on_exit.clone();
        let history = request.frames.is_none().then(|| request.game.id.clone());
        let control_system = request.game.system.clone();
        self.worker = Some(thread::spawn(move || {
            let result = (|| -> Result<Update, String> {
                let mut plan = prepare(request, &stop)?;
                let log_path = plan.work.path().join("session.log");
                let log = File::create(&log_path).map_err(|e| e.to_string())?;
                plan.command
                    .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
                    .stderr(Stdio::from(log))
                    .stdin(Stdio::piped());
                if stop.load(Ordering::Acquire) {
                    return Err("Test session cancelled".into());
                }
                let mut child = plan
                    .command
                    .spawn()
                    .map_err(|e| format!("Could not open the native player: {e}"))?;
                let _ = sender.send(Update::Started(log_path.clone(), history));
                ctx.request_repaint();
                let mut closing = None;
                let mut ready = false;
                let result = loop {
                    let controls_request = plan.work.path().join("controls-request");
                    if controls_request.is_file() && std::fs::remove_file(&controls_request).is_ok()
                    {
                        if restore_on_exit.swap(false, Ordering::AcqRel) {
                            if crate::kwin_restore::available() {
                                let _ = crate::kwin_restore::restore();
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
                        let _ = sender.send(Update::Controls(control_system.clone()));
                        ctx.request_repaint();
                    }
                    if !ready && log_tail(&log_path).contains("Native playback started") {
                        ready = true;
                        let _ = sender.send(Update::Ready);
                        ctx.request_repaint();
                    }
                    if stop.load(Ordering::Acquire) {
                        if closing.is_none() {
                            if let Some(mut stdin) = child.stdin.take() {
                                let _ = stdin.write_all(b"q");
                            }
                            closing = Some(Instant::now());
                        } else if closing
                            .is_some_and(|start| start.elapsed() > Duration::from_secs(15))
                        {
                            let _ = child.kill();
                            let _ = child.wait();
                            break Err("The game did not close in time; its last successful native save is retained.".into());
                        }
                    }
                    match child.try_wait() {
                        Ok(Some(status)) if status.success() => break Ok(()),
                        Ok(Some(_)) => break Err(error_tail(&log_path)),
                        Ok(None) => thread::sleep(Duration::from_millis(50)),
                        Err(error) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            break Err(format!("Could not monitor the player: {error}"));
                        }
                    }
                };
                Ok(Update::Finished(plan.work, result))
            })();
            let _ = sender.send(result.unwrap_or_else(Update::Failed));
            if restore_on_exit.swap(false, Ordering::AcqRel) {
                if crate::kwin_restore::available() {
                    if let Err(error) = crate::kwin_restore::restore() {
                        eprintln!("Could not restore the library: {error}");
                    }
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            ctx.request_repaint();
        }));
    }
    pub fn diagnostic_tail(&self) -> String {
        self.log
            .as_ref()
            .map_or_else(String::new, |path| log_tail(path))
    }
    pub fn poll(&mut self) {
        for update in self.receiver.try_iter() {
            match update {
                Update::Started(path, game) => {
                    self.started_game = game;
                    self.log = Some(path);
                    self.message = "Game running".into();
                }
                Update::Ready => self.ready = true,
                Update::Controls(system) => self.controls_request = Some(system),
                Update::Finished(work, result) => {
                    self.completed_options =
                        std::fs::read(work.path().join("console-options.json"))
                            .ok()
                            .filter(|data| data.len() <= 131072)
                            .and_then(|data| serde_json::from_slice(&data).ok());
                    self.completed_filter =
                        std::fs::read_to_string(work.path().join("filter-selection"))
                            .ok()
                            .filter(|key| key.len() <= 256 && !key.contains(['\n', '\0']));

                    self.ready = false;
                    self.busy = false;
                    self.last_success = Some(result.is_ok());
                    self.last_session = Some(work);
                    self.message = match result {
                        Ok(()) => "Game closed".into(),
                        Err(e) => format!("The test player stopped: {e}"),
                    };
                }
                Update::Failed(error) => {
                    self.ready = false;
                    self.busy = false;
                    self.last_success = Some(false);
                    self.message = error;
                }
            }
        }
    }
}
impl Drop for Launcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn log_tail(path: &Path) -> String {
    let result = (|| -> std::io::Result<String> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        file.seek(SeekFrom::Start(size.saturating_sub(8192)))?;
        let mut bytes = Vec::new();
        file.take(8192).read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    })();
    result.unwrap_or_else(|_| "See the session log for details.".into())
}
fn error_tail(path: &Path) -> String {
    log_tail(path)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("See the session log for details.")
        .chars()
        .take(512)
        .collect()
}

fn copy_bios(source: &Path, target: &Path, stop: &AtomicBool) -> Result<(), String> {
    fs::create_dir(target).map_err(|e| e.to_string())?;
    if !source.exists() {
        return Ok(());
    }
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    if !source.is_dir() {
        return Err("The configured system-files folder is unavailable".into());
    }
    let mut remaining = BIOS_BUDGET;
    let mut count = 0;
    let mut pending = vec![(source.clone(), target.to_path_buf(), 0usize)];
    while let Some((from, to, depth)) = pending.pop() {
        if depth > 8 {
            return Err("The system-files folder is nested too deeply for a test copy".into());
        }
        for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
            if stop.load(Ordering::Acquire) {
                return Err("Test session cancelled".into());
            }
            let entry = entry.map_err(|e| e.to_string())?;
            count += 1;
            if count > 4096 {
                return Err("The system-files folder has too many entries for a test copy".into());
            }
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let destination = to.join(entry.file_name());
            if kind.is_symlink() {
                return Err("Test sessions need system files stored directly in the configured folder. A symbolic link was found.".into());
            }
            if kind.is_dir() {
                fs::create_dir(&destination).map_err(|e| e.to_string())?;
                pending.push((entry.path(), destination, depth + 1));
            } else if kind.is_file() {
                let mut input = File::open(entry.path()).map_err(|e| e.to_string())?;
                let size = input.metadata().map_err(|e| e.to_string())?.len();
                if size > remaining || size > 64 * 1024 * 1024 {
                    return Err(
                        "The system-files folder exceeds the isolated test copy limit".into(),
                    );
                }
                let mut output = File::create(destination).map_err(|e| e.to_string())?;
                let copied =
                    std::io::copy(&mut Read::by_ref(&mut input).take(size + 1), &mut output)
                        .map_err(|e| e.to_string())?;
                if copied != size {
                    return Err("A system file changed while preparing the test session".into());
                }
                remaining -= copied;
            } else {
                return Err("The system-files folder contains a non-file entry".into());
            }
        }
    }
    Ok(())
}
fn prepare(request: Request, stop: &AtomicBool) -> Result<Plan, String> {
    if request
        .frames
        .is_some_and(|frames| !(1..=1800).contains(&frames))
    {
        return Err("Test launch frame limit must be 1..1800".into());
    }
    let persistent = request.frames.is_none()
        && request
            .settings
            .get("native.keep_progress")
            .is_none_or(|s| s != "0");
    let legacy_progress = crate::storage::native_fallback(&request.root, &request.progress_root);
    let (resume, state) = if let Some(path) = request.resume {
        let identity = emuluna_state_file::inspect(&path)?;
        let core = state_core(
            &request.root,
            &request.game.system,
            &request.game.id,
            &identity,
        )?;
        (Some(path), Some((identity, core)))
    } else if persistent
        && !request.start_from_boot
        && autosave_enabled(&request.settings, &request.game.id)
    {
        let path = autosave_path(
            &request.root,
            &legacy_progress,
            &request.game.system,
            &request.game.id,
        );
        let state = emuluna_state_file::inspect(&path)
            .ok()
            .and_then(|identity| {
                let core = state_core(
                    &request.root,
                    &request.game.system,
                    &request.game.id,
                    &identity,
                )
                .ok()?;
                Some((identity, core))
            });
        (state.as_ref().map(|_| path), state)
    } else {
        (None, None)
    };
    let (resume_identity, core) = if let Some((identity, core)) = state {
        (Some(identity), core)
    } else {
        (
            None,
            emuluna_launch_config::resolve_installed(
                &request.root,
                &request.game.system,
                request
                    .settings
                    .get(&format!("core.{}", request.game.system))
                    .map_or("auto", String::as_str),
            )?,
        )
    };
    if !request.game.rom.is_file() {
        return Err(
            "This game's ROM is missing. Reconnect its drive or locate it in the main app.".into(),
        );
    }
    let catalog: serde_json::Value = serde_json::from_str(CATALOG).map_err(|e| e.to_string())?;
    let catalog_id = core.catalog_id.as_deref().unwrap_or(&core.id);
    let definition = catalog
        .get(catalog_id)
        .ok_or("This imported core has no native launch profile yet")?;
    let mut gl = definition["hardware_required"].as_bool() == Some(true)
        || (definition.get("hardware_options").is_some()
            && request
                .settings
                .get("experimental.hardware_rendering")
                .is_none_or(|s| s != "0"));
    if let Some(identity) = &resume_identity {
        gl = identity.renderer == emuluna_state_file::Renderer::OpenGl;
        if !gl && definition["hardware_required"].as_bool() == Some(true) {
            return Err("The selected core requires a hardware-rendered save state".into());
        }
    }
    let mut options = saved_options(
        definition,
        catalog_id,
        &request.game.system,
        &request.game.id,
        &request.settings,
        &core.id,
    );
    if catalog_id == "parallel_n64"
        && request.game.system == "n64"
        && is_1080(&request.game.rom, &request.game.title)
    {
        options
            .entry("parallel-n64-cpucore".into())
            .or_insert_with(|| "cached_interpreter".into());
    }
    let system = &request.game.system;
    let profiles = (0..emuluna_native_input::player_count(system))
        .map(|player| {
            let record = emuluna_native_input::library_profile(
                system,
                request
                    .settings
                    .get(&format!("controls.{system}.{player}"))
                    .map_or("{}", String::as_str),
                request
                    .settings
                    .get(&format!(
                        "controls_migration.safe_special_buttons.{system}.{player}"
                    ))
                    .is_some_and(|s| s == "1"),
            )?;
            serde_json::from_str::<serde_json::Value>(&record).map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let work = tempfile::Builder::new()
        .prefix("emuluna-test-session-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let bios_source = request
        .settings
        .get("bios_directory")
        .map(PathBuf::from)
        .unwrap_or_else(|| request.root.join("system"));
    let bios = work.path().join("system");
    copy_bios(&bios_source, &bios, stop)?;
    let options_path = work.path().join("options.json");
    fs::write(
        &options_path,
        serde_json::to_vec(&options).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let controls_path = work.path().join("controls.json");
    fs::write(
        &controls_path,
        serde_json::to_vec(&profiles).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let player = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("emuluna-native-player");
    if !player.is_file() {
        return Err("Build the native player beside the native library first".into());
    }
    let host = crate::resources::path("build/cores/libemuluna_host.so");
    if !host.is_file() {
        return Err("The native core bridge has not been built yet".into());
    }
    // Game/core identities are validated by the resolver and state layer.
    if persistent
        && (request.game.id.len() != 64 || !request.game.id.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("Invalid game identity".into());
    }
    let native_media = |kind| {
        crate::storage::native_media(
            &request.root,
            &legacy_progress,
            &request.game.system,
            &request.game.id,
            &core.id,
            kind,
        )
    };
    let saves = if persistent {
        native_media("saves")
    } else {
        work.path().join("saves")
    };
    let mut command = Command::new(player);
    command
        .env("SDL_VIDEO_WAYLAND_WMCLASS", "emuluna")
        .env("SDL_VIDEO_X11_WMCLASS", "emuluna");
    let shader_prefix = format!("shader_parameters.{}.", request.game.system);
    let shader_resolution_key = format!("filter_resolution.{}", request.game.system);
    let shader_settings: BTreeMap<_, _> = request
        .settings
        .iter()
        .filter(|(key, _)| key.starts_with(&shader_prefix) || *key == &shader_resolution_key)
        .collect();
    let shader_settings_path = work.path().join("shader-settings.json");
    fs::write(
        &shader_settings_path,
        serde_json::to_vec(&shader_settings).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    command.env("EMULUNA_SHADER_SETTINGS_FILE", shader_settings_path);
    command.env(
        "EMULUNA_N64_AUTO_BORDERS",
        request
            .settings
            .get("n64.automatic_borders")
            .map_or("1", String::as_str),
    );
    command.env(
        "EMULUNA_N64_PRESERVE_CROP_ASPECT",
        request
            .settings
            .get("n64.preserve_crop_aspect")
            .map_or("0", String::as_str),
    );
    if request.frames.is_none() {
        command.env(
            "EMULUNA_FILTER_SELECTION_FILE",
            work.path().join("filter-selection"),
        );
        command.env(
            "EMULUNA_CONSOLE_OPTIONS_FILE",
            work.path().join("console-options.json"),
        );
        command.env("EMULUNA_CONSOLE_GAME_ID", &request.game.id);
        command.env(
            "EMULUNA_CONTROLS_REQUEST_FILE",
            work.path().join("controls-request"),
        );
    } else {
        command.env_remove("EMULUNA_FILTER_SELECTION_FILE");
    }
    command
        .arg(host)
        .arg(&core.path)
        .arg(&request.game.rom)
        .arg(&saves)
        .arg(bios)
        .arg(request.frames.unwrap_or(36000).to_string())
        .args([
            "--core-id",
            &core.id,
            "--catalog-id",
            catalog_id,
            "--system",
            &request.game.system,
            "--content-digest",
            &request.game.id,
        ])
        .arg("--options")
        .arg(options_path)
        .arg("--controls")
        .arg(controls_path)
        .args(["--overlay", "--theme", &request.theme]);
    if let Some(output) = request
        .settings
        .get("native.audio_output")
        .filter(|s| !s.is_empty())
    {
        command.arg("--audio-output").arg(output);
    }
    if let (Some(source), Some(identity)) = (&resume, &resume_identity) {
        let copied = work
            .path()
            .join("selected")
            .with_extension(emuluna_state_file::STATE_EXTENSION);
        // Validate and republish a private copy; never hand the original slot to
        // a core or write native progress back into the old application's files.
        emuluna_state_file::with_payload(source, identity, |raw| {
            emuluna_state_file::write_from_raw(&copied, raw, identity)
        })?;
        command.arg("--resume-file").arg(copied);
    }
    command.arg("--lock-aspect");
    let volume = request
        .settings
        .get("volume")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(80)
        .min(100);
    command.args(["--volume", &(volume as f32 / 100.).to_string()]);
    for (key, flag, default) in [
        ("fullscreen_default", "--fullscreen", false),
        ("pause_unfocused", "--pause-unfocused", true),
        ("integer_scale", "--integer-scale", false),
    ] {
        if request.settings.get(key).map_or(default, |s| s != "0")
            && !(request.frames.is_some() && matches!(flag, "--pause-unfocused" | "--fullscreen"))
        {
            command.arg(flag);
        }
    }
    if request
        .settings
        .get("aspect_mode")
        .is_some_and(|s| s == "square")
    {
        command.arg("--square-pixels");
    }
    if request
        .settings
        .get("hide_game_cursor")
        .is_some_and(|s| s == "0")
    {
        command.arg("--keep-cursor");
    }
    if request
        .settings
        .get("game_mode.keep_awake")
        .is_some_and(|s| s == "0")
    {
        command.arg("--allow-screensaver");
    }
    command.arg("--parent-control");
    if request.start_from_boot {
        command.arg("--start-from-boot");
    }
    if persistent {
        command
            .arg("--screenshot-dir")
            .arg(native_media("screenshots"));
        command
            .arg("--persistent")
            .arg("--state-dir")
            .arg(native_media("states").join(&core.sha256))
            .arg("--autosave-dir")
            .arg(crate::storage::native_media(
                &request.root,
                &legacy_progress,
                &request.game.system,
                &request.game.id,
                "autosave",
                "states",
            ));
        if autosave_enabled(&request.settings, &request.game.id) {
            command.arg("--auto-resume");
        }
    }
    command.args(["--title", &request.game.title]);
    command.arg("--theme-dir").arg(&request.theme_root);
    if request.frames.is_some() {
        command.arg("--mute");
    } else {
        command.arg("--interactive");
    }
    if gl {
        command.arg("--hardware-gl");
    }
    let shaders = crate::resources::path("build/rust/shader-pack");
    if request.preset != "nearest" || shaders.join("pack.json").is_file() {
        if !shaders.is_dir() {
            return Err("Export the native screen-filter pack before testing filters".into());
        }
        command
            .arg("--shader-pack")
            .arg(shaders)
            .args(["--preset", &request.preset]);
    }
    // Developer diagnostics must be explicitly requested by a test runner, not
    // accidentally inherited from an earlier terminal benchmark.
    for key in [
        "EMULUNA_TEST_INPUT",
        "EMULUNA_TEST_CAPTURE",
        "EMULUNA_TEST_FRAME_CAPTURE_DIR",
        "EMULUNA_TEST_UI_STALL_MS",
        "EMULUNA_TEST_FULLSCREEN",
    ] {
        command.env_remove(key);
    }
    command.stdin(Stdio::null()).current_dir(work.path());
    Ok(Plan { work, command })
}
fn is_1080(path: &Path, title: &str) -> bool {
    let mut header = [0_u8; 64];
    if File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_ok()
    {
        match &header[..4] {
            [0x37, 0x80, 0x40, 0x12] => {
                for chunk in header.as_chunks_mut::<2>().0 {
                    chunk.reverse();
                }
            }
            [0x40, 0x12, 0x37, 0x80] => {
                for chunk in header.as_chunks_mut::<4>().0 {
                    chunk.reverse();
                }
            }
            _ => {}
        }
        if header[..4] == [0x80, 0x37, 0x12, 0x40]
            && std::str::from_utf8(&header[32..52])
                .is_ok_and(|s| s.trim_matches([' ', '\0']) == "1080 SNOWBOARDING")
        {
            return true;
        }
    }
    let title = title.trim_start().to_lowercase();
    title
        .strip_prefix("1080")
        .map(|s| s.trim_start_matches(['°', 'º']).trim_start())
        .is_some_and(|s| {
            s.starts_with("snowboarding")
                && s["snowboarding".len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric())
        })
}
fn saved_options(
    definition: &serde_json::Value,
    catalog_id: &str,
    system: &str,
    game_id: &str,
    settings: &BTreeMap<String, String>,
    installed_id: &str,
) -> BTreeMap<String, String> {
    let mut options = BTreeMap::new();
    if let Some(definitions) = definition["runtime_options"][system].as_array() {
        for item in definitions {
            let Some(key) = item["key"].as_str() else {
                continue;
            };
            if let Some(value) = settings
                .get(&format!("core_option.{}.{}.{}", system, catalog_id, key))
                .filter(|v| {
                    item["enabled"].as_str() == Some(v.as_str())
                        || item["disabled"].as_str() == Some(v.as_str())
                        || item["choices"].as_array().is_some_and(|choices| {
                            choices
                                .iter()
                                .any(|choice| choice[0].as_str() == Some(v.as_str()))
                        })
                })
            {
                options.insert(key.into(), value.clone());
            }
        }
    }
    if let Some(accessories) = definition["accessories"].as_array() {
        for item in accessories {
            let Some(key) = item["key"].as_str() else {
                continue;
            };
            let count = item["count"].as_u64().unwrap_or(1).clamp(1, 4);
            for player in 1..=count {
                let key = key.replace("{player}", &player.to_string());
                if let Some(value) =
                    settings.get(&format!("core_option.{}.{}.{}", game_id, installed_id, key))
                {
                    if item["choices"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v[0].as_str() == Some(value)))
                    {
                        options.insert(key, value.clone());
                    }
                }
            }
        }
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_request_is_forwarded_to_the_library() {
        let mut launcher = Launcher::new(egui::Context::default());
        launcher
            .sender
            .send(Update::Controls("n64".into()))
            .unwrap();
        launcher.poll();
        assert_eq!(launcher.controls_request.as_deref(), Some("n64"));
    }
    #[test]
    fn snowboarding_compatibility_identifies_all_n64_byte_orders() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("game.n64");
        let mut header = [0_u8; 64];
        header[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        header[32..52].copy_from_slice(b"1080 SNOWBOARDING   ");
        for size in [1, 2, 4] {
            let mut bytes = header;
            if size != 1 {
                for chunk in bytes.chunks_exact_mut(size) {
                    chunk.reverse();
                }
            }
            fs::write(&file, bytes).unwrap();
            assert!(is_1080(&file, "Unrelated name"));
        }
        assert!(is_1080(&file, "1080° Snowboarding (USA)"));
        fs::write(&file, [0_u8; 64]).unwrap();
        assert!(!is_1080(&file, "Another N64 game"));
    }
    #[test]
    fn sega_lowpass_switches_reach_each_supported_core_and_console() {
        let catalog: serde_json::Value = serde_json::from_str(CATALOG).unwrap();
        for (core, system, key, default) in [
            (
                "genesis_plus_gx",
                "genesis",
                "genesis_plus_gx_audio_filter",
                "low-pass",
            ),
            (
                "genesis_plus_gx",
                "segacd",
                "genesis_plus_gx_audio_filter",
                "low-pass",
            ),
            ("picodrive", "genesis", "picodrive_audio_filter", "low-pass"),
            ("picodrive", "segacd", "picodrive_audio_filter", "low-pass"),
            ("picodrive", "sega32x", "picodrive_audio_filter", "low-pass"),
        ] {
            let options = emuluna_launch_config::launch_options(
                core,
                system,
                emuluna_launch_config::Renderer::Software,
                &BTreeMap::new(),
            )
            .unwrap();
            assert_eq!(options[key], default);
            assert_eq!(catalog[core]["runtime_options"][system][0]["key"], key);
            for choice in ["low-pass", "disabled"] {
                let settings =
                    BTreeMap::from([(format!("core_option.{system}.{core}.{key}"), choice.into())]);
                let overrides = saved_options(
                    &catalog[core],
                    core,
                    system,
                    "game",
                    &settings,
                    "imported-build",
                );
                let options = emuluna_launch_config::launch_options(
                    core,
                    system,
                    emuluna_launch_config::Renderer::Software,
                    &overrides,
                )
                .unwrap();
                assert_eq!(options[key], choice);
            }
        }
    }
    #[test]
    fn bios_test_copy_is_independent_and_rejects_symbolic_links() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("original");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("bios.bin"), b"original bytes").unwrap();
        fs::create_dir(source.join("dc")).unwrap();
        fs::write(source.join("dc/nvram.bin"), b"existing nvram").unwrap();
        let target = root.path().join("test");
        let stop = AtomicBool::new(false);
        copy_bios(&source, &target, &stop).unwrap();
        fs::write(target.join("bios.bin"), b"core wrote this").unwrap();
        fs::write(target.join("dc/nvram.bin"), b"core wrote this too").unwrap();
        assert_eq!(
            fs::read(source.join("bios.bin")).unwrap(),
            b"original bytes"
        );
        assert_eq!(
            fs::read(source.join("dc/nvram.bin")).unwrap(),
            b"existing nvram"
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(source.join("bios.bin"), source.join("link.bin")).unwrap();
            assert!(
                copy_bios(&source, &root.path().join("rejected"), &stop)
                    .unwrap_err()
                    .contains("symbolic link")
            );
        }
        let stopped = AtomicBool::new(true);
        assert!(
            copy_bios(&source, &root.path().join("cancelled"), &stopped)
                .unwrap_err()
                .contains("cancelled")
        );
    }
    #[test]
    fn settings_handoff_accepts_only_catalog_choices_and_correct_identity() {
        let definition = serde_json::json!({"runtime_options":{"nes":[{"key":"filter","enabled":"on","disabled":"off","default":"off"}]},"accessories":[{"key":"pak{player}","count":2,"choices":[["memory","Memory"],["rumble","Rumble"]]}]});
        let settings = BTreeMap::from([
            ("core_option.nes.nestopia.filter".into(), "on".into()),
            ("core_option.game.imported.pak1".into(), "memory".into()),
            (
                "core_option.game.imported.pak2".into(),
                "unsupported".into(),
            ),
            ("core_option.game.nestopia.pak2".into(), "rumble".into()),
        ]);
        let options = saved_options(
            &definition,
            "nestopia",
            "nes",
            "game",
            &settings,
            "imported",
        );
        assert_eq!(
            options,
            BTreeMap::from([
                ("filter".into(), "on".into()),
                ("pak1".into(), "memory".into())
            ])
        );
        let mut broken = settings;
        broken.insert("core_option.nes.nestopia.filter".into(), "invalid".into());
        assert!(
            !saved_options(&definition, "nestopia", "nes", "game", &broken, "imported")
                .contains_key("filter")
        );
    }
    #[test]
    fn parallel_n64_ignores_saved_upscaling() {
        let catalog: serde_json::Value = serde_json::from_str(CATALOG).unwrap();
        let key = "parallel-n64-upscaling";
        let saved = BTreeMap::from([(format!("core_option.n64.parallel_n64.{key}"), "4x".into())]);
        let options = saved_options(
            &catalog["parallel_n64"],
            "parallel_n64",
            "n64",
            "game",
            &saved,
            "parallel_n64",
        );
        assert!(!options.contains_key(key));
        assert_eq!(catalog["parallel_n64"]["options"][key], "1x");
    }
}
