use eframe::egui;

pub const KEY: &str = "library.console_region";
pub const DEFAULT: &str = "Europe";

pub fn picker(ui: &mut egui::Ui, region: &mut String) {
    ui.label("Console region");
    egui::ComboBox::from_id_salt(KEY)
        .width(180.)
        .selected_text(region.as_str())
        .show_ui(ui, |ui| {
            for name in ["USA", "Europe", "Japan"] {
                ui.selectable_value(region, name.to_owned(), name);
            }
        });
}

pub fn console_name(system: &str, region: &str) -> Option<&'static str> {
    match (system, region) {
        ("genesis", "USA") => Some("Sega Genesis"),
        ("genesis", _) => Some("Sega Mega Drive"),
        ("segacd", "USA") => Some("Sega CD"),
        ("segacd", _) => Some("Sega Mega-CD"),
        ("nes", "Japan") => Some("Nintendo Famicom"),
        ("nes", _) => Some("Nintendo (NES)"),
        ("snes", "Japan") => Some("Super Famicom"),
        ("snes", _) => Some("Super Nintendo (SNES)"),
        ("pce", "Japan") => Some("PC Engine"),
        ("pce", _) => Some("TurboGrafx-16"),
        ("pcecd", "Japan") => Some("PC Engine CD"),
        ("pcecd", _) => Some("TurboGrafx-CD"),
        _ => None,
    }
}

// A filename fallback retains meaningful parentheses such as (Director's Cut).
pub fn title_from_filename(name: &str) -> &str {
    let mut title = name.trim_end();
    while let Some((base, tag)) = title.strip_suffix(')').and_then(|s| s.rsplit_once(" (")) {
        let region = tag.split(", ").all(|part| {
            matches!(
                part,
                "USA"
                    | "Europe"
                    | "Japan"
                    | "World"
                    | "Asia"
                    | "Australia"
                    | "Brazil"
                    | "Canada"
                    | "China"
                    | "France"
                    | "Germany"
                    | "Italy"
                    | "Korea"
                    | "Spain"
                    | "UK"
                    | "Taiwan"
            )
        });
        let language = tag.split(',').all(|part| {
            matches!(
                part,
                "En" | "Fr" | "De" | "Es" | "It" | "Ja" | "Pt" | "Nl" | "Ko" | "Zh" | "Ru" | "Sv"
            )
        });
        let revision = tag.strip_prefix("Rev ").is_some_and(|rev| {
            !rev.is_empty()
                && rev
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
        });
        if !(region || language || revision) || base.is_empty() {
            break;
        }
        title = base;
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regional_names_and_filename_fallback() {
        assert_eq!(console_name("genesis", "USA"), Some("Sega Genesis"));
        assert_eq!(console_name("genesis", "Europe"), Some("Sega Mega Drive"));
        assert_eq!(title_from_filename("Sonic (USA, Europe) (En,Fr)"), "Sonic");
        assert_eq!(
            title_from_filename("Game (Director's Cut) (Japan)"),
            "Game (Director's Cut)"
        );
    }
}
