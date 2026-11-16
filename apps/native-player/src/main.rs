#![forbid(unsafe_code)]
//! Software-core vertical slice. SDL is a temporary diagnostic presenter, not the final egui UI.
use emuluna_audio_device::Device;
use emuluna_gl_presenter::{Filter, Presenter};
use emuluna_libretro_host::{Core, Frame};
use sdl2::{event::Event, keyboard::Keycode};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Default)]
struct WorkerStatistics {
    dropped: u64,
    executed: u64,
    run_total: Duration,
    run_peak: Duration,
    audio_wait: Duration,
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn take_argument(
    args: &mut Vec<std::ffi::OsString>,
    flag: &str,
) -> Result<Option<std::ffi::OsString>, String> {
    let Some(index) = args.iter().position(|s| s == flag) else {
        return Ok(None);
    };
    if index + 1 == args.len() {
        return Err(format!("{flag} needs a value"));
    }
    let value = args.remove(index + 1);
    args.remove(index);
    if args.iter().any(|s| s == flag) {
        return Err(format!("Repeated {flag}"));
    }
    Ok(Some(value))
}
fn run() -> Result<(), String> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let volume = take_argument(&mut args, "--volume")?
        .map(|s| {
            s.to_str()
                .ok_or("Invalid volume")
                .and_then(|s| s.parse::<f32>().map_err(|_| "Invalid volume"))
        })
        .transpose()?
        .unwrap_or(1.);
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err("Volume must be between zero and one".into());
    }
    let window_preferences = emuluna_gl_presenter::WindowPreferences {
        lock_aspect: args.iter().any(|s| s == "--lock-aspect"),
        integer_scaling: args.iter().any(|s| s == "--integer-scale"),
        square_pixels: args.iter().any(|s| s == "--square-pixels"),
        pause_unfocused: args.iter().any(|s| s == "--pause-unfocused"),
        hide_cursor: !args.iter().any(|s| s == "--keep-cursor"),
        keep_awake: !args.iter().any(|s| s == "--allow-screensaver"),
        volume,
    };
    let fullscreen = args.iter().any(|s| s == "--fullscreen");
    args.retain(|s| {
        ![
            "--lock-aspect",
            "--integer-scale",
            "--square-pixels",
            "--pause-unfocused",
            "--keep-cursor",
            "--allow-screensaver",
            "--fullscreen",
        ]
        .iter()
        .any(|flag| s == flag)
    });
    let screenshot_directory = take_argument(&mut args, "--screenshot-dir")?.map(PathBuf::from);
    let resume_file = take_argument(&mut args, "--resume-file")?.map(PathBuf::from);
    let autosave_directory = take_argument(&mut args, "--autosave-dir")?.map(PathBuf::from);
    let state_directory = take_argument(&mut args, "--state-dir")?.map(PathBuf::from);
    let start_from_boot = args.iter().any(|s| s == "--start-from-boot");
    let persistent = args.iter().any(|s| s == "--persistent");
    let auto_resume = args.iter().any(|s| s == "--auto-resume");
    let parent_control = args.iter().any(|s| s == "--parent-control");
    let audio_output = take_argument(&mut args, "--audio-output")?
        .map(|s| s.into_string().map_err(|_| "Invalid audio output"))
        .transpose()?;
    let title = take_argument(&mut args, "--title")?;
    let shader_root = take_argument(&mut args, "--shader-pack")?;
    let shader_preset = take_argument(&mut args, "--preset")?;
    let core_id = take_argument(&mut args, "--core-id")?;
    let catalog_id = take_argument(&mut args, "--catalog-id")?;
    let interactive = args.iter().any(|s| s == "--interactive");
    let identity_core = core_id
        .as_ref()
        .map(|id| id.to_str().map(str::to_owned).ok_or("Invalid core ID"))
        .transpose()?;
    let content_digest = take_argument(&mut args, "--content-digest")?;
    let system = take_argument(&mut args, "--system")?;
    let input_system = system
        .as_ref()
        .map(|id| id.to_str().map(str::to_owned).ok_or("Invalid input system"))
        .transpose()?;
    let options_file = take_argument(&mut args, "--options")?;
    let controls_file = take_argument(&mut args, "--controls")?;
    let controls = if let Some(path) = controls_file {
        let metadata =
            std::fs::metadata(&path).map_err(|e| format!("Could not read controls: {e}"))?;
        if !metadata.is_file()
            || metadata.len() > (65536 * emuluna_native_input::MAX_PLAYERS + 5) as u64
        {
            return Err("Controller profile exceeds its file limit".into());
        }
        if input_system.is_none() {
            return Err("Saved controls require a console launch profile".into());
        }
        Some(std::fs::read_to_string(path).map_err(|e| format!("Could not read controls: {e}"))?)
    } else {
        None
    };
    let overlay = args.iter().any(|s| s == "--overlay");
    let theme = take_argument(&mut args, "--theme")?;
    let theme_dir = take_argument(&mut args, "--theme-dir")?;
    let muted = args.iter().any(|s| s == "--mute");
    let hardware = args.iter().any(|s| s == "--hardware-gl");
    args.retain(|s| {
        s != "--mute"
            && s != "--hardware-gl"
            && s != "--overlay"
            && s != "--interactive"
            && s != "--persistent"
            && s != "--auto-resume"
            && s != "--parent-control"
            && s != "--start-from-boot"
    });
    if args.len() != 6 {
        return Err("Usage: emuluna-native-player HOST_SO CORE_SO ROM NEW_SAVE_DIR BIOS_DIR FRAMES [--mute] [--hardware-gl] [--core-id ID --system ID] [--options FILE.json] [--shader-pack DIR --preset ID]".into());
    }
    let paths: Vec<_> = args[..5].iter().map(PathBuf::from).collect();
    let mut frames: u32 = args[5]
        .to_str()
        .ok_or("Invalid frames")?
        .parse()
        .map_err(|_| "Invalid frames")?;
    if frames == 0 || frames > 36000 {
        return Err("Frame limit must be 1..36000".into());
    }
    if interactive {
        frames = u32::MAX;
    }
    if catalog_id.is_some() && core_id.is_none() {
        return Err("A catalog identity requires a core identity".into());
    }
    use emuluna_launch_config::{Options, Renderer};
    let overrides = match options_file {
        Some(path) => emuluna_launch_config::parse_options(
            &std::fs::read_to_string(path)
                .map_err(|e| format!("Could not read core options: {e}"))?,
        )?,
        None => Options::new(),
    };
    let options = match (core_id, system) {
        (None, None) => overrides,
        (Some(core), Some(system)) => emuluna_launch_config::launch_options(
            catalog_id
                .as_ref()
                .unwrap_or(&core)
                .to_str()
                .ok_or("Invalid catalog ID")?,
            system.to_str().ok_or("Invalid system ID")?,
            if hardware {
                Renderer::OpenGl
            } else {
                Renderer::Software
            },
            &overrides,
        )?,
        _ => return Err("Supply --core-id and --system together".into()),
    };
    let options: Vec<_> = options.into_iter().collect();
    let state_identity = if let Some(id) = identity_core.as_ref() {
        use emuluna_state_file::{digest_file, Identity, Renderer};
        let digest = match content_digest {
            Some(digest) => digest.into_string().map_err(|_| "Invalid content digest")?,
            None => digest_file(&paths[2])?,
        };
        Some(Identity::new(
            id,
            &digest_file(&paths[1])?,
            &digest,
            if hardware {
                Renderer::OpenGl
            } else {
                Renderer::Software
            },
        )?)
    } else {
        if content_digest.is_some() {
            return Err("A content digest requires a core/console launch profile".into());
        }
        None
    };
    if start_from_boot && resume_file.is_some() {
        return Err("Choose a saved state or start from boot".into());
    }
    if resume_file.is_some() && state_identity.is_none() {
        return Err("Resuming a state requires a core and game identity".into());
    }
    if auto_resume && (!persistent || state_identity.is_none()) {
        return Err("Automatic resume requires persistent saves and a core identity".into());
    }
    let _save_session = if persistent {
        Some(emuluna_state_file::SaveSession::acquire(&paths[3])?)
    } else {
        None
    };
    if let Some(path) = &state_directory {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    }
    let state_dir = state_directory.as_ref().unwrap_or(&paths[3]);
    let autosave_dir = autosave_directory.as_ref().unwrap_or(state_dir);
    if autosave_directory.is_some() {
        std::fs::create_dir_all(autosave_dir).map_err(|e| e.to_string())?;
    }
    let automatic = emuluna_state_file::automatic_path(autosave_dir);
    let legacy = emuluna_state_file::automatic_path(state_dir);
    let candidate = emuluna_state_file::readable_path(&automatic);
    let candidate = if candidate.is_file() {
        candidate
    } else {
        emuluna_state_file::readable_path(&legacy)
    };
    let initial_state = resume_file
        .as_ref()
        .or_else(|| {
            (auto_resume
                && !start_from_boot
                && emuluna_state_file::inspect(&candidate).ok().as_ref() == state_identity.as_ref())
            .then_some(&candidate)
        })
        .map(|path| {
            emuluna_state_file::prepare(
                path,
                state_identity.as_ref().ok_or("State identity is missing")?,
            )
        })
        .transpose()?;
    let sdl = sdl2::init()?;
    if parent_control {
        let sender = sdl.event()?.event_sender();
        thread::spawn(move || {
            use std::io::Read;
            let _ = std::io::stdin().read(&mut [0u8; 1]);
            let _ = sender.push_event(Event::Quit { timestamp: 0 });
        });
    }
    let video = sdl.video()?;
    let audio = sdl.audio()?;
    let mut presenter = Presenter::new(&video)?;
    if let Some(title) = title {
        presenter.set_title(title.to_str().ok_or("Invalid window title")?)?;
    }
    presenter.configure_window(window_preferences);
    if overlay {
        let id = theme
            .as_ref()
            .map(|s| s.to_str().ok_or("Invalid theme"))
            .transpose()?
            .unwrap_or("luna");
        if let Some(root) = theme_dir {
            presenter.enable_overlay_from(std::path::Path::new(&root), id)?;
        } else {
            presenter.enable_overlay(id)?;
        }
        if let (Some(system), Some(core)) = (input_system.as_deref(), identity_core.as_deref()) {
            let catalog = catalog_id.as_ref().and_then(|s| s.to_str()).unwrap_or(core);
            let game = std::env::var("EMULUNA_CONSOLE_GAME_ID").unwrap_or_default();
            presenter.configure_console_options(system, catalog, core, &game, &options);
        }
    }
    if input_system.as_deref() == Some("n64") {
        presenter.configure_n64_crop_aspect(
            std::env::var("EMULUNA_N64_PRESERVE_CROP_ASPECT").is_ok_and(|value| value == "1"),
        );
        if let Err(error) = presenter.configure_n64_auto_crop(
            std::env::var("EMULUNA_N64_AUTO_BORDERS").map_or(true, |v| v != "0"),
        ) {
            eprintln!("Automatic N64 screen adjustment unavailable: {error}");
        }
    }

    if let Some(path) = screenshot_directory {
        presenter.configure_screenshots(path);
    }
    if let Some(system) = input_system.as_deref() {
        presenter.configure_shader_settings(system);
    }
    let initial_filter = match std::env::var("EMULUNA_TEST_FILTER").as_deref() {
        Ok("linear") => Filter::Linear,
        Ok("scanlines") => Filter::Scanlines,
        _ => Filter::Nearest,
    };
    presenter.set_filter(initial_filter);
    match (shader_root, shader_preset) {
        (Some(root), Some(key)) => {
            presenter.configure_overlay_filters(std::path::Path::new(&root))?;
            presenter.select_preset(
                std::path::Path::new(&root),
                key.to_str().ok_or("Invalid preset ID")?,
            )?;
        }
        (None, None) => {}
        _ => return Err("Supply --shader-pack and --preset together".into()),
    }
    eprintln!("preset={:?}", presenter.preset_key());
    if fullscreen || std::env::var_os("EMULUNA_TEST_FULLSCREEN").is_some() {
        presenter.toggle_fullscreen()?;
    }
    eprintln!(
        "audio_driver={} filter={initial_filter:?}; F1 cycles filters; F11 fullscreen",
        audio.current_audio_driver()
    );
    if hardware {
        let paths: [PathBuf; 5] = paths.try_into().map_err(|_| "Invalid paths")?;
        let (stats, report) = presenter.hardware_playback_configured(
            paths,
            emuluna_gl_presenter::PlaybackOptions {
                frames,
                interactive,
                muted,
                audio_output,
                persistent,
                state_directory,
                automatic_path: Some(automatic),
                initial_state,
                start_from_boot,
                auto_resume,
                core_options: options,
                input_system,
                state_identity,
                controls,
            },
        )?;
        println!("displayed={} video_dropped={} audio_missing_frames={} audio_overflow_frames={} audio_accepted_frames={} audio_played_frames={} audio_pending_frames={} audio_tail_silence_frames={} audio_discarded_frames={} cpu_video_bytes={} core_gl_diagnostics={}",stats.presented,stats.dropped,report.missing,report.overflow,report.accepted,report.played,report.pending,report.tail_silence,report.discarded,stats.cpu_video_bytes,stats.core_gl_diagnostics);
        println!("core_time_ms={:.3} peak_core_ms={:.3} gpu_snapshot_calls_ms={:.3} audio_backpressure_ms={:.3}",stats.core_time.as_secs_f64()*1000.,stats.peak_core_time.as_secs_f64()*1000.,stats.snapshot_time.as_secs_f64()*1000.,stats.audio_wait.as_secs_f64()*1000.);
        return Ok(());
    }
    if !persistent {
        std::fs::create_dir(&paths[3]).map_err(|e| format!("Save directory must be new: {e}"))?;
    }
    let scripted_input = emuluna_session::input_script::InputScript::parse(
        &std::env::var("EMULUNA_TEST_INPUT").unwrap_or_default(),
    )?;
    let (mut device, sender) = Device::open_selected(&audio, audio_output.as_deref())?;
    let stop = Arc::new(AtomicBool::new(false));
    let mut input = emuluna_native_input::Players::new(
        &sdl,
        input_system.as_deref(),
        presenter.window_id(),
        controls.as_deref(),
    )?;
    let (mut input_publishers, mut input_readers): (Vec<_>, Vec<_>) = (0..input.port_count())
        .map(|_| emuluna_session::input::channel())
        .unzip();
    let (send, receive) = mpsc::sync_channel(1);
    let (recycle, recycled) = mpsc::sync_channel(2);
    let thread_stop = stop.clone();
    let (mut commands, mut control_worker) = emuluna_playback_control::channel();
    commands.set_volume(volume)?;
    if persistent && state_identity.is_some() {
        commands.configure_autosave(auto_resume);
    }
    if state_identity.is_some() {
        let directory = state_directory.as_ref().unwrap_or(&paths[3]);
        commands.configure_state_slot(if persistent {
            emuluna_state_file::slot_path(directory, 1)
        } else {
            directory.join("quick.state")
        });
    }
    let mut events = sdl.event_pump()?;
    let auto_preview = commands
        .autosave()
        .is_some()
        .then(|| automatic.with_extension("png"));
    let worker = thread::spawn(move || -> Result<WorkerStatistics, String> {
        let paths: [PathBuf; 5] = paths.try_into().map_err(|_| "Invalid paths")?;
        let mut core = Core::open_with_options(&paths, &options)?;
        if let Some(state) = initial_state {
            core.initialize_for_state()?;
            core.load_raw_state(state.raw_path())?;
            eprintln!("Native automatic state restored");
        }
        let mut sender = sender;
        let mut pacer = emuluna_session::pacing::Pacer::new(Instant::now());
        let mut timings = WorkerStatistics::default();
        let mut rate_control = emuluna_session::audio_clock::AudioRateControl::new(2400.);
        let epoch = Instant::now();
        let trace = std::env::var_os("EMULUNA_TRACE").is_some();
        let mut spare = Frame::default();
        for index in 0..frames {
            if thread_stop.load(Ordering::Acquire) {
                break;
            }
            let update =
                control_worker.service_operations(Some(&mut sender), &thread_stop, |command| {
                    use emuluna_session::Command;
                    match command {
                        Command::Reset => {
                            core.reset();
                            Ok(())
                        }
                        Command::CoreOption(key, value) => core.set_runtime_option(key, value),
                        Command::SaveState(path) => core.save_state(
                            path,
                            state_identity.as_ref().ok_or("State identity is missing")?,
                        ),
                        Command::LoadState(path) => core.load_state(
                            path,
                            state_identity.as_ref().ok_or("State identity is missing")?,
                        ),
                        _ => Err("Unsupported core operation".into()),
                    }
                })?;
            if !update.running {
                break;
            }
            if update.reset_clock {
                pacer = emuluna_session::pacing::Pacer::new(Instant::now());
                rate_control.reset();
            }
            let mut frame = recycled
                .try_recv()
                .unwrap_or_else(|_| std::mem::take(&mut spare));
            let ratio = rate_control.update(
                sender.queued() as f64,
                Duration::from_secs_f64(1. / core.fps()?),
            );
            core.set_audio_rate_adjustment(ratio);
            let run_started = Instant::now();
            for (port, reader) in input_readers.iter_mut().enumerate().skip(1) {
                let state = reader.latest();
                core.set_port_input(port as u32, state.buttons, state.axes);
            }
            let mut state = input_readers[0].latest();
            state.buttons |= scripted_input.buttons(index);
            core.set_analog(state.axes);
            if core.run_into(state.buttons, &mut frame)?
                == emuluna_libretro_host::FrameOutcome::Shutdown
            {
                break;
            }
            if index == 0 {
                eprintln!("Native playback started");
            }
            frame.generation = update.generation;
            let elapsed = run_started.elapsed();
            timings.executed += 1;
            timings.run_total += elapsed;
            timings.run_peak = timings.run_peak.max(elapsed);
            if muted {
                frame.audio.fill(0);
            }
            let timing = match sender.write(&frame.audio, &thread_stop) {
                Ok(timing) => timing,
                Err(_) if thread_stop.load(Ordering::Acquire) => break,
                Err(error) => return Err(error),
            };
            timings.audio_wait += timing.waited;
            if trace && index % 60 == 0 {
                eprintln!(
                    "audio_trace frame={index} t={:.3} accepted={} played={} queued={} overflow={} video={}x{} nonblack_samples={}",
                    epoch.elapsed().as_secs_f64(),
                    sender.statistics().accepted.load(Ordering::Relaxed),
                    sender.statistics().played.load(Ordering::Relaxed),
                    sender.queued(),
                    sender.statistics().overflow.load(Ordering::Relaxed),frame.width,frame.height,
                    frame.pixels.as_chunks::<4>().0.iter().step_by(32).filter(|pixel|pixel[0]|pixel[1]|pixel[2]!=0).count()
                );
            }
            // UI backpressure must never block emulation or audio production.
            match send.try_send(frame) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(frame)) => {
                    spare = frame;
                    timings.dropped += 1;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => break,
            }
            let wait = pacer.advance(
                Instant::now(),
                Duration::from_secs_f64(1. / core.fps()?),
                timing.waited,
                timing.clock_started,
            );
            if !wait.is_zero() {
                thread::sleep(wait);
            }
        }
        // Mark audio EOS before potentially slow state and battery writes.
        drop(sender);
        if control_worker.autosave_enabled() && timings.executed > 0 {
            core.save_state(
                &automatic,
                state_identity.as_ref().ok_or("State identity is missing")?,
            )?;
            eprintln!("Native automatic state saved");
        }
        Ok(timings)
    });
    let mut displayed = 0u64;
    let mut upload_total = Duration::ZERO;
    let mut present_total = Duration::ZERO;
    let mut present_peak = Duration::ZERO;
    let ui_started = Instant::now();
    let mut stalled = false;
    let stall_ms = std::env::var("EMULUNA_TEST_UI_STALL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
        .min(1000);
    let frame_capture_directory =
        std::env::var_os("EMULUNA_TEST_FRAME_CAPTURE_DIR").map(PathBuf::from);
    if frame_capture_directory
        .as_ref()
        .is_some_and(|path| !path.is_dir())
    {
        stop.store(true, Ordering::Release);
        drop(receive);
        let _ = worker.join();
        return Err("Diagnostic capture folder must already exist".into());
    }
    let presentation = (|| -> Result<(), String> {
        loop {
            for message in commands.messages() {
                if let Some(path) = &message.preview {
                    if let Err(error) = presenter.save_game_preview(path) {
                        presenter.overlay_message(format!(
                            "State saved, but its preview failed: {error}"
                        ));
                    }
                }
                presenter.overlay_message(message.text.clone());
                eprintln!(
                    "playback_message success={}: {}",
                    message.success, message.text
                );
            }
            if !stalled && stall_ms > 0 && ui_started.elapsed() > Duration::from_secs(1) {
                stalled = true;
                thread::sleep(Duration::from_millis(stall_ms));
            }
            for event in events.poll_iter() {
                presenter.window_event(&event, &mut commands)?;
                presenter.overlay_event(&event);
                if let Some(message) = device.handle_event(&event) {
                    presenter.overlay_message(message);
                }
                input.event(&event);
                if let Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } = event
                {
                    // A gameplay binding owns its key. Keep Escape and Ctrl shortcuts available.
                    if key != Keycode::Escape
                        && !keymod.intersects(
                            sdl2::keyboard::Mod::LCTRLMOD | sdl2::keyboard::Mod::RCTRLMOD,
                        )
                        && input.binds_key(key)
                    {
                        continue;
                    }
                }
                if presenter.intercept_overlay_shortcut(&event) {
                    continue;
                }
                if let Err(error) = commands.handle_event(&event) {
                    eprintln!("playback_message success=false: {error}");
                }
                match event {
                    Event::Quit { .. }
                    | Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => return Ok(()),
                    Event::KeyDown {
                        keycode: Some(Keycode::F11),
                        repeat: false,
                        ..
                    } => {
                        presenter.toggle_fullscreen()?;
                    }
                    Event::KeyDown {
                        keycode: Some(Keycode::F1),
                        repeat: false,
                        ..
                    } => {
                        presenter.set_filter(presenter.filter().next());
                        eprintln!("filter={:?}", presenter.filter());
                    }
                    _ => {}
                }
            }
            presenter.service_audio(&mut device);
            if presenter.service_overlay(&mut commands)? {
                return Ok(());
            }
            for (port, publisher) in input_publishers.iter_mut().enumerate() {
                publisher.publish(input.snapshot(port));
            }
            match receive.try_recv() {
                Ok(frame) => {
                    if frame.generation != commands.generation() {
                        let _ = recycle.try_send(frame);
                        continue;
                    }
                    let capture = frame_capture_directory
                        .as_ref()
                        .filter(|_| frame.sequence % 300 == 0)
                        .map(|dir| dir.join(format!("frame-{:06}.ppm", frame.sequence)))
                        .or_else(|| {
                            if frame.sequence + 1 == frames as u64 {
                                std::env::var_os("EMULUNA_TEST_CAPTURE").map(PathBuf::from)
                            } else {
                                None
                            }
                        });
                    let timing = if let Some(path) = capture {
                        presenter.draw_frame_captured(&frame, &path)?
                    } else {
                        presenter.draw_frame(&frame)?
                    };
                    upload_total += timing.upload;
                    present_total += timing.present;
                    present_peak = present_peak.max(timing.present);
                    displayed += 1;
                    let _ = recycle.try_send(frame);
                }
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {
                    if presenter.overlay_needs_redraw() {
                        presenter.redraw()?;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
        Ok(())
    })();
    let preview = if presentation.is_ok() && commands.autosave() == Some(true) {
        presenter.capture_game_preview().ok().flatten()
    } else {
        None
    };
    stop.store(true, Ordering::Release);
    drop(receive);
    let result = worker.join().map_err(|_| "Core worker panicked")?;
    let report = device.finish(presentation.is_ok() && result.is_ok());
    if result.is_ok() {
        if let (Some(path), Some(preview)) = (&auto_preview, preview) {
            if let Err(error) = preview.save(path) {
                eprintln!("State saved, but its preview failed: {error}");
            }
        }
    }
    presentation?;
    let timings = result?;
    let dropped = timings.dropped;
    let (output_width, output_height) = presenter.output_size();
    eprintln!(
        "output_width={output_width} output_height={output_height} filter={:?}",
        presenter.filter()
    );
    println!("displayed={displayed} video_dropped={dropped} audio_missing_frames={} audio_overflow_frames={} audio_accepted_frames={} audio_played_frames={} audio_pending_frames={} audio_tail_silence_frames={} audio_discarded_frames={}",report.missing,report.overflow,report.accepted,report.played,report.pending,report.tail_silence,report.discarded);
    println!("executed={} mean_core_snapshot_ms={:.3} peak_core_snapshot_ms={:.3} audio_backpressure_ms={:.3} mean_upload_call_ms={:.3} mean_present_call_ms={:.3} peak_present_call_ms={:.3}",
        timings.executed,
        timings.run_total.as_secs_f64() * 1000. / timings.executed.max(1) as f64,
        timings.run_peak.as_secs_f64() * 1000.,
        timings.audio_wait.as_secs_f64() * 1000.,
        upload_total.as_secs_f64() * 1000. / displayed.max(1) as f64,
        present_total.as_secs_f64() * 1000. / displayed.max(1) as f64,
        present_peak.as_secs_f64() * 1000.);
    Ok(())
}
