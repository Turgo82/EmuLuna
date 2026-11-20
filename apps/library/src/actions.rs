use eframe::egui;
use emuluna_library_model::Game;
use std::path::PathBuf;

pub(crate) fn menu_row(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: &str,
    enabled: bool,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.), egui::Sense::click());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
        let visuals = ui.visuals();
        let selected = response.hovered() || response.has_focus();
        let color = if selected {
            ui.painter()
                .rect_filled(rect, 4., visuals.selection.bg_fill);
            visuals.selection.stroke.color
        } else {
            visuals.text_color()
        };
        let font = egui::TextStyle::Button.resolve(ui.style());
        ui.painter().text(
            rect.left_center() + egui::vec2(11., 0.),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            color,
        );
        if shortcut == "✓" {
            let center = rect.right_center() - egui::vec2(18., 0.);
            ui.painter().line(
                vec![
                    center + egui::vec2(-5., 0.),
                    center + egui::vec2(-1.5, 3.5),
                    center + egui::vec2(5., -4.),
                ],
                egui::Stroke::new(1.6, color),
            );
        } else if !shortcut.is_empty() {
            ui.painter().text(
                rect.right_center() - egui::vec2(11., 0.),
                egui::Align2::RIGHT_CENTER,
                shortcut,
                font,
                color,
            );
        }
        response
    })
    .inner
}

pub(crate) fn search_frame(ui: &egui::Ui, theme: &emuluna_theme::Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.color("button", ui.visuals().widgets.inactive.bg_fill))
        .stroke(egui::Stroke::new(
            1_f32,
            theme.color("mid", ui.visuals().widgets.noninteractive.bg_stroke.color),
        ))
        .corner_radius(8.)
        .inner_margin(egui::Margin::symmetric(6, 1))
}

pub(crate) fn search_input(query: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(query)
        .frame(false)
        .return_key(None)
        .hint_text("Search")
        .horizontal_align(egui::Align::Min)
        .vertical_align(egui::Align::Center)
        .min_size(egui::vec2(0., 30.))
        .margin(egui::Margin::symmetric(6, 2))
}

pub enum Action {
    Play(Game),
    StartFromBoot(Game),
    SelectVersion(String, String),
    Details(String),
    Favorite(String, bool),
    Rating(String, u8),
    Reveal(PathBuf),
    FindCover(Game),
    Remove(Vec<String>),
    NewCollection(Vec<String>),
}
fn version_options(
    ui: &mut egui::Ui,
    group: &crate::versions::Group,
    games: &[Game],
    available: &[bool],
    playback: &Game,
) -> Option<Action> {
    ui.set_width(176.);
    ui.spacing_mut().item_spacing.y = 0.;
    for version in &group.versions {
        let candidate = &games[version.game];
        let marker = if candidate.id == playback.id { "✓" } else { "" };
        let filename = candidate.rom.file_name().unwrap_or_default().to_string_lossy();
        if menu_row(ui, &version.label, marker, available[version.game])
            .on_hover_text(filename)
            .clicked()
        {
            return Some(Action::SelectVersion(
                group.preference.clone(),
                candidate.id.clone(),
            ));
        }
    }
    None
}

pub fn version_badge(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    group: &crate::versions::Group,
    games: &[Game],
    available: &[bool],
    playback: &Game,
) -> Option<Action> {
    let count = group.versions.len();
    let label = if rect.width() >= 78. {
        format!("{count} versions")
    } else {
        count.to_string()
    };
    let popup = ui.id().with(("version-badge", &group.preference));
    let response = ui.interact(rect, popup.with("button"), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(
        egui::WidgetType::Button, true, format!("{count} versions"),
    ));
    let open = ui.memory(|memory| memory.is_popup_open(popup));
    let visuals = ui.visuals();
    let (fill, text) = if response.hovered() || open {
        (visuals.selection.bg_fill, visuals.selection.stroke.color)
    } else {
        (visuals.window_fill, visuals.text_color())
    };
    ui.painter().rect(
        rect, 6., fill, visuals.window_stroke, egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(7., 0.),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.),
        text,
    );
    let arrow = rect.right_center() - egui::vec2(8., 0.);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![arrow + egui::vec2(-3., -1.), arrow + egui::vec2(3., -1.), arrow + egui::vec2(0., 2.)],
        text,
        egui::Stroke::NONE,
    ));
    let response = response.on_hover_text("Choose a ROM version");
    if response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup));
    }
    let selection = egui::popup::popup_below_widget(
        ui,
        popup,
        &response,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| version_options(ui, group, games, available, playback),
    )
    .flatten();
    if selection.is_some() {
        ui.memory_mut(|memory| memory.close_popup());
    }
    selection
}

pub fn menu(
    ui: &mut egui::Ui,
    game: &Game,
    playback: &Game,
    versions: Option<&crate::versions::Group>,
    games: &[Game],
    available: &[bool],
    busy: bool,
    resume: bool,
    selected: Vec<String>,
    action: &mut Option<Action>,
) {
    ui.set_width(248.);
    ui.spacing_mut().item_spacing.y = 0.;
    if selected.len() > 1 {
        ui.weak(format!("{} games selected", selected.len()));
        selection_actions(ui, busy, selected, action);
        return;
    }
    if menu_row(ui, if resume { "Resume" } else { "Play" }, "", !busy).clicked() {
        *action = Some(Action::Play(playback.clone()));
        ui.close_menu();
    }
    if resume && menu_row(ui, "Start from beginning…", "", !busy).clicked() {
        *action = Some(Action::StartFromBoot(playback.clone()));
        ui.close_menu();
    }
    if let Some(group) = versions {
        ui.scope(|ui| {
            ui.spacing_mut().button_padding.x = 11.;
            ui.spacing_mut().interact_size.y = 32.;
            ui.menu_button("Versions", |ui| {
                if let Some(selection) = version_options(ui, group, games, available, playback) {
                    *action = Some(selection);
                    ui.close_menu();
                }
            });
        });
    }
    let favorite_label = if game.favorite {
        "Remove from favorites"
    } else {
        "Add to favorites"
    };
    if menu_row(ui, favorite_label, "", true).clicked() {
        *action = Some(Action::Favorite(game.id.clone(), !game.favorite));
        ui.close_menu();
    }
    ui.scope(|ui| {
        ui.spacing_mut().button_padding.x = 11.;
        ui.spacing_mut().interact_size.y = 32.;
        ui.menu_button("Rating", |ui| {
            ui.set_width(124.);
            ui.spacing_mut().item_spacing.y = 0.;
            for rating in 0..=5 {
                let label = if rating == 0 {
                    "Unrated".into()
                } else {
                    "★".repeat(rating as usize)
                };
                let check = if rating == game.rating { "✓" } else { "" };
                if menu_row(ui, &label, check, true).clicked() {
                    *action = Some(Action::Rating(game.id.clone(), rating));
                    ui.close_menu();
                }
            }
        });
    });
    create_collection_row(ui, &selected, action);
    ui.separator();
    if menu_row(ui, "Find cover art…", "", true).clicked() {
        *action = Some(Action::FindCover(game.clone()));
        ui.close_menu();
    }
    if menu_row(ui, "Open ROM folder", "", game.rom.parent().is_some()).clicked() {
        if let Some(parent) = game.rom.parent() {
            *action = Some(Action::Reveal(parent.to_owned()));
        }
        ui.close_menu();
    }
    ui.separator();
    if menu_row(ui, "Remove from library…", "", !busy).clicked() {
        *action = Some(Action::Remove(selected));
        ui.close_menu();
    }
    if menu_row(ui, "Game information…", "", true).clicked() {
        *action = Some(Action::Details(game.id.clone()));
        ui.close_menu();
    }
}

fn selection_actions(
    ui: &mut egui::Ui,
    busy: bool,
    selected: Vec<String>,
    action: &mut Option<Action>,
) {
    create_collection_row(ui, &selected, action);
    ui.separator();
    if menu_row(ui, "Remove from library…", "", !busy).clicked() {
        *action = Some(Action::Remove(selected));
        ui.close_menu();
    }
}
fn create_collection_row(ui: &mut egui::Ui, selected: &[String], action: &mut Option<Action>) {
    if menu_row(ui, "Create collection from selection…", "", true).clicked() {
        *action = Some(Action::NewCollection(selected.to_vec()));
        ui.close_menu();
    }
}
pub fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Only HTTPS links can be opened here".into());
    }
    let mut child = std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map_err(|e| format!("Could not open the browser: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
pub fn open_path(path: PathBuf) -> Result<(), String> {
    let mut child = std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Could not open the file manager: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[derive(Clone, Copy)]
pub enum ToolbarIcon {
    Grid,
    List,
    Search,
    Clear,
    Bell,
}
pub fn toolbar_button(
    ui: &mut egui::Ui,
    icon: ToolbarIcon,
    label: &str,
    selected: bool,
) -> egui::Response {
    let fill = if selected {
        ui.visuals().selection.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    let response = ui.add_sized(
        egui::vec2(30., 30.),
        egui::Button::new("")
            .fill(fill)
            .stroke(egui::Stroke::NONE)
            .corner_radius(7.),
    );
    if response.hovered() && !selected {
        ui.painter()
            .rect_filled(response.rect, 7., ui.visuals().widgets.hovered.weak_bg_fill);
    }
    paint_toolbar_icon(ui, icon, response.rect, selected);
    response.on_hover_text(label)
}
pub fn paint_toolbar_icon(ui: &egui::Ui, icon: ToolbarIcon, rect: egui::Rect, selected: bool) {
    let center = rect.center();
    let p = |x: f32, y: f32| center + egui::vec2(x, y);
    let color = if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    let stroke = egui::Stroke::new(1.6_f32, color);
    match icon {
        ToolbarIcon::Grid => {
            for x in [-6., 0., 6.] {
                for y in [-6., 0., 6.] {
                    ui.painter().rect_stroke(
                        egui::Rect::from_center_size(p(x, y), egui::vec2(3.5, 3.5)),
                        0.5,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        ToolbarIcon::List => {
            for y in [-6., 0., 6.] {
                ui.painter().circle_filled(p(-8., y), 1.3, color);
                ui.painter().line_segment([p(-3., y), p(8., y)], stroke);
            }
        }
        ToolbarIcon::Search => {
            ui.painter().circle_stroke(p(-2., -2.), 5.5, stroke);
            ui.painter().line_segment([p(2., 2.), p(7., 7.)], stroke);
        }
        ToolbarIcon::Clear => {
            ui.painter().line_segment([p(-5., -5.), p(5., 5.)], stroke);
            ui.painter().line_segment([p(-5., 5.), p(5., -5.)], stroke);
        }
        ToolbarIcon::Bell => {
            ui.painter().add(egui::Shape::line(
                vec![
                    p(-8., 6.),
                    p(-5., 2.),
                    p(-5., -3.),
                    p(-3., -7.),
                    p(0., -8.),
                    p(3., -7.),
                    p(5., -3.),
                    p(5., 2.),
                    p(8., 6.),
                    p(-8., 6.),
                ],
                stroke,
            ));
            ui.painter().line_segment([p(0., -10.), p(0., -8.)], stroke);
            ui.painter().add(egui::Shape::line(
                vec![p(-2., 9.), p(0., 10.), p(2., 9.)],
                stroke,
            ));
        }
    }
}

pub fn play_button_visible(ui: &egui::Ui, cover: &egui::Response, selected: bool) -> bool {
    selected || ui.rect_contains_pointer(cover.rect)
}

pub fn hover_game_controls(
    ui: &mut egui::Ui,
    game: &Game,
    rect: egui::Rect,
    visible: bool,
    busy: bool,
    resume: bool,
) -> Option<Action> {
    hover_controls(ui, &game.id, rect, visible, busy, resume)
        .then(|| Action::Play(game.clone()))
}

fn hover_opacity(ui: &egui::Ui, id: &str, visible: bool) -> f32 {
    ui.ctx()
        .animate_bool_with_time(ui.id().with(("hover-play-opacity", id)), visible, 0.16)
}

pub(crate) fn hover_controls(
    ui: &mut egui::Ui,
    id: &str,
    rect: egui::Rect,
    visible: bool,
    busy: bool,
    resume: bool,
) -> bool {
    let opacity = hover_opacity(ui, id, visible);
    if opacity <= 0. {
        return false;
    }
    let mut overlay = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("hover-play", id))
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    overlay.multiply_opacity(opacity);
    let toolbar = emuluna_theme::gameplay::bar_frame(1.);
    let button = egui::Button::new(
        egui::RichText::new(if resume { "▶ Resume" } else { "▶ Play" })
            .strong()
            .color(emuluna_theme::gameplay::OVERLAY_TEXT),
    )
    .fill(toolbar.fill)
    .stroke(toolbar.stroke)
    .min_size(rect.size())
    .corner_radius(8.)
    .sense(if visible { egui::Sense::click() } else { egui::Sense::hover() });
    overlay.add_enabled(!busy, button).clicked()
}

#[derive(Clone, Copy)]
pub enum CollectionIcon {
    All,
    Recent,
    Favorite,
    Added,
    Folder,
}
pub fn collection_row(
    ui: &mut egui::Ui,
    label: &str,
    count: usize,
    icon: CollectionIcon,
    selected: bool,
    theme: &emuluna_theme::Theme,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.), egui::Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            5.,
            if selected {
                theme.color(
                    "sidebar-selected-background",
                    ui.visuals().selection.bg_fill,
                )
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            },
        );
    }
    let color = if selected {
        theme.color("sidebar-selected-text", ui.visuals().text_color())
    } else {
        ui.visuals().text_color()
    };
    let center = egui::pos2(rect.left() + 17., rect.center().y);
    let p = |x, y| center + egui::vec2(x, y);
    let stroke = egui::Stroke::new(1.5_f32, color);
    match icon {
        CollectionIcon::All => {
            for y in [-6., 0., 6.] {
                ui.painter().circle_filled(p(-7., y), 1.5, color);
                ui.painter().line_segment([p(-2., y), p(8., y)], stroke);
            }
        }
        CollectionIcon::Recent => {
            ui.painter().circle_stroke(center, 8., stroke);
            ui.painter().line_segment([p(0., -5.), p(0., 0.)], stroke);
            ui.painter().line_segment([p(0., 0.), p(4., 3.)], stroke);
        }
        CollectionIcon::Favorite => {
            let points = vec![
                p(0., 7.),
                p(-7., 0.),
                p(-8., -4.),
                p(-5., -7.),
                p(-2., -7.),
                p(0., -4.),
                p(2., -7.),
                p(5., -7.),
                p(8., -4.),
                p(7., 0.),
                p(0., 7.),
            ];
            ui.painter().add(egui::Shape::line(points, stroke));
        }
        CollectionIcon::Added => {
            ui.painter().rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(16., 15.)),
                2.,
                stroke,
                egui::StrokeKind::Inside,
            );
            for x in [-4., 4.] {
                ui.painter().line_segment([p(x, -10.), p(x, -4.)], stroke);
            }
            ui.painter().line_segment([p(-7., -2.), p(7., -2.)], stroke);
            ui.painter().line_segment([p(0., 0.), p(0., 5.)], stroke);
            ui.painter().line_segment([p(-3., 2.5), p(3., 2.5)], stroke);
        }
        CollectionIcon::Folder => {
            ui.painter().add(egui::Shape::closed_line(
                vec![
                    p(-8., 7.),
                    p(-8., -7.),
                    p(-2., -7.),
                    p(0., -4.),
                    p(8., -4.),
                    p(8., 7.),
                ],
                stroke,
            ));
        }
    }
    ui.painter().text(
        egui::pos2(rect.left() + 34., rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        emuluna_theme::fonts::bold(14.),
        color,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 6., rect.center().y),
        egui::Align2::RIGHT_CENTER,
        {
            let digits = count.to_string();
            digits
                .chars()
                .enumerate()
                .fold(String::new(), |mut text, (index, digit)| {
                    if index > 0 && (digits.len() - index).is_multiple_of(3) {
                        text.push(',');
                    }
                    text.push(digit);
                    text
                })
        },
        egui::FontId::proportional(13.),
        theme.color("placeholder-text", egui::Color32::GRAY),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resume_uses_a_single_click_target() {
        for (x, expected) in [(180., true), (265., false)] {
            let ctx = egui::Context::default();
            let mut clicked = false;
            for frame in 0..5 {
                let mut events = vec![egui::Event::PointerMoved(egui::pos2(x, 200.))];
                if frame == 2 || frame == 3 {
                    events.push(egui::Event::PointerButton {
                        pos: egui::pos2(x, 200.),
                        button: egui::PointerButton::Primary,
                        pressed: frame == 2,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960., 720.),
                    )),
                    events,
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if hover_controls(
                            ui,
                            "game",
                            egui::Rect::from_center_size(
                                egui::pos2(200., 200.),
                                egui::vec2(112., 36.),
                            ),
                            true,
                            false,
                            true,
                        ) {
                            clicked = true;
                        }
                    });
                });
            }
            assert_eq!(clicked, expected);
        }
    }
    #[test]
    fn play_button_stays_visible_when_pointer_is_over_the_button() {
        let ctx = egui::Context::default();
        let mut visibility = Vec::new();
        for frame in 0..24 {
            let pointer = if frame < 20 {
                egui::pos2(200., 200.)
            } else {
                egui::pos2(650., 500.)
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960., 720.),
                )),
                events: vec![egui::Event::PointerMoved(pointer)],
                ..Default::default()
            };
            let mut visible = false;
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let rect = egui::Rect::from_center_size(
                        egui::pos2(200., 200.),
                        egui::vec2(200., 180.),
                    );
                    let cover = ui.interact(rect, ui.id().with("cover"), egui::Sense::click());
                    visible = play_button_visible(ui, &cover, false);
                    if visible {
                        hover_controls(
                            ui,
                            "cover",
                            egui::Rect::from_center_size(rect.center(), egui::vec2(100., 34.)),
                            visible,
                            false,
                            true,
                        );
                    }
                });
            });
            visibility.push(visible);
        }
        assert!(
            visibility[4..20].iter().all(|v| *v),
            "Hover visibility oscillated: {visibility:?}"
        );
        assert!(
            visibility[22..].iter().all(|v| !*v),
            "The button must hide after leaving the card"
        );
    }
    #[test]
    fn hover_opacity_animates_in_and_out() {
        let ctx = egui::Context::default();
        let mut opacity = Vec::new();
        for frame in 0..32 {
            let _ = ctx.run(
                egui::RawInput { time: Some(f64::from(frame) / 60.), ..Default::default() },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        opacity.push(hover_opacity(ui, "cover", (4..19).contains(&frame)));
                    });
                },
            );
        }
        assert_eq!(opacity[0], 0.);
        assert!(opacity[7] > 0. && opacity[7] < 1.);
        assert_eq!(opacity[18], 1.);
        assert!(opacity[22] > 0. && opacity[22] < 1.);
        assert_eq!(opacity[31], 0.);
    }

    #[test]
    fn hover_controls_do_not_change_virtual_row_height_or_cursor() {
        for offset in [0., 120., 480., 1800.] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960., 720.),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .vertical_scroll_offset(offset)
                        .show_rows(ui, 250., 100, |ui, rows| {
                            let mut previous: Option<f32> = None;
                            for row in rows {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(800., 250.),
                                    egui::Sense::hover(),
                                );
                                let cursor = ui.cursor();
                                hover_controls(
                                    ui,
                                    &row.to_string(),
                                    egui::Rect::from_center_size(
                                        rect.center(),
                                        egui::vec2(100., 34.),
                                    ),
                                    true,
                                    false,
                                    true,
                                );
                                assert_eq!(
                                    ui.cursor(),
                                    cursor,
                                    "A hover button must not allocate space in the scrolling row"
                                );
                                if let Some(top) = previous {
                                    assert!(
                                        (rect.top() - top - 250. - ui.spacing().item_spacing.y)
                                            .abs()
                                            < 0.01
                                    );
                                }
                                previous = Some(rect.top());
                            }
                        });
                });
            });
        }
    }
}
