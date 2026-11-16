//! Title initials match the original NFKD-based alphabet index.
use eframe::egui;
use std::{collections::BTreeMap, sync::OnceLock};
pub fn initial(title: &str) -> char {
    static FOLDS: OnceLock<BTreeMap<char, char>> = OnceLock::new();
    let folds = FOLDS.get_or_init(|| {
        serde_json::from_str(include_str!("alphabet-folds.json")).expect("alphabet folds")
    });
    for character in title.chars() {
        if let Some(letter) = folds.get(&character) {
            return *letter;
        }
        if character.is_alphanumeric() {
            return character
                .to_uppercase()
                .next()
                .filter(char::is_ascii_uppercase)
                .unwrap_or('#');
        }
    }
    '#'
}
pub fn show(ui: &mut egui::Ui, targets: &BTreeMap<char, usize>) -> Option<usize> {
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        let height = ((ui.available_height() - 8.) / 27.).clamp(12., 24.);
        ui.add_space(((ui.available_height() - height * 27.) * 0.5).max(0.));
        ui.spacing_mut().item_spacing.y = 0.;
        ui.spacing_mut().button_padding = egui::Vec2::ZERO;
        ui.spacing_mut().interact_size.y = height;
        ui.style_mut().override_font_id = Some(egui::FontId::proportional((height - 2.).min(13.)));
        ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
        ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
        ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.hovered.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.active.bg_stroke = egui::Stroke::NONE;
        let mut chosen = None;
        for letter in "#ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars() {
            let target = targets.get(&letter);
            let label = if letter == '#' {
                "numbers and other titles".into()
            } else {
                letter.to_string()
            };
            if ui
                .add_enabled(
                    target.is_some(),
                    egui::Button::new(letter.to_string())
                        .frame(true)
                        .corner_radius(6.)
                        .min_size(egui::vec2(24., height)),
                )
                .on_hover_text(if target.is_some() {
                    format!("Jump to {label}")
                } else {
                    format!("No matching games: {letter}")
                })
                .clicked()
            {
                chosen = target.copied();
            }
        }
        chosen
    })
    .inner
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clicking_an_available_letter_returns_its_target_and_empty_letters_are_disabled() {
        for (letter_index, expected) in [(2, Some(9)), (3, None)] {
            let ctx = egui::Context::default();
            let targets = BTreeMap::from([('#', 0), ('A', 3), ('B', 9)]);
            let point = egui::pos2(12., 26. + letter_index as f32 * 24. + 12.);
            let mut chosen = None;
            for pressed in [None, Some(true), Some(false), None] {
                let mut events = vec![egui::Event::PointerMoved(point)];
                if let Some(pressed) = pressed {
                    events.push(egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(28., 700.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| {
                                if let Some(target) = show(ui, &targets) {
                                    chosen = Some(target);
                                }
                            });
                    },
                );
            }
            assert_eq!(chosen, expected);
        }
    }
    #[test]
    fn initials_follow_original_title_rules() {
        for (title, expected) in [
            ("007 GoldenEye", '#'),
            ("Écco", 'E'),
            ("  !Ångström", 'A'),
            ("The Legend of Zelda", 'T'),
            ("Ｆ-Zero", 'F'),
            ("ßpecial", 'S'),
            ("日本語", '#'),
            ("", '#'),
        ] {
            assert_eq!(initial(title), expected, "{title}");
        }
    }
}
