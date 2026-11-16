//! Safe playback contracts shared by the native interface and core workers.
#![forbid(unsafe_code)]
pub mod audio_clock;
pub mod input;
pub mod input_script;
pub mod pacing;

/// Distinguishes resources across reset, resize and session replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation(pub u64);

/// Emulation and video content advance independently on duplicate callbacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameIdentity {
    pub generation: Generation,
    pub emulation_sequence: u64,
    pub content_sequence: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Pause(bool),
    Reset,
    SaveState(std::path::PathBuf),
    LoadState(std::path::PathBuf),
    Volume(f32),
    CoreOption(String, String),
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Stopped,
    Running,
    Paused,
}

/// Control state owned by the core worker, never the interface thread.
#[derive(Debug)]
pub struct SessionControl {
    pub state: SessionState,
    generation: Generation,
    volume: f32,
}

impl Default for SessionControl {
    fn default() -> Self {
        Self {
            state: SessionState::Stopped,
            generation: Generation(0),
            volume: 1.0,
        }
    }
}

impl SessionControl {
    pub fn generation(&self) -> Generation {
        self.generation
    }
    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn start(&mut self) {
        self.invalidate_frames();
        self.state = SessionState::Running;
    }

    fn invalidate_frames(&mut self) {
        self.generation.0 = self
            .generation
            .0
            .checked_add(1)
            .expect("generation exhausted");
    }

    pub fn apply(&mut self, command: Command) {
        match command {
            Command::Pause(paused) if self.state != SessionState::Stopped => {
                self.state = if paused {
                    SessionState::Paused
                } else {
                    SessionState::Running
                };
            }
            Command::Reset | Command::LoadState(_) if self.state != SessionState::Stopped => {
                self.invalidate_frames()
            }
            Command::Stop => {
                self.invalidate_frames();
                self.state = SessionState::Stopped;
            }
            Command::Volume(value) if value.is_finite() => self.volume = value.clamp(0.0, 1.0),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resets_and_restarts_invalidate_old_frames_without_unpausing() {
        let mut session = SessionControl::default();
        session.start();
        let old = session.generation();
        session.apply(Command::Pause(true));
        session.apply(Command::Reset);
        assert_ne!(old, session.generation());
        assert_eq!(session.state, SessionState::Paused);
        session.apply(Command::Stop);
        session.apply(Command::Pause(false));
        assert_eq!(session.state, SessionState::Stopped);
    }

    #[test]
    fn volume_rejects_invalid_values_and_clamps_valid_values() {
        let mut session = SessionControl::default();
        session.apply(Command::Volume(0.25));
        session.apply(Command::Volume(f32::NAN));
        assert_eq!(session.volume(), 0.25);
        session.apply(Command::Volume(3.0));
        assert_eq!(session.volume(), 1.0);
    }
}
