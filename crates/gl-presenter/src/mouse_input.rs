use super::Presenter;
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::{Keycode, Mod},
    mouse::{MouseButton, MouseUtil, MouseWheelDirection},
};
use std::sync::{Arc, Mutex};

// Button bits use the libretro RETRO_DEVICE_ID_MOUSE_* positions.
const LEFT: u32 = 1 << 2;
const RIGHT: u32 = 1 << 3;
const WHEEL_UP: u32 = 1 << 4;
const WHEEL_DOWN: u32 = 1 << 5;
const MIDDLE: u32 = 1 << 6;

#[derive(Default)]
struct Pending {
    dx: i16,
    dy: i16,
    held: u32,
    pressed: u32,
    remainder: [f32; 2],
}

impl Pending {
    fn motion(&mut self, x: i32, y: i32) {
        let add = |axis: i16, delta: i32| {
            i32::from(axis)
                .saturating_add(delta)
                .clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
        };
        self.dx = add(self.dx, x);
        self.dy = add(self.dy, y);
    }

    fn tuned_motion(&mut self, x: i32, y: i32, speed: f32, acceleration: f32) {
        let distance = (x as f32).hypot(y as f32);
        let gain = speed * (1.0 + acceleration * 0.5 * (distance / 24.0).min(1.0));
        let mut steps = [0; 2];
        for ((step, remainder), delta) in steps.iter_mut().zip(&mut self.remainder).zip([x, y]) {
            let movement = delta as f32 * gain + *remainder;
            *step = movement.trunc() as i32;
            *remainder = movement.fract();
        }
        self.motion(steps[0], steps[1]);
    }

    fn button(&mut self, button: u32, down: bool) {
        if down {
            self.held |= button;
            self.pressed |= button;
        } else {
            self.held &= !button;
        }
    }

    fn take_frame(&mut self) -> (i16, i16, u32) {
        (
            std::mem::take(&mut self.dx),
            std::mem::take(&mut self.dy),
            self.held | std::mem::take(&mut self.pressed),
        )
    }
}

pub struct MouseFrames(Option<Arc<Mutex<Pending>>>);

impl MouseFrames {
    pub fn update(&self, core: &mut emuluna_libretro_host::Core) {
        if let Some(pending) = &self.0 {
            let (dx, dy, buttons) = pending.lock().unwrap().take_frame();
            core.set_mouse_input(dx, dy, buttons);
        }
    }
}

pub struct GameMouse {
    pending: Option<Arc<Mutex<Pending>>>,
    mouse: MouseUtil,
    captured: bool,
    window_id: u32,
}

impl Presenter {
    pub fn game_mouse(&mut self, options: &[(String, String)]) -> (GameMouse, MouseFrames) {
        let pending = emuluna_libretro_host::mouse_enabled(options)
            .then(|| Arc::new(Mutex::new(Pending::default())));
        if let Some(overlay) = &mut self.overlay {
            overlay.mouse_port_active = pending.is_some();
        }
        (
            GameMouse {
                pending: pending.clone(),
                mouse: self.window.subsystem().sdl().mouse(),
                captured: false,
                window_id: self.window.id(),
            },
            MouseFrames(pending),
        )
    }

    pub fn game_mouse_event(&mut self, mouse: &mut GameMouse, event: &Event, paused: bool) -> bool {
        let captured = mouse.captured;
        let (consumed, message) = mouse.event(event, paused, self.mouse_tuning());
        if captured != mouse.captured {
            self.set_mouse_captured(mouse.captured);
        }
        if let Some(message) = message {
            self.overlay_message(message);
        }
        consumed
    }

    pub fn service_mouse_grab(&mut self, mouse: &mut GameMouse) {
        let requested = self
            .overlay
            .as_mut()
            .is_some_and(|overlay| std::mem::take(&mut overlay.grab_mouse_requested));
        if requested {
            let message = mouse.grab();
            self.set_mouse_captured(mouse.captured);
            self.overlay_message(message);
        }
    }
}

impl GameMouse {
    fn release(&mut self) {
        if self.captured {
            self.mouse.set_relative_mouse_mode(false);
            self.mouse.show_cursor(true);
            self.captured = false;
            *self.pending.as_ref().unwrap().lock().unwrap() = Pending::default();
        }
    }

    fn grab(&mut self) -> String {
        if self.pending.is_none() {
            return "Choose a mouse port and reopen the game".into();
        }
        self.mouse.set_relative_mouse_mode(true);
        self.captured = self.mouse.relative_mouse_mode();
        if self.captured {
            "Mouse grabbed. Press Esc to release".into()
        } else {
            let error = sdl2::get_error();
            eprintln!("Could not grab gameplay mouse: {error}");
            if error.is_empty() {
                "Could not grab the mouse".into()
            } else {
                format!("Could not grab the mouse: {error}")
            }
        }
    }

    fn event(
        &mut self,
        event: &Event,
        paused: bool,
        (speed, acceleration): (f32, f32),
    ) -> (bool, Option<String>) {
        let Some(pending) = &self.pending else {
            return (false, None);
        };
        let Some(window_id) = event.get_window_id() else {
            return (false, None);
        };
        if window_id != self.window_id {
            return (false, None);
        }
        match *event {
            Event::Window {
                win_event: WindowEvent::FocusLost | WindowEvent::Minimized | WindowEvent::Hidden,
                ..
            } => {
                self.release();
                return (false, None);
            }
            Event::KeyDown {
                keycode: Some(Keycode::M),
                keymod,
                repeat,
                ..
            } if keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) => {
                if repeat {
                    return (true, None);
                }
                if self.captured {
                    self.release();
                    return (true, Some("Mouse released".into()));
                }
                return (true, Some(self.grab()));
            }
            Event::KeyDown {
                keycode: Some(Keycode::Escape),
                repeat,
                ..
            } if self.captured || repeat => {
                let message = self.captured.then(|| "Mouse released".into());
                self.release();
                return (true, message);
            }
            _ => {}
        }
        if !self.captured {
            return (false, None);
        }
        match *event {
            Event::MouseMotion { xrel, yrel, .. } => {
                if !paused {
                    pending
                        .lock()
                        .unwrap()
                        .tuned_motion(xrel, yrel, speed, acceleration);
                }
            }
            Event::MouseButtonDown { mouse_btn, .. } | Event::MouseButtonUp { mouse_btn, .. } => {
                let button = match mouse_btn {
                    MouseButton::Left => LEFT,
                    MouseButton::Right => RIGHT,
                    MouseButton::Middle => MIDDLE,
                    _ => 0,
                };
                let down = matches!(event, Event::MouseButtonDown { .. });
                if !paused || !down {
                    pending.lock().unwrap().button(button, down);
                }
            }
            Event::MouseWheel { y, direction, .. } => {
                if y != 0 && !paused {
                    let up = (y > 0) != (direction == MouseWheelDirection::Flipped);
                    pending.lock().unwrap().pressed |= if up { WHEEL_UP } else { WHEEL_DOWN };
                }
            }
            _ => return (false, None),
        }
        (true, None)
    }
}

impl Drop for GameMouse {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_keeps_fractional_motion_and_acceleration_scales_larger_movements() {
        let mut pending = Pending::default();
        pending.tuned_motion(-1, 1, 0.5, 0.);
        assert_eq!(pending.take_frame(), (0, 0, 0));
        pending.tuned_motion(-1, 1, 0.5, 0.);
        assert_eq!(pending.take_frame(), (-1, 1, 0));
        pending.tuned_motion(24, 0, 1., 2.);
        assert_eq!(pending.take_frame(), (48, 0, 0));
    }

    #[test]
    fn motion_and_clicks_are_delivered_once_while_held_buttons_persist() {
        let mut pending = Pending::default();
        pending.motion(9, -5);
        pending.motion(-3, 2);
        pending.button(LEFT, true);
        pending.button(RIGHT, true);
        pending.button(RIGHT, false);
        assert_eq!(pending.take_frame(), (6, -3, LEFT | RIGHT));
        assert_eq!(pending.take_frame(), (0, 0, LEFT));
        pending.button(LEFT, false);
        assert_eq!(pending.take_frame(), (0, 0, 0));
    }
}
