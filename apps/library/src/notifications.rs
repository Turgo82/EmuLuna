use eframe::egui;
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub struct Notifications {
    history: Vec<Notice>,
    seen: HashMap<&'static str, String>,
    unread: u8,
    open: bool,
}

struct Notice {
    source: &'static str,
    active: bool,
    message: String,
    time: String,
}

impl Notifications {
    pub fn update(&mut self, source: &'static str, message: &str, active: bool) {
        let message = message.trim();
        let previous = self.seen.entry(source).or_default();
        let changed = previous != message;
        if changed {
            *previous = message.into();
        }
        let existing = self
            .history
            .iter_mut()
            .find(|notice| notice.source == source && notice.active);
        if let Some(notice) = existing {
            if !message.is_empty() && changed {
                notice.message = message.chars().take(2000).collect();
                notice.time = timestamp();
            }
            notice.active = active;
            if !active && !self.open {
                self.unread = (self.unread + 1).min(99);
            }
            return;
        }
        if message.is_empty() || !changed {
            return;
        }
        if !active
            && self
                .history
                .first()
                .is_some_and(|notice| notice.message == message)
        {
            return;
        }
        self.history.insert(
            0,
            Notice {
                source,
                active,
                message: message.chars().take(2000).collect(),
                time: timestamp(),
            },
        );
        if !self.open {
            self.unread = (self.unread + 1).min(99);
        }
        if self.history.len() > 100 {
            if let Some(index) = self.history.iter().rposition(|notice| !notice.active) {
                self.history.remove(index);
            }
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, activity: impl FnOnce(&mut egui::Ui) -> bool) {
        let label = if self.unread == 0 {
            "Notifications".to_owned()
        } else {
            format!("Notifications · {} unread", self.unread)
        };
        let bell = crate::actions::toolbar_button(
            ui,
            crate::actions::ToolbarIcon::Bell,
            &label,
            self.open,
        );
        if self.unread > 0 {
            let dot = bell.rect.right_top() + egui::vec2(-6., 6.);
            ui.painter()
                .circle_filled(dot, 5., ui.visuals().window_fill);
            ui.painter()
                .circle_filled(dot, 3.5, ui.visuals().selection.bg_fill);
        }
        let id = egui::Id::new("notification-panel");
        if bell.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(id));
        }
        self.open = ui.memory(|memory| memory.is_popup_open(id));
        if !self.open {
            return;
        }
        self.unread = 0;
        let ctx = ui.ctx();
        let screen = ctx.screen_rect();
        let width = 380_f32.min(screen.width() - 16.).max(160.);
        let top = bell.rect.bottom() + 6.;
        let height = 440_f32.min(screen.bottom() - top - 8.).max(100.);
        let pos = egui::pos2((bell.rect.right() - width).max(screen.left() + 8.), top);
        let mut close = false;
        let area = egui::Area::new(id)
            .kind(egui::UiKind::Popup)
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .default_size(egui::vec2(width, height))
            .movable(false)
            .fade_in(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(ui.visuals().window_fill)
                    .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                    .corner_radius(8.)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(width - 26.);
                        let content_top = ui.cursor().top();
                        ui.horizontal(|ui| {
                            ui.strong("Notifications");
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(
                                            self.history.iter().any(|notice| !notice.active),
                                            egui::Button::new("Clear"),
                                        )
                                        .clicked()
                                    {
                                        self.history.retain(|notice| notice.active);
                                    }
                                },
                            );
                        });
                        ui.separator();
                        close = activity(ui);
                        let remaining = (height - 26. - (ui.cursor().top() - content_top)).max(0.);
                        if self.history.is_empty() {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), remaining),
                                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                                |ui| {
                                    ui.weak("You’re all caught up.");
                                },
                            );
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("recent-notifications")
                                .max_height(remaining)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for notice in &self.history {
                                        ui.weak(&notice.time);
                                        ui.add(egui::Label::new(&notice.message).wrap());
                                        ui.separator();
                                    }
                                });
                        }
                    });
            });
        let outside = ctx.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| {
                    !bell.rect.contains(pos) && !area.response.rect.contains(pos)
                })
        });
        if close || outside || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            ctx.memory_mut(|memory| memory.close_popup());
            self.open = false;
        }
    }
}

fn timestamp() -> String {
    let minutes = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 60;
    format!("{:02}:{:02} UTC", minutes / 60 % 24, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_reuses_a_row_and_completion_marks_it_unread() {
        let mut panel = Notifications::default();
        panel.update("import", "Importing game 1", true);
        panel.update("import", "Importing game 2", true);
        assert_eq!(panel.history.len(), 1);
        assert_eq!(panel.unread, 1);
        panel.update("import", "Imported 2 games", false);
        assert_eq!(panel.history[0].message, "Imported 2 games");
        assert!(!panel.history[0].active);
        assert_eq!(panel.unread, 2);
    }

    #[test]
    fn open_panel_fits_and_leaves_room_for_scrolling_history() {
        let ctx = egui::Context::default();
        let mut panel = Notifications::default();
        for index in 0..110 {
            panel.update("session", &format!("Notice {index}"), false);
        }
        assert_eq!(panel.history.len(), 100);
        assert_eq!(panel.unread, 99);
        let id = egui::Id::new("notification-panel");
        ctx.memory_mut(|memory| memory.open_popup(id));
        for _ in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            panel.show(ui, |_| false);
                        });
                    });
                },
            );
        }
        let bounds = ctx.memory(|memory| memory.area_rect(id)).unwrap();
        assert!(ctx.screen_rect().contains_rect(bounds));
        assert!((bounds.height() - 440.).abs() <= 4., "{bounds:?}");
        assert_eq!(panel.unread, 0);
    }
}
