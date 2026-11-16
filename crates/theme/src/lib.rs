#![forbid(unsafe_code)]
pub mod fonts;
pub mod gameplay;
use egui::{self, Color32, Stroke};
use serde_json::Value;
use std::path::Path;
pub struct Theme {
    pub id: String,
    pub name: String,
    value: Value,
}
impl Theme {
    pub fn builtin(id: &str) -> Result<Self, String> {
        let text = match id {
            "classic" => include_str!("../../../data/themes/classic/theme.json"),
            "daylight" => include_str!("../../../data/themes/daylight/theme.json"),
            "luna" => include_str!("../../../data/themes/luna/theme.json"),
            _ => return Err("Unknown built-in theme".into()),
        };
        let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        Ok(Self {
            id: id.into(),
            name: value["name"].as_str().unwrap_or(id).into(),
            value,
        })
    }

    pub fn load(root: &Path, id: &str) -> Result<Self, String> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("Invalid theme ID".into());
        }
        let value: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(id).join("theme.json"))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if value["format"].as_u64() != Some(1) {
            return Err("Unsupported theme format".into());
        }
        Ok(Self {
            id: id.into(),
            name: value["name"].as_str().unwrap_or(id).into(),
            value,
        })
    }
    pub fn color(&self, key: &str, fallback: Color32) -> Color32 {
        let Some(value) = self.value["colors"][key]
            .as_str()
            .or_else(|| self.value["palette"][key].as_str())
        else {
            return fallback;
        };
        let Some(hex) = value.strip_prefix('#').filter(|s| s.len() == 6) else {
            return fallback;
        };
        u32::from_str_radix(hex, 16)
            .map(|v| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
            .unwrap_or(fallback)
    }
    pub fn strong_titles(&self) -> bool {
        self.value["fonts"]["game-title-weight"]
            .as_u64()
            .unwrap_or(700)
            >= 600
    }
    pub fn asset(&self, key: &str) -> Option<&str> {
        self.value["assets"][key].as_str().filter(|path| {
            let path = std::path::Path::new(path);
            !path.is_absolute()
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
        })
    }
    pub fn apply(&self, ctx: &egui::Context) {
        ctx.style_mut(|style| self.apply_to_style(style));
    }

    /// Apply a palette to a local UI without changing the application's theme.
    pub fn apply_to_style(&self, style: &mut egui::Style) {
        let bg = self.color("window", Color32::from_gray(32));
        let mut v = if u32::from(bg.r()) + u32::from(bg.g()) + u32::from(bg.b()) > 450 {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        v.panel_fill = bg;
        v.window_fill = bg;
        v.extreme_bg_color = self.color("alternate-base", self.color("base", bg));
        v.faint_bg_color = self.color("alternate-base", bg);
        v.override_text_color = Some(self.color("window-text", Color32::WHITE));
        v.selection.bg_fill = self.color("highlight", Color32::from_rgb(160, 135, 200));
        v.selection.stroke = Stroke::new(1_f32, self.color("highlighted-text", Color32::WHITE));
        let button = self.color("button", bg);
        let border = self.color("mid", Color32::from_gray(60));
        let accent = v.selection.bg_fill;
        v.widgets.inactive.bg_fill = button;
        v.widgets.inactive.weak_bg_fill = button;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1_f32, border);
        for (widget, amount) in [
            (&mut v.widgets.hovered, 0.14),
            (&mut v.widgets.active, 0.22),
            (&mut v.widgets.open, 0.18),
        ] {
            let fill = button.lerp_to_gamma(accent, amount);
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke.color = border.lerp_to_gamma(accent, amount);
            widget.fg_stroke.color = self.color(
                "button-text",
                v.override_text_color.unwrap_or(widget.fg_stroke.color),
            );
        }
        v.hyperlink_color = self.color("link", v.selection.bg_fill);
        style.visuals = v;
        style.animation_time = 0.12;
        style.spacing.scroll.floating = false;
        style.spacing.item_spacing = egui::vec2(8., 6.);
        style.spacing.interact_size.y = 32.;
        style.spacing.button_padding = egui::vec2(10., 6.);
        let interface_font = egui::FontId::proportional(14.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, interface_font.clone());
        style
            .text_styles
            .insert(egui::TextStyle::Button, interface_font);
    }
}
pub fn discover(root: &Path) -> Vec<(String, String)> {
    let mut themes = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(id) = entry.file_name().to_str() {
                    if let Ok(theme) = Theme::load(root, id) {
                        themes.push((theme.id, theme.name));
                    }
                }
            }
        }
    }
    themes.sort_by(|a, b| a.1.cmp(&b.1));
    themes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_themes_are_independent_and_keep_palette_colors() {
        for id in ["classic", "daylight", "luna"] {
            let theme = Theme::builtin(id).unwrap();
            assert_eq!(theme.value["id"].as_str(), Some(id));
            assert!(theme.value.get("extends").is_none());
            assert!(theme.value["palette"]["window"].is_string());
            assert_ne!(
                theme.color("window", Color32::TRANSPARENT),
                Color32::TRANSPARENT
            );
        }
        assert!(Theme::builtin("unknown").is_err());
    }
}
