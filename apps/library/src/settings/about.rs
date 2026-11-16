use super::*;
use std::time::{Duration, Instant};
#[derive(Default)]
pub struct About {
    clicks: u8,
    spin: Option<Instant>,
    legal: bool,
    unlock_sound_pending: bool,
}
impl About {
    fn click_logo(&mut self, prefs: &mut Preferences, library: &Library) {
        self.clicks += 1;
        if self.clicks == 4 {
            self.clicks = 0;
            let unlocked = value(prefs, library, "advanced.unlocked", "0") != "1";
            prefs.set("advanced.unlocked", if unlocked { "1" } else { "0" });
            self.spin = Some(Instant::now());
            self.unlock_sound_pending = unlocked;
        }
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        prefs: &mut Preferences,
        env: &mut Environment<'_>,
        open: &mut bool,
        updates: &mut updates::Checker,
    ) -> bool {
        let mut advanced = false;
        let mut close = false;
        egui::Window::new(emuluna_theme::fonts::window_title("About EmuLuna")).id(egui::Id::new("about-emuluna")).open(open).resizable(false).collapsible(false).default_width(500.).show(ctx,|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.);
                let (rect,response)=ui.allocate_exact_size(egui::vec2(260.,240.),egui::Sense::click());
                if let Some(texture)=env.artwork.texture(&env.data.join("branding/emuluna-logo.png")) {
                    let mut mesh=egui::Mesh::with_texture(texture);
                    mesh.add_rect_with_uv(env.artwork.fitted_rect(&env.data.join("branding/emuluna-logo.png"),rect),egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),Color32::WHITE);
                    if let Some(start)=self.spin {let t=(start.elapsed().as_secs_f32()/0.65).min(1.);let angle=(1.-(1.-t).powi(3))*std::f32::consts::TAU;let rotation=egui::emath::Rot2::from_angle(angle);for vertex in &mut mesh.vertices {vertex.pos=rect.center()+rotation*(vertex.pos-rect.center());}if t<1.{ctx.request_repaint_after(Duration::from_millis(16));}else{self.spin=None;}}
                    ui.painter().add(egui::Shape::mesh(mesh));
                }
                if response.clicked() { self.click_logo(prefs,env.library); }
                let version=updates::VERSION.trim();
                ui.heading(format!("EmuLuna {version}"));ui.weak("Rust native preview");ui.add_space(12.);
                ui.label("An independent game library and emulator frontend.\nRuns standard libretro cores directly.");
                let catalog:serde_json::Value=serde_json::from_str(include_str!("../../../../data/cores.json")).expect("catalog");
                ui.add_space(10.);ui.label(format!("{} systems · {} core profiles",env.library.consoles.len(),catalog.as_object().map_or(0,|v|v.len())));
                ui.label("Artwork: OpenVGDB and Libretro thumbnails.");
                ui.add_space(12.);
                for (label,url) in [("Project website","https://github.com/Turgo82/EmuLuna"),("Capabilities and credits","https://github.com/Turgo82/EmuLuna/blob/main/THIRD_PARTY_NOTICES.md")] {
                    if ui.link(RichText::new(label).color(ui.visuals().hyperlink_color)).clicked(){if let Err(e)=crate::actions::open_url(url){ui.label(e);}}
                }
                if ui.link(RichText::new("Disc backup legal agreement").color(ui.visuals().hyperlink_color)).clicked(){self.legal=true;}
                if value(prefs,env.library,"advanced.unlocked","0")=="1" && ui.link(RichText::new("Advanced settings unlocked").strong().color(ui.visuals().hyperlink_color)).clicked(){advanced=true;close=true;}
                ui.add_space(12.);updates.show(ui);
                ui.add_space(12.);if ui.button("OK").clicked(){close=true;}
            });
        });
        if self.unlock_sound_pending {
            self.unlock_sound_pending = false;
            if value(prefs, env.library, "volume", "80") != "0" {
                play_unlock_sound(prefs.get("native.audio_output").unwrap_or("").to_owned());
            }
        }
        if close {
            *open = false;
        }
        advanced
    }
    pub fn show_legal(&mut self, ctx: &egui::Context, prefs: &mut Preferences, library: &Library) {
        if !self.legal {
            return;
        }
        let mut open = true;
        let mut done = false;
        egui::Window::new(emuluna_theme::fonts::window_title("Disc backup legal agreement")).open(&mut open).default_width(580.).resizable(false).show(ctx,|ui| {
            ui.heading("Before you back up a disc");
            ui.label("Use this feature only for backups you are legally entitled to make.");
            heading(ui,"U.S. Copyright Act — 17 U.S.C. § 117");
            ui.label("Section 117(a) concerns the owner of a copy of a computer program. An archival copy under § 117(a)(2) must be for archival purposes only, and must be destroyed when continued possession of the program ceases to be rightful.");
            ui.label("Section 117(b) places conditions on transferring copies and adaptations. Section 117 does not automatically authorize copying other works, including music or films. Law and license terms may affect your rights. Checking the box does not grant permission to copy a game.");
            for (label,url) in [("Read Section 117 — U.S. Copyright Office","https://www.copyright.gov/title17/92chap1.html#117"),("Software backup FAQ — U.S. Copyright Office","https://www.copyright.gov/help/faq/faq-digital.html")] {if ui.link(RichText::new(label).color(ui.visuals().hyperlink_color)).clicked(){if let Err(e)=crate::actions::open_url(url){ui.label(e);}}}
            ui.add_space(12.);
            let mut accepted=value(prefs,library,"disc_backup.agreement_version","")=="1";
            if ui.checkbox(&mut accepted,"I own the discs I back up and agree to these conditions.").changed(){prefs.set("disc_backup.agreement_version",if accepted{"1"}else{""});}
            ui.weak("Uncheck this to show the agreement before your next backup.");
            if ui.button("Done").clicked(){done=true;}
        });
        self.legal = open && !done;
    }
}

fn play_unlock_sound(output: String) {
    std::thread::spawn(move || {
        let result = (|| {
            let sdl = sdl2::init()?;
            let audio = sdl.audio()?;
            let mut stream = sdl2::rwops::RWops::from_bytes(include_bytes!(
                "../../../../data/sounds/unlock-pop.wav"
            ))?;
            let wav = sdl2::audio::AudioSpecWAV::load_wav_rw(&mut stream)?;
            if wav.format != sdl2::audio::AudioFormat::S16LSB {
                return Err("Unsupported unlock sound format".to_owned());
            }
            let samples = wav
                .buffer()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| (i16::from_le_bytes(*b) as f32 * 0.6) as i16)
                .collect::<Vec<_>>();
            let spec = sdl2::audio::AudioSpecDesired {
                freq: Some(wav.freq),
                channels: Some(wav.channels),
                samples: Some(1024),
            };
            let selected = (!output.is_empty()).then_some(output.as_str());
            let device = audio
                .open_queue::<i16, _>(selected, &spec)
                .or_else(|_| audio.open_queue::<i16, _>(None, &spec))?;
            device.queue_audio(&samples)?;
            device.resume();
            let deadline = Instant::now() + Duration::from_secs(5);
            while device.size() > 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok::<_, String>(())
        })();
        if let Err(e) = result {
            eprintln!("Unlock sound unavailable: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advanced_unlock_persists_until_four_more_logo_clicks() {
        let root = tempfile::tempdir().unwrap();
        let library = Library {
            root: root.path().into(),
            games: Vec::new(),
            consoles: Vec::new(),
            collections: Vec::new(),
            theme: "classic".into(),
            settings: Default::default(),
        };
        let path = root.path().join("settings.json");
        let mut prefs = Preferences::default();
        let mut about = About::default();
        for _ in 0..3 {
            about.click_logo(&mut prefs, &library);
        }
        assert_ne!(prefs.get("advanced.unlocked"), Some("1"));
        about.click_logo(&mut prefs, &library);
        prefs.save(&path).unwrap();
        let mut prefs = Preferences::load(&path).unwrap();
        assert_eq!(prefs.get("advanced.unlocked"), Some("1"));
        assert!(about.unlock_sound_pending);
        let mut about = About::default();
        for _ in 0..4 {
            about.click_logo(&mut prefs, &library);
        }
        assert_eq!(prefs.get("advanced.unlocked"), Some("0"));
    }
}
