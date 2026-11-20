//! Interactive samples of EmuLuna's controls. The preview never saves settings.
use super::*;
use crate::{actions, activity, alphabet, library_tabs, Page as LibraryPage};
use emuluna_theme::gameplay;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Section {
    #[default]
    Library,
    Forms,
    Menus,
    Gameplay,
    Feedback,
    Artwork,
    Palette,
}
impl Section {
    const ALL: [(Self, &'static str); 7] = [
        (Self::Library, "Library"),
        (Self::Forms, "Forms"),
        (Self::Menus, "Menus"),
        (Self::Gameplay, "Gameplay"),
        (Self::Feedback, "Feedback"),
        (Self::Artwork, "Artwork"),
        (Self::Palette, "Palette"),
    ];
}

#[derive(Default)]
pub(super) struct Gallery {
    pub open: bool,
    section: Section,
    theme_id: String,
    preview: Option<Theme>,
    error: String,
    samples: Samples,
}
impl Gallery {
    pub fn show(&mut self, ui: &mut egui::Ui, env: &mut Environment<'_>) {
        let selection = if self.theme_id.is_empty() {
            format!("Current theme · {}", env.theme.name)
        } else if self.theme_id == "system" {
            "System".into()
        } else {
            env.themes
                .iter()
                .find(|(id, _)| id == &self.theme_id)
                .map_or_else(|| self.theme_id.clone(), |(_, name)| name.clone())
        };
        let previous = self.theme_id.clone();
        ui.horizontal_wrapped(|ui| {
            if button(ui, "‹ Advanced").clicked() {
                self.open = false;
            }
            ui.add_space(5.);
            ui.label(RichText::new("UI Gallery").font(fonts::bold(24.)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("gallery-theme")
                    .width(190.)
                    .selected_text(selection)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.theme_id, String::new(), "Current app theme");
                        ui.selectable_value(&mut self.theme_id, "system".into(), "System");
                        for (id, name) in env.themes {
                            ui.selectable_value(&mut self.theme_id, id.clone(), name);
                        }
                    })
                    .response
                    .on_hover_text("Preview theme");
            });
        });
        ui.weak("Try the controls here. Theme choices and sample values stay in the gallery.");
        let id = effective_theme(ui.ctx(), &self.theme_id);
        if self.theme_id != previous
            || (self.theme_id == "system"
                && self.preview.as_ref().is_some_and(|theme| theme.id != id))
        {
            self.preview = None;
            self.error.clear();
            if !id.is_empty() {
                match Theme::load(&theme_root(env.data, env.preferences_path, id), id) {
                    Ok(theme) => self.preview = Some(theme),
                    Err(error) => self.error = format!("Could not preview the theme: {error}"),
                }
            }
        }
        if !self.error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.error);
        }
        let theme = self.preview.as_ref().unwrap_or(env.theme);
        let mut style = (**ui.style()).clone();
        theme.apply_to_style(&mut style);
        let panel_bottom = ui.max_rect().bottom();
        ui.scope(|ui| {
            ui.set_style(style);
            let border = ui.visuals().widgets.noninteractive.bg_stroke;
            egui::Frame::new()
                .fill(ui.visuals().window_fill)
                .stroke(border)
                .corner_radius(10.)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        for (section, label) in Section::ALL {
                            let mut tab = egui::Button::new(label);
                            if self.section == section {
                                let selection = ui.visuals().selection;
                                tab = egui::Button::new(RichText::new(label).color(
                                    theme.color("sidebar-selected-text", selection.stroke.color),
                                ))
                                .fill(
                                    theme
                                        .color("navigation-selected-background", selection.bg_fill),
                                );
                            }
                            let response =
                                ui.add(tab.min_size(egui::vec2(75., 32.)).corner_radius(7.));
                            if response.clicked() {
                                self.section = section;
                                self.samples.dialog = false;
                                self.samples.info.clear();
                            }
                        }
                    });
                    ui.add_space(5.);
                    ui.separator();
                    let height = (panel_bottom - ui.cursor().top() - 15.).max(0.);
                    egui::ScrollArea::vertical()
                        .id_salt(("ui-gallery-samples", self.section as u8))
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| match self.section {
                            Section::Library => self.samples.library(ui, theme),
                            Section::Forms => self.samples.forms(ui, env),
                            Section::Menus => self.samples.menus(ui),
                            Section::Gameplay => self.samples.gameplay(ui),
                            Section::Feedback => self.samples.feedback(ui),
                            Section::Artwork => self.samples.artwork(ui, env, theme),
                            Section::Palette => palette(ui, theme),
                        });
                });
        });
    }
}

struct Samples {
    library_page: LibraryPage,
    list: bool,
    collection: usize,
    query: String,
    rating: u8,
    enabled: bool,
    choice: usize,
    input: usize,
    number: i32,
    progress: f32,
    slot: u8,
    paused: bool,
    volume: f32,
    shader: usize,
    menu_open: bool,
    rating_menu: bool,
    dialog: bool,
    info: String,
}
impl Default for Samples {
    fn default() -> Self {
        Self {
            library_page: LibraryPage::Library,
            list: false,
            collection: 0,
            query: "GoldenEye".into(),
            rating: 3,
            enabled: true,
            choice: 0,
            input: 0,
            number: 30,
            progress: 0.54,
            slot: 1,
            paused: false,
            volume: 0.72,
            shader: 0,
            menu_open: true,
            rating_menu: false,
            dialog: false,
            info: String::new(),
        }
    }
}

fn button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .corner_radius(7.)
            .min_size(egui::vec2(76., 34.)),
    )
}
fn label(ui: &mut egui::Ui, name: &str) {
    ui.add_space(12.);
    ui.label(RichText::new(name).font(fonts::bold(16.)));
    ui.add_space(4.);
}
fn surface(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .inner_margin(12)
        .corner_radius(8.)
}
fn sample_popup(
    ui: &mut egui::Ui,
    label: &str,
    contents: impl FnOnce(&mut egui::Ui) -> bool,
) -> egui::Response {
    let id = ui.id().with(label);
    let style = ui.style().clone();
    let response = ui.add(egui::Button::new(label).corner_radius(7.));
    if response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }
    let close = egui::popup::popup_below_widget(
        ui,
        id,
        &response,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_style(style);
            contents(ui)
        },
    );
    if close == Some(true) {
        ui.memory_mut(|memory| memory.close_popup());
    }
    response
}
impl Samples {
    fn library(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        use actions::{CollectionIcon, ToolbarIcon};
        label(ui, "Library toolbar");
        surface(ui)
            .fill(theme.color("navigation-background", ui.visuals().panel_fill))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let accent = ui.visuals().selection;
                    ui.visuals_mut().selection.bg_fill =
                        theme.color("navigation-selected-background", accent.bg_fill);
                    ui.visuals_mut().selection.stroke.color =
                        theme.color("sidebar-selected-text", ui.visuals().text_color());
                    if actions::toolbar_button(ui, ToolbarIcon::Grid, "Grid view", !self.list)
                        .clicked()
                    {
                        self.list = false;
                    }
                    if actions::toolbar_button(ui, ToolbarIcon::List, "List view", self.list)
                        .clicked()
                    {
                        self.list = true;
                    }
                    ui.visuals_mut().selection = accent;
                    library_tabs(ui, theme, &mut self.library_page);
                    actions::search_frame(ui, theme).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.;
                        ui.add_sized([132., 30.], actions::search_input(&mut self.query));
                        if actions::toolbar_button(ui, ToolbarIcon::Clear, "Clear search", false)
                            .clicked()
                        {
                            self.query.clear();
                        }
                    });
                    if actions::toolbar_button(
                        ui,
                        ToolbarIcon::Search,
                        "Search games",
                        !self.query.is_empty(),
                    )
                    .clicked()
                    {
                        self.info = "Search is active in the gallery.".into();
                    }
                    if actions::toolbar_button(ui, ToolbarIcon::Bell, "Notifications", false)
                        .clicked()
                    {
                        self.info = "No new notifications.".into();
                    }
                });
            });
        if !self.info.is_empty() {
            ui.weak(&self.info);
        }
        label(ui, "Collections and ratings");
        ui.horizontal_top(|ui| {
            surface(ui)
                .fill(theme.color("sidebar-background", ui.visuals().panel_fill))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(230.);
                        for (index, (name, count, icon)) in [
                            ("All Games", 245, CollectionIcon::All),
                            ("Recently Played", 12, CollectionIcon::Recent),
                            ("Favorites", 8, CollectionIcon::Favorite),
                            ("Recently Added", 4, CollectionIcon::Added),
                            ("My Collection", 16, CollectionIcon::Folder),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            if actions::collection_row(
                                ui,
                                name,
                                count,
                                icon,
                                self.collection == index,
                                theme,
                            )
                            .clicked()
                            {
                                self.collection = index;
                            }
                        }
                    });
                });
            ui.vertical(|ui| {
                ui.label("Rating stars");
                ui.horizontal(|ui| {
                    for star in 1..=5 {
                        let filled = star <= self.rating;
                        let color = theme.color(
                            if filled {
                                "rating-filled"
                            } else {
                                "rating-empty"
                            },
                            ui.visuals().weak_text_color(),
                        );
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(if filled { "★" } else { "☆" })
                                        .color(color)
                                        .size(18.),
                                )
                                .frame(false),
                            )
                            .on_hover_text(format!("Rate {star} stars"))
                            .clicked()
                        {
                            self.rating = star;
                        }
                    }
                });
                ui.weak("Cover actions");
                let space = ui.allocate_space(egui::vec2(112., 36.)).1;
                let _ = actions::hover_controls(ui, "gallery-cover", space, true, false, true);
            });
        });
    }

    fn forms(&mut self, ui: &mut egui::Ui, env: &mut Environment<'_>) {
        label(ui, "Settings navigation icons");
        ui.horizontal_wrapped(|ui| {
            for (_, title, icon) in Page::ALL {
                let (rect, response) = ui.allocate_exact_size(
                    egui::Vec2::splat(SETTINGS_ICON_POINTS + 6.),
                    egui::Sense::hover(),
                );
                paint_navigation_icon(ui, rect.shrink(3.), env, icon);
                response.on_hover_text(title);
            }
        });
        label(ui, "Buttons, text, and links");
        ui.horizontal_wrapped(|ui| {
            button(ui, "Choose…").on_hover_text("Example hover tooltip");
            ui.add(
                egui::Button::new(RichText::new("Done").color(ui.visuals().selection.stroke.color))
                    .fill(ui.visuals().selection.bg_fill)
                    .corner_radius(7.)
                    .min_size(egui::vec2(86., 34.)),
            );
            ui.add_enabled(
                false,
                egui::Button::new("Unavailable")
                    .corner_radius(7.)
                    .min_size(egui::vec2(110., 34.)),
            );
            ui.link("Project website")
                .on_hover_text("Link appearance only");
        });
        label(ui, "Inputs and selectors");
        egui::Grid::new("gallery-forms")
            .spacing([18., 9.])
            .show(ui, |ui| {
                ui.label("Text field");
                ui.add_sized(
                    [240., 34.],
                    egui::TextEdit::singleline(&mut self.query).hint_text("Enter a name"),
                );
                ui.end_row();
                ui.label("Dropdown");
                let style = ui.style().clone();
                egui::ComboBox::from_id_salt("gallery-dropdown")
                    .width(226.)
                    .selected_text(["Automatic", "Keyboard", "Controller"][self.input])
                    .show_ui(ui, |ui| {
                        ui.set_style(style);
                        for (index, name) in ["Automatic", "Keyboard", "Controller"]
                            .into_iter()
                            .enumerate()
                        {
                            ui.selectable_value(&mut self.input, index, name);
                        }
                    });
                ui.end_row();
                ui.label("Checkbox");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.enabled, "Enabled");
                    ui.add_enabled(false, egui::Checkbox::new(&mut false, "Unavailable"));
                });
                ui.end_row();
                ui.label("Radio buttons");
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut self.choice, 0, "Keep ROM files");
                    ui.radio_value(&mut self.choice, 1, "Move to Trash");
                });
                ui.end_row();
                ui.label("Number input");
                ui.add(
                    egui::DragValue::new(&mut self.number)
                        .range(0..=100)
                        .suffix(" days"),
                );
                ui.end_row();
                ui.label("Slider");
                ui.add(egui::Slider::new(&mut self.progress, 0.0..=1.0).trailing_fill(true));
                ui.end_row();
            });
    }

    fn menus(&mut self, ui: &mut egui::Ui) {
        label(ui, "Library + menu");
        gameplay::game_menu_frame(ui).show(ui, |ui| {
            ui.set_width(250.);
            ui.spacing_mut().item_spacing.y = 0.;
            for (label, shortcut, enabled) in [
                ("Import games…", "Ctrl+O", true),
                ("Import folder…", "Ctrl+Shift+O", true),
                ("Back up disc…", "", true),
                ("New collection…", "", true),
                ("New smart collection…", "", false),
            ] {
                if actions::menu_row(ui, label, shortcut, enabled).clicked() {
                    self.info = format!("Previewed {label}");
                }
            }
        });
        ui.weak("The disabled row shows how unavailable actions look.");
        label(ui, "Context menu and submenus");
        sample_popup(ui, "Open example menu", |ui| {
            ui.set_width(200.);
            if actions::menu_row(ui, "Rating", "▶", true).clicked() {
                self.rating_menu = !self.rating_menu;
            }
            if self.rating_menu {
                for rating in 1..=5 {
                    if ui
                        .selectable_label(self.rating == rating, "★".repeat(rating as usize))
                        .clicked()
                    {
                        self.rating = rating;
                        return true;
                    }
                }
            }
            ui.separator();
            if ui.button("Remove from library…").clicked() {
                self.dialog = true;
                return true;
            }
            false
        });
        if !self.info.is_empty() {
            ui.weak(&self.info);
        }
        self.dialog(ui);
    }

    fn gameplay(&mut self, ui: &mut egui::Ui) {
        use gameplay::{Icon, MenuMarker};
        label(ui, "In-game toolbar");
        egui::Frame::new()
            .fill(Color32::BLACK)
            .corner_radius(8.)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let scale = 0.72;
                ui.scope(|ui| {
                    gameplay::bar_style(ui, scale);
                    gameplay::bar_frame(scale).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if gameplay::icon_button(ui, Icon::Stop, "Stop game", true, scale)
                                .clicked()
                            {
                                self.dialog = true;
                            }
                            if gameplay::icon_button(
                                ui,
                                if self.paused { Icon::Play } else { Icon::Pause },
                                "Pause / Resume",
                                true,
                                scale,
                            )
                            .clicked()
                            {
                                self.paused = !self.paused;
                            }
                            gameplay::icon_button(ui, Icon::Reset, "Restart game", true, scale);
                            gameplay::icon_button(ui, Icon::Save, "Save state", true, scale);
                            gameplay::icon_button(
                                ui,
                                Icon::Load,
                                "Load state (disabled)",
                                false,
                                scale,
                            );
                            let style = ui.style().clone();
                            egui::ComboBox::from_id_salt("gallery-slot")
                                .width(68.)
                                .selected_text(format!("Slot {}", self.slot))
                                .show_ui(ui, |ui| {
                                    ui.set_style(style);
                                    for slot in 1..=10 {
                                        ui.selectable_value(
                                            &mut self.slot,
                                            slot,
                                            format!("Slot {slot}"),
                                        );
                                    }
                                });
                            gameplay::icon_button(ui, Icon::Screenshot, "Screenshot", true, scale);
                            let filter = sample_popup(ui, "      Filter", |ui| {
                                for (i, filter) in ["Nearest Neighbor", "Linear", "CRT Geom"]
                                    .into_iter()
                                    .enumerate()
                                {
                                    if ui.selectable_value(&mut self.shader, i, filter).clicked() {
                                        return true;
                                    }
                                }
                                false
                            });
                            gameplay::paint_icon(
                                ui.painter(),
                                Icon::Filter,
                                filter.rect.left_center() + egui::vec2(22. * scale, 0.),
                                scale,
                                gameplay::OVERLAY_TEXT,
                            );
                            if gameplay::icon_button(ui, Icon::Settings, "Game menu", true, scale)
                                .clicked()
                            {
                                self.menu_open = !self.menu_open;
                            }
                            let speaker = ui
                                .allocate_exact_size(egui::vec2(20., 30.), egui::Sense::hover())
                                .0;
                            gameplay::paint_icon(
                                ui.painter(),
                                if self.volume > 0. {
                                    Icon::Speaker
                                } else {
                                    Icon::Muted
                                },
                                speaker.center(),
                                scale,
                                gameplay::OVERLAY_TEXT,
                            );
                            gameplay::volume_slider(ui, &mut self.volume, scale);
                            gameplay::icon_button(ui, Icon::Fullscreen, "Full screen", true, scale);
                        });
                    });
                });
            });
        label(ui, "Game menu and filter flyout");
        ui.horizontal_top(|ui| {
            gameplay::game_menu_frame(ui).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(250.);
                    ui.spacing_mut().item_spacing.y = 0.;
                    gameplay::menu_row(ui, "Pause", "Ctrl+P", MenuMarker::Icon(Icon::Pause), false);
                    gameplay::menu_row(ui, "Save state", "F5", MenuMarker::Icon(Icon::Save), false);
                    if gameplay::menu_row(ui, "Video Filter", "▶", MenuMarker::None, self.menu_open)
                        .clicked()
                    {
                        self.menu_open = !self.menu_open;
                    }
                    gameplay::menu_row(ui, "Controls", "F1", MenuMarker::None, false);
                });
            });
            if self.menu_open {
                gameplay::game_menu_frame(ui).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(200.);
                        ui.spacing_mut().item_spacing.y = 0.;
                        gameplay::menu_row(ui, "Configure Shader…", "", MenuMarker::None, false);
                        for (i, filter) in ["Nearest Neighbor", "Linear", "CRT Geom"]
                            .into_iter()
                            .enumerate()
                        {
                            if gameplay::menu_row(
                                ui,
                                filter,
                                "",
                                if i == self.shader {
                                    MenuMarker::Checked
                                } else {
                                    MenuMarker::None
                                },
                                false,
                            )
                            .clicked()
                            {
                                self.shader = i;
                            }
                        }
                    });
                });
            }
        });
        self.dialog(ui);
    }

    fn feedback(&mut self, ui: &mut egui::Ui) {
        label(ui, "Progress bars");
        ui.add(egui::Slider::new(&mut self.progress, 0.0..=1.0).text("Sample progress"));
        let done = (self.progress * 100.).round() as u64;
        ui.horizontal_wrapped(|ui| {
            surface(ui).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(230.);
                    ui.strong("Library activity");
                    activity::progress(ui, 230., 9., done, 100, true);
                    ui.weak("Importing games");
                });
            });
            surface(ui).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(320.);
                    ui.strong("Disc backup");
                    activity::progress(ui, 320., 18., done, 100, true);
                    ui.weak(format!("{done}% complete"));
                });
            });
        });
        ui.label("Waiting for a total");
        activity::progress(ui, ui.available_width().min(320.), 9., 0, 0, true);
        label(ui, "Status messages and confirmation");
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.label("Checking library files…");
            ui.colored_label(ui.visuals().warn_fg_color, "Needs attention");
            ui.colored_label(ui.visuals().error_fg_color, "File missing");
        });
        if button(ui, "Open confirmation").clicked() {
            self.dialog = true;
        }
        label(ui, "System file status");
        surface(ui).show(ui, |ui| {
            egui::Grid::new("gallery-firmware-table")
                .striped(true)
                .spacing([18., 5.])
                .show(ui, |ui| {
                    ui.strong("File");
                    ui.strong("Status");
                    ui.end_row();
                    for (file, verified) in [("bios_CD_U.bin", true), ("bios_CD_E.bin", false)] {
                        ui.horizontal(|ui| {
                            let (check, _) =
                                ui.allocate_exact_size(egui::vec2(15., 15.), egui::Sense::hover());
                            super::maintenance::paint_firmware_check(
                                ui,
                                check,
                                verified,
                                ui.visuals().text_color(),
                            );
                            ui.label(file);
                        });
                        if verified {
                            ui.colored_label(super::maintenance::VERIFIED_TEXT, "Verified");
                        } else {
                            ui.label("Missing");
                        }
                        ui.end_row();
                    }
                });
        });
        self.dialog(ui);
    }

    fn dialog(&mut self, ui: &egui::Ui) {
        if self.dialog {
            let dialog = egui::Modal::new(egui::Id::new("gallery-confirmation"))
                .frame(egui::Frame::window(ui.style()).inner_margin(18))
                .show(ui.ctx(), |child| {
                    child.set_style(ui.style().clone());
                    child.set_width(340.);
                    child.heading("Remove from library?");
                    child.label("This is a preview. Your games and files are kept.");
                    child.add_space(8.);
                    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |child| {
                        if button(child, "Confirm").clicked() || button(child, "Cancel").clicked() {
                            self.dialog = false;
                        }
                    });
                });
            if dialog.should_close() {
                self.dialog = false;
            }
        }
    }

    fn artwork(&mut self, ui: &mut egui::Ui, env: &mut Environment<'_>, theme: &Theme) {
        label(ui, "Artwork and image surfaces");
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
            |ui| {
                image_sample(
                    ui,
                    env.artwork,
                    "About and onboarding logo",
                    &env.data.join("branding/emuluna-logo.png"),
                    [210., 185.],
                    false,
                );
                image_sample(
                    ui,
                    env.artwork,
                    "Controller artwork",
                    &env.data.join("controllers/snes.webp"),
                    [260., 185.],
                    false,
                );
                if let Some(path) = env
                    .library
                    .games
                    .iter()
                    .find_map(|game| game.cover.as_deref())
                {
                    image_sample(ui, env.artwork, "Library box art", path, [166., 185.], true);
                }
            },
        );
        label(ui, "Alphabet index and surfaces");
        ui.horizontal_top(|ui| {
            let bounds = ui.allocate_space(egui::vec2(36., 350.)).1;
            let mut strip = ui.new_child(egui::UiBuilder::new().max_rect(bounds));
            strip.set_clip_rect(ui.clip_rect().intersect(bounds));
            strip.spacing_mut().interact_size.y = 0.;
            let letters = BTreeMap::from([
                ('#', 0),
                ('A', 1),
                ('B', 2),
                ('E', 3),
                ('G', 4),
                ('M', 5),
                ('S', 6),
            ]);
            if let Some(i) = alphabet::show(&mut strip, &letters) {
                self.info = format!("Jump to {}", letters.keys().nth(i).unwrap_or(&'#'));
            }
            ui.vertical(|ui| {
                surface(ui)
                    .fill(theme.color("base", ui.visuals().window_fill))
                    .show(ui, |ui| ui.label("Library canvas"));
                surface(ui)
                    .fill(theme.color("sidebar-background", ui.visuals().window_fill))
                    .show(ui, |ui| ui.label("Library sidebar"));
                if !self.info.is_empty() {
                    ui.weak(&self.info);
                }
            });
        });
    }
}

fn image_sample(
    ui: &mut egui::Ui,
    artwork: &mut Artwork,
    title: &str,
    path: &Path,
    size: [f32; 2],
    box_art: bool,
) {
    surface(ui).show(ui, |ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(size[0], 0.),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.strong(title);
                let bounds = ui.allocate_space(egui::vec2(size[0], size[1])).1;
                let texture = if box_art {
                    artwork.box_art_texture(path)
                } else {
                    artwork.texture(path)
                };
                if let Some(id) = texture {
                    ui.painter().image(
                        id,
                        artwork.fitted_rect(path, bounds),
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                        Color32::WHITE,
                    );
                }
            },
        );
    });
}

fn palette(ui: &mut egui::Ui, theme: &Theme) {
    label(ui, "Theme colors");
    ui.weak("Hover a swatch for its role name and value.");
    let columns = ((ui.available_width() + 8.) / 132.).floor().max(1.) as usize;
    egui::Grid::new("gallery-swatches")
        .spacing([8., 10.])
        .show(ui, |ui| {
            for (index, (name, key, fallback)) in [
                ("Window", "window", ui.visuals().window_fill),
                ("Canvas", "base", ui.visuals().panel_fill),
                (
                    "Inset surface",
                    "alternate-base",
                    ui.visuals().faint_bg_color,
                ),
                ("Sidebar", "sidebar-background", ui.visuals().panel_fill),
                (
                    "Navigation",
                    "navigation-background",
                    ui.visuals().panel_fill,
                ),
                ("Window text", "window-text", ui.visuals().text_color()),
                ("Button", "button", ui.visuals().widgets.inactive.bg_fill),
                ("Button text", "button-text", ui.visuals().text_color()),
                ("Selection", "highlight", ui.visuals().selection.bg_fill),
                (
                    "Selected text",
                    "highlighted-text",
                    ui.visuals().selection.stroke.color,
                ),
                ("Accent", "accent", ui.visuals().selection.bg_fill),
                (
                    "Border",
                    "mid",
                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                ),
                ("Link", "link", ui.visuals().hyperlink_color),
                (
                    "Disabled text",
                    "disabled-text",
                    ui.visuals().weak_text_color(),
                ),
                ("Filled star", "rating-filled", Color32::YELLOW),
                ("Empty star", "rating-empty", ui.visuals().weak_text_color()),
            ]
            .into_iter()
            .enumerate()
            {
                ui.vertical(|ui| {
                    let color = theme.color(key, fallback);
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(124., 34.), egui::Sense::hover());
                    let hex = format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
                    ui.painter().rect(
                        rect,
                        6.,
                        color,
                        ui.visuals().widgets.noninteractive.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                    response.on_hover_text(format!("{key} · {hex}"));
                    ui.strong(name);
                    ui.monospace(hex);
                });
                if (index + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
}
