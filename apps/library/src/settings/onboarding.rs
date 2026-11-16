//! First-run setup assistant and its read-only developer preview.
use super::*;
use std::{
    collections::HashSet,
    sync::mpsc,
    time::{Duration, Instant},
};
#[derive(Clone, Copy)]
enum Pick {
    Create,
    Existing,
}

pub(crate) struct SetupChoice {
    pub folder: PathBuf,
    pub existing: bool,
    pub legacy: Option<PathBuf>,
    pub theme: String,
    pub region: String,
    pub copy: bool,
}

pub struct Preview {
    pub open: bool,
    step: usize,
    started: Instant,
    transition: Instant,
    theme: String,
    region: String,
    theme_preview: Option<(String, Option<Theme>)>,
    folder: PathBuf,
    default_folder: PathBuf,
    legacy: Option<PathBuf>,
    migrate: bool,
    existing: bool,
    live: bool,
    ready: Option<SetupChoice>,
    copy: bool,
    automatic: bool,
    selected: HashSet<String>,
    original_fullscreen: Option<bool>,
    fullscreen: bool,
    picker: Option<(Pick, mpsc::Receiver<Result<Option<PathBuf>, String>>)>,
    message: String,
}
impl Default for Preview {
    fn default() -> Self {
        Self {
            open: false,
            step: 0,
            started: Instant::now(),
            transition: Instant::now(),
            theme: String::new(),
            region: crate::region::DEFAULT.into(),
            theme_preview: None,
            folder: PathBuf::new(),
            default_folder: PathBuf::new(),
            legacy: None,
            migrate: false,
            existing: false,
            live: false,
            ready: None,
            copy: true,
            automatic: true,
            selected: HashSet::new(),
            original_fullscreen: None,
            fullscreen: false,
            picker: None,
            message: String::new(),
        }
    }
}
impl Preview {
    fn appearance(&mut self, ui: &mut egui::Ui, env: &mut Environment<'_>) {
        let width = ui.available_width();
        let split = width >= 540.;
        let picker_width = if split { 190. } else { width };
        let mut show = |ui: &mut egui::Ui| {
            let mut picker = |ui: &mut egui::Ui| {
                title(ui, "Appearance");
                ui.add_space(12.);
                egui::ComboBox::from_id_salt("onboarding-theme")
                    .width(picker_width.min(240.))
                    .selected_text(
                        env.themes
                            .iter()
                            .find(|(id, _)| id == &self.theme)
                            .map_or(self.theme.as_str(), |(_, name)| name),
                    )
                    .show_ui(ui, |ui| {
                        for (id, name) in env.themes {
                            ui.selectable_value(&mut self.theme, id.clone(), name);
                        }
                    });
                ui.add_space(14.);
                crate::region::picker(ui, &mut self.region);
            };
            ui.allocate_ui_with_layout(
                egui::vec2(picker_width, 0.),
                egui::Layout::top_down(egui::Align::Min),
                &mut picker,
            );
            ui.add_space(12.);
            let id = effective_theme(ui.ctx(), &self.theme);
            if self
                .theme_preview
                .as_ref()
                .is_none_or(|(previous, _)| previous != id)
            {
                self.theme_preview = Some((
                    id.to_owned(),
                    Theme::load(&theme_root(env.data, env.preferences_path, id), id).ok(),
                ));
            }
            let theme = self
                .theme_preview
                .as_ref()
                .and_then(|(_, theme)| theme.as_ref())
                .unwrap_or(env.theme);
            let width = ui.available_width().min(460.);
            let screenshot = env
                .data
                .join("themes")
                .join(id)
                .join("library-preview.webp");
            if matches!(id, "luna" | "classic" | "daylight")
                && theme_root(env.data, env.preferences_path, id) == env.data.join("themes")
                && screenshot.is_file()
            {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, width * 0.64), egui::Sense::hover());
                if let Some(texture) = env.artwork.texture(&screenshot) {
                    let bounds = env.artwork.fitted_rect(&screenshot, rect);
                    egui::Image::new((texture, bounds.size()))
                        .corner_radius(8.)
                        .paint_at(ui, bounds);
                } else {
                    ui.put(rect, egui::Spinner::new());
                }
            } else {
                library_preview(ui, theme, width);
            }
        };
        if split {
            ui.horizontal_top(show);
        } else {
            show(ui);
        }
    }

    pub(crate) fn setup(default_folder: PathBuf, legacy: Option<PathBuf>) -> Self {
        let mut result = Self {
            live: true,
            existing: crate::storage::managed(&default_folder),
            migrate: legacy.is_some(),
            legacy,
            folder: default_folder.clone(),
            default_folder,
            ..Self::default()
        };
        result.request();
        result
    }

    pub(crate) fn take_choice(&mut self) -> Option<SetupChoice> {
        self.ready.take()
    }

    pub(crate) fn show_error(&mut self, message: String) {
        self.message = message;
        self.step = 1;
    }

    fn pick(&mut self, ctx: &egui::Context, pick: Pick) {
        let (tx, rx) = mpsc::channel();
        self.picker = Some((pick, rx));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let title = match pick {
                Pick::Create => "Choose a location for the EmuLuna folder",
                Pick::Existing => "Open an existing EmuLuna library",
            };
            let _ = tx.send(super::tools::pick_path(super::tools::Picker::Folder, title));
            ctx.request_repaint();
        });
    }

    fn library_location(&mut self, ui: &mut egui::Ui) {
        title(ui, "Library location");
        ui.label("Keep your games, saves, system files, and themes together.");
        ui.add_space(8.);
        ui.horizontal_wrapped(|ui| {
            if ui.add(location_button("Use default")).clicked() {
                self.folder = self.default_folder.clone();
                self.existing = crate::storage::managed(&self.folder);
                self.message.clear();
            }
            if ui
                .add_enabled(
                    self.picker.is_none(),
                    location_button("Choose another location…"),
                )
                .clicked()
            {
                self.pick(ui.ctx(), Pick::Create);
            }
            if ui
                .add_enabled(
                    self.picker.is_none(),
                    location_button("Open existing library…"),
                )
                .clicked()
            {
                self.pick(ui.ctx(), Pick::Existing);
            }
        });
        ui.add_space(8.);
        ui.strong(if self.existing {
            "Open this library"
        } else {
            "Library folder"
        });
        ui.monospace(self.folder.display().to_string());
        if let Some(source) = &self.legacy {
            if !self.existing {
                ui.add_space(8.);
                ui.checkbox(&mut self.migrate, "Migrate my current library");
                if self.migrate {
                    ui.label(format!("Copy from {}", source.display()));
                    ui.label(
                        RichText::new("Close other EmuLuna windows. The old copy is kept.")
                            .size(12.)
                            .color(Color32::from_rgb(188, 170, 222)),
                    );
                }
            }
        }
        if !self.existing && !self.migrate {
            ui.checkbox(&mut self.copy, "Copy imported games into the library");
        }
    }

    pub fn request_page(&mut self, step: usize, manual_cores: bool) {
        self.request();
        self.step = step.min(4);
        self.automatic = !manual_cores;
    }
    pub fn request(&mut self) {
        self.open = true;
        self.step = 0;
        self.started = Instant::now();
        self.transition = self.started;
        self.original_fullscreen = None;
    }
    pub(crate) fn close(&mut self, ctx: &egui::Context) {
        self.open = false;
        if let Some(original) = self.original_fullscreen.take() {
            if original != self.fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(original));
            }
        }
    }
    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen = !self.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
    }

    pub fn show(&mut self, ctx: &egui::Context, env: &mut Environment<'_>) {
        if !self.open {
            return;
        }
        if self.original_fullscreen.is_none() {
            self.fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            self.original_fullscreen = Some(self.fullscreen);
            self.theme = env.theme.id.clone();
            if !self.live {
                self.default_folder = crate::storage::default_root();
                self.folder = self.default_folder.clone();
                self.existing = false;
                self.legacy = crate::storage::legacy_root();
                self.migrate = self.legacy.is_some();
            }
        }
        if let Some((pick, receiver)) = &self.picker {
            if let Ok(result) = receiver.try_recv() {
                let pick = *pick;
                self.picker = None;
                match result {
                    Ok(Some(path)) => {
                        if matches!(pick, Pick::Existing)
                            && path.join("library.sqlite3").is_file()
                            && !crate::storage::managed(&path)
                        {
                            self.legacy = Some(path);
                            self.migrate = true;
                            self.existing = false;
                        } else {
                            self.existing = matches!(pick, Pick::Existing);
                            self.folder = if self.existing {
                                path
                            } else {
                                path.join(crate::storage::FOLDER)
                            };
                        }
                        self.message.clear();
                    }
                    Ok(None) => {}
                    Err(e) => self.message = e,
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.fullscreen {
                self.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else {
                self.close(ctx);
                return;
            }
        }
        if ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::F11,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                        ..
                    }
                )
            })
        }) {
            self.toggle_fullscreen(ctx);
        }
        let screen = ctx.screen_rect();
        let t = self.started.elapsed().as_secs_f32();
        let mut close = false;
        let mut next = None;
        let last_step = if self.live { 2 } else { 4 };
        let choosing_theme = self.live && !self.existing && !self.migrate;
        let finish = self.live && (self.step == last_step || (self.step == 1 && !choosing_theme));
        egui::Area::new(egui::Id::new("setup-assistant-preview")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx,|ui|{
            ui.set_min_size(screen.size());ui.interact(screen,ui.id().with("backdrop"),egui::Sense::click());
            background(ui.painter(),screen,t);
            ui.visuals_mut().override_text_color=Some(Color32::from_rgb(244,240,255));
            ui.visuals_mut().window_fill=Color32::from_rgb(24,19,49);ui.visuals_mut().extreme_bg_color=Color32::from_rgb(29,22,59);
            ui.visuals_mut().selection.bg_fill=Color32::from_rgb(130,88,220);ui.visuals_mut().selection.stroke.color=Color32::WHITE;
            { let visuals=ui.visuals_mut();for widget in [&mut visuals.widgets.inactive,&mut visuals.widgets.hovered] {widget.fg_stroke.color=Color32::WHITE;widget.bg_fill=Color32::from_rgb(65,45,108);widget.weak_bg_fill=Color32::from_rgb(65,45,108);} }
            let top=egui::Rect::from_min_size(screen.min+egui::vec2(28.,24.),egui::vec2(screen.width()-56.,32.));
            let mut top_ui=ui.new_child(egui::UiBuilder::new().max_rect(top).layout(egui::Layout::left_to_right(egui::Align::Center)));
            top_ui.label(RichText::new("EMULUNA  /  SETUP ASSISTANT").font(fonts::bold(12.)).color(Color32::from_rgb(143,211,255)));
            top_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                if ui.add_sized([120.,30.],egui::Button::new(if self.live {"Quit setup"} else {"Close preview"}).corner_radius(8.)).clicked(){close=true;}
                if ui.add_sized([100.,30.],egui::Button::new(if self.fullscreen{"Windowed"}else{"Fullscreen"}).corner_radius(8.)).clicked(){self.toggle_fullscreen(ctx);}
                ui.label(RichText::new("F11").size(12.).color(Color32::from_rgb(188,170,222)));
            });
            let logo_slot = egui::Rect::from_center_size(
                egui::pos2(screen.left() + screen.width() * 0.23, screen.center().y - 25.),
                egui::vec2((screen.width() * 0.34).min(500.), (screen.height() * 0.52).min(500.)),
            );
            let logo_bounds = logo_slot.translate(egui::vec2(0., (t * 0.8).sin() * 6.));
            glow(ui.painter(),logo_bounds.center(),logo_bounds.size()*0.62,Color32::from_rgba_unmultiplied(116,42,245,55));
            let logo=env.data.join("branding/emuluna-logo.png");
            if let Some(texture)=env.artwork.texture(&logo){let bounds=env.artwork.fitted_rect(&logo,logo_bounds);let mut mesh=egui::Mesh::with_texture(texture);mesh.add_rect_with_uv(bounds,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),Color32::WHITE);let rotate=egui::emath::Rot2::from_angle((t*0.65).sin()*0.035);for vertex in &mut mesh.vertices{vertex.pos=bounds.center()+rotate*(vertex.pos-bounds.center());}ui.painter().add(egui::Shape::mesh(mesh));}
            ui.painter().text(
                logo_slot.center_bottom() + egui::vec2(0., 24.),
                egui::Align2::CENTER_TOP,
                "Every world. One library.",
                fonts::bold(18.),
                Color32::from_rgb(225,209,255),
            );
            let desired_height=if self.step==3 && !self.automatic {(screen.height()-180.).min(720.)}else{match self.step {1=>520_f32,2=>520.,_=>400.}.min(screen.height()-150.)};
            let card_top=((screen.height()-desired_height)*0.5).max(76.);
            let card=egui::Rect::from_min_max(egui::pos2(screen.left()+screen.width()*0.46,screen.top()+card_top),screen.right_bottom()-egui::vec2(36.,36.));
            let mut card_ui=ui.new_child(egui::UiBuilder::new().max_rect(card).layout(egui::Layout::top_down(egui::Align::Min)));
            egui::Frame::new().fill(Color32::from_rgba_unmultiplied(18,14,41,238)).stroke(egui::Stroke::new(1_f32,Color32::from_rgba_unmultiplied(159,121,241,110))).corner_radius(24.).inner_margin(26.).show(&mut card_ui,|ui| {
                ui.set_width(card.width()-52.);
                ui.label(RichText::new(if self.live {"FIRST RUN"} else {"DEVELOPER PREVIEW"}).font(fonts::bold(11.)).color(Color32::from_rgb(157,224,255)));
                ui.add_space(8.);
                let opacity=(0.2+self.transition.elapsed().as_secs_f32()/0.25).min(1.);
                ui.scope(|ui| {
                    ui.set_opacity(opacity);
                    egui::ScrollArea::vertical().id_salt(("onboarding-step",self.step)).auto_shrink([false, true]).max_height(if self.step == 1 { (card.height()-124.).max(170.) } else { (desired_height-180.).max(170.) }).show(ui,|ui| {
                        match self.step {
                            0=>{title(ui,"Your next adventure\nstarts here.");ui.add_space(14.);ui.label("All your favorite worlds, in one place.");ui.add_space(12.);ui.label("Choose a look, explore your console library layout, and get your emulator cores ready.");ui.add_space(12.);ui.label("These choices can be revisited in Settings.");}
                            1=>{
                                self.library_location(ui);
                                if !self.message.is_empty() {
                                    ui.label(RichText::new(&self.message).color(Color32::from_rgb(255,179,192)));
                                }
                            }
                            2=>self.appearance(ui, env),
                            3=>{title(ui,"Ready to play");ui.label("Cores provide emulation for each console.");ui.checkbox(&mut self.automatic,"Download recommended cores as needed");if !self.automatic{ui.add_space(10.);ui.label("Choose consoles to prepare, or leave them unchecked to install cores later.");for console in &env.library.consoles{ui.horizontal(|ui|{if let Some(texture)=env.artwork.texture(&env.data.join("icons").join(&console.icon)){let rect=ui.allocate_exact_size(egui::vec2(24.,24.),egui::Sense::hover()).0;ui.painter().image(texture,rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),Color32::WHITE);}let mut selected=self.selected.contains(&console.id);if ui.checkbox(&mut selected,&console.name).changed(){if selected{self.selected.insert(console.id.clone());}else{self.selected.remove(&console.id);}}});}}ui.add_space(12.);ui.label("Some consoles need system files from your own hardware. You can check these in Settings > System Files.");}
                            _=>{title(ui,"Your library awaits");ui.add_space(14.);ui.label(format!("Appearance: {}",env.themes.iter().find(|(id,_)|id==&self.theme).map_or(self.theme.as_str(),|(_,name)|name)));ui.label(format!("Library folder: {}",self.folder.display()));ui.label(if self.copy{"Imports: copied into the library"}else{"Imports: kept in their current folders"});ui.label(if self.automatic{"Cores: download as needed".into()}else{format!("Cores: {} selected consoles",self.selected.len())});ui.add_space(14.);ui.label("This was a preview. Your library and settings have not changed.");}
                        }
                    });
                });
                ui.add_space(14.);
                ui.separator();
                if !self.live {
                    ui.label(RichText::new("Preview only · No files or settings will change.").size(12.).color(Color32::from_rgb(188,170,222)));
                }
                ui.add_space(8.);
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.step>0,egui::Button::new("Back").min_size(egui::vec2(80.,36.)).corner_radius(9.)).clicked() {
                        next=Some(self.step-1);
                    }
                    ui.label(format!("{} of {}",self.step+1,if self.live && !choosing_theme {2} else {last_step+1}));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| {
                        let label = if finish {
                            if self.existing {"Open library"} else if self.migrate {"Migrate library"} else {"Create library"}
                        } else if self.step==last_step {"Finish preview"} else {"Next"};
                        if ui.add_enabled(self.picker.is_none(),egui::Button::new(RichText::new(label).font(fonts::bold(14.))).min_size(egui::vec2(126.,36.)).fill(Color32::from_rgb(125,77,219)).corner_radius(9.)).clicked() {
                            if finish {
                                self.ready = Some(SetupChoice {
                                    folder: self.folder.clone(), existing: self.existing,
                                    legacy: (!self.existing && self.migrate).then(|| self.legacy.clone()).flatten(),
                                    theme: self.theme.clone(), region: self.region.clone(), copy: self.copy,
                                });
                            } else if self.step==last_step {
                                close=true;
                            } else {
                                next=Some(self.step+1);
                            }
                        }
                    });
                });
            });
        });
        if let Some(step) = next {
            self.step = step;
            self.transition = Instant::now();
        }
        if close {
            self.close(ctx);
        } else {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }
}
fn location_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(label)
        .corner_radius(8.)
        .min_size(egui::vec2(0., 34.))
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(fonts::bold(30.))
            .color(Color32::from_rgb(248, 242, 255)),
    );
}
fn glow(painter: &egui::Painter, center: egui::Pos2, size: egui::Vec2, color: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color);
    for index in 0..=24 {
        let angle = index as f32 / 24. * std::f32::consts::TAU;
        mesh.colored_vertex(
            center + egui::vec2(angle.cos() * size.x, angle.sin() * size.y),
            Color32::TRANSPARENT,
        );
        if index > 0 {
            mesh.add_triangle(0, index, index + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}
fn random(seed: f32) -> f32 {
    (seed.sin() * 43758.547).fract().abs()
}
fn background(painter: &egui::Painter, rect: egui::Rect, time: f32) {
    let mut mesh = egui::Mesh::default();
    for (pos, color) in [
        (rect.left_top(), Color32::from_rgb(4, 6, 23)),
        (rect.right_top(), Color32::from_rgb(10, 7, 32)),
        (rect.right_bottom(), Color32::from_rgb(48, 15, 85)),
        (rect.left_bottom(), Color32::from_rgb(21, 28, 72)),
    ] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
    glow(
        painter,
        egui::pos2(
            rect.left() + rect.width() * 0.22,
            rect.top() + rect.height() * 0.55,
        ),
        rect.size() * egui::vec2(0.48, 0.5),
        Color32::from_rgba_unmultiplied(71, 28, 139, 105),
    );
    for index in 0..130 {
        let seed = index as f32 * 7.13 + 1.;
        let x = rect.left() + random(seed) * rect.width();
        let y = rect.top() + random(seed + 37.) * rect.height() * 0.92;
        let pulse = 0.3 + 0.7 * ((time * (0.5 + random(seed + 12.)) + seed).sin() * 0.5 + 0.5);
        let alpha = (pulse * 170. + 35.) as u8;
        let radius = 0.7 + random(seed + 9.) * 1.1;
        painter.circle_filled(
            egui::pos2(x, y),
            radius,
            Color32::from_rgba_unmultiplied(205, 221, 255, alpha),
        );
        if index % 17 == 0 {
            let center = egui::pos2(x, y);
            let stroke = egui::Stroke::new(
                0.8_f32,
                Color32::from_rgba_unmultiplied(222, 211, 255, alpha),
            );
            painter.line_segment(
                [center - egui::vec2(4., 0.), center + egui::vec2(4., 0.)],
                stroke,
            );
            painter.line_segment(
                [center - egui::vec2(0., 4.), center + egui::vec2(0., 4.)],
                stroke,
            );
        }
    }
    for layer in 0..3 {
        for index in 0..12 {
            let seed = (layer * 17 + index) as f32;
            let width = rect.width() * (0.14 + random(seed + 5.) * 0.12);
            let x = rect.left()
                + (index as f32 * rect.width() / 8.
                    + time * (9. + layer as f32 * 5.)
                    + layer as f32 * 193.)
                    .rem_euclid(rect.width() + width * 2.)
                - width;
            let y = rect.bottom() - rect.height() * (0.015 + layer as f32 * 0.055)
                + random(seed + 3.) * 35.;
            glow(
                painter,
                egui::pos2(x, y),
                egui::vec2(width, 60. + layer as f32 * 20.),
                Color32::from_rgba_unmultiplied(
                    108 + layer as u8 * 12,
                    57,
                    171 + layer as u8 * 15,
                    22 + layer as u8 * 6,
                ),
            );
        }
    }
    let cycle = (time / 7.5).floor();
    let phase = time.rem_euclid(7.5) / 1.7;
    if phase < 1. {
        let start = egui::pos2(
            rect.left() - rect.width() * 0.15,
            rect.top() + rect.height() * (0.02 + random(cycle * 13. + 4.) * 0.3),
        );
        let travel = egui::vec2(rect.width() * 1.3, rect.height() * 0.46);
        let head = start + travel * phase;
        let tail = head - travel * 0.18;
        let opacity = (phase * 8.).min(1.) * ((1. - phase) * 8.).min(1.);
        painter.line_segment(
            [tail, head],
            egui::Stroke::new(
                2_f32,
                Color32::from_rgba_unmultiplied(142, 200, 255, (opacity * 150.) as u8),
            ),
        );
        glow(
            painter,
            head,
            egui::vec2(12., 12.),
            Color32::from_rgba_unmultiplied(168, 210, 255, (opacity * 130.) as u8),
        );
        painter.circle_filled(
            head,
            2.,
            Color32::from_rgba_unmultiplied(245, 247, 255, (opacity * 255.) as u8),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_steps_do_not_write_files_or_reset_the_animation_clock() {
        let folder = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        let theme = Theme::builtin("luna").unwrap();
        theme.apply(&ctx);
        let library = Library {
            root: folder.path().into(),
            games: Vec::new(),
            consoles: Vec::new(),
            collections: Vec::new(),
            theme: "luna".into(),
            settings: Default::default(),
        };
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let preferences = folder.path().join("settings.json");
        let originals = HashMap::new();
        let themes = vec![("luna".into(), "Luna".into())];
        let mut art = Artwork::new(ctx.clone());
        let mut preview = Preview::default();
        preview.request();
        let clock = preview.started;
        for step in 0..5 {
            preview.step = step;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280., 720.),
                )),
                events: if step < 4 {
                    vec![egui::Event::Key {
                        key: egui::Key::F11,
                        physical_key: None,
                        pressed: step != 2,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }]
                } else {
                    Vec::new()
                },
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                preview.show(
                    ctx,
                    &mut Environment {
                        library: &library,
                        presets: &[],
                        themes: &themes,
                        data: &data,
                        theme: &theme,
                        artwork: &mut art,
                        original_covers: &originals,
                        preferences_path: &preferences,
                        playing: false,
                    },
                )
            });
            assert_eq!(preview.started, clock);
            assert_eq!(preview.fullscreen, step < 3);
        }
        preview.close(&ctx);
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
    }
}

// Draw a sample library from the selected palette so custom themes preview too.
fn library_preview(ui: &mut egui::Ui, theme: &Theme, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, width * 0.55), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let scale = width / 480.;
    let at = |x, y| rect.min + egui::vec2(x, y) * scale;
    let area = |x, y, w, h| egui::Rect::from_min_size(at(x, y), egui::vec2(w, h) * scale);
    let color = |key, fallback| theme.color(key, fallback);
    let base = color("base", Color32::from_gray(36));
    let text = color("text", Color32::WHITE);
    let line = color("mid", Color32::GRAY);
    let accent = color("accent", Color32::LIGHT_BLUE);
    let selected = color("sidebar-selected-background", accent);
    let selected_text = color("sidebar-selected-text", text);
    let muted = color("placeholder-text", text);
    let font = |size: f32| egui::FontId::proportional(size * scale);
    let bold = |size: f32| fonts::bold(size * scale);
    p.rect_filled(rect, 8., base);
    p.rect_filled(
        area(1., 32., 117., 231.),
        egui::CornerRadius {
            sw: 7,
            ..Default::default()
        },
        color("sidebar-background", base),
    );
    p.rect_filled(
        area(1., 1., 478., 32.),
        egui::CornerRadius {
            nw: 7,
            ne: 7,
            ..Default::default()
        },
        color("navigation-background", base),
    );
    p.vline(
        at(118., 0.).x,
        at(0., 33.).y..=at(0., 263.).y,
        egui::Stroke::new(1_f32, line),
    );
    p.hline(
        rect.x_range(),
        at(0., 33.).y,
        egui::Stroke::new(1_f32, line),
    );
    p.text(
        at(13., 16.),
        egui::Align2::LEFT_CENTER,
        "EmuLuna",
        bold(11.),
        text,
    );
    p.rect_filled(area(154., 6., 61., 21.), 10., selected);
    for (x, label, fg) in [
        (184., "Library", selected_text),
        (255., "Save States", text),
        (336., "Screenshots", text),
    ] {
        p.text(
            at(x, 16.),
            egui::Align2::CENTER_CENTER,
            label,
            bold(10.),
            fg,
        );
    }
    p.text(
        at(12., 52.),
        egui::Align2::LEFT_CENTER,
        "Consoles",
        bold(10.),
        muted,
    );
    p.rect_filled(area(6., 64., 106., 20.), 4., selected);
    for (y, label, fg) in [
        (74., "Nintendo 64", selected_text),
        (96., "Sega Saturn", text),
        (118., "PlayStation", text),
        (155., "Collections", muted),
        (177., "All Games", text),
        (199., "Favorites", text),
    ] {
        p.text(at(12., y), egui::Align2::LEFT_CENTER, label, font(11.), fg);
    }
    for (i, (title, sky, ground)) in [
        ("Star Circuit", [28, 34, 93], [117, 58, 173]),
        ("Alpine Rally", [37, 99, 124], [31, 75, 92]),
        ("Solar Drift", [135, 55, 58], [96, 36, 68]),
        ("Moon Harbor", [33, 53, 100], [36, 106, 122]),
        ("Wild Trails", [70, 105, 77], [47, 77, 49]),
        ("Night Run", [79, 44, 100], [139, 62, 102]),
    ]
    .into_iter()
    .enumerate()
    {
        let x = 132. + (i % 3) as f32 * 114.;
        let y = 47. + (i / 3) as f32 * 102.;
        let cover = area(x, y, 96., 65.);
        if i == 0 {
            p.rect_stroke(
                cover.expand(2. * scale),
                4.,
                egui::Stroke::new(1.5_f32, accent),
                egui::StrokeKind::Outside,
            );
        }
        p.rect_filled(cover, 3., Color32::from_rgb(sky[0], sky[1], sky[2]));
        p.circle_filled(
            at(x + 70., y + 20.),
            11. * scale,
            Color32::from_rgb(255, 218, 164),
        );
        p.add(egui::Shape::convex_polygon(
            vec![
                at(x + 2., y + 63.),
                at(x + 40., y + 23.),
                at(x + 94., y + 63.),
            ],
            Color32::from_rgb(ground[0], ground[1], ground[2]),
            egui::Stroke::NONE,
        ));
        p.text(
            at(x + 48., y + 50.),
            egui::Align2::CENTER_CENTER,
            title,
            bold(10.),
            Color32::WHITE,
        );
        p.text(
            at(x + 48., y + 77.),
            egui::Align2::CENTER_CENTER,
            title,
            font(10.),
            text,
        );
    }
    p.rect_stroke(
        rect,
        8.,
        egui::Stroke::new(1_f32, line),
        egui::StrokeKind::Inside,
    );
}
