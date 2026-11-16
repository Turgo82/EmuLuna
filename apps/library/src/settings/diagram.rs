use super::*;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Deserialize)]
pub struct Diagram {
    pub size: [f32; 2],
    pub image: Option<String>,
    pub controls: Vec<Part>,
}
#[derive(Deserialize)]
pub struct Part {
    pub action: String,
    pub rect: [f32; 4],
    pub ellipse: bool,
    #[serde(default)]
    pub rotation: f32,
}
pub fn get(id: &str) -> Option<&'static Diagram> {
    static DIAGRAMS: OnceLock<BTreeMap<String, Diagram>> = OnceLock::new();
    DIAGRAMS
        .get_or_init(|| {
            let mut layouts: BTreeMap<String, Diagram> =
                serde_json::from_str(include_str!("../../../../data/controllers/layouts.json"))
                    .expect("controller layouts");
            layouts.extend(
                serde_json::from_str::<BTreeMap<String, Diagram>>(include_str!(
                    "../../../../data/controllers/imported_layouts.json"
                ))
                .expect("imported controller layouts"),
            );
            layouts
        })
        .get(id)
}
impl Part {
    fn bounds(&self) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(self.rect[0], self.rect[1]),
            egui::vec2(self.rect[2], self.rect[3]),
        )
    }
    pub fn contains(&self, point: egui::Pos2) -> bool {
        let bounds = self.bounds();
        let local = bounds.center()
            + egui::emath::Rot2::from_angle(-self.rotation.to_radians())
                * (point - bounds.center());
        if !bounds.contains(local) {
            return false;
        }
        !self.ellipse || ((local - bounds.center()) / (bounds.size() * 0.5)).length_sq() <= 1.
    }
    pub fn highlight(
        &self,
        painter: &egui::Painter,
        image: egui::Rect,
        size: egui::Vec2,
        accent: Color32,
    ) {
        let bounds = self.bounds().expand(3.);
        let rotation = egui::emath::Rot2::from_angle(self.rotation.to_radians());
        let transform = |point: egui::Pos2| {
            let point = bounds.center() + rotation * (point - bounds.center());
            image.min + point.to_vec2() / size * image.size()
        };
        let points = if self.ellipse {
            (0..48)
                .map(|index| {
                    let angle = index as f32 * std::f32::consts::TAU / 48.;
                    transform(
                        bounds.center()
                            + bounds.size() * egui::vec2(angle.cos(), angle.sin()) * 0.5,
                    )
                })
                .collect()
        } else {
            [
                bounds.left_top(),
                bounds.right_top(),
                bounds.right_bottom(),
                bounds.left_bottom(),
            ]
            .into_iter()
            .map(transform)
            .collect()
        };
        painter.add(egui::Shape::convex_polygon(
            points,
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 55),
            egui::Stroke::new(2_f32, accent),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_controller_targets_match_every_console_and_rotated_shapes() {
        let specs: serde_json::Value =
            serde_json::from_str(include_str!("../../../../data/controller_profiles.json"))
                .unwrap();
        for spec in specs.as_object().unwrap().values() {
            let layout = get(spec["diagram"].as_str().unwrap()).unwrap();
            for part in &layout.controls {
                assert!(part.contains(part.bounds().center()));
                assert!(!part.contains(egui::pos2(-100., -100.)));
            }
        }
        let rotated = Part {
            action: "button:1".into(),
            rect: [100., 100., 80., 20.],
            ellipse: false,
            rotation: 90.,
        };
        assert!(rotated.contains(egui::pos2(140., 145.)));
        assert!(!rotated.contains(egui::pos2(175., 110.)));
        let ellipse = Part {
            rotation: 0.,
            ellipse: true,
            ..rotated
        };
        assert!(!ellipse.contains(egui::pos2(101., 101.)));
    }
}
