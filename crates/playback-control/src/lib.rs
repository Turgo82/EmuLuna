//! Bounded window-to-worker commands; core mutation stays on its owner thread.
#![forbid(unsafe_code)]
use emuluna_audio_device::Producer;
use emuluna_session::{Command, SessionControl, SessionState};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
pub struct Message {
    pub success: bool,
    pub text: String,
    pub preview: Option<std::path::PathBuf>,
}
pub struct Controller {
    sender: mpsc::SyncSender<Command>,
    generation: Arc<AtomicU64>,
    desired_pause: bool,
    desired_volume: f32,
    state_slot: Option<std::path::PathBuf>,
    autosave_available: bool,
    autosave: Arc<AtomicBool>,
    slot_number: u8,
    messages: mpsc::Receiver<Message>,
}
pub struct Worker {
    receiver: mpsc::Receiver<Command>,
    generation: Arc<AtomicU64>,
    state: SessionControl,
    autosave: Arc<AtomicBool>,
    messages: mpsc::SyncSender<Message>,
}
pub struct Update {
    pub running: bool,
    pub reset_clock: bool,
    pub generation: u64,
}
pub fn channel() -> (Controller, Worker) {
    let (sender, receiver) = mpsc::sync_channel(16);
    let (messages, message_receiver) = mpsc::sync_channel(16);
    let mut state = SessionControl::default();
    state.start();
    let generation = Arc::new(AtomicU64::new(state.generation().0));
    let autosave = Arc::new(AtomicBool::new(false));
    (
        Controller {
            sender,
            generation: generation.clone(),
            desired_pause: false,
            desired_volume: 1.,
            state_slot: None,
            autosave_available: false,
            autosave: autosave.clone(),
            slot_number: 1,
            messages: message_receiver,
        },
        Worker {
            receiver,
            generation,
            state,
            autosave,
            messages,
        },
    )
}
impl Controller {
    /// Temporary diagnostic hotkeys; the final UI calls the typed methods.
    pub fn handle_event(&mut self, event: &sdl2::event::Event) -> Result<(), String> {
        use sdl2::{event::Event, keyboard::Keycode};
        if let Event::KeyDown {
            keycode: Some(key),
            repeat: false,
            keymod,
            ..
        } = *event
        {
            match key {
                Keycode::F5 if self.state_slot.is_some() => self.save_state()?,
                Keycode::F8 if self.state_slot.is_some() => self.load_state()?,
                Keycode::F6 => self.toggle_pause()?,
                Keycode::P
                    if keymod.intersects(
                        sdl2::keyboard::Mod::LCTRLMOD | sdl2::keyboard::Mod::RCTRLMOD,
                    ) =>
                {
                    self.toggle_pause()?
                }
                Keycode::F7 => self.reset()?,
                Keycode::Minus => self.set_volume(self.volume() - 0.1)?,
                Keycode::Equals => self.set_volume(self.volume() + 0.1)?,
                _ => {}
            }
        }
        Ok(())
    }

    pub fn configure_autosave(&mut self, enabled: bool) {
        self.autosave_available = true;
        self.autosave.store(enabled, Ordering::Release);
    }
    pub fn autosave(&self) -> Option<bool> {
        self.autosave_available
            .then(|| self.autosave.load(Ordering::Acquire))
    }
    pub fn set_autosave(&self, enabled: bool) -> Result<(), String> {
        if !self.autosave_available {
            return Err("Autosave is unavailable for this session".into());
        }
        self.autosave.store(enabled, Ordering::Release);
        Ok(())
    }
    pub fn configure_state_slot(&mut self, path: std::path::PathBuf) {
        self.state_slot = Some(path);
    }
    pub fn state_slot_number(&self) -> u8 {
        self.slot_number
    }
    pub fn select_state_slot(&mut self, slot: u8) -> Result<(), String> {
        if !(1..=10).contains(&slot) {
            return Err("Choose a save slot from 1 to 10".into());
        }
        let current = self
            .state_slot
            .as_ref()
            .ok_or("Save states are not configured")?;
        self.state_slot = Some(emuluna_state_file::slot_path(
            current.parent().unwrap_or(std::path::Path::new(".")),
            slot,
        ));
        self.slot_number = slot;
        Ok(())
    }
    pub fn save_state(&self) -> Result<(), String> {
        self.send(Command::SaveState(
            self.state_slot
                .clone()
                .ok_or("Save states are not configured")?,
        ))
    }
    pub fn load_state(&self) -> Result<(), String> {
        let path = self
            .state_slot
            .as_deref()
            .ok_or("Save states are not configured")?;
        self.send(Command::LoadState(emuluna_state_file::readable_path(path)))
    }
    pub fn messages(&self) -> impl Iterator<Item = Message> + '_ {
        self.messages.try_iter()
    }
    fn send(&self, command: Command) -> Result<(), String> {
        self.sender
            .try_send(command)
            .map_err(|e| format!("Playback command could not be delivered: {e}"))
    }
    pub fn toggle_pause(&mut self) -> Result<(), String> {
        let paused = !self.desired_pause;
        self.send(Command::Pause(paused))?;
        self.desired_pause = paused;
        Ok(())
    }
    pub fn reset(&mut self) -> Result<(), String> {
        self.send(Command::Reset)
    }
    pub fn set_core_option(&self, key: String, value: String) -> Result<(), String> {
        self.send(Command::CoreOption(key, value))
    }
    pub fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        if !volume.is_finite() {
            return Err("Invalid playback volume".into());
        }
        let volume = volume.clamp(0., 1.);
        self.send(Command::Volume(volume))?;
        self.desired_volume = volume;
        Ok(())
    }
    pub fn paused(&self) -> bool {
        self.desired_pause
    }
    pub fn states_available(&self) -> bool {
        self.state_slot.is_some()
    }
    pub fn volume(&self) -> f32 {
        self.desired_volume
    }
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}
impl Worker {
    pub fn autosave_enabled(&self) -> bool {
        self.autosave.load(Ordering::Acquire)
    }

    /// Called before retro_run, in the owning core/context thread. Pausing
    /// waits for commands with a timeout so closing the window still cancels.
    pub fn service(
        &mut self,
        audio: Option<&mut Producer>,
        stop: &AtomicBool,
        mut reset: impl FnMut() -> Result<(), String>,
    ) -> Result<Update, String> {
        self.service_operations(audio, stop, |command| match command {
            Command::Reset => reset(),
            _ => Err("State operations are not configured".into()),
        })
    }
    pub fn service_operations(
        &mut self,
        audio: Option<&mut Producer>,
        stop: &AtomicBool,
        operation: impl FnMut(&Command) -> Result<(), String>,
    ) -> Result<Update, String> {
        let result = self.service_inner(audio, stop, operation);
        if result.is_err() && stop.load(Ordering::Acquire) {
            return Ok(Update {
                running: false,
                reset_clock: false,
                generation: self.state.generation().0,
            });
        }
        result
    }
    fn service_inner(
        &mut self,
        mut audio: Option<&mut Producer>,
        stop: &AtomicBool,
        mut operation: impl FnMut(&Command) -> Result<(), String>,
    ) -> Result<Update, String> {
        let mut reset_clock = false;
        let mut processed = 0;
        loop {
            if stop.load(Ordering::Acquire) || self.state.state == SessionState::Stopped {
                return Ok(Update {
                    running: false,
                    reset_clock,
                    generation: self.state.generation().0,
                });
            }
            let command = if self.state.state == SessionState::Paused {
                match self.receiver.recv_timeout(Duration::from_millis(10)) {
                    Ok(command) => Some(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Ok(Update {
                            running: false,
                            reset_clock,
                            generation: self.state.generation().0,
                        })
                    }
                }
            } else {
                match self.receiver.try_recv() {
                    Ok(command) => Some(command),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Ok(Update {
                            running: false,
                            reset_clock,
                            generation: self.state.generation().0,
                        })
                    }
                }
            };
            let Some(command) = command else {
                break;
            };
            match command {
                Command::Pause(paused) => {
                    if let Some(audio) = audio.as_deref_mut() {
                        audio.set_paused(paused, stop)?;
                    }
                    self.state.apply(Command::Pause(paused));
                    reset_clock = true;
                }
                command @ (Command::Reset | Command::SaveState(_) | Command::LoadState(_)) => {
                    if let Some(audio) = audio.as_deref_mut() {
                        audio.set_paused(true, stop)?;
                    }
                    let changes_video = matches!(&command, Command::Reset | Command::LoadState(_));
                    let result = operation(&command);
                    if changes_video {
                        // Even a failed core unserialization may change internal
                        // state. Never retain an old video/filter generation.
                        self.state.apply(command.clone());
                        self.generation
                            .store(self.state.generation().0, Ordering::Release);
                    }
                    let preview = if result.is_ok() {
                        match &command {
                            Command::SaveState(path) => Some(path.with_extension("png")),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    let text = match result {
                        Ok(()) => match command {
                            Command::Reset => "Game reset".into(),
                            Command::SaveState(path) => state_notice("State saved", &path),
                            Command::LoadState(path) => state_notice("State loaded", &path),
                            _ => unreachable!(),
                        },
                        Err(ref error) => error.clone(),
                    };
                    let _ = self.messages.try_send(Message {
                        success: result.is_ok(),
                        text,
                        preview,
                    });
                    if self.state.state == SessionState::Running {
                        if let Some(audio) = audio.as_deref_mut() {
                            audio.set_paused(false, stop)?;
                        }
                    }
                    reset_clock = true;
                }
                command @ Command::CoreOption(_, _) => {
                    if let Err(error) = operation(&command) {
                        let _ = self.messages.try_send(Message {
                            success: false,
                            text: error,
                            preview: None,
                        });
                    }
                }
                Command::Volume(volume) => {
                    self.state.apply(Command::Volume(volume));
                    if let Some(audio) = audio.as_deref_mut() {
                        audio.set_volume(self.state.volume());
                    }
                }
                Command::Stop => self.state.apply(Command::Stop),
            }
            processed += 1;
            if processed >= 16 && self.state.state == SessionState::Running {
                break;
            }
        }
        Ok(Update {
            running: true,
            reset_clock,
            generation: self.state.generation().0,
        })
    }
}
fn state_notice(label: &str, path: &std::path::Path) -> String {
    let slot = path
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("slot-"))
        .and_then(|slot| slot.parse::<u8>().ok())
        .filter(|slot| (1..=10).contains(slot));
    slot.map_or_else(|| label.to_owned(), |slot| format!("{label} · Slot {slot}"))
}
#[cfg(test)]
mod autosave_tests {
    use super::*;

    #[test]
    fn session_autosave_updates_the_worker_without_queueing_a_core_operation() {
        let (mut controls, worker) = channel();
        assert!(controls.set_autosave(true).is_err());
        controls.configure_autosave(true);
        assert!(worker.autosave_enabled());
        controls.set_autosave(false).unwrap();
        assert!(!worker.autosave_enabled());
        assert_eq!(controls.autosave(), Some(false));
        controls.set_autosave(true).unwrap();
        assert!(worker.autosave_enabled());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_save_slot_routes_commands_and_rejects_invalid_slots() {
        let (mut controller, mut worker) = channel();
        controller.configure_state_slot(std::path::PathBuf::from(
            "private/states/slot-1.emuluna-state",
        ));
        controller.select_state_slot(7).unwrap();
        assert!(controller.select_state_slot(0).is_err());
        assert!(controller.select_state_slot(11).is_err());
        controller.save_state().unwrap();
        controller.load_state().unwrap();
        let mut count = 0;
        worker
            .service_operations(None, &AtomicBool::new(false), |command| {
                let path = match command {
                    Command::SaveState(path) | Command::LoadState(path) => path,
                    _ => panic!("Wrong command"),
                };
                assert_eq!(
                    path,
                    std::path::Path::new("private/states/slot-7.emuluna-state")
                );
                count += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(controller.state_slot_number(), 7);
    }
    #[test]
    fn reset_runs_on_worker_and_invalidates_queued_video() {
        let (mut controller, mut worker) = channel();
        let old = controller.generation();
        controller.reset().unwrap();
        let mut resets = 0;
        let update = worker
            .service(None, &AtomicBool::new(false), || {
                resets += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(resets, 1);
        assert!(update.reset_clock && update.running);
        assert_ne!(controller.generation(), old);
        assert_eq!(update.generation, controller.generation());
    }
    #[test]
    fn core_options_reach_the_worker_without_resetting_playback() {
        let (controller, mut worker) = channel();
        let generation = controller.generation();
        controller
            .set_core_option("audio_filter".into(), "low-pass".into())
            .unwrap();
        let mut received = false;
        let update = worker
            .service_operations(None, &AtomicBool::new(false), |command| {
                assert_eq!(
                    command,
                    &Command::CoreOption("audio_filter".into(), "low-pass".into())
                );
                received = true;
                Ok(())
            })
            .unwrap();
        assert!(received && update.running && !update.reset_clock);
        assert_eq!(controller.generation(), generation);
    }
    #[test]
    fn paused_worker_accepts_reset_and_resume_without_advancing_core() {
        let (mut controller, mut worker) = channel();
        controller.toggle_pause().unwrap();
        let handle = std::thread::spawn(move || {
            worker
                .service(None, &AtomicBool::new(false), || Ok(()))
                .unwrap()
        });
        std::thread::sleep(Duration::from_millis(20));
        assert!(!handle.is_finished());
        controller.reset().unwrap();
        controller.toggle_pause().unwrap();
        let update = handle.join().unwrap();
        assert!(update.running && update.reset_clock);
    }
    #[test]
    fn paused_close_does_not_wait_for_resume() {
        let (mut controller, mut worker) = channel();
        controller.toggle_pause().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let handle =
            std::thread::spawn(move || worker.service(None, &worker_stop, || Ok(())).unwrap());
        stop.store(true, Ordering::Release);
        assert!(!handle.join().unwrap().running);
    }
    #[test]
    fn full_command_queue_returns_without_blocking_window() {
        let (mut controller, _worker) = channel();
        for _ in 0..16 {
            controller.reset().unwrap();
        }
        assert!(controller.reset().is_err());
    }
    #[test]
    fn state_errors_report_to_window_and_playback_continues() {
        let (mut controller, mut worker) = channel();
        controller.configure_state_slot(std::path::PathBuf::from("private.state"));
        let old = controller.generation();
        controller.save_state().unwrap();
        let update = worker
            .service_operations(None, &AtomicBool::new(false), |command| {
                assert!(matches!(command, Command::SaveState(_)));
                Ok(())
            })
            .unwrap();
        assert!(update.running && update.reset_clock);
        assert_eq!(controller.generation(), old);
        assert!(controller.messages().next().unwrap().success);
        controller.load_state().unwrap();
        let update = worker
            .service_operations(None, &AtomicBool::new(false), |_| {
                Err("Damaged state".into())
            })
            .unwrap();
        assert!(update.running && update.reset_clock);
        assert_ne!(controller.generation(), old);
        let message = controller.messages().next().unwrap();
        assert!(!message.success);
        assert_eq!(message.text, "Damaged state");
    }
}
