//! Prefer familiar native desktop fonts, with bundled egui fonts as fallback.
use egui::{self, FontFamily, FontId};
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, paths, family) in [
        (
            "native-regular",
            &[
                "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/liberation-sans-fonts/LiberationSans-Regular.ttf",
            ][..],
            FontFamily::Proportional,
        ),
        (
            "native-bold",
            &[
                "/usr/share/fonts/google-noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/liberation-sans-fonts/LiberationSans-Bold.ttf",
            ][..],
            FontFamily::Name("bold".into()),
        ),
    ] {
        let mut names = fonts.families[&FontFamily::Proportional].clone();
        if let Some(bytes) = paths.iter().find_map(|path| {
            std::fs::metadata(path)
                .ok()
                .filter(|m| m.is_file() && m.len() < 10 * 1024 * 1024)
                .and_then(|_| std::fs::read(path).ok())
        }) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(bytes).into());
            names.insert(0, name.into());
        }
        fonts.families.insert(family, names);
    }
    ctx.set_fonts(fonts);
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn window_title(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).size(14.)
}
