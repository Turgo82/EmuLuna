//! Window preferences and ratio-preserving resize grip, on the display thread.
use super::*;
use sdl2::{
    event::{Event, WindowEvent},
    mouse::MouseButton,
};
#[derive(Clone, Copy)]
pub struct WindowPreferences {
    pub lock_aspect: bool,
    pub integer_scaling: bool,
    pub square_pixels: bool,
    pub pause_unfocused: bool,
    pub hide_cursor: bool,
    pub keep_awake: bool,
    pub volume: f32,
}
impl Default for WindowPreferences {
    fn default() -> Self {
        Self {
            lock_aspect: false,
            integer_scaling: false,
            square_pixels: false,
            pause_unfocused: false,
            hide_cursor: true,
            keep_awake: true,
            volume: 1.,
        }
    }
}
#[derive(Default)]
pub(super) struct WindowState {
    pub preferences: WindowPreferences,
    ratio: Option<f64>,
    drag: Option<(i32, i32, u32, u32)>,
    paused_by_focus: bool,
}
fn locked_size(width: u32, height: u32, dx: i32, dy: i32, ratio: f64) -> (u32, u32) {
    let desired = if dx.unsigned_abs() as f64 >= dy.unsigned_abs() as f64 * ratio {
        f64::from(width) + f64::from(dx)
    } else {
        (f64::from(height) + f64::from(dy)) * ratio
    };
    let width = desired
        .clamp(320_f64.max(240. * ratio), 7680_f64.min(4320. * ratio))
        .round() as u32;
    (
        width,
        (f64::from(width) / ratio).round().clamp(240., 4320.) as u32,
    )
}
impl Presenter {
    pub fn configure_window(&mut self, preferences: WindowPreferences) {
        self.window_state.preferences = preferences;
        self.window.set_resizable(!preferences.lock_aspect);
        if preferences.keep_awake {
            self.window.subsystem().disable_screen_saver();
        } else {
            self.window.subsystem().enable_screen_saver();
        }
    }
    pub fn initial_volume(&self) -> f32 {
        self.window_state.preferences.volume
    }
    pub fn window_event(
        &mut self,
        event: &Event,
        commands: &mut emuluna_playback_control::Controller,
    ) -> Result<(), String> {
        match *event {
            Event::MouseButtonDown {
                window_id,
                mouse_btn: MouseButton::Left,
                x,
                y,
                ..
            } if window_id == self.window.id()
                && self.window_state.preferences.lock_aspect
                && self.window.fullscreen_state() == FullscreenType::Off =>
            {
                let (w, h) = self.window.size();
                if x >= w as i32 - 24 && y >= h as i32 - 24 {
                    self.window_state.drag = Some((x, y, w, h));
                }
            }
            Event::MouseButtonUp {
                mouse_btn: MouseButton::Left,
                ..
            } => self.window_state.drag = None,
            Event::MouseMotion {
                window_id, x, y, ..
            } if window_id == self.window.id() => {
                if let Some((start_x, start_y, w, h)) = self.window_state.drag {
                    if let Some(ratio) = self.window_state.ratio {
                        let (w, h) = locked_size(w, h, x - start_x, y - start_y, ratio);
                        self.window.set_size(w, h).map_err(|e| e.to_string())?;
                    }
                }
            }
            Event::Window {
                window_id,
                win_event: WindowEvent::FocusLost,
                ..
            } if window_id == self.window.id() => {
                self.window_state.drag = None;
                if self.window_state.preferences.pause_unfocused && !commands.paused() {
                    commands.toggle_pause()?;
                    self.window_state.paused_by_focus = true;
                }
            }
            Event::Window {
                window_id,
                win_event: WindowEvent::FocusGained,
                ..
            } if window_id == self.window.id() && self.window_state.paused_by_focus => {
                if commands.paused() {
                    commands.toggle_pause()?;
                }
                self.window_state.paused_by_focus = false;
            }
            _ => {}
        }
        Ok(())
    }
    pub(super) fn update_window_aspect(&mut self, input: FrameView) -> Result<(), String> {
        let ratio = if self.window_state.preferences.square_pixels {
            f64::from(input.image.width) / f64::from(input.image.height.max(1))
        } else {
            input.aspect
        };
        if !ratio.is_finite() || !(0.2..=5.).contains(&ratio) {
            return Ok(());
        }
        if self
            .window_state
            .ratio
            .is_none_or(|last| (last - ratio).abs() > 0.002)
        {
            self.window_state.ratio = Some(ratio);
            if self.window_state.preferences.lock_aspect
                && self.window.fullscreen_state() == FullscreenType::Off
            {
                let (mut w, h) = self.window.size();
                if let Ok(bounds) = self
                    .window
                    .display_index()
                    .and_then(|index| self.window.subsystem().display_usable_bounds(index))
                {
                    w = w
                        .min(bounds.width().saturating_sub(32))
                        .min((f64::from(bounds.height().saturating_sub(64)) * ratio) as u32);
                }
                let (w, h) = locked_size(w, h, 0, 0, ratio);
                self.window.set_size(w, h).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    pub(super) fn content_viewport(
        &self,
        width: u32,
        height: u32,
        view: FrameView,
    ) -> (i32, i32, i32, i32) {
        let source = self.last_source.unwrap_or(view);
        let ratio = if self.window_state.preferences.square_pixels
            && !(self.n64_preserve_aspect && self.n64_crop.is_some())
        {
            f64::from(source.image.width) / f64::from(source.image.height.max(1))
        } else {
            view.aspect
        };
        if self.window_state.preferences.integer_scaling && source.image.height > 0 {
            let base_h = f64::from(source.image.height);
            let base_w = base_h * ratio;
            let scale = (f64::from(width) / base_w)
                .min(f64::from(height) / base_h)
                .floor();
            if scale >= 1. {
                let w = (base_w * scale).round() as i32;
                let h = (base_h * scale).round() as i32;
                return ((width as i32 - w) / 2, (height as i32 - h) / 2, w, h);
            }
        }
        viewport(width, height, ratio)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corner_resize_always_preserves_core_ratio() {
        for ratio in [4. / 3., 8. / 7., 16. / 9., 1.] {
            for (dx, dy) in [(120, 0), (0, 170), (-100, -200), (1000, 40), (-5000, -5000)] {
                let (w, h) = locked_size(960, 720, dx, dy, ratio);
                assert!((f64::from(w) / f64::from(h) - ratio).abs() < 0.006);
                assert!(w >= 320 && h >= 240);
            }
        }
    }
}
