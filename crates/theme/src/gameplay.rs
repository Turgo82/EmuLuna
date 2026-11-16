//! Shared visuals for the player controls and the UI gallery.
use egui;

#[derive(Clone, Copy)]
pub enum Icon {
    Stop,
    Pause,
    Play,
    Reset,
    Save,
    Load,
    Folder,
    Fullscreen,
    Screenshot,
    Filter,
    Speaker,
    Muted,
    Settings,
}
fn bar_fill() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(16, 25, 35, 190)
}
fn button_fill(accent: egui::Color32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        42 + accent.r() / 8,
        42 + accent.g() / 8,
        50 + accent.b() / 8,
        105,
    )
}
pub const OVERLAY_TEXT: egui::Color32 = egui::Color32::from_rgb(241, 244, 248);
fn power_fill() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(183, 20, 34, 220)
}
pub fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    scale: f32,
) -> egui::Response {
    let mut button = egui::Button::new("")
        .corner_radius(12. * scale)
        .fill(button_fill(ui.visuals().selection.bg_fill))
        .stroke(egui::Stroke::NONE);
    let width = if matches!(icon, Icon::Stop) { 66. } else { 52. };
    if matches!(icon, Icon::Stop) {
        button = button.fill(power_fill());
    }
    let response = ui.add_enabled(
        enabled,
        button.min_size(egui::vec2(width * scale, 42. * scale)),
    );
    paint_icon(
        ui.painter(),
        icon,
        response.rect.center(),
        scale * 1.15,
        if enabled {
            OVERLAY_TEXT
        } else {
            egui::Color32::from_rgba_unmultiplied(215, 223, 234, 85)
        },
    );
    response.on_hover_text(label)
}
pub fn paint_icon(
    painter: &egui::Painter,
    icon: Icon,
    center: egui::Pos2,
    scale: f32,
    color: egui::Color32,
) {
    let p = |x: f32, y: f32| center + egui::vec2(x, y) * scale;
    let stroke = egui::Stroke::new(1.8 * scale, color);
    let line = |points: Vec<egui::Pos2>| painter.add(egui::Shape::line(points, stroke));
    match icon {
        Icon::Stop => {
            line(
                (0..25)
                    .map(|i| {
                        let a = -std::f32::consts::FRAC_PI_2
                            + 0.65
                            + i as f32 * (std::f32::consts::TAU - 1.3) / 24.;
                        p(a.cos() * 8., a.sin() * 8.)
                    })
                    .collect(),
            );
            line(vec![p(0., -10.), p(0., 0.)]);
        }
        Icon::Pause => {
            for x in [-5., 3.] {
                painter.rect_filled(
                    egui::Rect::from_min_size(p(x, -7.), egui::vec2(3., 14.) * scale),
                    scale,
                    color,
                );
            }
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![p(-5., -8.), p(8., 0.), p(-5., 8.)],
                color,
                egui::Stroke::NONE,
            ));
        }
        Icon::Reset => {
            let tip = p(6.9, -4.);
            let mut arc: Vec<_> = (0..24)
                .map(|i| {
                    let angle = egui::lerp(0.42..=5.76, i as f32 / 24.);
                    p(angle.cos() * 8., angle.sin() * 8.)
                })
                .collect();
            arc.push(tip);
            line(arc);
            line(vec![p(6.9, -9.5), tip, p(1.4, -4.)]);
        }
        Icon::Save => {
            line(vec![
                p(-8., -9.),
                p(5., -9.),
                p(9., -5.),
                p(9., 9.),
                p(-8., 9.),
                p(-8., -9.),
            ]);
            line(vec![p(-4., -9.), p(-4., -1.), p(4., -1.), p(4., -9.)]);
            line(vec![p(-4., 9.), p(-4., 3.), p(5., 3.), p(5., 9.)]);
        }
        Icon::Load => {
            line(vec![p(-9., 3.), p(-9., 9.), p(9., 9.), p(9., 3.)]);
            line(vec![p(0., -10.), p(0., 3.)]);
            line(vec![p(-5., -2.), p(0., 3.), p(5., -2.)]);
        }
        Icon::Folder => {
            line(vec![
                p(-9., 7.),
                p(-9., -7.),
                p(-3., -7.),
                p(0., -4.),
                p(9., -4.),
                p(9., 7.),
                p(-9., 7.),
            ]);
        }
        Icon::Filter => {
            for (y, x) in [(-7., 3.), (0., -4.), (7., 2.)] {
                line(vec![p(-10., y), p(x - 3., y)]);
                line(vec![p(x + 3., y), p(10., y)]);
                painter.circle_stroke(p(x, y), 2.7 * scale, stroke);
            }
        }
        Icon::Settings => {
            painter.circle_stroke(center, 8. * scale, stroke);
            painter.circle_stroke(center, 3. * scale, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                line(vec![
                    p(a.cos() * 8., a.sin() * 8.),
                    p(a.cos() * 11., a.sin() * 11.),
                ]);
            }
        }
        Icon::Speaker | Icon::Muted => {
            line(vec![
                p(-9., -4.),
                p(-5., -4.),
                p(1., -9.),
                p(1., 9.),
                p(-5., 4.),
                p(-9., 4.),
                p(-9., -4.),
            ]);
            if matches!(icon, Icon::Muted) {
                line(vec![p(5., -4.), p(12., 4.)]);
                line(vec![p(5., 4.), p(12., -4.)]);
            } else {
                for radius in [6., 10.] {
                    line(
                        (0..17)
                            .map(|i| {
                                let angle = -0.8 + i as f32 * 1.6 / 16.;
                                p(2. + radius * angle.cos(), radius * angle.sin())
                            })
                            .collect(),
                    );
                }
            }
        }
        Icon::Screenshot => {
            line(vec![
                p(-9., -5.),
                p(-4., -5.),
                p(-2., -8.),
                p(4., -8.),
                p(6., -5.),
                p(9., -5.),
                p(9., 8.),
                p(-9., 8.),
                p(-9., -5.),
            ]);
            painter.circle_stroke(p(0., 1.), 4. * scale, stroke);
        }
        Icon::Fullscreen => {
            for (x, y) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
                line(vec![
                    p(x * 3., y * 8.),
                    p(x * 8., y * 8.),
                    p(x * 8., y * 3.),
                ]);
            }
        }
    }
}

pub fn bar_style(ui: &mut egui::Ui, scale: f32) {
    ui.spacing_mut().item_spacing = egui::vec2(12. * scale, 10. * scale);
    ui.spacing_mut().interact_size.y = 42. * scale;
    ui.spacing_mut().button_padding = egui::vec2(12. * scale, 7. * scale);
    ui.style_mut().override_font_id = Some(egui::FontId::proportional(17. * scale));
    let visuals = ui.visuals_mut();
    visuals.override_text_color = Some(OVERLAY_TEXT);
    let button_color = button_fill(visuals.selection.bg_fill);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.bg_fill = button_color;
        widget.weak_bg_fill = button_color;
        widget.bg_stroke = egui::Stroke::NONE;
        widget.fg_stroke.color = OVERLAY_TEXT;
        widget.corner_radius = (12. * scale).into();
    }
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgba_unmultiplied(90, 104, 122, 200);
    visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
    visuals.widgets.active.bg_fill = egui::Color32::from_rgba_unmultiplied(110, 123, 142, 220);
}

pub const MENU_ROW_HEIGHT: f32 = 32.;
pub const MENU_PADDING: i8 = 6;

#[derive(Clone, Copy)]
pub enum MenuMarker {
    None,
    Icon(Icon),
    Checked,
}

pub fn menu_row(
    ui: &mut egui::Ui,
    label: &str,
    trailing: &str,
    marker: MenuMarker,
    open: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), MENU_ROW_HEIGHT),
        egui::Sense::click(),
    );
    let painter = ui.painter_at(rect);
    let highlighted = ui.is_enabled() && (open || response.hovered() || response.has_focus());
    let visuals = ui.visuals();
    let accent = visuals.selection.bg_fill;
    let color = if highlighted {
        painter.rect_filled(rect, 4., accent);
        visuals.selection.stroke.color
    } else if ui.is_enabled() {
        visuals.text_color()
    } else {
        visuals.weak_text_color()
    };
    let left = rect.left_center();
    match marker {
        MenuMarker::None => {}
        MenuMarker::Icon(icon) => {
            paint_icon(&painter, icon, left + egui::vec2(10., 0.), 0.66, color)
        }
        MenuMarker::Checked => {
            let center = left + egui::vec2(9., 0.);
            let stroke = egui::Stroke::new(1.5_f32, color);
            painter.line_segment(
                [center + egui::vec2(-3., 0.), center + egui::vec2(-1., 2.)],
                stroke,
            );
            painter.line_segment(
                [center + egui::vec2(-1., 2.), center + egui::vec2(3., -3.)],
                stroke,
            );
        }
    }
    painter.text(
        left + egui::vec2(30., 0.),
        egui::Align2::LEFT_CENTER,
        label,
        egui::TextStyle::Button.resolve(ui.style()),
        color,
    );
    if trailing == "▶" {
        let center = rect.right_center() - egui::vec2(12., 0.);
        painter.add(egui::Shape::convex_polygon(
            vec![
                center + egui::vec2(-2., -4.),
                center + egui::vec2(2., 0.),
                center + egui::vec2(-2., 4.),
            ],
            color.gamma_multiply(0.75),
            egui::Stroke::NONE,
        ));
    } else if !trailing.is_empty() {
        painter.text(
            rect.right_center() - egui::vec2(70., 0.),
            egui::Align2::LEFT_CENTER,
            trailing,
            egui::TextStyle::Button.resolve(ui.style()),
            color,
        );
    }
    response
}

pub fn game_menu_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::popup(ui.style())
        .fill(ui.visuals().window_fill)
        .inner_margin(MENU_PADDING)
        .corner_radius(8)
}

pub fn bar_frame(scale: f32) -> egui::Frame {
    egui::Frame::new()
        .fill(bar_fill())
        .stroke(egui::Stroke::new(
            1_f32,
            egui::Color32::from_rgba_unmultiplied(92, 107, 122, 75),
        ))
        .corner_radius(22. * scale)
        .inner_margin(12. * scale)
}

pub fn volume_slider(ui: &mut egui::Ui, volume: &mut f32, scale: f32) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = 130. * scale;
        ui.spacing_mut().interact_size.y = 20. * scale;
        ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::from_rgb(91, 100, 112);
        ui.visuals_mut().widgets.inactive.corner_radius = (4. * scale).into();
        ui.spacing_mut().slider_rail_height = 7. * scale;
        // Keep egui's keyboard/accessibility handling, then paint a crisp white thumb.
        let response = ui
            .add(
                egui::Slider::new(volume, 0.0..=1.0)
                    .show_value(false)
                    .trailing_fill(true),
            )
            .on_hover_text("Volume");
        let radius = 7. * scale;
        let center = egui::pos2(
            egui::lerp(
                response.rect.left() + response.rect.height() / 2.5
                    ..=response.rect.right() - response.rect.height() / 2.5,
                *volume,
            ),
            response.rect.center().y,
        );
        ui.painter().circle_filled(
            center,
            radius + 3. * scale,
            egui::Color32::from_rgba_unmultiplied(230, 236, 246, 55),
        );
        ui.painter().circle_filled(center, radius, OVERLAY_TEXT);
        response
    })
    .inner
}
