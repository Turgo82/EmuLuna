//! Theme-aware progress shared by the library sidebar and disc backup.
use eframe::egui;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct Recent {
    title: String,
    detail: String,
    progress: (u64, u64),
    last_drawn: f64,
}
impl Recent {
    pub(crate) fn get(ctx: &egui::Context) -> Option<Self> {
        let last =
            ctx.data(|data| data.get_temp::<Self>(egui::Id::new("sidebar-last-activity")))?;
        let remaining = 0.45 - (ctx.input(|input| input.time) - last.last_drawn);
        if remaining > 0. {
            ctx.request_repaint_after(Duration::from_secs_f64(remaining));
            Some(last)
        } else {
            None
        }
    }
    pub(crate) fn show(&self, ui: &mut egui::Ui) {
        draw_sidebar(ui, &self.title, &self.detail, self.progress, None, false);
    }
}
pub(crate) fn progress(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    done: u64,
    total: u64,
    moving: bool,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width.max(0.), height), egui::Sense::hover());
    let visuals = ui.visuals();
    let painter = ui.painter();
    let radius = 4.;
    painter.rect(
        rect,
        radius,
        visuals.extreme_bg_color,
        egui::Stroke::new(1_f32, visuals.widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    let track = rect.shrink(1.);
    let time = ui.input(|input| input.time) as f32;
    let fill = if total == 0 {
        let span = track.width() * 0.25;
        let x = track.left() + (time * 0.75).fract() * (track.width() + span) - span;
        egui::Rect::from_min_size(egui::pos2(x, track.top()), egui::vec2(span, track.height()))
            .intersect(track)
    } else {
        egui::Rect::from_min_size(
            track.min,
            egui::vec2(
                track.width() * (done as f32 / total as f32).clamp(0., 1.),
                track.height(),
            ),
        )
    };
    if fill.is_positive() {
        painter.rect_filled(fill, 3., visuals.selection.bg_fill);
        if moving && total != 0 && done < total {
            let painter = painter.with_clip_rect(fill.shrink(1.));
            let accent = visuals.selection.bg_fill;
            let lighten = |channel: u8| channel + (255 - channel) / 7;
            let shine = egui::Color32::from_rgb(
                lighten(accent.r()),
                lighten(accent.g()),
                lighten(accent.b()),
            );
            let mut x = track.left() - 32. + (time * 37.5).rem_euclid(32.);
            while x < fill.right() {
                painter.line_segment(
                    [
                        egui::pos2(x, fill.bottom()),
                        egui::pos2(x + 16., fill.top()),
                    ],
                    egui::Stroke::new(6_f32, shine),
                );
                x += 32.;
            }
        }
    }
    if moving {
        ui.ctx().request_repaint_after(Duration::from_millis(40));
    }
}

pub(crate) fn sidebar(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    progress_counts: (u64, u64),
    cancel: Option<bool>,
) -> bool {
    let last_drawn = ui.input(|input| input.time);
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("sidebar-last-activity"),
            Recent {
                title: title.to_owned(),
                detail: detail.to_owned(),
                progress: progress_counts,
                last_drawn,
            },
        );
    });
    draw_sidebar(
        ui,
        title,
        detail,
        progress_counts,
        cancel,
        cancel != Some(false),
    )
}

fn draw_sidebar(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    progress_counts: (u64, u64),
    cancel: Option<bool>,
    moving: bool,
) -> bool {
    ui.strong(title);
    let clicked = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.;
            let width = ui.available_width() - if cancel.is_some() { 26. } else { 0. };
            progress(ui, width, 9., progress_counts.0, progress_counts.1, moving);
            if let Some(enabled) = cancel {
                ui.add_enabled_ui(enabled, |ui| {
                    let button = ui.add_sized(
                        [20., 20.],
                        egui::Button::new("")
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(5.),
                    );
                    if button.hovered() {
                        ui.painter().rect_filled(
                            button.rect,
                            5.,
                            ui.visuals().widgets.hovered.weak_bg_fill,
                        );
                    }
                    crate::actions::paint_toolbar_icon(
                        ui,
                        crate::actions::ToolbarIcon::Clear,
                        button.rect,
                        false,
                    );
                    button.on_hover_text("Cancel").clicked()
                })
                .inner
            } else {
                false
            }
        })
        .inner;
    ui.add(egui::Label::new(detail).truncate())
        .on_hover_text(detail);
    clicked
}
