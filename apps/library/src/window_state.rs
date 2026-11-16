//! Persist normal geometry separately from maximized/minimized geometry.
use crate::preferences::Preferences;
use eframe::egui::{self, ViewportBuilder, ViewportInfo};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
const KEY: &str = "native.window";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct State {
    size: [f32; 2],
    position: Option<[f32; 2]>,
    maximized: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            size: [1360., 900.],
            position: None,
            maximized: false,
        }
    }
}
fn read(prefs: &Preferences) -> State {
    prefs
        .get(KEY)
        .and_then(|s| serde_json::from_str::<State>(s).ok())
        .filter(|s| {
            s.size
                .iter()
                .all(|n| n.is_finite() && (560.0..=16384.).contains(n))
                && s.position
                    .is_none_or(|p| p.iter().all(|n| n.is_finite() && n.abs() <= 100000.))
        })
        .unwrap_or_default()
}
pub fn restore(builder: ViewportBuilder, prefs: &Preferences) -> ViewportBuilder {
    let state = read(prefs);
    let mut builder = builder
        .with_inner_size(state.size)
        .with_maximized(state.maximized);
    if let Some(pos) = state.position {
        builder = builder.with_position(pos);
    }
    builder
}
pub struct Tracker {
    state: State,
    dirty: Option<Instant>,
    pub enabled: bool,
}
impl Tracker {
    pub fn new(prefs: &Preferences, enabled: bool) -> Self {
        Self {
            state: read(prefs),
            dirty: None,
            enabled,
        }
    }
    fn observe(&mut self, v: &ViewportInfo) -> bool {
        if v.minimized == Some(true) || v.fullscreen == Some(true) {
            return false;
        }
        let old = self.state.clone();
        if let Some(maximized) = v.maximized {
            self.state.maximized = maximized;
        }
        if !self.state.maximized {
            if let Some(rect) = v.inner_rect {
                let size = [rect.width(), rect.height()];
                if size[0] >= 680. && size[1] >= 480. && size.iter().all(|n| n.is_finite()) {
                    self.state.size = size;
                }
            }
            // Wayland may withhold positions; preserve the last known one in that case.
            if let Some(rect) = v.outer_rect {
                let pos = [rect.min.x, rect.min.y];
                if pos.iter().all(|n| n.is_finite()) {
                    self.state.position = Some(pos);
                }
            }
        }
        self.state != old
    }
    pub fn update(&mut self, ctx: &egui::Context, prefs: &mut Preferences) -> bool {
        if !self.enabled {
            return false;
        }
        let v = ctx.input(|i| i.viewport().clone());
        if self.observe(&v) {
            self.dirty = Some(Instant::now());
        }
        if self.dirty.is_some() {
            if v.close_requested()
                || self
                    .dirty
                    .is_some_and(|t| t.elapsed() >= Duration::from_millis(300))
            {
                prefs.set(
                    KEY,
                    serde_json::to_string(&self.state).expect("finite geometry"),
                );
                self.dirty = None;
                return true;
            }
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        false
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maximized_and_minimized_frames_preserve_normal_geometry() {
        let mut t = Tracker::new(&Preferences::default(), true);
        let mut v = ViewportInfo {
            inner_rect: Some(egui::Rect::from_min_size(
                egui::pos2(100., 100.),
                egui::vec2(1100., 720.),
            )),
            outer_rect: Some(egui::Rect::from_min_size(
                egui::pos2(90., 70.),
                egui::vec2(1120., 760.),
            )),
            maximized: Some(false),
            ..Default::default()
        };
        assert!(t.observe(&v));
        v.maximized = Some(true);
        v.inner_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1920., 1080.),
        ));
        assert!(t.observe(&v));
        assert_eq!(t.state.size, [1100., 720.]);
        assert_eq!(t.state.position, Some([90., 70.]));
        v.minimized = Some(true);
        v.maximized = Some(false);
        assert!(!t.observe(&v));
        assert!(t.state.maximized);
    }
    #[test]
    fn geometry_round_trips_and_corrupt_values_fall_back() {
        let mut p = Preferences::default();
        let state = State {
            size: [1200., 800.],
            position: Some([-1200., 20.]),
            maximized: true,
        };
        p.set(KEY, serde_json::to_string(&state).unwrap());
        assert_eq!(read(&p), state);
        p.set(KEY, "{\"size\":[0,0],\"position\":null,\"maximized\":true}");
        assert_eq!(read(&p), State::default());
    }
}
