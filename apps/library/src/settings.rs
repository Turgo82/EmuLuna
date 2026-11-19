//! Native settings, sharing the original catalog and isolated preferences.
mod about;
pub(crate) mod appimage_update;
mod controls;
pub(crate) mod conversion;
mod diagram;
mod gallery;
mod maintenance;
pub(crate) mod onboarding;
pub(crate) mod tools;
mod updates;
use crate::{
    artwork::{Artwork, SETTINGS_ICON_POINTS},
    preferences::Preferences,
};
use eframe::egui::{self, emath::GuiRounding, Color32, RichText};
use emuluna_library_model::Library;
use emuluna_theme::{fonts, Theme};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Library,
    Gameplay,
    Controls,
    Cores,
    Firmware,
    Advanced,
}
impl Page {
    const ALL: [(Self, &'static str, &'static str); 7] = [
        (Self::General, "General", "general"),
        (Self::Library, "Library", "library"),
        (Self::Gameplay, "Gameplay", "gameplay"),
        (Self::Controls, "Controls", "controls"),
        (Self::Cores, "Cores", "cores"),
        (Self::Firmware, "System Files", "bios"),
        (Self::Advanced, "Advanced", "advanced"),
    ];
}
pub struct Environment<'a> {
    pub library: &'a Library,
    pub presets: &'a [(String, String)],
    pub themes: &'a [(String, String)],
    pub data: &'a Path,
    pub theme: &'a Theme,
    pub artwork: &'a mut Artwork,
    pub original_covers: &'a HashMap<String, PathBuf>,
    pub preferences_path: &'a Path,
    pub playing: bool,
}
pub struct Settings {
    pub open: bool,
    pub about_open: bool,
    pub reload_themes: bool,
    pub rebuild_artwork: bool,
    pub move_library: bool,
    tab: Page,
    console: String,
    audio: Option<sdl2::AudioSubsystem>,
    outputs: Vec<String>,
    choices: HashMap<String, Vec<emuluna_launch_config::CoreChoice>>,
    pub message: String,
    controls: controls::Controls,
    conversion: conversion::Conversion,
    maintenance: maintenance::Maintenance,
    tools: tools::Tools,
    about: about::About,
    updates: updates::Checker,
    onboarding: onboarding::Preview,
    gallery: gallery::Gallery,
    new_theme: String,
    forced_advanced: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            open: false,
            about_open: false,
            reload_themes: false,
            rebuild_artwork: false,
            move_library: false,
            tab: Page::General,
            console: String::new(),
            audio: None,
            outputs: Vec::new(),
            choices: HashMap::new(),
            message: String::new(),
            controls: controls::Controls::default(),
            conversion: conversion::Conversion::default(),
            maintenance: maintenance::Maintenance::default(),
            tools: tools::Tools::default(),
            about: about::About::default(),
            updates: updates::Checker::default(),
            onboarding: onboarding::Preview::default(),
            gallery: gallery::Gallery::default(),
            new_theme: String::new(),
            forced_advanced: false,
        }
    }
}
pub fn effective_theme<'a>(ctx: &egui::Context, id: &'a str) -> &'a str {
    if id == "system" {
        if ctx.system_theme() == Some(egui::Theme::Light) {
            "daylight"
        } else {
            "classic"
        }
    } else {
        id
    }
}
pub fn theme_root(data: &Path, preferences: &Path, id: &str) -> PathBuf {
    let user = crate::storage::themes(preferences);
    if user.join(id).join("theme.json").is_file() {
        user
    } else {
        data.join("themes")
    }
}
pub fn discover_themes(data: &Path, preferences: &Path) -> Vec<(String, String)> {
    let mut choices = emuluna_theme::discover(&data.join("themes"));
    for choice in emuluna_theme::discover(&crate::storage::themes(preferences)) {
        choices.retain(|(id, _)| id != &choice.0);
        choices.push(choice);
    }
    choices
}
pub(crate) fn value<'a>(
    prefs: &'a Preferences,
    library: &'a Library,
    key: &str,
    default: &'a str,
) -> &'a str {
    prefs
        .get(key)
        .or_else(|| library.settings.get(key).map(String::as_str))
        .unwrap_or(default)
}
fn toggle(
    ui: &mut egui::Ui,
    prefs: &mut Preferences,
    library: &Library,
    key: &str,
    label: &str,
    default: bool,
) {
    let mut enabled = value(prefs, library, key, if default { "1" } else { "0" }) != "0";
    if ui.checkbox(&mut enabled, label).changed() {
        prefs.set(key, if enabled { "1" } else { "0" });
    }
}
fn heading(ui: &mut egui::Ui, label: &str) {
    ui.add_space(14.);
    ui.label(RichText::new(label).font(fonts::bold(16.)));
    ui.add_space(5.);
}
impl Settings {
    pub fn sidebar_busy(&self) -> bool {
        self.tools.busy() || self.maintenance.busy() || self.conversion.busy()
    }
    pub fn sidebar_activity(&mut self, ui: &mut egui::Ui) {
        if self.conversion.sidebar(ui) {
            return;
        }
        let title = if self.tools.busy() {
            "Managing cores"
        } else {
            "Checking library files"
        };
        crate::activity::sidebar(ui, title, "Working in the background", (0, 0), None);
    }
    pub fn take_converted_artwork(&mut self) -> Option<(PathBuf, conversion::Index)> {
        self.conversion.changed.take()
    }
    pub fn select_console(&mut self, id: &str) {
        self.console = id.to_owned();
        self.controls.select_console(id);
    }
    pub fn update_notification(&self) -> String {
        self.updates.notification()
    }

    pub fn update_release_page(&self) -> Option<&str> {
        self.updates.links().map(|(page, _)| page)
    }

    pub fn poll_updates(&mut self, ctx: &egui::Context, prefs: &Preferences, library: &Library) {
        self.updates.poll(ctx, value(prefs, library, updates::AUTO, "1") == "1");
    }

    pub fn show_update_prompt(&mut self, ctx: &egui::Context) {
        if !self.open && !self.about_open && !self.onboarding.open {
            self.updates.show_prompt(ctx);
        }
    }

    pub fn restart_for_update(&mut self, ctx: &egui::Context) {
        self.updates.restart_if_ready(ctx);
    }

    pub fn open_tab(&mut self, tab: &str) -> Result<(), String> {
        if let Some(step) = [
            "onboarding",
            "onboarding-storage",
            "onboarding-appearance",
            "onboarding-consoles",
            "onboarding-ready",
        ]
        .iter()
        .position(|name| *name == tab)
        {
            self.onboarding.request_page(step, step == 3);
            return Ok(());
        }
        if tab == "about" {
            self.about_open = true;
            return Ok(());
        }
        self.tab = Page::ALL
            .iter()
            .find(|(_, _, id)| *id == tab || (tab == "system-files" && *id == "bios"))
            .map(|(p, _, _)| *p)
            .ok_or("Unknown settings tab")?;
        self.forced_advanced = self.tab == Page::Advanced;
        self.open = true;
        if self.tab == Page::General {
            self.refresh_audio();
        }
        Ok(())
    }
    pub fn refresh_audio(&mut self) {
        match sdl2::init().and_then(|s| s.audio()) {
            Ok(audio) => {
                self.outputs = emuluna_audio_device::Device::outputs(&audio);
                self.audio = Some(audio);
            }
            Err(e) => self.message = format!("Audio outputs unavailable: {e}"),
        }
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        prefs: &mut Preferences,
        env: &mut Environment<'_>,
    ) -> bool {
        if !self.open || self.tab != Page::Controls || self.onboarding.open {
            self.controls.cancel_capture();
        }
        let before = prefs.values.clone();
        let mut reload = false;
        self.conversion.poll(ctx, &mut self.message);
        self.maintenance.poll(ctx, prefs, &mut self.message);
        self.tools.poll(ctx, prefs, &mut self.message);
        if self.tools.cores_changed {
            reload = true;
            self.choices.clear();
            self.maintenance.refresh_firmware();
            self.tools.cores_changed = false;
        }
        if self.about_open
            && self
                .about
                .show(ctx, prefs, env, &mut self.about_open, &mut self.updates)
        {
            self.open = true;
            self.tab = Page::Advanced;
        }
        self.about.show_legal(ctx, prefs, env.library);
        self.onboarding.show(ctx, env);
        if self.onboarding.open {
            return prefs.values != before || reload;
        }
        if !self.open {
            return prefs.values != before || reload;
        }
        let advanced =
            self.forced_advanced || value(prefs, env.library, "advanced.unlocked", "0") == "1";
        if !advanced && self.tab == Page::Advanced {
            self.tab = Page::General;
        }
        let mut open = self.open;
        let size = ctx.screen_rect().size();
        let mut done = false;
        let mut restore = false;
        let settings_style = ctx.style();
        let mut contents = |ui: &mut egui::Ui| {
            ui.set_style(settings_style.clone());
            ui.set_min_size(size);
            ui.set_max_size(size);
            ui.interact(
                ui.max_rect(),
                ui.id().with("settings-body"),
                egui::Sense::click(),
            );
            ui.spacing_mut().item_spacing = egui::vec2(10., 10.);
            let sidebar_width = (size.x * 0.17).clamp(168., 190.);
            let rect = ui.allocate_exact_size(size, egui::Sense::hover()).0;
            let left = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(rect.left() + sidebar_width, rect.bottom()),
            );
            let right_panel =
                egui::Rect::from_min_max(egui::pos2(left.right() + 1., rect.top()), rect.max);
            let mut right = right_panel.shrink(22.);
            right.min.x += if size.x >= 1000. { 12. } else { 8. };
            ui.painter().line_segment(
                [left.right_top(), left.right_bottom()],
                egui::Stroke::new(1_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
            );
            let mut nav = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("settings-navigation")
                    .max_rect(left.shrink2(egui::vec2(12., 20.))),
            );
            nav.set_clip_rect(left);
            nav.label(RichText::new("Settings").font(fonts::bold(24.)));
            nav.add_space(20.);
            for (page, label, icon) in Page::ALL {
                if page == Page::Advanced && !advanced {
                    continue;
                }
                let (row, response) = nav.allocate_exact_size(
                    egui::vec2(sidebar_width - 24., 46.),
                    egui::Sense::click(),
                );
                let selected = self.tab == page;
                if selected || response.hovered() {
                    nav.painter().rect_filled(
                        row,
                        8.,
                        if selected {
                            env.theme.color(
                                "navigation-selected-background",
                                nav.visuals().selection.bg_fill,
                            )
                        } else {
                            nav.visuals().widgets.hovered.weak_bg_fill
                        },
                    );
                }
                let color = if selected {
                    env.theme
                        .color("accent", nav.visuals().selection.stroke.color)
                } else {
                    nav.visuals().text_color()
                };
                paint_navigation_icon(
                    &nav,
                    egui::Rect::from_center_size(
                        egui::pos2(row.left() + 24., row.center().y),
                        egui::Vec2::splat(SETTINGS_ICON_POINTS),
                    ),
                    env,
                    icon,
                );
                nav.painter().text(
                    egui::pos2(row.left() + 52., row.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    fonts::bold(14.),
                    color,
                );
                if response.clicked() {
                    self.tab = page;
                    self.gallery.open = false;
                    self.message.clear();
                }
            }
            let footer_top = right.bottom() - 42.;
            let right = right.with_max_y(footer_top - 8.);
            let scroll_page =
                self.tab != Page::Controls && !(self.tab == Page::Advanced && self.gallery.open);
            let body_rect = if scroll_page {
                right.with_max_x(right_panel.right() - 4.)
            } else {
                right
            };
            let mut body = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("settings-content")
                    .max_rect(body_rect),
            );
            body.set_clip_rect(body_rect.expand(4.).intersect(right_panel));
            if self.tab == Page::Controls {
                self.controls.show(&mut body, prefs, env);
            } else if self.tab == Page::Advanced && self.gallery.open {
                self.gallery.show(&mut body, env);
            } else {
                egui::ScrollArea::vertical()
                    .id_salt(("settings-page", self.tab as u8))
                    .auto_shrink([false, false])
                    .max_height(body.available_height())
                    .show(&mut body, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                right: 18,
                                ..egui::Margin::ZERO
                            })
                            .show(ui, |ui| {
                                match self.tab {
                                    Page::General => reload |= self.general(ui, prefs, env),
                                    Page::Library => self.library(ui, prefs, env),
                                    Page::Gameplay => self.gameplay(ui, prefs, env),
                                    Page::Cores => self.cores(ui, prefs, env),
                                    Page::Firmware => {
                                        self.maintenance.firmware(ui, prefs, env, &mut self.message)
                                    }
                                    Page::Advanced => self.advanced(ui, prefs, env),
                                    Page::Controls => {}
                                }
                                if !self.message.is_empty() {
                                    ui.separator();
                                    ui.label(&self.message);
                                }
                            });
                    });
            }
            let footer_rect = egui::Rect::from_min_max(
                egui::pos2(right.left(), footer_top),
                egui::pos2(right_panel.right() - 22., footer_top + 42.),
            );
            let mut footer = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("settings-footer")
                    .max_rect(footer_rect),
            );
            footer.separator();
            footer.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    done = true;
                }
                if ui
                    .add_enabled(!self.sidebar_busy(), egui::Button::new("Restore defaults"))
                    .clicked()
                {
                    restore = true;
                }
            });
        };
        ctx.style_mut(|style| style.spacing.interact_size.y = 22.);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(ctx.style().visuals.panel_fill))
            .show(ctx, &mut contents);
        if ctx.input(|input| input.viewport().close_requested()) {
            open = false;
        }
        ctx.set_style(settings_style);
        if restore {
            self.restore_tab_defaults(prefs, env.library);
            ctx.request_repaint();
        }
        if done {
            open = false;
        }
        if self.controls.close_requested {
            self.controls.close_requested = false;
            open = false;
        }
        if self.maintenance.close_requested {
            self.maintenance.close_requested = false;
            open = false;
        }
        self.open = open;
        if !open {
            self.gallery.open = false;
        }
        prefs.values != before || reload
    }
    fn restore_tab_defaults(&mut self, prefs: &mut Preferences, library: &Library) {
        let defaults: &[(&str, &str)] = match self.tab {
            Page::General => &[
                ("theme", "luna"),
                (updates::AUTO, "1"),
                ("native.audio_output", ""),
                ("volume", "80"),
                ("experimental.minimize_library_during_game", "1"),
            ],
            Page::Library => &[
                ("library.show_alphabet_index", "1"),
                ("library.rounded_box_art", "1"),
                (crate::region::KEY, crate::region::DEFAULT),
                ("library.hide_empty_consoles", "1"),
            ],
            Page::Gameplay => &[
                ("fullscreen_default", "0"),
                ("pause_unfocused", "1"),
                ("game_mode.keep_awake", "1"),
                ("hide_game_cursor", "1"),
                ("integer_scale", "0"),
                ("aspect_mode", "system"),
                ("native.filter", "nearest"),
                ("native.keep_progress", "1"),
                ("native.auto_resume", "0"),
            ],
            Page::Advanced => &[
                ("experimental.hardware_rendering", "1"),
                ("artwork.webp_quality", "85"),
            ],
            Page::Controls => {
                self.controls.cancel_capture();
                let keys: Vec<_> = library
                    .settings
                    .keys()
                    .filter(|key| key.starts_with("controls."))
                    .cloned()
                    .collect();
                prefs.values.retain(|key, _| !key.starts_with("controls."));
                for key in keys {
                    prefs.set(&key, "{}");
                }
                &[]
            }
            Page::Cores => {
                let is_core_preference = |key: &str| {
                    key.starts_with("core.")
                        || key
                            .strip_prefix("core_option.")
                            .and_then(|suffix| suffix.split_once('.'))
                            .is_some_and(|(system, _)| {
                                library.consoles.iter().any(|console| console.id == system)
                            })
                };
                prefs.values.retain(|key, _| !is_core_preference(key));
                for key in library
                    .settings
                    .keys()
                    .filter(|key| is_core_preference(key))
                {
                    prefs.set(key, "");
                }
                &[]
            }
            Page::Firmware => {
                self.maintenance.restore_firmware_defaults();
                &[]
            }
        };
        for (key, value) in defaults {
            prefs.set(key, *value);
        }
        self.message.clear();
    }

    fn general(
        &mut self,
        ui: &mut egui::Ui,
        prefs: &mut Preferences,
        env: &Environment<'_>,
    ) -> bool {
        heading(ui, "Appearance");
        let old = prefs
            .get("theme")
            .unwrap_or(
                if env
                    .library
                    .settings
                    .get("appearance.use_system_theme")
                    .is_some_and(|s| s == "1")
                {
                    "system"
                } else {
                    &env.library.theme
                },
            )
            .to_owned();
        let mut selected = old.clone();
        egui::ComboBox::from_id_salt("settings-theme")
            .width(280.)
            .selected_text(if selected == "system" {
                "System theme"
            } else {
                env.themes
                    .iter()
                    .find(|(id, _)| id == &selected)
                    .map_or(selected.as_str(), |(_, name)| name)
            })
            .show_ui(ui, |ui| {
                let selected_text = ui.visuals().selection.stroke.color;
                for (id, name) in std::iter::once(("system", "System theme")).chain(
                    env.themes
                        .iter()
                        .map(|(id, name)| (id.as_str(), name.as_str())),
                ) {
                    let active = selected == id;
                    let label = if active {
                        RichText::new(name).color(selected_text)
                    } else {
                        RichText::new(name)
                    };
                    ui.selectable_value(&mut selected, id.to_owned(), label);
                }
            });
        if selected != old {
            prefs.set("theme", selected);
        }
        let mut reload = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Reload themes").clicked() {
                self.reload_themes = true;
                self.rebuild_artwork = true;
                if prefs.get("theme").is_none() {
                    prefs.set("theme", old.clone());
                }
                reload = true;
            }
            if ui.button("Open themes folder").clicked() {
                let folder = crate::storage::themes(env.preferences_path);
                match std::fs::create_dir_all(&folder)
                    .map_err(|e| e.to_string())
                    .and_then(|_| crate::actions::open_path(folder))
                {
                    Ok(()) => {}
                    Err(e) => self.message = e,
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_theme)
                    .desired_width(220.)
                    .hint_text("New theme name"),
            );
            if ui.button("Create theme copy").clicked() {
                match tools::copy_theme(env, &self.new_theme) {
                    Ok(id) => {
                        prefs.set("theme", id);
                        self.reload_themes = true;
                        reload = true;
                        self.message =
                            "Your standalone theme is ready. Edit its files, then reload themes."
                                .into();
                    }
                    Err(e) => self.message = e,
                }
            }
        });
        heading(ui, "Updates");
        toggle(
            ui,
            prefs,
            env.library,
            updates::AUTO,
            "Check for updates at startup",
            true,
        );
        ui.add_space(12.);
        heading(ui, "Audio");
        let old = prefs.get("native.audio_output").unwrap_or("").to_owned();
        let mut selected = old.clone();
        egui::ComboBox::from_id_salt("settings-output")
            .width(340.)
            .selected_text(if selected.is_empty() {
                "System default"
            } else {
                &selected
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected, String::new(), "System default");
                for name in &self.outputs {
                    ui.selectable_value(&mut selected, name.clone(), name);
                }
            });
        if old != selected {
            prefs.set("native.audio_output", selected);
        }
        if ui.button("Refresh audio outputs").clicked() {
            self.refresh_audio();
        }
        ui.weak("System default follows desktop output changes.");
        let mut volume = value(prefs, env.library, "volume", "80")
            .parse::<u32>()
            .unwrap_or(80)
            .min(100);
        if ui
            .add(
                egui::Slider::new(&mut volume, 0..=100)
                    .text("Game volume")
                    .suffix("%"),
            )
            .changed()
        {
            prefs.set("volume", volume.to_string());
        }
        ui.weak(
            "These defaults apply to the next game. Use the game overlay to adjust volume or mute.",
        );
        heading(ui, "Game window");
        toggle(
            ui,
            prefs,
            env.library,
            "experimental.minimize_library_during_game",
            "Minimize the library while a game is running",
            true,
        );
        if crate::kwin_restore::wayland() && !crate::kwin_restore::available() {
            ui.weak("On Wayland, the library stays behind the game so it is available when the game closes.");
        }
        reload
    }
    fn gameplay(&mut self, ui: &mut egui::Ui, prefs: &mut Preferences, env: &Environment<'_>) {
        heading(ui, "Game window");
        for (key, label, default) in [
            ("fullscreen_default", "Open games in fullscreen", false),
            (
                "pause_unfocused",
                "Pause when switching to another window",
                true,
            ),
            (
                "game_mode.keep_awake",
                "Keep the display awake while playing",
                true,
            ),
            (
                "hide_game_cursor",
                "Hide the pointer with the gameplay controls",
                true,
            ),
            (
                "integer_scale",
                "Use integer scaling for sharp pixels",
                false,
            ),
        ] {
            toggle(ui, prefs, env.library, key, label, default);
        }
        let mut aspect = value(prefs, env.library, "aspect_mode", "system").to_owned();
        egui::ComboBox::from_id_salt("settings-aspect")
            .width(260.)
            .selected_text(if aspect == "square" {
                "Square pixels"
            } else {
                "System aspect ratio"
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut aspect, "system".into(), "System aspect ratio");
                ui.selectable_value(&mut aspect, "square".into(), "Square pixels");
            });
        if aspect != value(prefs, env.library, "aspect_mode", "system") {
            prefs.set("aspect_mode", aspect);
        }
        heading(ui, "Video filter");
        let mut selected = prefs.get("native.filter").unwrap_or("nearest").to_owned();
        egui::ComboBox::from_id_salt("settings-filter")
            .width(300.)
            .selected_text(
                env.presets
                    .iter()
                    .find(|(id, _)| id == &selected)
                    .map_or("Nearest Neighbor", |(_, name)| name),
            )
            .show_ui(ui, |ui| {
                for (id, name) in env.presets {
                    ui.selectable_value(&mut selected, id.clone(), name);
                }
            });
        if selected != prefs.get("native.filter").unwrap_or("nearest") {
            prefs.set("native.filter", selected);
        }
        heading(ui, "Saved progress");
        toggle(
            ui,
            prefs,
            env.library,
            "native.keep_progress",
            "Keep game progress between sessions",
            true,
        );
        ui.add_enabled_ui(
            value(prefs, env.library, "native.keep_progress", "1") != "0",
            |ui| {
                toggle(
                    ui,
                    prefs,
                    env.library,
                    "native.auto_resume",
                    "Autosave and resume automatically",
                    false,
                )
            },
        );
        ui.weak("Native progress is stored separately from the original application's saves.");
        ui.add_space(12.);
        ui.weak("These defaults apply when the next game starts. In-game controls remain available in the overlay.");
    }
    fn cores(&mut self, ui: &mut egui::Ui, prefs: &mut Preferences, env: &mut Environment<'_>) {
        if self.console.is_empty() {
            self.console = env
                .library
                .consoles
                .first()
                .map_or(String::new(), |c| c.id.clone());
        }
        heading(ui, "Console");
        egui::ComboBox::from_id_salt("settings-console")
            .width(300.)
            .selected_text(
                env.library
                    .consoles
                    .iter()
                    .find(|c| c.id == self.console)
                    .map_or(self.console.as_str(), |c| c.name.as_str()),
            )
            .show_ui(ui, |ui| {
                for console in &env.library.consoles {
                    ui.selectable_value(&mut self.console, console.id.clone(), &console.name);
                }
            });
        if !self.choices.contains_key(&self.console) {
            match emuluna_launch_config::installed_choices(&env.library.root, &self.console) {
                Ok(choices) => {
                    self.choices.insert(self.console.clone(), choices);
                }
                Err(e) => {
                    self.message = e;
                }
            }
        }
        let key = format!("core.{}", self.console);
        let recommended = emuluna_launch_config::recommended_core(&self.console);
        let saved = value(prefs, env.library, &key, "");
        if let Some(choices) = self.choices.get(&self.console) {
            let old = emuluna_launch_config::explicit_core(saved)
                .or_else(|| choices.first().map(|core| core.id.as_str()))
                .unwrap_or_default()
                .to_owned();
            let mut selected = old.clone();
            let core_label = |core: &emuluna_launch_config::CoreChoice| {
                if Some(core.id.as_str()) == recommended {
                    format!("{} (Recommended)", core.name)
                } else {
                    core.name.clone()
                }
            };
            heading(ui, "Playback core");
            ui.add_enabled_ui(!choices.is_empty(), |ui| {
                egui::ComboBox::from_id_salt("settings-core")
                    .width(300.)
                    .selected_text(choices.iter().find(|c| c.id == selected).map_or_else(
                        || {
                            if selected.is_empty() {
                                "No core installed".into()
                            } else {
                                "Selected core unavailable".into()
                            }
                        },
                        core_label,
                    ))
                    .show_ui(ui, |ui| {
                        for core in choices {
                            if ui
                                .selectable_label(selected == core.id, core_label(core))
                                .clicked()
                            {
                                selected = core.id.clone();
                            }
                        }
                    });
            });
            if old != selected {
                prefs.set(&key, selected.clone());
            }
            let core = choices.iter().find(|c| c.id == selected);
            if let Some(core) = core {
                let catalog: serde_json::Value =
                    serde_json::from_str(include_str!("../../../data/cores.json"))
                        .expect("built-in catalog");
                if let Some(options) =
                    catalog[&core.catalog_id]["runtime_options"][&self.console].as_array()
                {
                    for option in options {
                        let (Some(name), Some(label), Some(on), Some(off)) = (
                            option["key"].as_str(),
                            option["label"].as_str(),
                            option["enabled"].as_str(),
                            option["disabled"].as_str(),
                        ) else {
                            continue;
                        };
                        if let Some(group) = option["group"].as_str() {
                            heading(ui, group);
                        }
                        let key =
                            format!("core_option.{}.{}.{}", self.console, core.catalog_id, name);
                        let enabled = value(
                            prefs,
                            env.library,
                            &key,
                            option["default"].as_str().unwrap_or(off),
                        );
                        let mut enabled = if enabled.is_empty() {
                            option["default"].as_str() == Some(on)
                        } else {
                            enabled == on
                        };
                        if ui.checkbox(&mut enabled, label).changed() {
                            prefs.set(&key, if enabled { on } else { off });
                        }
                        if let Some(description) = option["description"].as_str() {
                            ui.weak(description);
                        }
                    }
                }
            } else {
                ui.weak("No compatible core is installed for this console.");
            }
        }
        self.tools.cores(ui, env, &self.console, &mut self.message);
    }
    fn library(&mut self, ui: &mut egui::Ui, prefs: &mut Preferences, env: &Environment<'_>) {
        let folder = &env.library.root;
        let mut region = value(
            prefs,
            env.library,
            crate::region::KEY,
            crate::region::DEFAULT,
        )
        .to_owned();
        let previous = region.clone();
        crate::region::picker(ui, &mut region);
        if region != previous {
            prefs.set(crate::region::KEY, region);
        }
        ui.add_space(12.);
        heading(ui, "Library folder");
        ui.monospace(folder.display().to_string());
        ui.horizontal_wrapped(|ui| {
            if ui.button("Open folder").clicked() {
                if let Err(error) = crate::actions::open_path(folder.clone()) {
                    self.message = error;
                }
            }
            if crate::storage::managed(folder)
                && env.preferences_path == crate::storage::preferences_path(folder)
                && ui.button("Move library…").clicked()
            {
                self.move_library = true;
            }
        });
        ui.add_space(12.);
        toggle(
            ui,
            prefs,
            env.library,
            "library.show_alphabet_index",
            "Show alphabet index",
            true,
        );
        toggle(
            ui,
            prefs,
            env.library,
            "library.rounded_box_art",
            "Round box-art and preview corners",
            true,
        );
        toggle(
            ui,
            prefs,
            env.library,
            "library.hide_empty_consoles",
            "Hide consoles with no games",
            true,
        );
    }
    fn advanced(&mut self, ui: &mut egui::Ui, prefs: &mut Preferences, env: &mut Environment<'_>) {
        heading(ui, "Rendering");
        toggle(
            ui,
            prefs,
            env.library,
            "experimental.hardware_rendering",
            "Use OpenGL for cores with an optional hardware renderer",
            true,
        );
        ui.weak("Cores that require OpenGL always use it. The native display uses OpenGL.");
        heading(ui, "Library audit");
        self.maintenance.audit(ui, prefs, env, &mut self.message);
        self.maintenance.export_inventory(ui, env);
        heading(ui, "Developer options");
        if ui
            .add(
                egui::Button::new("UI Gallery…")
                    .corner_radius(7.)
                    .min_size(egui::vec2(118., 34.)),
            )
            .on_hover_text("Preview controls and compare themes")
            .clicked()
        {
            self.gallery.open = true;
        }
        if ui.button("Preview onboarding…").clicked() {
            self.onboarding.request();
        }
        if ui.button("Rebuild artwork previews").clicked() {
            self.rebuild_artwork = true;
            self.message =
                "Artwork previews will be decoded again. Original covers are kept.".into();
        }
        self.conversion.show(ui, prefs, env, &mut self.message);
        if ui.button("Open native settings folder").clicked() {
            if let Some(parent) = env.preferences_path.parent() {
                if let Err(e) = crate::actions::open_path(parent.to_owned()) {
                    self.message = e;
                }
            }
        }
        ui.weak(
            "The Advanced page stays unlocked until you click the About logo four times again.",
        );
    }
}

fn paint_navigation_icon(ui: &egui::Ui, rect: egui::Rect, env: &mut Environment<'_>, icon: &str) {
    let path = env.data.join("icons/settings").join(format!("{icon}.svg"));
    if let Some(texture) = env.artwork.texture(&path) {
        ui.painter().image(
            texture,
            rect.round_to_pixels(ui.pixels_per_point()),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn console_shortcuts_open_requested_settings_page() {
        let mut settings = Settings::default();
        settings.select_console("n64");
        settings.open_tab("controls").unwrap();
        assert!(settings.open && settings.tab == Page::Controls && settings.console == "n64");
        settings.select_console("segacd");
        settings.open_tab("cores").unwrap();
        assert!(settings.open && settings.tab == Page::Cores && settings.console == "segacd");
    }

    #[test]
    fn every_settings_page_keeps_the_same_window_size() {
        let directory = tempfile::tempdir().unwrap();
        let library = Library {
            root: directory.path().into(),
            games: Vec::new(),
            consoles: vec![emuluna_library_model::Console {
                id: "nes".into(),
                name: "Nintendo (NES)".into(),
                icon: "nes.png".into(),
                cover_size: [220, 320],
            }],
            collections: Vec::new(),
            theme: "luna".into(),
            settings: Default::default(),
        };
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let path = directory.path().join("settings.json");
        let originals = HashMap::new();
        let theme = Theme::builtin("luna").unwrap();
        for screen in [egui::vec2(1280., 720.), egui::vec2(850., 650.)] {
            let ctx = egui::Context::default();
            fonts::install(&ctx);
            theme.apply(&ctx);
            let mut artwork = Artwork::new(ctx.clone());
            let mut prefs = Preferences::default();
            prefs.set("advanced.unlocked", "1");
            let mut settings = Settings {
                open: true,
                ..Default::default()
            };
            let mut expected = None;
            for (page, _, _) in Page::ALL.into_iter().chain(Page::ALL) {
                settings.tab = page;
                settings.message = "A status message that must fit inside the page.".repeat(6);
                for _ in 0..3 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
                            ..Default::default()
                        },
                        |ctx| {
                            settings.show(
                                ctx,
                                &mut prefs,
                                &mut Environment {
                                    library: &library,
                                    presets: &[],
                                    themes: &[],
                                    data: &data,
                                    theme: &theme,
                                    artwork: &mut artwork,
                                    original_covers: &originals,
                                    preferences_path: &path,
                                    playing: false,
                                },
                            );
                        },
                    );
                }
                let rect = ctx.screen_rect();
                let expected = *expected.get_or_insert(rect.size());
                assert_eq!(rect.size(), expected, "Settings page changed viewport size");
            }
        }
    }
}
