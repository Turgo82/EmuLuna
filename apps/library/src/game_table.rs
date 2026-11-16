use eframe::egui::{self, Color32, FontId, Rect, Ui};
use emuluna_theme::fonts;

pub const COVER_SIZE: egui::Vec2 = egui::vec2(34., 36.);
pub const COVER_LEFT_INSET: f32 = 12.;

const HEADERS: [&str; 8] = [
    "Game title",
    "System",
    "Rating",
    "Last played",
    "Date added",
    "Play count",
    "Core",
    "File status",
];
const WIDTHS: [f32; 8] = [0.235, 0.18, 0.08, 0.105, 0.105, 0.095, 0.105, 0.095];

pub fn paint(ui: &Ui, rect: Rect, values: [&str; 8], header: bool, rating: Color32) {
    let mut left = rect.left();
    for (column, (text, fraction)) in values.into_iter().zip(WIDTHS).enumerate() {
        let right = left + rect.width() * fraction;
        let inset = if column == 0 {
            COVER_LEFT_INSET + COVER_SIZE.x + 8.
        } else {
            8.
        };
        let width = (right - left - inset - 6.).max(0.);
        if width > 0. {
            let font = if header || column == 0 {
                fonts::bold(14.)
            } else {
                FontId::proportional(14.)
            };
            let color = if !header && column == 2 && text != "—" {
                rating
            } else {
                ui.visuals().text_color()
            };
            let mut job = egui::text::LayoutJob::simple(text.into(), font, color, width);
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(
                egui::pos2(left + inset, rect.center().y - galley.size().y * 0.5),
                galley,
                color,
            );
        }
        left = right;
    }
}

pub fn header(ui: &mut Ui, width: f32) {
    let rect = ui
        .allocate_exact_size(egui::vec2(width, 32.), egui::Sense::hover())
        .0;
    ui.painter()
        .rect_filled(rect, 0., ui.visuals().widgets.inactive.bg_fill);
    paint(ui, rect, HEADERS, true, Color32::TRANSPARENT);
}

pub fn date(timestamp: f64) -> String {
    if !timestamp.is_finite() || !(0. ..253_402_300_800.).contains(&timestamp) || timestamp == 0. {
        return "Never".into();
    }
    let seconds = timestamp as u64;
    let mut days = seconds / 86_400;
    let mut year = 1970;
    let leap = |year| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    while days >= 365 + u64::from(leap(year)) {
        days -= 365 + u64::from(leap(year));
        year += 1;
    }
    let lengths = [
        31,
        28 + u64::from(leap(year)),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for length in lengths {
        if days < length {
            break;
        }
        days -= length;
        month += 1;
    }
    format!("{year:04}-{month:02}-{:02}", days + 1)
}

pub fn core_name<'a>(id: &'a str, system: &str) -> &'a str {
    static CATALOG: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let id = emuluna_launch_config::explicit_core(id)
        .or_else(|| emuluna_launch_config::recommended_core(system))
        .unwrap_or("—");
    let catalog = CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../data/cores.json")).expect("core catalog")
    });
    catalog[id]["name"].as_str().unwrap_or(id)
}
