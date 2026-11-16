//! Diagnostic proof of two-context GPU frame ownership. Not a libretro backend.
use super::*;
mod core_source;
use core_source::CoreSource;
use sdl2::{event::Event, keyboard::Keycode};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::{ffi::CString, ptr, sync::mpsc, thread};

#[derive(Debug, Default)]
pub struct SharedProbeStatistics {
    pub produced: u32,
    pub presented: u32,
    pub dropped: u32,
    pub audio_frames: u64,
    pub core_gl_diagnostics: u64,
    pub cpu_video_bytes: u64,
    pub source_width: u32,
    pub source_height: u32,
    pub core_time: Duration,
    pub peak_core_time: Duration,
    pub snapshot_time: Duration,
    pub audio_wait: Duration,
}
// Created detached and moved exactly once. The parent pins a cloned Window on
// the window thread until the scoped worker has joined. No SDL window operations
// run on the worker except binding/unbinding this compatible context.
struct DetachedContext {
    window: *mut sdl2::sys::SDL_Window,
    context: sdl2::sys::SDL_GLContext,
}
// SAFETY: private construction enforces the scope/window lifetime and detachment
// contract above. The context has one owner and is never concurrently current.
unsafe impl Send for DetachedContext {}
impl Drop for DetachedContext {
    fn drop(&mut self) {
        unsafe {
            if sdl2::sys::SDL_GL_GetCurrentContext() == self.context {
                sdl2::sys::SDL_GL_MakeCurrent(self.window, ptr::null_mut());
            }
            sdl2::sys::SDL_GL_DeleteContext(self.context);
        }
    }
}
// GLsync is share-group scoped. Transfer ownership as an opaque integer; only
// the receiving context waits/deletes it. This type is private and not Clone.
struct Fence(usize);
impl Fence {
    unsafe fn create(gl: &glow::Context) -> Result<Self, String> {
        let fence = gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0)?;
        if fence.0.is_null() {
            return Err("Driver returned an empty GPU fence".into());
        }
        gl.flush();
        Ok(Self(fence.0 as usize))
    }
    unsafe fn consume(self, gl: &glow::Context) {
        let fence = glow::NativeFence(self.0 as *mut _);
        gl.wait_sync(fence, 0, glow::TIMEOUT_IGNORED);
        gl.delete_sync(fence);
    }
}
struct Ready {
    slot: usize,
    fence: Option<Fence>,
    sequence: u32,
    generation: u64,
    width: u32,
    height: u32,
    aspect: f64,
    top_left: bool,
    releases: mpsc::SyncSender<Released>,
}
impl Drop for Ready {
    fn drop(&mut self) {
        if let Some(fence) = self.fence.take() {
            // Unconsumed queued frames still return their slot when a receiver
            // closes or an error unwinds. At most three leases can exist.
            let _ = self.releases.try_send(Released {
                slot: self.slot,
                fence: Some(fence),
            });
        }
    }
}
struct Released {
    slot: usize,
    fence: Option<Fence>,
}
type ProducerResult = Result<(u32, u32, u64, u64, Duration, Duration, Duration, Duration), String>;
struct Slot {
    texture: glow::Texture,
    framebuffer: glow::Framebuffer,
    dimensions: (u32, u32),
}
#[derive(Default)]
pub struct PlaybackOptions {
    pub frames: u32,
    pub interactive: bool,
    pub muted: bool,
    pub audio_output: Option<String>,
    pub persistent: bool,
    pub state_directory: Option<PathBuf>,
    pub automatic_path: Option<PathBuf>,
    pub initial_state: Option<emuluna_state_file::PreparedState>,
    pub start_from_boot: bool,
    pub auto_resume: bool,
    pub core_options: Vec<(String, String)>,
    pub input_system: Option<String>,
    pub state_identity: Option<emuluna_state_file::Identity>,
    pub controls: Option<String>,
}
#[derive(Default)]
struct RunOptions<'a> {
    audio_device: Option<&'a mut emuluna_audio_device::Device>,
    state_directory: Option<PathBuf>,
    automatic_path: Option<PathBuf>,
    initial_state: Option<emuluna_state_file::PreparedState>,
    persistent: bool,
    auto_resume: bool,
    frames: u32,
    interactive: bool,
    stall: Duration,
    abort_after: Option<u32>,
    core_paths: Option<[PathBuf; 5]>,
    verify_fixture: bool,
    audio_output: Option<emuluna_audio_device::Producer>,
    muted: bool,
    launch_options: Vec<(String, String)>,
    input_system: Option<String>,
    state_identity: Option<emuluna_state_file::Identity>,
    controls: Option<String>,
}
impl Presenter {
    // Explicit diagnostic readback, never enabled in ordinary playback.
    pub(super) fn capture_diagnostic(&self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Write;
        let (width, height) = self.window.drawable_size();
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err("Unsupported diagnostic capture dimensions".into());
        }
        let mut rgba = vec![0; width as usize * height as usize * 4];
        unsafe {
            self.gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut rgba)),
            );
            if self.gl.get_error() != glow::NO_ERROR {
                return Err("Diagnostic capture failed".into());
            }
        }
        let file = std::fs::File::create_new(path).map_err(|e| e.to_string())?;
        let mut output = std::io::BufWriter::new(file);
        write!(output, "P6\n{width} {height}\n255\n").map_err(|e| e.to_string())?;
        for row in rgba.chunks_exact(width as usize * 4).rev() {
            for pixel in row.as_chunks::<4>().0 {
                output.write_all(&pixel[..3]).map_err(|e| e.to_string())?;
            }
        }
        output.flush().map_err(|e| e.to_string())
    }
    /// Synthetic rotating-FBO benchmark. Verifies sharing before using a core.
    /// Every lease is returned behind a consumer-complete GPU fence. A full UI
    /// queue drops the notification without blocking the producer thread.
    pub fn shared_texture_probe(
        &mut self,
        frames: u32,
        stall: Duration,
    ) -> Result<SharedProbeStatistics, String> {
        self.shared_run(RunOptions {
            frames,
            stall,
            ..RunOptions::default()
        })
    }
    /// Hardware diagnostic only: runs a core and snapshots its stable FBO into
    /// leased shared textures. Audio is counted but not played by this probe.
    /// Requires a new disposable save folder, like the software diagnostic.
    pub fn hardware_core_probe(
        &mut self,
        paths: [PathBuf; 5],
        frames: u32,
    ) -> Result<SharedProbeStatistics, String> {
        std::fs::create_dir(&paths[3]).map_err(|e| format!("Save directory must be new: {e}"))?;
        self.shared_run(RunOptions {
            frames,
            core_paths: Some(paths),
            ..RunOptions::default()
        })
    }
    /// Experimental hardware playback with the same native audio transport as
    /// software sessions. Input/filter/window events remain on the window thread.
    pub fn hardware_playback(
        &mut self,
        paths: [PathBuf; 5],
        frames: u32,
        muted: bool,
    ) -> Result<(SharedProbeStatistics, emuluna_audio_device::Report), String> {
        self.hardware_playback_configured(
            paths,
            PlaybackOptions {
                frames,
                muted,
                ..PlaybackOptions::default()
            },
        )
    }
    pub fn hardware_playback_configured(
        &mut self,
        paths: [PathBuf; 5],
        options: PlaybackOptions,
    ) -> Result<(SharedProbeStatistics, emuluna_audio_device::Report), String> {
        let mut options = options;
        if let Some(state) = &options.initial_state {
            if Some(state.identity()) != options.state_identity.as_ref() {
                return Err("Prepared state identity does not match this session".into());
            }
        }
        if options.initial_state.is_none() && options.auto_resume && !options.start_from_boot && options.automatic_path.is_none() {
            let automatic = emuluna_state_file::readable_path(&emuluna_state_file::automatic_path(
                options.state_directory.as_ref().unwrap_or(&paths[3]),
            ));
            if automatic.is_file() {
                options.initial_state = Some(emuluna_state_file::prepare(
                    &automatic,
                    options
                        .state_identity
                        .as_ref()
                        .ok_or("State identity is missing")?,
                )?);
            }
        }
        if (options.auto_resume || options.initial_state.is_some())
            && options.state_identity.is_none()
        {
            return Err("Resuming a state requires a core and game identity".into());
        }
        emuluna_native_input::Profile::for_system_saved(
            options.input_system.as_deref(),
            options.controls.as_deref(),
        )?;
        if options
            .state_identity
            .as_ref()
            .is_some_and(|id| id.renderer != emuluna_state_file::Renderer::OpenGl)
        {
            return Err("GL playback requires an OpenGL state identity".into());
        }
        if !options.persistent {
            std::fs::create_dir(&paths[3])
                .map_err(|e| format!("Save directory must be new: {e}"))?;
        }
        let audio = self.window.subsystem().sdl().audio()?;
        let (mut device, producer) =
            emuluna_audio_device::Device::open_selected(&audio, options.audio_output.as_deref())?;
        let result = self.shared_run(RunOptions {
            frames: options.frames,
            interactive: options.interactive,
            core_paths: Some(paths),
            audio_output: Some(producer),
            audio_device: Some(&mut device),
            persistent: options.persistent,
            state_directory: options.state_directory,
            automatic_path: options.automatic_path,
            initial_state: options.initial_state,
            auto_resume: options.auto_resume,
            muted: options.muted,
            launch_options: options.core_options,
            input_system: options.input_system,
            state_identity: options.state_identity,
            controls: options.controls,
            ..RunOptions::default()
        });
        let report = device.finish(result.is_ok());
        Ok((result?, report))
    }
    fn shared_run(&mut self, options: RunOptions<'_>) -> Result<SharedProbeStatistics, String> {
        let RunOptions {
            mut audio_device,
            persistent,
            state_directory,
            automatic_path,
            initial_state,
            auto_resume,
            frames,
            interactive,
            stall,
            abort_after,
            core_paths,
            verify_fixture,
            audio_output,
            muted,
            launch_options,
            input_system,
            state_identity,
            controls,
        } = options;
        if stall > Duration::from_secs(1) {
            return Err("Probe stall must not exceed one second".into());
        }
        if frames == 0 || (!interactive && frames > 3600) {
            return Err("Probe frame limit must be 1..3600".into());
        }
        self.window.gl_make_current(&self.context)?;
        // Keep the native window alive on its owning thread throughout the scope.
        let video = self.window.subsystem();
        let mut events = video.sdl().event_pump()?;
        let mut input = emuluna_native_input::Players::new(
            &video.sdl(),
            input_system.as_deref(),
            self.window.id(),
            controls.as_deref(),
        )?;
        let (mut input_publishers, mut input_readers): (Vec<_>, Vec<_>) = (0..input.port_count())
            .map(|_| emuluna_session::input::channel())
            .unzip();
        // EGL does not permit two current contexts to use one window surface.
        // Give the producer its own hidden surface in the same share group.
        let pinned_window = video
            .window("EmuLuna core GPU worker", 64, 64)
            .opengl()
            .hidden()
            .build()
            .map_err(|e| e.to_string())?;
        video.gl_attr().set_share_with_current_context(true);
        let raw = unsafe { sdl2::sys::SDL_GL_CreateContext(pinned_window.raw()) };
        video.gl_attr().set_share_with_current_context(false);
        if raw.is_null() {
            return Err(sdl2::get_error());
        }
        // Creating a context made it current here; detach it before transfer.
        let detached = DetachedContext {
            window: pinned_window.raw(),
            context: raw,
        };
        video.gl_release_current_context()?;
        self.window.gl_make_current(&self.context)?;
        // The consumer can retain the last frame while drawing another.
        // Reserve one slot each for those frames, the queue, and the producer.
        const SLOT_COUNT: usize = 4;
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Ready>(1);
        let (return_tx, return_rx) = mpsc::sync_channel::<Released>(SLOT_COUNT);
        let (names_tx, names_rx) = mpsc::sync_channel(1);
        // The scope joins even on consumer error/unwind. Dropping the ready
        // receiver tells the worker to finish, then it drains every outstanding
        // consumer lease before deleting textures or destroying its context.
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (mut commands, mut control_worker) = emuluna_playback_control::channel();
        commands.set_volume(self.initial_volume())?;
        if persistent && state_identity.is_some() {
            commands.configure_autosave(auto_resume);
        }
        if state_identity.is_some() {
            if let Some(paths) = core_paths.as_ref() {
                let directory = state_directory.as_ref().unwrap_or(&paths[3]);
                commands.configure_state_slot(if persistent {
                    emuluna_state_file::slot_path(directory, 1)
                } else {
                    directory.join("quick.state")
                });
            }
        }
        let scripted_input = emuluna_session::input_script::InputScript::parse(
            &std::env::var("EMULUNA_TEST_INPUT").unwrap_or_default(),
        )?;
        let release_on_drop = return_tx.clone();
        let verify_synthetic = core_paths.is_none();
        let core_diagnostics = Arc::new(AtomicU64::new(0));
        let worker_diagnostics = core_diagnostics.clone();
        let automatic = core_paths.as_ref().map(|paths| {
            automatic_path.unwrap_or_else(|| emuluna_state_file::automatic_path(
                state_directory.as_ref().unwrap_or(&paths[3]),
            ))
        });
        let automatic_preview_path = automatic.as_ref().map(|path| path.with_extension("png"));
        thread::scope(|scope| {
            let producer = scope.spawn(move || -> ProducerResult {
                let context = detached;
                if unsafe { sdl2::sys::SDL_GL_MakeCurrent(context.window, context.context) } != 0 {
                    return Err(sdl2::get_error());
                }
                let mut gl = unsafe {
                    glow::Context::from_loader_function(|name| {
                        let name = CString::new(name).unwrap();
                        sdl2::sys::SDL_GL_GetProcAddress(name.as_ptr()).cast()
                    })
                };
                if std::env::var_os("EMULUNA_GL_DEBUG").is_some()
                    && gl.supported_extensions().contains("GL_KHR_debug")
                {
                    unsafe {
                        gl.enable(glow::DEBUG_OUTPUT);
                        gl.enable(glow::DEBUG_OUTPUT_SYNCHRONOUS);
                        gl.debug_message_callback(|_, kind, id, _, message| {
                            if kind == glow::DEBUG_TYPE_ERROR {
                                eprintln!("Rust worker GL error {id}: {message}");
                                if std::env::var_os("EMULUNA_GL_DEBUG_TRACE").is_some(){
                                    eprintln!("{}",std::backtrace::Backtrace::force_capture());
                                }
                            }
                        });
                    }
                }
                let mut source = core_paths
                    .as_ref()
                    .map(|paths| CoreSource::new(&gl, paths, &launch_options))
                    .transpose()?;
                if let (Some(source),Some(state))=(source.as_mut(),initial_state) {
                    if Some(state.identity())!=state_identity.as_ref(){return Err("Prepared state identity does not match this session".into());}
                    source.load_prepared_state(&state)?;eprintln!("Native automatic state restored");
                }
                // Local ownership drops audio/EOS before core battery IO, also
                // when a frame operation returns early with an error.
                let mut audio_output=audio_output;
                let mut slots = Vec::with_capacity(SLOT_COUNT);
                unsafe {
                    gl.active_texture(glow::TEXTURE0);
                    for _ in 0..SLOT_COUNT {
                        let texture = gl.create_texture()?;
                        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                        gl.tex_image_2d(
                            glow::TEXTURE_2D,
                            0,
                            glow::RGBA8 as i32,
                            64,
                            64,
                            0,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelUnpackData::Slice(None),
                        );
                        let framebuffer = gl.create_framebuffer()?;
                        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                        gl.framebuffer_texture_2d(
                            glow::FRAMEBUFFER,
                            glow::COLOR_ATTACHMENT0,
                            glow::TEXTURE_2D,
                            Some(texture),
                            0,
                        );
                        if gl.check_framebuffer_status(glow::FRAMEBUFFER)
                            != glow::FRAMEBUFFER_COMPLETE
                        {
                            return Err("Shared GPU framebuffer incomplete".into());
                        }
                        slots.push(Slot {
                            texture,
                            framebuffer,
                            dimensions: (64, 64),
                        });
                    }
                    core_source::finish_transport(&gl);
                    gl.flush();
                    let error=gl.get_error();
                    if error!=glow::NO_ERROR{return Err(format!("Shared GPU allocation failed: {error:#x}"));}
                }
                names_tx
                    .send(slots.iter().map(|s| s.texture).collect::<Vec<_>>())
                    .map_err(|e| e.to_string())?;
                let mut free: Vec<_> = (0..SLOT_COUNT).collect();
                let mut outstanding = 0;
                let mut produced = 0;
                let mut dropped = 0;
                let mut audio_frames = 0;
                let mut cpu_video_bytes = 0;
                let mut core_time = Duration::ZERO;
                let mut peak_core_time = Duration::ZERO;
                let mut snapshot_time = Duration::ZERO;
                let mut audio_wait = Duration::ZERO;
                let mut rate_control=emuluna_session::audio_clock::AudioRateControl::new(2400.);
                let epoch = Instant::now();
                let mut pacer=emuluna_session::pacing::Pacer::new(epoch);
                let trace = std::env::var_os("EMULUNA_TRACE").is_some();
                for sequence in 0..frames {
                    if thread_stop.load(Ordering::Acquire) {
                        break;
                    }
                    let update = control_worker.service_operations(audio_output.as_mut(),&thread_stop,|command| {
                        use emuluna_session::Command;
                        match command {
                            Command::Reset=>{if let Some(source)=source.as_mut() {source.reset()?;}Ok(())},
                            Command::CoreOption(key,value)=>source.as_mut().ok_or("No game is running")?.set_runtime_option(key,value),
                            Command::SaveState(path)=>source.as_mut().ok_or("No game is running")?.save_state(path,state_identity.as_ref().ok_or("State identity is missing")?),
                            Command::LoadState(path)=>source.as_mut().ok_or("No game is running")?.load_state(path,state_identity.as_ref().ok_or("State identity is missing")?),
                            _=>Err("Unsupported core operation".into()),
                        }
                    })?;
                    if produced==0 && source.is_some(){eprintln!("Native playback started");}
                    worker_diagnostics.store(source.as_ref().map_or(0,CoreSource::diagnostics),Ordering::Relaxed);
                    if !update.running { break; }
                    if update.reset_clock { pacer=emuluna_session::pacing::Pacer::new(Instant::now()); rate_control.reset(); }
                    while let Ok(released) = return_rx.try_recv() {
                        unsafe {
                            if let Some(fence) = released.fence {
                                fence.consume(&gl);
                            }
                        }
                        free.push(released.slot);
                        outstanding -= 1;
                    }
                    let slot = free.pop().ok_or("GPU slot ownership exhausted")?;

                    let cadence = source
                        .as_ref()
                        .map(CoreSource::frame_duration)
                        .transpose()?
                        .unwrap_or(Duration::from_millis(4));
                    let metadata = if let Some(source) = source.as_mut() {
                        if let Some(output)=audio_output.as_ref() {
                            source.set_audio_rate_adjustment(rate_control.update(output.queued() as f64,cadence));
                        }
                        let mut states = [emuluna_session::input::Snapshot::default(); emuluna_native_input::MAX_PLAYERS];
                        for (state, reader) in states.iter_mut().zip(&mut input_readers) {
                            *state = reader.latest();
                        }
                        states[0].buttons |= scripted_input.buttons(sequence);
                        match source.render(&gl,&mut slots[slot],&states[..input_readers.len()])? {
                            Some(metadata)=>metadata,
                            None=>{worker_diagnostics.store(source.diagnostics(),Ordering::Relaxed);free.push(slot);break;}
                        }
                    } else {
                        unsafe {
                            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(slots[slot].framebuffer));
                            gl.viewport(0, 0, 64, 64);
                            let red = sequence % 2 == 0;
                            gl.clear_color(
                                if red { 1. } else { 0. },
                                if red { 0. } else { 1. },
                                0.,
                                1.,
                            );
                            gl.clear(glow::COLOR_BUFFER_BIT);
                        }
                        core_source::Metadata {
                            width: 64,
                            height: 64,
                            aspect: 1.,
                            top_left: false,
                            audio_frames: 0,
                            cpu_video_bytes: 0,
                            core_time:Duration::ZERO,snapshot_time:Duration::ZERO,
                        }
                    };
                    worker_diagnostics.store(source.as_ref().map_or(0,CoreSource::diagnostics),Ordering::Relaxed);
                    core_time += metadata.core_time;
                    peak_core_time = peak_core_time.max(metadata.core_time);
                    snapshot_time += metadata.snapshot_time;
                    let mut waited=Duration::ZERO;
                    let mut clock_started=false;
                    if let (Some(source),Some(output)) = (source.as_mut(),audio_output.as_mut()) {
                        match output.write(source.audio(muted),&thread_stop) {
                            Ok(timing)=>{audio_wait+=timing.waited;waited=timing.waited;clock_started=timing.clock_started;},
                            Err(_) if thread_stop.load(Ordering::Acquire)=>break,
                            Err(error)=>return Err(error),
                        }
                        if trace && (sequence < 30 || sequence % 60 == 0) {
                            eprintln!("hardware_trace frame={sequence} elapsed={:.3} core_ms={:.3} snapshot_ms={:.3} batch={} queue={} missing={} accepted={} played={}",
                                epoch.elapsed().as_secs_f64(),metadata.core_time.as_secs_f64()*1000.,metadata.snapshot_time.as_secs_f64()*1000.,
                                metadata.audio_frames,output.queued(),output.statistics().missing.load(Ordering::Relaxed),
                                output.statistics().accepted.load(Ordering::Relaxed),output.statistics().played.load(Ordering::Relaxed));
                        }
                    }
                    audio_frames += metadata.audio_frames;
                    cpu_video_bytes += metadata.cpu_video_bytes;
                    produced += 1;
                    let frame = Ready {
                        slot,
                        sequence,
                        generation: update.generation,
                        width: metadata.width,
                        height: metadata.height,
                        aspect: metadata.aspect,
                        top_left: metadata.top_left,
                        fence: Some(unsafe { Fence::create(&gl)? }),
                        releases: release_on_drop.clone(),
                    };
                    match ready_tx.try_send(frame) {
                        Ok(()) => outstanding += 1,
                        Err(mpsc::TrySendError::Full(mut frame)) => {
                            unsafe {
                                frame.fence.take().unwrap().consume(&gl);
                            }
                            free.push(frame.slot);
                            dropped += 1;
                        }
                        Err(mpsc::TrySendError::Disconnected(mut frame)) => {
                            unsafe {
                                frame.fence.take().unwrap().consume(&gl);
                            }
                            free.push(frame.slot);
                            break;
                        }
                    }
                    let wait=pacer.advance(Instant::now(),cadence,waited,clock_started);
                    if !wait.is_zero() {thread::sleep(wait);}
                }
                drop(ready_tx);
                while outstanding > 0 {
                    let released = return_rx
                        .recv_timeout(Duration::from_secs(2))
                        .map_err(|_| "Consumer did not return its GPU lease before shutdown")?;
                    unsafe {
                        if let Some(fence) = released.fence {
                            fence.consume(&gl);
                        }
                    }
                    outstanding -= 1;
                }
                unsafe {
                    // GL queues retain referenced storage through completion.
                    for slot in slots {
                        gl.delete_framebuffer(slot.framebuffer);
                        gl.delete_texture(slot.texture);
                    }
                    let error = gl.get_error();
                    if error != glow::NO_ERROR {
                        return Err(format!("Shared producer GL error: {error:#x}"));
                    }
                }
                drop(audio_output);
                if control_worker.autosave_enabled() && produced>0 {
                    if let (Some(source),Some(path),Some(identity))=(source.as_mut(),automatic.as_ref(),state_identity.as_ref()) {
                        source.save_state(path,identity)?;eprintln!("Native automatic state saved");
                    }
                }
                drop(source);
                drop(context);
                Ok((produced, dropped, audio_frames, cpu_video_bytes,core_time,peak_core_time,snapshot_time,audio_wait))
            });
            let names = match names_rx.recv() {
                Ok(names) => names,
                Err(_) => {
                    drop(ready_rx);
                    drop(return_tx);
                    return match producer.join() {
                        Ok(Err(error)) => Err(error),
                        _ => Err("GPU producer did not initialize".into()),
                    };
                }
            };
            let mut presented = 0;
            let mut source_width = 0;
            let mut source_height = 0;
            let mut consumer_error = None;
            let mut retained_slot = None;
            'display: loop {
                for message in commands.messages() {
                    if let Some(path) = &message.preview {
                        if retained_slot.is_some() {
                            if let Err(error) = self.save_game_preview(path) {
                                self.overlay_message(format!(
                                    "State saved, but its preview failed: {error}"
                                ));
                            }
                        }
                    }
                    self.overlay_message(message.text.clone());
                    eprintln!(
                        "playback_message success={}: {}",
                        message.success, message.text
                    );
                }
                for event in events.poll_iter() {
                    self.window_event(&event, &mut commands)?;
                    self.overlay_event(&event);
                    if let Some(device) = audio_device.as_deref_mut() {
                        if let Some(message) = device.handle_event(&event) {
                            self.overlay_message(message);
                        }
                    }
                    input.event(&event);
                    if let Event::KeyDown { keycode: Some(key), keymod, .. } = event {
                        // A gameplay binding owns its key. Keep Escape and Ctrl shortcuts available.
                        if key != Keycode::Escape
                            && !keymod.intersects(sdl2::keyboard::Mod::LCTRLMOD | sdl2::keyboard::Mod::RCTRLMOD)
                            && input.binds_key(key)
                        {
                            continue;
                        }
                    }
                    if self.intercept_overlay_shortcut(&event) {
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
                        } => {
                            stop.store(true, Ordering::Release);
                            break 'display;
                        }
                        Event::KeyDown {
                            keycode: Some(Keycode::F1),
                            repeat: false,
                            ..
                        } => self.set_filter(self.filter().next()),
                        Event::KeyDown {
                            keycode: Some(Keycode::F11),
                            repeat: false,
                            ..
                        } => {
                            if let Err(error) = self.toggle_fullscreen() {
                                consumer_error = Some(error);
                                stop.store(true, Ordering::Release);
                                break 'display;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(device) = audio_device.as_deref_mut() {
                    self.service_audio(device);
                }
                if self.service_overlay(&mut commands)? {
                    break;
                }
                for (port, publisher) in input_publishers.iter_mut().enumerate() {
                    publisher.publish(input.snapshot(port));
                }
                let mut frame = match ready_rx.recv_timeout(Duration::from_millis(8)) {
                    Ok(frame) => frame,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if self.overlay_needs_redraw() {
                            if let Err(error) = self.redraw() {
                                consumer_error = Some(error);
                                break;
                            }
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };

                if frame.generation != commands.generation() {
                    continue;
                }
                self.accept_generation(frame.generation);
                source_width = frame.width;
                source_height = frame.height;
                unsafe {
                    frame.fence.take().unwrap().consume(&self.gl);
                }
                let draw = self.present_texture(
                    FrameView {
                        image: chain::Image {
                            texture: names[frame.slot],
                            width: frame.width,
                            height: frame.height,
                        },
                        aspect: frame.aspect,
                        top_left: frame.top_left,
                        frame: frame.sequence as u64,
                    },
                    false,
                );
                // Readback is diagnostic-only, to prove content from the worker
                // matches its sequence and was not overwritten while sampled.
                if draw.is_ok() && (verify_synthetic || verify_fixture) {
                    let (w, h) = self.window.drawable_size();
                    let mut pixel = [0u8; 4];
                    unsafe {
                        self.gl.read_pixels(
                            (w / 2) as i32,
                            (h / 2) as i32,
                            1,
                            1,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelPackData::Slice(Some(&mut pixel)),
                        );
                    }
                    let expected = if verify_fixture {
                        match frame.sequence {
                            0..=2 => [255, 0, 0, 255],
                            3..=4 => [17, 34, 51, 255],
                            5 => [0, 0, 255, 255],
                            _ => [0, 255, 0, 255],
                        }
                    } else if frame.sequence % 2 == 0 {
                        [255, 0, 0, 255]
                    } else {
                        [0, 255, 0, 255]
                    };
                    if pixel != expected {
                        consumer_error = Some(format!(
                            "Shared texture content mismatch: {pixel:?} expected {expected:?}"
                        ));
                    }
                    self.window.gl_swap_window();
                } else if let Err(error) = draw {
                    consumer_error = Some(error);
                } else {
                    if frame.sequence + 1 == frames {
                        if let Some(path) = std::env::var_os("EMULUNA_TEST_CAPTURE") {
                            if let Err(error) = self.capture_diagnostic(std::path::Path::new(&path))
                            {
                                consumer_error = Some(error);
                            }
                        }
                    }
                    self.window.gl_swap_window();
                }
                // Keep the current source leased for UI-only redraws. Releasing
                // a prior slot fences every read, including paused UI repaints.
                let release = if self.overlay.is_some() {
                    retained_slot.replace(frame.slot)
                } else {
                    Some(frame.slot)
                };
                if let Some(slot) = release {
                    let fence = match unsafe { Fence::create(&self.gl) } {
                        Ok(fence) => Some(fence),
                        Err(error) => {
                            unsafe {
                                self.gl.finish();
                            }
                            consumer_error = Some(error);
                            None
                        }
                    };
                    if return_tx.send(Released { slot, fence }).is_err() {
                        consumer_error = Some("GPU producer stopped accepting leases".into());
                    }
                }
                presented += 1;
                if presented == 2 {
                    thread::sleep(stall);
                }
                if abort_after == Some(presented) {
                    consumer_error = Some("Injected consumer shutdown".into());
                }
                if consumer_error.is_some() {
                    break;
                }
            }
            let preview = if commands.autosave() == Some(true)
                && consumer_error.is_none()
                && retained_slot.is_some()
            {
                self.capture_game_preview().ok().flatten()
            } else {
                None
            };
            let preview_path = automatic_preview_path;
            stop.store(true, Ordering::Release);
            self.cached = None;
            self.last_source = None;
            if let Some(slot) = retained_slot.take() {
                let fence = match unsafe { Fence::create(&self.gl) } {
                    Ok(fence) => Some(fence),
                    Err(error) => {
                        unsafe {
                            self.gl.finish();
                        }
                        consumer_error = Some(error);
                        None
                    }
                };
                let _ = return_tx.send(Released { slot, fence });
            }
            // Dropping the receiver automatically returns queued Ready leases.
            drop(ready_rx);
            drop(return_tx);
            let (
                produced,
                dropped,
                audio_frames,
                cpu_video_bytes,
                core_time,
                peak_core_time,
                snapshot_time,
                audio_wait,
            ) = producer.join().map_err(|_| "GPU producer panicked")??;
            if let Some(error) = consumer_error {
                return Err(error);
            }
            if let (Some(path), Some(preview)) = (preview_path, preview) {
                if let Err(error) = preview.save(&path) {
                    eprintln!("State saved, but its preview failed: {error}");
                }
            }
            Ok(SharedProbeStatistics {
                produced,
                presented,
                dropped,
                audio_frames,
                core_gl_diagnostics: core_diagnostics.load(Ordering::Relaxed),
                cpu_video_bytes,
                source_width,
                source_height,
                core_time,
                peak_core_time,
                snapshot_time,
                audio_wait,
            })
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires desktop shared OpenGL contexts"]
    fn live_shared_gpu_worker_preserves_leases_under_stall() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        let stats = presenter
            .shared_texture_probe(180, Duration::from_millis(200))
            .unwrap();
        assert_eq!(stats.produced, 180);
        assert!(stats.presented > 50);
        assert!(stats.dropped > 20);
        assert_eq!(stats.produced, stats.presented + stats.dropped);
        println!("{stats:?}");
    }
    #[test]
    fn dropping_unconsumed_frames_returns_their_lease() {
        let (tx, rx) = mpsc::sync_channel(3);
        let frame = Ready {
            slot: 2,
            fence: Some(Fence(123)),
            sequence: 0,
            generation: 1,
            width: 64,
            height: 64,
            aspect: 1.,
            top_left: false,
            releases: tx,
        };
        drop(frame);
        let returned = rx.try_recv().unwrap();
        assert_eq!(returned.slot, 2);
        assert_eq!(returned.fence.unwrap().0, 123);
    }
    #[test]
    #[ignore = "requires desktop shared OpenGL contexts"]
    fn live_shared_gpu_recovers_after_early_consumer_shutdown() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        let error = presenter
            .shared_run(RunOptions {
                frames: 180,
                abort_after: Some(2),
                ..RunOptions::default()
            })
            .unwrap_err();
        assert_eq!(error, "Injected consumer shutdown");
        let stats = presenter.shared_texture_probe(30, Duration::ZERO).unwrap();
        assert_eq!(stats.produced, 30);
        assert_eq!(stats.produced, stats.presented + stats.dropped);
    }
    #[test]
    #[ignore = "requires desktop GL, native host and EMULUNA_RUST_HW_FIXTURE"]
    fn live_libretro_hardware_duplicates_and_software_fallback() {
        let fixture = PathBuf::from(
            std::env::var_os("EMULUNA_RUST_HW_FIXTURE")
                .expect("Set EMULUNA_RUST_HW_FIXTURE to the compiled original fixture"),
        );
        let host = PathBuf::from(
            std::env::var_os("EMULUNA_RUST_HOST")
                .expect("Set EMULUNA_RUST_HOST to the native bridge"),
        );
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "emuluna-rust-hardware-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("Diagnostic.bin"), [0]).unwrap();
        std::fs::create_dir(root.join("bios")).unwrap();
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        let diagnostic = std::env::var_os("EMULUNA_RUST_HW_DIAGNOSTIC_FIXTURE").map(PathBuf::from);
        for session in 0..2 {
            let selected = if session == 1 {
                diagnostic.as_ref().unwrap_or(&fixture)
            } else {
                &fixture
            };
            let saves = root.join(format!("saves-{session}"));
            std::fs::create_dir(&saves).unwrap();
            let paths = [
                host.clone(),
                selected.clone(),
                root.join("Diagnostic.bin"),
                saves,
                root.join("bios"),
            ];
            let stats = presenter
                .shared_run(RunOptions {
                    frames: 8,
                    core_paths: Some(paths),
                    verify_fixture: true,
                    ..RunOptions::default()
                })
                .unwrap();
            assert_eq!(
                stats.core_gl_diagnostics,
                2 * u64::from(session == 1 && diagnostic.is_some())
            );
            assert_eq!(stats.produced, 8);
            assert_eq!(stats.audio_frames, 6400);
            // Only the two CPU callbacks cross native pixel storage. Hardware
            // frames, NULL duplicates and no-callback duplicates copy no pixels.
            assert_eq!(stats.cpu_video_bytes, 128);
            assert_eq!((stats.source_width, stats.source_height), (4, 4));
            assert_eq!(stats.produced, stats.presented + stats.dropped);
            assert!(stats.presented >= 5);
            println!("hardware session {session}: {stats:?}");
        }
        drop(presenter);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires desktop/audio, host and EMULUNA_RUST_HW_BURST_FIXTURE"]
    fn live_hardware_audio_startup_burst_preserves_every_sample() {
        let fixture = PathBuf::from(
            std::env::var_os("EMULUNA_RUST_HW_BURST_FIXTURE").expect("Set burst fixture path"),
        );
        let host =
            PathBuf::from(std::env::var_os("EMULUNA_RUST_HOST").expect("Set native host path"));
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "emuluna-rust-audio-burst-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("Diagnostic.bin"), [0]).unwrap();
        std::fs::create_dir(root.join("bios")).unwrap();
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        let (stats, audio) = presenter
            .hardware_playback(
                [
                    host,
                    fixture,
                    root.join("Diagnostic.bin"),
                    root.join("saves"),
                    root.join("bios"),
                ],
                8,
                true,
            )
            .unwrap();
        assert!((24500..=24750).contains(&stats.audio_frames));
        assert_eq!(audio.accepted, stats.audio_frames);
        assert_eq!(audio.played, audio.accepted);
        assert_eq!(audio.missing, 0);
        assert_eq!(audio.overflow, 0);
        assert_eq!(audio.pending, 0);
        println!("startup burst: {stats:?} {audio:?}");
        drop(presenter);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires desktop GL, host and original hardware fixture"]
    fn live_core_requested_shutdown_finishes_normally() {
        let host = PathBuf::from(std::env::var_os("EMULUNA_RUST_HOST").expect("host"));
        let fixture = PathBuf::from(std::env::var_os("EMULUNA_RUST_HW_FIXTURE").expect("fixture"));
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("emuluna-core-quit-{}-{unique}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("bios")).unwrap();
        std::fs::create_dir(root.join("saves")).unwrap();
        std::fs::write(root.join("original.bin"), [0]).unwrap();
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        let stats = presenter
            .shared_run(RunOptions {
                frames: 120,
                verify_fixture: true,
                core_paths: Some([
                    host,
                    fixture,
                    root.join("original.bin"),
                    root.join("saves"),
                    root.join("bios"),
                ]),
                launch_options: vec![("fixture_shutdown_frame".into(), "5".into())],
                ..RunOptions::default()
            })
            .unwrap();
        assert_eq!(stats.produced, 5);
        assert_eq!(stats.produced, stats.presented + stats.dropped);
        assert!(stats.presented >= 3);
        assert_eq!(stats.cpu_video_bytes, 128);
        assert_eq!(stats.core_gl_diagnostics, 0);
        drop(presenter);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires desktop/audio, host and original hardware fixture"]
    fn live_pause_reset_resume_and_close_while_paused() {
        let host = PathBuf::from(std::env::var_os("EMULUNA_RUST_HOST").expect("host"));
        let fixture = PathBuf::from(std::env::var_os("EMULUNA_RUST_HW_FIXTURE").expect("fixture"));
        let diagnostic = std::env::var_os("EMULUNA_RUST_HW_DIAGNOSTIC_FIXTURE").map(PathBuf::from);
        let fixture = diagnostic.as_ref().unwrap_or(&fixture).clone();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("emuluna-controls-{}-{unique}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("diagnostic.bin"), [0]).unwrap();
        std::fs::create_dir(root.join("bios")).unwrap();
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        presenter.enable_overlay("classic").unwrap();
        for close_paused in [false, true] {
            let sender = sdl.event().unwrap().event_sender();
            let window_id = presenter.window.id();
            let audio_subsystem = sdl.audio().unwrap();
            let (mut device, producer) =
                emuluna_audio_device::Device::open(&audio_subsystem).unwrap();
            let statistics = device.statistics();
            let injector = std::thread::spawn(move || {
                // Start only after actual playback, rather than assuming device
                // and GL startup finish within a fixed wall-clock interval.
                let until = Instant::now() + Duration::from_secs(5);
                while statistics.played.load(Ordering::Acquire) < 8000 && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(statistics.played.load(Ordering::Acquire) >= 8000);
                let schedule = if close_paused {
                    vec![(200, Keycode::F6), (350, Keycode::Escape)]
                } else {
                    vec![
                        (200, Keycode::F8), // missing state must not end playback
                        (350, Keycode::F6),
                        (500, Keycode::F5), // save while paused
                        (650, Keycode::F7),
                        (800, Keycode::F8), // restore saved nonzero core frame
                        (950, Keycode::F6),
                        (1100, Keycode::Minus),
                        (1300, Keycode::F7),
                    ]
                };
                let mut previous = 0;
                for (at, key) in schedule {
                    std::thread::sleep(Duration::from_millis(at - previous));
                    previous = at;
                    sender
                        .push_event(Event::KeyDown {
                            timestamp: 0,
                            window_id,
                            keycode: Some(key),
                            scancode: None,
                            keymod: sdl2::keyboard::Mod::NOMOD,
                            repeat: false,
                        })
                        .unwrap();
                }
            });
            let start = Instant::now();
            let saves = root.join(format!("save-{close_paused}"));
            std::fs::create_dir(&saves).unwrap();
            let identity = emuluna_state_file::Identity::new(
                "fixture",
                &emuluna_state_file::digest_file(&fixture).unwrap(),
                &emuluna_state_file::digest_file(&root.join("diagnostic.bin")).unwrap(),
                emuluna_state_file::Renderer::OpenGl,
            )
            .unwrap();
            let slot = saves.join("quick.state");
            let result = presenter.shared_run(RunOptions {
                frames: if close_paused { u32::MAX } else { 120 },
                interactive: close_paused,
                core_paths: Some([
                    host.clone(),
                    fixture.clone(),
                    root.join("diagnostic.bin"),
                    saves,
                    root.join("bios"),
                ]),
                audio_output: Some(producer),
                muted: true,
                state_identity: Some(identity.clone()),
                ..RunOptions::default()
            });
            let audio = device.finish(result.is_ok());
            let stats = result.unwrap();
            assert_eq!(
                stats.core_gl_diagnostics,
                if diagnostic.is_some() {
                    if close_paused {
                        2
                    } else {
                        7
                    }
                } else {
                    0
                }
            );
            injector.join().unwrap();
            assert_eq!(audio.missing, 0);
            assert_eq!(audio.overflow, 0);
            assert_eq!(audio.pending, 0);
            assert_eq!(audio.accepted, audio.played + audio.discarded);
            assert!(audio.discarded > 0);
            if close_paused {
                assert!(start.elapsed() < Duration::from_secs(3));
                assert!(stats.produced < 120);
            } else {
                assert_eq!(stats.produced, 120);
                assert_eq!(stats.cpu_video_bytes, 2 * 128);
                assert_eq!(emuluna_state_file::inspect(&slot).unwrap(), identity);
                emuluna_state_file::with_payload(&slot, &identity, |raw| {
                    let bytes = std::fs::read(raw).unwrap();
                    let frame = u32::from_ne_bytes(bytes[..4].try_into().unwrap());
                    assert!((5..120).contains(&frame));
                    Ok(())
                })
                .unwrap();
            }
            println!("control session close_paused={close_paused}: {stats:?} {audio:?}");
        }
        drop(presenter);
        std::fs::remove_dir_all(root).unwrap();
    }
}
