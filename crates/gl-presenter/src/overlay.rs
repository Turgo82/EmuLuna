//! Native controls draw in the display context. No core calls or audio locks.
use super::*;
use emuluna_playback_control::Controller;
use sdl2::{event::Event, mouse::MouseButton};

use emuluna_theme::gameplay::{
    bar_frame, bar_style, game_menu_frame, icon_button, menu_row, paint_icon, volume_slider, Icon,
    MenuMarker, MENU_PADDING, MENU_ROW_HEIGHT, OVERLAY_TEXT,
};

const GAME_MENU_SEPARATOR_HEIGHT: f32 = 9.;

#[derive(Clone, Copy)]
enum Action {
    Pause,
    Reset,
    Save,
    Load,
    LoadConfirmed,
    Volume(f32),
    Slot(u8),
    Fullscreen,
    Close,
    Screenshot,
    Controls,
    IntegerScale(bool),
    Autosave(bool),
    PreserveCropAspect(bool),
}
pub(super) struct Overlay {
    ctx: egui::Context,
    painter: egui_glow::Painter,
    events: Vec<egui::Event>,
    actions: Vec<Action>,
    paused: bool,
    states: bool,
    focused: bool,
    volume: f32,
    remembered_volume: f32,
    slot: u8,
    filter_root: Option<std::path::PathBuf>,
    filter_choices: Vec<(String, String)>,
    filter_selected: String,
    filter_request: Option<String>,
    shader_controls_open: bool,
    shader_fields: Vec<(emuluna_shader_pack::Parameter, f32)>,
    shader_fields_changed: bool,
    shader_resolution: u32,
    shader_resolution_request: Option<u32>,
    console_options: Vec<ConsoleOption>,
    option_requests: Vec<(String, String, String, bool)>,
    n64_auto: Option<bool>,
    n64_preserve_aspect: bool,
    n64_auto_request: Option<bool>,
    integer_scaling: bool,
    game_menu_open: bool,
    game_submenu: Option<u8>,
    game_menu_anchor: egui::Rect,
    submenu_anchor: egui::Rect,
    screenshot_directory: Option<std::path::PathBuf>,
    message: Option<(String, Instant)>,
    started: Instant,
    activity: Instant,
    confirm_reset: bool,
    confirm_load: bool,
    autosave: Option<bool>,
    confirm_disable_autosave: bool,
    last_paint: Instant,
    dirty: bool,
    repaint_at: Option<Instant>,
    #[cfg(test)]
    pause_rect: Option<egui::Rect>,
    #[cfg(test)]
    bar_rect: Option<egui::Rect>,
    #[cfg(test)]
    mute_rect: Option<egui::Rect>,
    #[cfg(test)]
    notice_visible: bool,
}
struct ConsoleOption {
    label: String,
    key: String,
    preference: String,
    value: String,
    choices: Vec<(String, String)>,
    restart: bool,
    description: String,
}
fn is_accessory(option: &ConsoleOption) -> bool {
    option.key.contains("pak") || option.key.contains("memcard")
}
fn is_resolution(option: &ConsoleOption) -> bool {
    option.key.contains("upscaling")
        || option.key.contains("screensize")
        || option.key == "mupen64plus-EnableNativeResFactor"
}
fn is_other_option(option: &ConsoleOption) -> bool {
    !is_accessory(option) && !is_resolution(option)
}
fn show_console_options(
    ui: &mut egui::Ui,
    options: &mut [ConsoleOption],
    requests: &mut Vec<(String, String, String, bool)>,
    include: fn(&ConsoleOption) -> bool,
) {
    for option in options.iter_mut().filter(|option| include(option)) {
        if option.choices.len() == 2 && option.choices[0].1 == "On" && option.choices[1].1 == "Off"
        {
            let mut enabled = option.value == option.choices[0].0;
            if ui
                .checkbox(&mut enabled, &option.label)
                .on_hover_text(&option.description)
                .changed()
            {
                option.value = option.choices[usize::from(!enabled)].0.clone();
                requests.push((
                    option.key.clone(),
                    option.value.clone(),
                    option.preference.clone(),
                    option.restart,
                ));
            }
        } else {
            let label = option
                .choices
                .iter()
                .find(|(value, _)| value == &option.value)
                .map_or(option.value.as_str(), |(_, label)| label.as_str())
                .to_owned();
            ui.horizontal(|ui| {
                ui.label(&option.label);
                egui::ComboBox::from_id_salt(&option.key)
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (value, label) in &option.choices {
                            if ui
                                .selectable_value(&mut option.value, value.clone(), label)
                                .changed()
                            {
                                requests.push((
                                    option.key.clone(),
                                    value.clone(),
                                    option.preference.clone(),
                                    option.restart,
                                ));
                            }
                        }
                    });
            });
        }
        if option.restart {
            ui.weak("Applies when the game is restarted.");
        }
    }
}
impl Overlay {
    fn new(gl: Arc<glow::Context>, theme: emuluna_theme::Theme) -> Result<Self, String> {
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        emuluna_theme::fonts::install(&ctx);
        let painter = egui_glow::Painter::new(gl, "", None, false).map_err(|e| e.to_string())?;
        Ok(Self {
            ctx,
            painter,
            events: Vec::new(),
            actions: Vec::new(),
            paused: false,
            states: false,
            focused: true,
            volume: 1.,
            remembered_volume: 1.,
            slot: 1,
            filter_root: None,
            filter_choices: vec![
                ("nearest".into(), "Off · sharp pixels".into()),
                ("linear".into(), "Smooth pixels".into()),
                ("scanlines".into(), "Scanlines".into()),
            ],
            filter_selected: "nearest".into(),
            filter_request: None,
            shader_controls_open: false,
            shader_fields: Vec::new(),
            shader_fields_changed: false,
            shader_resolution: 0,
            shader_resolution_request: None,
            console_options: Vec::new(),
            option_requests: Vec::new(),
            n64_auto: None,
            n64_preserve_aspect: false,
            n64_auto_request: None,
            integer_scaling: false,
            game_menu_open: false,
            game_submenu: None,
            game_menu_anchor: egui::Rect::NOTHING,
            submenu_anchor: egui::Rect::NOTHING,
            screenshot_directory: None,
            message: None,
            started: Instant::now(),
            activity: Instant::now(),
            confirm_reset: false,
            confirm_load: false,
            autosave: None,
            confirm_disable_autosave: false,
            last_paint: Instant::now(),
            dirty: true,
            repaint_at: None,
            #[cfg(test)]
            pause_rect: None,
            #[cfg(test)]
            bar_rect: None,
            #[cfg(test)]
            mute_rect: None,
            #[cfg(test)]
            notice_visible: false,
        })
    }
    pub fn destroy(&mut self) {
        self.painter.destroy();
    }
    fn game_menu(&mut self, ctx: &egui::Context) {
        let mut action = None;
        let screen = ctx.screen_rect();
        let frame_width = 2. * (f32::from(MENU_PADDING) + 1.);
        let main_width = 250_f32.min(screen.width() * 0.54 - frame_width);
        let flyout_width = 224_f32.min(screen.width() - main_width - 2. * frame_width - 8.);
        let menu_x = self
            .game_menu_anchor
            .left()
            .min(screen.right() - main_width - flyout_width - 2. * frame_width - 4.)
            .max(screen.left());
        let parent = egui::Area::new("game-menu-main".into())
            .order(egui::Order::Foreground)
            .fade_in(false)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(egui::pos2(menu_x, self.game_menu_anchor.top() - 6.))
            .show(ctx, |ui| {
                game_menu_frame(ui).show(ui, |ui| {
                    ui.set_width(main_width);
                    ui.spacing_mut().item_spacing.y = 0.;
                    let pause_label = if self.paused { "Resume" } else { "Pause" };
                    let pause_icon = if self.paused { Icon::Play } else { Icon::Pause };
                    for (label, shortcut, icon, item, section, enabled) in [
                        (
                            pause_label,
                            "Ctrl+P",
                            Some(pause_icon),
                            Some(Action::Pause),
                            None,
                            true,
                        ),
                        ("Reset", "Ctrl+R", Some(Icon::Reset), None, None, true),
                        (
                            "Save state",
                            "F5",
                            Some(Icon::Save),
                            Some(Action::Save),
                            None,
                            self.states,
                        ),
                        (
                            "Load state",
                            "F8",
                            Some(Icon::Folder),
                            Some(Action::Load),
                            None,
                            self.states,
                        ),
                        ("Save slot", "▶", None, None, Some(1), self.states),
                        (
                            "Screenshot",
                            "F12",
                            None,
                            Some(Action::Screenshot),
                            None,
                            self.screenshot_directory.is_some(),
                        ),
                        (
                            "Controller Accessories",
                            "▶",
                            None,
                            None,
                            Some(2),
                            self.console_options.iter().any(is_accessory),
                        ),
                        (
                            "Video Filter",
                            "▶",
                            None,
                            None,
                            Some(3),
                            !self.filter_choices.is_empty(),
                        ),
                        ("Scaling", "▶", None, None, Some(4), true),
                        (
                            "Console options",
                            "▶",
                            None,
                            None,
                            Some(5),
                            self.console_options.iter().any(is_other_option),
                        ),
                        (
                            "Controls",
                            "F1",
                            None,
                            Some(Action::Controls),
                            None,
                            std::env::var_os("EMULUNA_CONTROLS_REQUEST_FILE").is_some(),
                        ),
                    ] {
                        if section == Some(5) && !enabled {
                            continue;
                        }
                        if matches!(label, "Save state" | "Screenshot") {
                            ui.add(egui::Separator::default().spacing(GAME_MENU_SEPARATOR_HEIGHT));
                        }
                        let active = section.is_some() && section == self.game_submenu;
                        let response = ui
                            .add_enabled_ui(enabled, |ui| {
                                menu_row(
                                    ui,
                                    label,
                                    shortcut,
                                    icon.map_or(MenuMarker::None, MenuMarker::Icon),
                                    active,
                                )
                            })
                            .inner;
                        if response.clicked() {
                            if let Some(item) = item {
                                action = Some(item);
                                self.game_menu_open = false;
                                self.game_submenu = None;
                            } else if label == "Reset" {
                                self.confirm_reset = true;
                                self.game_menu_open = false;
                                self.game_submenu = None;
                            } else if section.is_some() {
                                self.game_submenu = if active { None } else { section };
                            }
                        }
                        if section.is_some() && self.game_submenu == section {
                            self.submenu_anchor = response.rect;
                        }
                    }
                    if let Some(enabled) = self.autosave {
                        ui.add(egui::Separator::default().spacing(GAME_MENU_SEPARATOR_HEIGHT));
                        let response = menu_row(ui, "Autosave", "", MenuMarker::None, false)
                            .on_hover_text(if enabled {
                                "Autosave is on"
                            } else {
                                "Autosave is off"
                            });
                        let switch = egui::Rect::from_center_size(
                            response.rect.right_center() - egui::vec2(26., 0.),
                            egui::vec2(32., 16.),
                        );
                        let track = if enabled {
                            egui::Color32::from_rgb(31, 129, 77)
                        } else {
                            egui::Color32::from_gray(105)
                        };
                        ui.painter().rect_filled(switch, 8., track);
                        ui.painter().circle_filled(
                            switch.center() + egui::vec2(if enabled { 8. } else { -8. }, 0.),
                            5.,
                            egui::Color32::WHITE,
                        );
                        if response.clicked() {
                            self.game_menu_open = false;
                            self.game_submenu = None;
                            if enabled {
                                self.confirm_disable_autosave = true;
                            } else {
                                action = Some(Action::Autosave(true));
                            }
                        }
                    }
                })
            })
            .response
            .rect;
        let mut submenu_rect = egui::Rect::NOTHING;
        if let Some(section) = self.game_submenu {
            let available_height = (parent.bottom() - screen.top() - 8. - frame_width).max(32.);
            let desired_height = match section {
                1 => 10. * MENU_ROW_HEIGHT,
                3 => (self.filter_choices.len() + 1) as f32 * MENU_ROW_HEIGHT + 2.,
                4 => 255.,
                _ => 190.,
            };
            let content_height = if desired_height > available_height && matches!(section, 1 | 3) {
                (available_height / MENU_ROW_HEIGHT).floor() * MENU_ROW_HEIGHT
            } else {
                desired_height.min(available_height)
            };
            let pos = egui::pos2(
                parent.right(),
                (self.submenu_anchor.top() - f32::from(MENU_PADDING))
                    .min(parent.bottom() - content_height - frame_width)
                    .max(screen.top() + 8.),
            );
            submenu_rect = egui::Area::new(egui::Id::new(("game-menu-submenu", section)))
                .order(egui::Order::Foreground)
                .fade_in(false)
                .default_height(content_height + frame_width)
                .constrain_to(screen)
                .fixed_pos(pos)
                .show(ctx, |ui| {
                    game_menu_frame(ui).show(ui, |ui| {
                        ui.set_width(flyout_width);
                        ui.spacing_mut().item_spacing.y = 0.;
                        ui.spacing_mut().interact_size.y = MENU_ROW_HEIGHT;
                        ui.spacing_mut().scroll.floating = false;
                        ui.spacing_mut().scroll.bar_width = 5.;
                        match section {
                            1 => {
                                egui::ScrollArea::vertical()
                                    .max_height(content_height)
                                    .show(ui, |ui| {
                                        for slot in 1..=10 {
                                            let marker = if self.slot == slot {
                                                MenuMarker::Checked
                                            } else {
                                                MenuMarker::None
                                            };
                                            if menu_row(
                                                ui,
                                                &format!("Slot {slot}"),
                                                "",
                                                marker,
                                                false,
                                            )
                                            .clicked()
                                            {
                                                action = Some(Action::Slot(slot));
                                                self.game_menu_open = false;
                                                self.game_submenu = None;
                                            }
                                        }
                                    });
                            }
                            2 => show_console_options(
                                ui,
                                &mut self.console_options,
                                &mut self.option_requests,
                                is_accessory,
                            ),
                            3 => {
                                if menu_row(ui, "Configure Shader…", "", MenuMarker::None, false)
                                    .clicked()
                                {
                                    self.shader_controls_open = true;
                                    self.game_menu_open = false;
                                    self.game_submenu = None;
                                }
                                ui.add(egui::Separator::default().spacing(1.));
                                let list_height =
                                    (content_height - MENU_ROW_HEIGHT - 1.).max(MENU_ROW_HEIGHT);
                                egui::ScrollArea::vertical()
                                    .min_scrolled_height(list_height)
                                    .max_height(list_height)
                                    .show(ui, |ui| {
                                        let mut community = false;
                                        for (id, name) in &self.filter_choices {
                                            if !community && id.starts_with("shader:") {
                                                ui.separator();
                                                community = true;
                                            }
                                            let marker = if id == &self.filter_selected {
                                                MenuMarker::Checked
                                            } else {
                                                MenuMarker::None
                                            };
                                            if menu_row(ui, name, "", marker, false).clicked() {
                                                self.filter_request = Some(id.clone());
                                                self.game_menu_open = false;
                                                self.game_submenu = None;
                                            }
                                        }
                                    });
                            }
                            4 => {
                                egui::ScrollArea::vertical().max_height(content_height).show(ui, |ui| {
                                    let marker = |enabled| if enabled { MenuMarker::Checked } else { MenuMarker::None };
                                    if menu_row(ui, "Integer scaling", "", marker(self.integer_scaling), false).clicked() {
                                        action = Some(Action::IntegerScale(!self.integer_scaling));
                                    }
                                    if let Some(enabled) = self.n64_auto {
                                        if menu_row(ui, "Adjust screen borders", "", marker(enabled), false).clicked() {
                                            self.n64_auto_request = Some(!enabled);
                                        }
                                        if ui.add_enabled_ui(enabled, |ui| {
                                            menu_row(ui, "Keep aspect ratio", "", marker(self.n64_preserve_aspect), false)
                                                .on_hover_text("Preserve the picture’s proportions when cropping borders.")
                                        }).inner.clicked() {
                                            action = Some(Action::PreserveCropAspect(!self.n64_preserve_aspect));
                                        }
                                    }
                                    for option in self.console_options.iter_mut().filter(|option| is_resolution(option)) {
                                        ui.add(egui::Separator::default().spacing(GAME_MENU_SEPARATOR_HEIGHT));
                                        ui.weak(&option.label);
                                        for (value, label) in &option.choices {
                                            if menu_row(ui, label, "", marker(option.value == *value), false)
                                                .on_hover_text("Applies when the game is restarted.")
                                                .clicked() && option.value != *value
                                            {
                                                option.value = value.clone();
                                                self.option_requests.push((option.key.clone(), value.clone(), option.preference.clone(), option.restart));
                                            }
                                        }
                                    }
                                });
                            }
                            5 => show_console_options(
                                ui,
                                &mut self.console_options,
                                &mut self.option_requests,
                                is_other_option,
                            ),
                            _ => {}
                        }
                    })
                })
                .response
                .rect;
        }
        if let Some(action) = action {
            self.actions.push(action);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.game_menu_open = false;
            self.game_submenu = None;
        }
        if ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|pos| {
                    !parent.contains(pos)
                        && !submenu_rect.contains(pos)
                        && !self.game_menu_anchor.contains(pos)
                })
        }) && !ctx.memory(|m| m.any_popup_open())
        {
            self.game_menu_open = false;
            self.game_submenu = None;
        }
    }
    fn shader_controls(&mut self, ctx: &egui::Context) {
        let frame = egui::Frame::window(&ctx.style()).inner_margin(16);
        let response = egui::Modal::new(egui::Id::new("configure-shader"))
            .backdrop_color(egui::Color32::TRANSPARENT)
            .frame(frame)
            .show(ctx, |ui| {
                let width = (ctx.screen_rect().width() - 52.).clamp(220., 500.);
                ui.set_width(width);
                ui.spacing_mut().interact_size.y = 30.;
                ui.heading("Configure Shader");
                let selected = self
                    .filter_choices
                    .iter()
                    .find(|(key, _)| key == &self.filter_selected)
                    .map_or(self.filter_selected.as_str(), |(_, name)| name.as_str());
                egui::Grid::new("shader-selectors").show(ui, |ui| {
                    ui.label("Video filter");
                    egui::ComboBox::from_id_salt("shader-filter")
                        .width((width - 140.).max(120.))
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for (id, name) in &self.filter_choices {
                                if ui
                                    .selectable_label(id == &self.filter_selected, name)
                                    .clicked()
                                {
                                    self.filter_request = Some(id.clone());
                                }
                            }
                        });
                    ui.end_row();
                    ui.label("Filter resolution");
                    let resolutions = [
                        (0, "Match window"),
                        (1080, "Up to 1080p"),
                        (720, "Up to 720p"),
                    ];
                    let selected = resolutions
                        .iter()
                        .find(|(height, _)| *height == self.shader_resolution)
                        .map_or("Match window", |(_, label)| *label);
                    egui::ComboBox::from_id_salt("shader-resolution")
                        .width((width - 140.).max(120.))
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for (height, label) in resolutions {
                                if ui
                                    .selectable_label(height == self.shader_resolution, label)
                                    .clicked()
                                {
                                    self.shader_resolution_request = Some(height);
                                }
                            }
                        });
                    ui.end_row();
                });
                ui.separator();
                if self.shader_fields.is_empty() {
                    ui.label("This filter has no adjustable parameters.");
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt(&self.filter_selected)
                        .max_height((ctx.screen_rect().height() - 225.).clamp(55., 450.))
                        .show(ui, |ui| {
                            ui.spacing_mut().slider_width = (width - 100.).max(110.);
                            for (parameter, value) in &mut self.shader_fields {
                                ui.label(&parameter.label);
                                ui.horizontal(|ui| {
                                    let mut slider = egui::Slider::new(
                                        value,
                                        parameter.minimum..=parameter.maximum,
                                    )
                                    .show_value(false);
                                    if parameter.step.is_finite() && parameter.step > 0. {
                                        slider = slider.step_by(f64::from(parameter.step));
                                    }
                                    self.shader_fields_changed |= ui.add(slider).changed();
                                    let precision =
                                        (-parameter.step.log10()).ceil().clamp(0., 5.) as usize;
                                    ui.label(format!("{value:.precision$}"));
                                });
                            }
                        });
                }
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_sized([80., 32.], egui::Button::new("Close"))
                        .clicked()
                    {
                        self.shader_controls_open = false;
                    }
                    if ui
                        .add_enabled(
                            !self.shader_fields.is_empty(),
                            egui::Button::new("Restore defaults"),
                        )
                        .clicked()
                    {
                        for (parameter, value) in &mut self.shader_fields {
                            *value = parameter.default;
                        }
                        self.shader_fields_changed = true;
                    }
                });
            });
        if response.should_close() {
            self.shader_controls_open = false;
        }
    }
    fn paint(&mut self, window: &Window, hide_cursor: bool, resize_grip: bool) {
        let (width, height) = window.size();
        let drawable = window.drawable_size();
        if width == 0 || height == 0 || drawable.0 == 0 || drawable.1 == 0 {
            return;
        }
        let scale = drawable.0 as f32 / width as f32;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32),
            )),
            time: Some(self.started.elapsed().as_secs_f64()),
            focused: self.focused,
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        if let Some(viewport) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.native_pixels_per_point = Some(scale);
        }
        #[cfg(test)]
        {
            self.pause_rect = None;
            self.bar_rect = None;
            self.notice_visible = false;
        }
        let context = self.ctx.clone();
        let output = context.run(input, |ctx| {
            let keep_open = self.game_menu_open
                || self.paused
                || self.confirm_reset
                || self.confirm_load
                || self.confirm_disable_autosave
                || self.shader_controls_open
                || ctx.memory(|m| m.any_popup_open())
                || ctx.input(|i| i.pointer.any_down());
            if keep_open {
                self.activity = Instant::now();
            }
            if keep_open || self.activity.elapsed() < Duration::from_millis(2500) {
                egui::Area::new("playback-controls".into())
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0., -18.))
                    .show(ctx, |ui| {
                        let scale = ((width as f32 - 48.) / 1000.).clamp(0.65, 1.);
                        bar_style(ui, scale);
                        let bar = bar_frame(scale).show(ui, |ui| {
                            ui.set_max_width((width as f32 - 48.).max(100.));
                            ui.horizontal_wrapped(|ui| {
                                if icon_button(ui, Icon::Stop, "Stop game", true, scale).clicked() {
                                    self.actions.push(Action::Close);
                                }
                                let pause = icon_button(
                                    ui,
                                    if self.paused { Icon::Play } else { Icon::Pause },
                                    if self.paused {
                                        "Resume (Ctrl+P / F6)"
                                    } else {
                                        "Pause (Ctrl+P / F6)"
                                    },
                                    true,
                                    scale,
                                );
                                #[cfg(test)]
                                {
                                    self.pause_rect = Some(pause.rect);
                                }
                                if pause.clicked() {
                                    self.actions.push(Action::Pause);
                                }
                                if icon_button(
                                    ui,
                                    Icon::Reset,
                                    "Restart game (Ctrl+R / F7)",
                                    true,
                                    scale,
                                )
                                .clicked()
                                {
                                    self.confirm_reset = true;
                                }
                                ui.add_space(12. * scale);
                                if icon_button(
                                    ui,
                                    Icon::Save,
                                    "Save quick state (F5)",
                                    self.states,
                                    scale,
                                )
                                .clicked()
                                {
                                    self.actions.push(Action::Save);
                                }
                                if icon_button(
                                    ui,
                                    Icon::Load,
                                    "Load quick state (F8)",
                                    self.states,
                                    scale,
                                )
                                .clicked()
                                {
                                    self.actions.push(Action::Load);
                                }
                                ui.add_enabled_ui(self.states, |ui| {
                                    egui::ComboBox::from_id_salt("overlay-slot")
                                        .width(94. * scale)
                                        .height(6. * MENU_ROW_HEIGHT * scale)
                                        .icon(move |ui, rect, visuals, _, _| {
                                            let c = rect.center();
                                            ui.painter().add(egui::Shape::line(
                                                vec![
                                                    c + egui::vec2(-4., -2.) * scale,
                                                    c + egui::vec2(0., 2.) * scale,
                                                    c + egui::vec2(4., -2.) * scale,
                                                ],
                                                egui::Stroke::new(
                                                    1.8 * scale,
                                                    visuals.fg_stroke.color,
                                                ),
                                            ));
                                        })
                                        .selected_text(format!("Slot {}", self.slot))
                                        .show_ui(ui, |ui| {
                                            for slot in 1..=10 {
                                                let selected = slot == self.slot;
                                                let mut label = egui::RichText::new(format!("Slot {slot}"));
                                                if selected {
                                                    label = label.color(ui.visuals().selection.stroke.color);
                                                }
                                                if ui
                                                    .selectable_label(selected, label)
                                                    .clicked()
                                                {
                                                    self.actions.push(Action::Slot(slot));
                                                    ui.close_menu();
                                                }
                                            }
                                        })
                                        .response
                                        .on_hover_text("Quick-save slot");
                                });
                                ui.add_space(12. * scale);
                                if self.screenshot_directory.is_some()
                                    && icon_button(
                                        ui,
                                        Icon::Screenshot,
                                        "Take screenshot (F12)",
                                        true,
                                        scale,
                                    )
                                    .clicked()
                                {
                                    self.actions.push(Action::Screenshot);
                                }
                                {
                                    let response =
                                        icon_button(ui, Icon::Settings, "Game menu", true, scale);
                                    self.game_menu_anchor = response.rect;
                                    if response.clicked() {
                                        self.game_menu_open = !self.game_menu_open;
                                        self.game_submenu = None;
                                    }
                                }
                                let separator = ui
                                    .allocate_exact_size(
                                        egui::vec2(10. * scale, 42. * scale),
                                        egui::Sense::hover(),
                                    )
                                    .0;
                                ui.painter().line_segment(
                                    [separator.center_top(), separator.center_bottom()],
                                    egui::Stroke::new(
                                        1_f32,
                                        egui::Color32::from_rgba_unmultiplied(220, 229, 237, 110),
                                    ),
                                );
                                let mute =
                                    ui.add(
                                        egui::Button::new("")
                                            .frame(false)
                                            .min_size(egui::vec2(32. * scale, 42. * scale)),
                                    )
                                    .on_hover_text(
                                        if self.volume > 0. { "Mute" } else { "Unmute" },
                                    );
                                #[cfg(test)]
                                {
                                    self.mute_rect = Some(mute.rect);
                                }
                                paint_icon(
                                    ui.painter(),
                                    if self.volume > 0. {
                                        Icon::Speaker
                                    } else {
                                        Icon::Muted
                                    },
                                    mute.rect.center(),
                                    scale,
                                    OVERLAY_TEXT,
                                );
                                if mute.clicked() {
                                    if self.volume > 0. {
                                        self.remembered_volume = self.volume;
                                        self.volume = 0.;
                                    } else {
                                        self.volume = self.remembered_volume;
                                    }
                                    self.actions.push(Action::Volume(self.volume));
                                }
                                if volume_slider(ui, &mut self.volume, scale).changed() {
                                    self.actions.push(Action::Volume(self.volume));
                                }
                                if icon_button(
                                    ui,
                                    Icon::Fullscreen,
                                    "Fullscreen (F11)",
                                    true,
                                    scale,
                                )
                                .clicked()
                                {
                                    self.actions.push(Action::Fullscreen);
                                }
                            });
                        });
                        #[cfg(test)]
                        {
                            self.bar_rect = Some(bar.response.rect);
                        }
                        #[cfg(not(test))]
                        let _ = bar;
                    });
            }
            if self.game_menu_open {
                self.game_menu(ctx);
            }
            // The resize grip remains reachable after the hover bar hides.
            if resize_grip {
                egui::Area::new("resize-grip".into())
                    .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-2., -2.))
                    .interactable(false)
                    .show(ctx, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(18., 18.), egui::Sense::hover());
                        for offset in [4., 9., 14.] {
                            ui.painter().line_segment(
                                [
                                    rect.right_bottom() - egui::vec2(offset, 0.),
                                    rect.right_bottom() - egui::vec2(0., offset),
                                ],
                                egui::Stroke::new(1_f32, ui.visuals().weak_text_color()),
                            );
                        }
                    });
            }
            if self.shader_controls_open {
                self.shader_controls(ctx);
            }
            if self.confirm_reset {
                let response =
                    egui::Modal::new(egui::Id::new("confirm-game-reset")).show(ctx, |ui| {
                        ui.set_max_width(340.);
                        ui.heading("Restart this game?");
                        ui.label(
                            "Progress since your last in-game save or saved state will be lost.",
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                self.confirm_reset = false;
                            }
                            if ui.button("Restart").clicked() {
                                self.confirm_reset = false;
                                self.actions.push(Action::Reset);
                            }
                        });
                    });
                if response.should_close() {
                    self.confirm_reset = false;
                }
            }
            if self.confirm_load {
                let response = egui::Modal::new(egui::Id::new("confirm-load-state")).show(ctx, |ui| {
                    ui.set_max_width(340.);
                    ui.heading(format!("Load Slot {}?", self.slot));
                    ui.label("Autosave is on. When you close the game, the next autosave will replace your current resume point.");
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.confirm_load = false;
                        }
                        if ui.button("Load state").clicked() {
                            self.confirm_load = false;
                            self.actions.push(Action::LoadConfirmed);
                        }
                    });
                });
                if response.should_close() {
                    self.confirm_load = false;
                }
            }
            if self.confirm_disable_autosave {
                let response = egui::Modal::new(egui::Id::new("confirm-disable-autosave"))
                    .show(ctx, |ui| {
                        ui.set_max_width(340.);
                        ui.heading("Turn off autosave?");
                        ui.label("EmuLuna won’t save a new automatic state when you close this game. You can still save manually.");
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                self.confirm_disable_autosave = false;
                            }
                            if ui.button("Turn off").clicked() {
                                self.confirm_disable_autosave = false;
                                self.actions.push(Action::Autosave(false));
                            }
                        });
                    });
                if response.should_close() {
                    self.confirm_disable_autosave = false;
                }
            }
            if let Some((message, time)) = &self.message {
                if time.elapsed() < Duration::from_millis(2400) {
                    #[cfg(test)]
                    {
                        self.notice_visible = true;
                    }
                    egui::Area::new("playback-message".into())
                        .interactable(false)
                        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0., 20.))
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .corner_radius(10.)
                                .inner_margin(12.)
                                .show(ui, |ui| {
                                    ui.set_max_width((width as f32 - 48.).min(460.));
                                    ui.label(egui::RichText::new(message).strong().size(16.));
                                });
                        });
                }
            }
        });
        self.repaint_at = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .and_then(|v| Instant::now().checked_add(v.repaint_delay));
        if let Some((_, time)) = &self.message {
            if time.elapsed() < Duration::from_millis(2400) {
                let expiry = *time + Duration::from_millis(2400);
                self.repaint_at = Some(self.repaint_at.map_or(expiry, |next| next.min(expiry)));
            }
        }
        if !self.paused && self.activity.elapsed() < Duration::from_millis(2500) {
            let expiry = self.activity + Duration::from_millis(2500);
            self.repaint_at = Some(self.repaint_at.map_or(expiry, |next| next.min(expiry)));
        }
        if self.focused {
            window.subsystem().sdl().mouse().show_cursor(
                !hide_cursor
                    || self.ctx.input(|i| i.pointer.any_down())
                    || self.paused
                    || self.activity.elapsed() < Duration::from_millis(2500),
            );
        }
        let meshes = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        // The game presenter binds its own sampling object; egui uses texture
        // parameters instead. Do not let the game sampler override its atlas.
        // SAFETY: display context is current throughout Presenter drawing.
        unsafe {
            self.painter.gl().bind_sampler(0, None);
        }
        self.painter.paint_and_update_textures(
            [drawable.0, drawable.1],
            output.pixels_per_point,
            &meshes,
            &output.textures_delta,
        );
        self.last_paint = Instant::now();
        self.dirty = false;
    }
}
impl Presenter {
    pub fn configure_n64_crop_aspect(&mut self, enabled: bool) {
        self.n64_preserve_aspect = enabled;
        self.cached = None;
        if let Some(overlay) = &mut self.overlay {
            overlay.n64_preserve_aspect = enabled;
            overlay.dirty = true;
        }
    }
    pub fn configure_n64_auto_crop(&mut self, enabled: bool) -> Result<(), String> {
        self.ensure_current()?;
        self.cached = None;
        if enabled && self.n64_crop.is_none() {
            self.n64_crop = Some(super::n64_crop::AutoCrop::new(self.gl.clone())?);
        }
        if !enabled {
            if let Some(mut crop) = self.n64_crop.take() {
                unsafe {
                    crop.destroy();
                }
            }
        }
        if let Some(overlay) = &mut self.overlay {
            overlay.n64_auto = Some(enabled);
            overlay.dirty = true;
        }
        Ok(())
    }
    pub fn configure_console_options(
        &mut self,
        system: &str,
        catalog_id: &str,
        core_id: &str,
        game_id: &str,
        initial: &[(String, String)],
    ) {
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        let Ok(catalog) =
            serde_json::from_str::<serde_json::Value>(include_str!("../../../data/cores.json"))
        else {
            return;
        };
        let definition = &catalog[catalog_id];
        let value = |key: &str, default: &str| {
            initial
                .iter()
                .find(|(k, _)| k == key)
                .map_or_else(|| default.to_owned(), |(_, v)| v.clone())
        };
        if let Some(items) = definition["runtime_options"][system].as_array() {
            for item in items {
                let Some(key) = item["key"].as_str() else {
                    continue;
                };
                let choices: Vec<(String, String)> = if let (Some(enabled), Some(disabled)) =
                    (item["enabled"].as_str(), item["disabled"].as_str())
                {
                    vec![
                        (enabled.into(), "On".into()),
                        (disabled.into(), "Off".into()),
                    ]
                } else {
                    item["choices"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|choice| {
                            Some((choice[0].as_str()?.into(), choice[1].as_str()?.into()))
                        })
                        .collect()
                };
                if choices.is_empty() {
                    continue;
                }
                let default = item["default"].as_str().unwrap_or(&choices[0].0);
                overlay.console_options.push(ConsoleOption {
                    label: item["label"].as_str().unwrap_or("Core option").into(),
                    key: key.into(),
                    preference: format!("core_option.{system}.{catalog_id}.{key}"),
                    value: value(key, default),
                    choices,
                    restart: item["restart"].as_bool().unwrap_or(false),
                    description: item["description"].as_str().unwrap_or("").into(),
                });
            }
        }
        if let Some(items) = definition["accessories"].as_array() {
            for item in items {
                let Some(template) = item["key"].as_str() else {
                    continue;
                };
                let choices: Vec<_> = item["choices"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| Some((v[0].as_str()?.to_owned(), v[1].as_str()?.to_owned())))
                    .collect();
                if choices.is_empty() {
                    continue;
                }
                let count = item["count"].as_u64().unwrap_or(1).clamp(1, 4);
                for player in 1..=count {
                    let key = template.replace("{player}", &player.to_string());
                    let default = item["defaults"]
                        .get((player - 1) as usize)
                        .and_then(|v| v.as_str())
                        .or_else(|| item["default"].as_str())
                        .unwrap_or(&choices[0].0);
                    let label = item["label"].as_str().unwrap_or("Accessory");
                    overlay.console_options.push(ConsoleOption {
                        label: if count > 1 {
                            format!("Player {player} {label}")
                        } else {
                            label.into()
                        },
                        preference: format!("core_option.{game_id}.{core_id}.{key}"),
                        value: value(&key, default),
                        key,
                        choices: choices.clone(),
                        restart: item["restart"].as_bool().unwrap_or(false),
                        description: String::new(),
                    });
                }
            }
        }
    }
    /// Opt-in while the native player is being integrated with the library UI.
    pub fn enable_overlay(&mut self, theme: &str) -> Result<(), String> {
        self.ensure_current()?;
        let overlay = Overlay::new(self.gl.clone(), emuluna_theme::Theme::builtin(theme)?)?;
        if let Some(mut old) = self.overlay.replace(overlay) {
            old.destroy();
        }
        Ok(())
    }
    pub fn enable_overlay_from(&mut self, root: &std::path::Path, id: &str) -> Result<(), String> {
        self.ensure_current()?;
        let overlay = Overlay::new(self.gl.clone(), emuluna_theme::Theme::load(root, id)?)?;
        if let Some(mut old) = self.overlay.replace(overlay) {
            old.destroy();
        }
        Ok(())
    }
    pub fn overlay_event(&mut self, event: &Event) {
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        if matches!(
            event,
            Event::MouseMotion { .. } | Event::MouseButtonDown { .. } | Event::MouseButtonUp { .. }
        ) {
            overlay.activity = Instant::now();
        }
        match *event {
            Event::MouseMotion {
                window_id, x, y, ..
            } if window_id == self.window.id() => {
                overlay
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(x as f32, y as f32)));
                overlay.dirty = true;
            }
            Event::MouseButtonDown {
                window_id,
                mouse_btn: MouseButton::Left,
                x,
                y,
                ..
            }
            | Event::MouseButtonUp {
                window_id,
                mouse_btn: MouseButton::Left,
                x,
                y,
                ..
            } if window_id == self.window.id() => {
                overlay.events.push(egui::Event::PointerButton {
                    pos: egui::pos2(x as f32, y as f32),
                    button: egui::PointerButton::Primary,
                    pressed: matches!(event, Event::MouseButtonDown { .. }),
                    modifiers: egui::Modifiers::NONE,
                });
                overlay.dirty = true;
            }
            Event::MouseWheel {
                window_id,
                precise_y,
                direction,
                mouse_x,
                mouse_y,
                ..
            } if window_id == self.window.id()
                && (overlay.game_submenu.is_some()
                    || overlay.shader_controls_open
                    || overlay.ctx.memory(|memory| memory.any_popup_open())) =>
            {
                let delta = if direction == sdl2::mouse::MouseWheelDirection::Flipped {
                    -precise_y
                } else {
                    precise_y
                };
                overlay.events.push(egui::Event::PointerMoved(egui::pos2(
                    mouse_x as f32,
                    mouse_y as f32,
                )));
                overlay.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0., delta),
                    modifiers: egui::Modifiers::NONE,
                });
                overlay.dirty = true;
            }
            Event::Window {
                window_id,
                win_event,
                ..
            } if window_id == self.window.id() => {
                use sdl2::event::WindowEvent;
                match win_event {
                    WindowEvent::FocusLost => {
                        overlay.focused = false;
                        overlay.events.push(egui::Event::WindowFocused(false));
                        overlay.events.push(egui::Event::PointerGone);
                    }
                    WindowEvent::FocusGained => {
                        overlay.focused = true;
                        overlay.events.push(egui::Event::WindowFocused(true));
                    }
                    WindowEvent::Leave => overlay.events.push(egui::Event::PointerGone),
                    _ => {}
                }
                overlay.dirty = true;
            }
            _ => {}
        }
    }
    /// Dispatch UI intentions through the same bounded worker command channel
    /// as hotkeys. Failure remains a visible message, never ends the game.
    pub fn configure_screenshots(&mut self, directory: std::path::PathBuf) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.screenshot_directory = Some(directory);
            overlay.dirty = true;
        }
    }
    pub fn take_screenshot(&mut self) -> Result<(), String> {
        let directory = self
            .overlay
            .as_ref()
            .and_then(|o| o.screenshot_directory.clone())
            .ok_or("Screenshots are not configured")?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = directory.join(format!("screenshot-{stamp}.png"));
        self.save_game_preview(&path)?;
        self.overlay_message("Screenshot saved".into());
        Ok(())
    }
    pub fn configure_overlay_filters(&mut self, root: &std::path::Path) -> Result<(), String> {
        if let Some(overlay) = self.overlay.as_mut() {
            let pack = emuluna_shader_pack::Pack::open(root)?;
            overlay.filter_choices = pack
                .keys()
                .map(|(id, name)| (id.to_owned(), name.to_owned()))
                .collect();
            overlay
                .filter_choices
                .sort_by(|(id, name), (other_id, other_name)| {
                    id.starts_with("shader:")
                        .cmp(&other_id.starts_with("shader:"))
                        .then_with(|| name.to_lowercase().cmp(&other_name.to_lowercase()))
                });
            overlay.filter_root = Some(root.to_path_buf());
            overlay.dirty = true;
        }
        Ok(())
    }
    pub fn service_audio(&mut self, device: &mut emuluna_audio_device::Device) {
        if let Some(message) = device.service() {
            self.overlay_message(message);
        }
    }
    pub(super) fn refresh_shader_controls(&mut self) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.filter_selected = self.preset_key.clone().unwrap_or_else(|| {
                match self.filter {
                    Filter::Nearest => "nearest",
                    Filter::Linear => "linear",
                    Filter::Scanlines => "scanlines",
                }
                .to_owned()
            });
            overlay.shader_fields = self
                .preset
                .as_ref()
                .map_or_else(Vec::new, chain::Pipeline::editable_parameters);
            overlay.shader_fields_changed = false;
            overlay.shader_resolution = self.shader_resolution;
            overlay.dirty = true;
        }
    }
    fn remember_shader_setting(&mut self, key: String, value: String) {
        self.shader_settings.insert(key.clone(), value.clone());
        self.shader_pending.insert(key, value);
    }
    fn service_shader_controls(&mut self) -> Result<(), String> {
        let Some(overlay) = self.overlay.as_mut() else {
            return Ok(());
        };
        let dragging = overlay.ctx.input(|input| input.pointer.primary_down());
        let changed = std::mem::take(&mut overlay.shader_fields_changed);
        let resolution = overlay.shader_resolution_request.take();
        if changed {
            if let (Some(preset), Some(key)) = (&mut self.preset, &self.preset_key) {
                let values = overlay
                    .shader_fields
                    .iter()
                    .map(|(parameter, value)| (parameter.key.clone(), *value))
                    .collect::<BTreeMap<_, _>>();
                preset.apply_parameters(&values);
                self.cached = None;
                overlay.dirty = true;
                let key = format!("shader_parameters.{}.{key}", self.shader_system);
                let value = serde_json::to_string(&values).map_err(|error| error.to_string())?;
                self.remember_shader_setting(key, value);
            }
        }
        if let Some(height) = resolution {
            self.shader_resolution = height;
            self.cached = None;
            if let Some(overlay) = self.overlay.as_mut() {
                overlay.shader_resolution = height;
                overlay.dirty = true;
            }
            self.remember_shader_setting(
                format!("filter_resolution.{}", self.shader_system),
                height.to_string(),
            );
        }
        if !dragging {
            self.flush_shader_settings()?;
        }
        Ok(())
    }
    pub(super) fn flush_shader_settings(&mut self) -> Result<(), String> {
        if !self.shader_pending.is_empty() {
            let pending = std::mem::take(&mut self.shader_pending);
            Self::persist_session_options(pending)?;
        }
        Ok(())
    }
    fn persist_session_options(options: BTreeMap<String, String>) -> Result<(), String> {
        let Some(path) = std::env::var_os("EMULUNA_CONSOLE_OPTIONS_FILE") else {
            return Ok(());
        };
        let path = std::path::Path::new(&path);
        let mut saved: BTreeMap<String, String> = std::fs::read(path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        saved.extend(options);
        let parent = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
        serde_json::to_writer(file.as_file(), &saved).map_err(|error| error.to_string())?;
        file.persist(path).map_err(|error| error.to_string())?;
        Ok(())
    }
    pub fn service_overlay(&mut self, commands: &mut Controller) -> Result<bool, String> {
        if let Err(error) = self.service_shader_controls() {
            self.overlay_message(format!("Could not remember shader settings: {error}"));
        }
        if let Some(enabled) = self
            .overlay
            .as_mut()
            .and_then(|o| o.n64_auto_request.take())
        {
            match self.configure_n64_auto_crop(enabled) {
                Ok(()) => {
                    if let Some(path) = std::env::var_os("EMULUNA_CONSOLE_OPTIONS_FILE") {
                        let mut saved: std::collections::BTreeMap<String, String> =
                            std::fs::read(&path)
                                .ok()
                                .and_then(|data| serde_json::from_slice(&data).ok())
                                .unwrap_or_default();
                        saved.insert(
                            "n64.automatic_borders".into(),
                            if enabled { "1" } else { "0" }.into(),
                        );
                        if let Err(error) = serde_json::to_vec(&saved)
                            .map_err(|e| e.to_string())
                            .and_then(|data| std::fs::write(path, data).map_err(|e| e.to_string()))
                        {
                            self.overlay_message(format!(
                                "Could not remember screen adjustment: {error}"
                            ));
                        }
                    }
                    self.overlay_message(
                        if enabled {
                            "Automatic screen adjustment on"
                        } else {
                            "Automatic screen adjustment off"
                        }
                        .into(),
                    );
                }
                Err(error) => {
                    if let Some(overlay) = &mut self.overlay {
                        overlay.n64_auto = Some(self.n64_crop.is_some());
                    }
                    self.overlay_message(error);
                }
            }
        }
        let requests = self
            .overlay
            .as_mut()
            .map(|o| std::mem::take(&mut o.option_requests))
            .unwrap_or_default();
        for (key, value, preference, restart) in requests {
            if !restart {
                if let Err(error) = commands.set_core_option(key, value.clone()) {
                    self.overlay_message(error);
                    continue;
                }
            }
            if let Some(path) = std::env::var_os("EMULUNA_CONSOLE_OPTIONS_FILE") {
                let mut saved: std::collections::BTreeMap<String, String> = std::fs::read(&path)
                    .ok()
                    .and_then(|data| serde_json::from_slice(&data).ok())
                    .unwrap_or_default();
                saved.insert(preference, value);
                if let Err(error) = serde_json::to_vec(&saved)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()))
                {
                    self.overlay_message(format!("Could not remember console option: {error}"));
                }
            }
            if restart {
                self.overlay_message("Console option will apply when the game restarts".into());
            }
        }
        let filter = self.overlay.as_mut().and_then(|overlay| {
            overlay
                .filter_request
                .take()
                .map(|key| (key, overlay.filter_root.clone()))
        });
        if let Some((key, root)) = filter {
            let result = match key.as_str() {
                "nearest" => {
                    self.set_filter(Filter::Nearest);
                    Ok(())
                }
                "linear" => {
                    self.set_filter(Filter::Linear);
                    Ok(())
                }
                "scanlines" => {
                    self.set_filter(Filter::Scanlines);
                    Ok(())
                }
                _ => root
                    .ok_or_else(|| "The screen-filter pack is unavailable".to_owned())
                    .and_then(|root| self.select_preset(&root, &key)),
            };
            match result {
                Err(error) => self.overlay_message(error),
                Ok(()) => {
                    if let Some(path) = std::env::var_os("EMULUNA_FILTER_SELECTION_FILE") {
                        if let Err(error) = std::fs::write(path, &key) {
                            self.overlay_message(format!("Could not remember filter: {error}"));
                        }
                    }
                }
            }
        }
        let actions = self
            .overlay
            .as_mut()
            .map(|o| std::mem::take(&mut o.actions))
            .unwrap_or_default();
        let mut close = false;
        for action in actions {
            let result = match action {
                Action::Pause => commands.toggle_pause(),
                Action::PreserveCropAspect(enabled) => {
                    self.configure_n64_crop_aspect(enabled);
                    Self::persist_session_options(BTreeMap::from([(
                        "n64.preserve_crop_aspect".into(),
                        if enabled { "1" } else { "0" }.into(),
                    )]))
                }

                Action::Autosave(enabled) => commands.set_autosave(enabled).and_then(|()| {
                    if let Ok(game) = std::env::var("EMULUNA_CONSOLE_GAME_ID") {
                        Self::persist_session_options(BTreeMap::from([(
                            format!("game.{game}.autosave"),
                            if enabled { "1" } else { "0" }.into(),
                        )]))
                        .map_err(|error| format!("Could not remember autosave: {error}"))?;
                    }
                    Ok(())
                }),
                Action::Reset => commands.reset(),
                Action::Save => commands.save_state(),
                Action::Load if commands.autosave() == Some(true) => {
                    if let Some(overlay) = &mut self.overlay {
                        overlay.confirm_load = true;
                        overlay.activity = Instant::now();
                        overlay.dirty = true;
                    }
                    Ok(())
                }
                Action::Load | Action::LoadConfirmed => commands.load_state(),
                Action::Volume(v) => commands.set_volume(v),
                Action::Slot(slot) => commands.select_state_slot(slot),
                Action::Screenshot => self.take_screenshot(),
                Action::Controls => std::env::var_os("EMULUNA_CONTROLS_REQUEST_FILE")
                    .ok_or("Controller settings are unavailable".to_owned())
                    .and_then(|path| {
                        std::fs::write(path, b"open")
                            .map_err(|error| format!("Could not open controller settings: {error}"))
                    }),
                Action::IntegerScale(enabled) => {
                    self.window_state.preferences.integer_scaling = enabled;
                    if let Some(path) = std::env::var_os("EMULUNA_CONSOLE_OPTIONS_FILE") {
                        let mut saved: std::collections::BTreeMap<String, String> =
                            std::fs::read(&path)
                                .ok()
                                .and_then(|data| serde_json::from_slice(&data).ok())
                                .unwrap_or_default();
                        saved.insert(
                            "integer_scale".into(),
                            if enabled { "1" } else { "0" }.into(),
                        );
                        serde_json::to_vec(&saved)
                            .map_err(|e| e.to_string())
                            .and_then(|data| std::fs::write(path, data).map_err(|e| e.to_string()))
                    } else {
                        Ok(())
                    }
                }
                Action::Fullscreen => self.toggle_fullscreen(),
                Action::Close => {
                    close = true;
                    Ok(())
                }
            };
            if let Err(error) = result {
                self.overlay_message(error);
            }
        }
        let filter_key = self.preset_key.as_deref().unwrap_or(match self.filter {
            Filter::Nearest => "nearest",
            Filter::Linear => "linear",
            Filter::Scanlines => "scanlines",
        });
        if let Some(overlay) = self.overlay.as_mut() {
            if overlay.paused != commands.paused() || overlay.volume != commands.volume() {
                overlay.dirty = true;
            }
            overlay.paused = commands.paused();
            overlay.volume = commands.volume();
            overlay.integer_scaling = self.window_state.preferences.integer_scaling;
            if overlay.volume > 0. {
                overlay.remembered_volume = overlay.volume;
            }
            overlay.states = commands.states_available();
            if overlay.autosave != commands.autosave() {
                overlay.autosave = commands.autosave();
                overlay.dirty = true;
            }
            if overlay.slot != commands.state_slot_number() || overlay.filter_selected != filter_key
            {
                overlay.dirty = true;
            }
            overlay.slot = commands.state_slot_number();
            if overlay.filter_selected != filter_key {
                overlay.filter_selected = filter_key.to_owned();
            }
        }
        Ok(close)
    }
    pub fn intercept_overlay_shortcut(&mut self, event: &Event) -> bool {
        let Some(overlay) = self.overlay.as_mut() else {
            return false;
        };
        match event {
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::F8),
                repeat,
                ..
            } if overlay.states => {
                if !repeat {
                    overlay.actions.push(Action::Load);
                    overlay.activity = Instant::now();
                    overlay.dirty = true;
                }
                true
            }
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::Escape),
                ..
            } if overlay.shader_controls_open => {
                if overlay.ctx.memory(|memory| memory.any_popup_open()) {
                    overlay.ctx.memory_mut(|memory| memory.close_popup());
                } else {
                    overlay.shader_controls_open = false;
                }
                overlay.dirty = true;
                true
            }
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::F7),
                repeat: false,
                ..
            } => {
                overlay.confirm_reset = true;
                overlay.activity = Instant::now();
                overlay.dirty = true;
                true
            }
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::Escape),
                ..
            } if overlay.confirm_reset || overlay.confirm_load || overlay.confirm_disable_autosave => {
                overlay.confirm_reset = false;
                overlay.confirm_load = false;
                overlay.confirm_disable_autosave = false;
                overlay.activity = Instant::now();
                overlay.dirty = true;
                true
            }
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::R),
                keymod,
                repeat: false,
                ..
            } if keymod
                .intersects(sdl2::keyboard::Mod::LCTRLMOD | sdl2::keyboard::Mod::RCTRLMOD) =>
            {
                overlay.confirm_reset = true;
                overlay.activity = Instant::now();
                overlay.dirty = true;
                true
            }
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::F1),
                repeat: false,
                ..
            } if std::env::var_os("EMULUNA_CONTROLS_REQUEST_FILE").is_some() => {
                overlay.actions.push(Action::Controls);
                overlay.activity = Instant::now();
                overlay.dirty = true;
                true
            }
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::F12),
                repeat: false,
                ..
            } => {
                overlay.actions.push(Action::Screenshot);
                true
            }
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::Escape),
                ..
            } if overlay.game_menu_open || overlay.ctx.memory(|memory| memory.any_popup_open()) => {
                overlay.game_menu_open = false;
                overlay.game_submenu = None;
                overlay.ctx.memory_mut(|memory| memory.close_popup());
                overlay.activity = Instant::now();
                overlay.dirty = true;
                true
            }
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::Escape),
                repeat,
                ..
            } if self.window.fullscreen_state() != sdl2::video::FullscreenType::Off => {
                if !repeat {
                    overlay.actions.push(Action::Fullscreen);
                }
                true
            }
            Event::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::Escape),
                repeat: true,
                ..
            } => true,
            _ => false,
        }
    }
    pub fn overlay_message(&mut self, text: String) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.message = Some((text, Instant::now()));
            overlay.dirty = true;
        }
    }
    pub fn overlay_needs_redraw(&self) -> bool {
        self.overlay.as_ref().is_some_and(|o| {
            o.last_paint.elapsed() >= Duration::from_millis(16)
                && (o.dirty || o.repaint_at.is_some_and(|time| time <= Instant::now()))
        })
    }
    pub(super) fn paint_overlay(&mut self) -> Result<(), String> {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.paint(
                &self.window,
                self.window_state.preferences.hide_cursor,
                self.window_state.preferences.lock_aspect
                    && self.window.fullscreen_state() == sdl2::video::FullscreenType::Off,
            );
            // egui changes GL bindings; the next game draw sets all of its state.
            self.last_uniforms = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires GL and EMULUNA_TEST_SHADER_PACK"]
    fn live_shader_controls_update_rendering_and_remember_values() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        presenter.enable_overlay("luna").unwrap();
        presenter.configure_shader_settings("nes");
        let root = std::path::PathBuf::from(std::env::var_os("EMULUNA_TEST_SHADER_PACK").unwrap());
        presenter.configure_overlay_filters(&root).unwrap();
        let key = "slang:dither";
        presenter.select_preset(&root, key).unwrap();
        let (mut commands, _worker) = emuluna_playback_control::channel();
        presenter.service_overlay(&mut commands).unwrap();
        let output = tempfile::tempdir().unwrap();
        let before = output.path().join("before.ppm");
        let after = output.path().join("after.ppm");
        let overlay = presenter.overlay.as_mut().unwrap();
        overlay.activity = Instant::now() - Duration::from_secs(5);
        presenter
            .draw_inner(4, 3, 4. / 3., &[128; 48], false)
            .unwrap();
        presenter.capture_diagnostic(&before).unwrap();

        let overlay = presenter.overlay.as_mut().unwrap();
        overlay.shader_controls_open = true;
        let (_, size) = overlay
            .shader_fields
            .iter_mut()
            .find(|(parameter, _)| parameter.key == "dither_size")
            .unwrap();
        *size = 0.85;
        overlay.shader_fields_changed = true;
        presenter.service_overlay(&mut commands).unwrap();
        let overlay = presenter.overlay.as_mut().unwrap();
        overlay.shader_controls_open = false;
        overlay.activity = Instant::now() - Duration::from_secs(5);
        // A parameter edit redraws the last frame even when no core frame arrives.
        presenter.redraw_inner(false).unwrap();
        presenter.capture_diagnostic(&after).unwrap();
        assert_ne!(
            std::fs::read(before).unwrap(),
            std::fs::read(after).unwrap()
        );

        presenter.set_filter(Filter::Nearest);
        presenter.select_preset(&root, key).unwrap();
        presenter.service_overlay(&mut commands).unwrap();
        let overlay = presenter.overlay.as_mut().unwrap();
        let (_, size) = overlay
            .shader_fields
            .iter()
            .find(|(parameter, _)| parameter.key == "dither_size")
            .unwrap();
        assert_eq!(*size, 0.85);
        overlay.game_menu_open = true;
        overlay.game_submenu = Some(3);
        for _ in 0..4 {
            presenter.redraw_inner(false).unwrap();
        }
        let overlay = presenter.overlay.as_ref().unwrap();
        let flyout = overlay.ctx.memory(|memory| {
            memory
                .area_rect(egui::Id::new(("game-menu-submenu", 3u8)))
                .unwrap()
        });
        let configure = egui::pos2(flyout.center().x, flyout.top() + 7. + MENU_ROW_HEIGHT / 2.);
        for pressed in [true, false] {
            let overlay = presenter.overlay.as_mut().unwrap();
            overlay.events.push(egui::Event::PointerMoved(configure));
            overlay.events.push(egui::Event::PointerButton {
                pos: configure,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            presenter.redraw_inner(false).unwrap();
        }
        assert!(presenter.overlay.as_ref().unwrap().shader_controls_open);
        for _ in 0..4 {
            presenter.redraw_inner(false).unwrap();
            std::thread::sleep(Duration::from_millis(40));
        }
        if let Some(path) = std::env::var_os("EMULUNA_TEST_SHADER_CONTROLS_CAPTURE") {
            presenter
                .capture_diagnostic(std::path::Path::new(&path))
                .unwrap();
        }
    }
    #[test]
    #[ignore = "requires GL and EMULUNA_TEST_SHADER_PACK"]
    fn live_game_menu_flyout_opens_on_click_and_fits() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        presenter.enable_overlay("luna").unwrap();
        let pack = std::env::var_os("EMULUNA_TEST_SHADER_PACK").unwrap();
        presenter
            .configure_overlay_filters(std::path::Path::new(&pack))
            .unwrap();
        let screenshots = tempfile::tempdir().unwrap();
        presenter.configure_screenshots(screenshots.path().into());
        let overlay = presenter.overlay.as_mut().unwrap();
        overlay.states = true;
        overlay.autosave = Some(true);
        overlay.game_menu_open = true;
        overlay.console_options.push(ConsoleOption {
            label: "Controller Pak".into(),
            key: "test-pak".into(),
            preference: String::new(),
            value: "memory".into(),
            choices: vec![
                ("memory".into(), "Memory Pak".into()),
                ("rumble".into(), "Rumble Pak".into()),
            ],
            restart: false,
            description: String::new(),
        });
        let mut backdrop = std::env::var_os("EMULUNA_TEST_MENU_BACKDROP")
            .map(|path| image::open(path).unwrap().to_rgba8())
            .unwrap_or_else(|| image::RgbaImage::from_pixel(4, 3, image::Rgba([70, 70, 70, 255])));
        let (width, height) = backdrop.dimensions();
        for pixel in backdrop.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        presenter
            .draw_inner(
                width,
                height,
                f64::from(width) / f64::from(height),
                &backdrop,
                false,
            )
            .unwrap();
        for _ in 0..3 {
            presenter.redraw_inner(false).unwrap();
        }

        let overlay = presenter.overlay.as_mut().unwrap();
        let parent = overlay
            .ctx
            .memory(|m| m.area_rect(egui::Id::new("game-menu-main")).unwrap());
        let filter_row = egui::pos2(
            parent.center().x,
            parent.top() + 7. + 7.5 * MENU_ROW_HEIGHT + 2. * GAME_MENU_SEPARATOR_HEIGHT,
        );
        overlay.events.push(egui::Event::PointerMoved(filter_row));
        presenter.redraw_inner(false).unwrap();
        assert_eq!(presenter.overlay.as_ref().unwrap().game_submenu, None);

        for pressed in [true, false] {
            presenter
                .overlay
                .as_mut()
                .unwrap()
                .events
                .push(egui::Event::PointerButton {
                    pos: filter_row,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            presenter.redraw_inner(false).unwrap();
        }
        for _ in 0..3 {
            presenter.redraw_inner(false).unwrap();
        }

        let overlay = presenter.overlay.as_ref().unwrap();
        assert_eq!(overlay.game_submenu, Some(3));
        let flyout = overlay.ctx.memory(|m| {
            m.area_rect(egui::Id::new(("game-menu-submenu", 3u8)))
                .unwrap()
        });
        assert!((flyout.left() - parent.right()).abs() < 1.);
        assert!(
            flyout.bottom() <= parent.bottom() + 1.,
            "{flyout:?} {parent:?}"
        );
        assert!(
            overlay.ctx.screen_rect().contains_rect(flyout),
            "{flyout:?}"
        );
        if let Some(path) = std::env::var_os("EMULUNA_TEST_MENU_CAPTURE") {
            presenter
                .capture_diagnostic(std::path::Path::new(&path))
                .unwrap();
        }
        let last_row = egui::pos2(
            flyout.center().x,
            flyout.bottom() - 7. - MENU_ROW_HEIGHT / 2.,
        );
        presenter.overlay_event(&Event::MouseWheel {
            timestamp: 0,
            window_id: presenter.window_id(),
            which: 0,
            x: 0,
            y: -80,
            direction: sdl2::mouse::MouseWheelDirection::Normal,
            precise_x: 0.,
            precise_y: -80.,
            mouse_x: last_row.x as i32,
            mouse_y: last_row.y as i32,
        });
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(16));
            presenter.redraw_inner(false).unwrap();
        }
        for pressed in [true, false] {
            presenter
                .overlay
                .as_mut()
                .unwrap()
                .events
                .push(egui::Event::PointerButton {
                    pos: last_row,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            presenter.redraw_inner(false).unwrap();
        }
        let overlay = presenter.overlay.as_ref().unwrap();
        assert_eq!(
            overlay.filter_request.as_ref(),
            overlay.filter_choices.last().map(|(id, _)| id)
        );
        assert!(!overlay.game_menu_open);
        if let Some(path) = std::env::var_os("EMULUNA_TEST_SCALING_MENU_CAPTURE") {
            let overlay = presenter.overlay.as_mut().unwrap();
            overlay.game_menu_open = true;
            overlay.game_submenu = Some(4);
            overlay.n64_auto = Some(true);
            overlay.console_options.push(ConsoleOption {
                label: "Internal resolution".into(),
                key: "parallel-n64-upscaling".into(),
                preference: String::new(),
                value: "1x".into(),
                choices: [("1x", "1×"), ("2x", "2×"), ("4x", "4×")]
                    .map(|(key, label)| (key.into(), label.into()))
                    .to_vec(),
                restart: true,
                description: String::new(),
            });
            for _ in 0..4 {
                presenter.redraw_inner(false).unwrap();
            }
            presenter
                .capture_diagnostic(std::path::Path::new(&path))
                .unwrap();
            presenter.overlay.as_mut().unwrap().game_submenu = None;
        }

        presenter.overlay.as_mut().unwrap().game_menu_open = true;
        for _ in 0..3 {
            presenter.redraw_inner(false).unwrap();
        }
        let overlay = presenter.overlay.as_ref().unwrap();
        let menu = overlay
            .ctx
            .memory(|m| m.area_rect(egui::Id::new("game-menu-main")).unwrap());
        let autosave = egui::pos2(menu.center().x, menu.bottom() - 7. - MENU_ROW_HEIGHT / 2.);
        for pressed in [true, false] {
            let overlay = presenter.overlay.as_mut().unwrap();
            overlay.events.push(egui::Event::PointerMoved(autosave));
            overlay.events.push(egui::Event::PointerButton {
                pos: autosave,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            presenter.redraw_inner(false).unwrap();
        }
        let overlay = presenter.overlay.as_ref().unwrap();
        assert!(overlay.confirm_disable_autosave);
        assert_eq!(overlay.autosave, Some(true));
        assert!(!overlay
            .actions
            .iter()
            .any(|action| matches!(action, Action::Autosave(false))));
    }
    #[test]
    #[ignore = "requires desktop GL"]
    fn live_reference_toolbar_stays_on_screen_at_small_window_sizes() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        presenter.enable_overlay("luna").unwrap();
        let screenshots = tempfile::tempdir().unwrap();
        presenter.configure_screenshots(screenshots.path().into());
        {
            let overlay = presenter.overlay.as_mut().unwrap();
            overlay.states = true;
            overlay.filter_choices = vec![("nearest".into(), "Nearest Neighbor".into())];
            overlay.volume = 0.55;
        }
        for width in [1280, 960, 640, 400] {
            presenter.window.set_size(width, 720).unwrap();
            // Let the compositor apply the drawable size before reading pixels.
            std::thread::sleep(Duration::from_millis(60));
            sdl.event_pump().unwrap().pump_events();
            presenter.overlay.as_mut().unwrap().activity = Instant::now();
            presenter
                .draw_inner(2, 2, 4. / 3., &[128; 16], false)
                .unwrap();
            std::thread::sleep(Duration::from_millis(200));
            presenter.redraw_inner(false).unwrap();
            presenter.redraw_inner(false).unwrap();
            let overlay = presenter.overlay.as_ref().unwrap();
            let rect = overlay.bar_rect.unwrap();
            assert!(
                rect.left() >= 0. && rect.right() <= width as f32,
                "{width}: {rect:?}"
            );
            assert!(
                rect.top() > 0. && rect.bottom() <= 720.,
                "{width}: {rect:?}"
            );
            assert!(rect.contains_rect(overlay.pause_rect.unwrap()));
            if let Some(root) = std::env::var_os("EMULUNA_TEST_OVERLAY_CAPTURE_DIR") {
                presenter
                    .capture_diagnostic(
                        &std::path::Path::new(&root).join(format!("toolbar-{width}.ppm")),
                    )
                    .unwrap();
            }
        }
        let (mut commands, _worker) = emuluna_playback_control::channel();
        commands.set_volume(0.55).unwrap();
        presenter.service_overlay(&mut commands).unwrap();
        for expected in [0., 0.55] {
            let center = presenter
                .overlay
                .as_ref()
                .unwrap()
                .mute_rect
                .unwrap()
                .center();
            for pressed in [true, false] {
                let overlay = presenter.overlay.as_mut().unwrap();
                overlay.events.push(egui::Event::PointerMoved(center));
                overlay.events.push(egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
                presenter.redraw_inner(false).unwrap();
            }
            presenter.service_overlay(&mut commands).unwrap();
            assert_eq!(commands.volume(), expected);
            assert!(!presenter
                .overlay
                .as_ref()
                .unwrap()
                .ctx
                .memory(|m| m.any_popup_open()));
        }
    }
    #[test]
    #[ignore = "requires desktop GL"]
    fn live_auto_hide_notices_and_ratio_locked_resize() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        presenter.enable_overlay("luna").unwrap();
        presenter.configure_window(super::super::WindowPreferences {
            lock_aspect: true,
            pause_unfocused: true,
            volume: 0.4,
            ..Default::default()
        });
        let (mut commands, _worker) = emuluna_playback_control::channel();
        commands.set_volume(presenter.initial_volume()).unwrap();
        presenter.service_overlay(&mut commands).unwrap();
        let pixels = vec![128; 320 * 240 * 4];
        presenter
            .draw_inner(320, 240, 4. / 3., &pixels, false)
            .unwrap();
        assert!(presenter.overlay.as_ref().unwrap().pause_rect.is_some());
        presenter.overlay.as_mut().unwrap().activity = Instant::now() - Duration::from_secs(3);
        presenter.redraw_inner(false).unwrap();
        assert!(presenter.overlay.as_ref().unwrap().pause_rect.is_none());
        presenter.overlay_message("State loaded · Slot 2".into());
        presenter.redraw_inner(false).unwrap();
        assert!(presenter.overlay.as_ref().unwrap().notice_visible);
        assert!(presenter.overlay.as_ref().unwrap().pause_rect.is_none());
        presenter
            .overlay
            .as_mut()
            .unwrap()
            .message
            .as_mut()
            .unwrap()
            .1 = Instant::now() - Duration::from_secs(3);
        presenter.redraw_inner(false).unwrap();
        assert!(!presenter.overlay.as_ref().unwrap().notice_visible);
        let id = presenter.window.id();
        let (w, h) = presenter.window.size();
        presenter
            .window_event(
                &sdl2::event::Event::MouseButtonDown {
                    timestamp: 0,
                    window_id: id,
                    which: 0,
                    mouse_btn: sdl2::mouse::MouseButton::Left,
                    clicks: 1,
                    x: w as i32 - 5,
                    y: h as i32 - 5,
                },
                &mut commands,
            )
            .unwrap();
        presenter
            .window_event(
                &sdl2::event::Event::MouseMotion {
                    timestamp: 0,
                    window_id: id,
                    which: 0,
                    mousestate: sdl2::mouse::MouseState::from_sdl_state(1),
                    x: w as i32 - 85,
                    y: h as i32 - 5,
                    xrel: -80,
                    yrel: 0,
                },
                &mut commands,
            )
            .unwrap();
        let (w, h) = presenter.window.size();
        assert!((f64::from(w) / f64::from(h) - 4. / 3.).abs() < 0.005);
        for win_event in [
            sdl2::event::WindowEvent::FocusLost,
            sdl2::event::WindowEvent::FocusGained,
        ] {
            presenter
                .window_event(
                    &sdl2::event::Event::Window {
                        timestamp: 0,
                        window_id: id,
                        win_event,
                    },
                    &mut commands,
                )
                .unwrap();
            assert_eq!(
                commands.paused(),
                win_event == sdl2::event::WindowEvent::FocusLost
            );
        }
        commands.toggle_pause().unwrap();
        presenter.service_overlay(&mut commands).unwrap();
        presenter.redraw_inner(false).unwrap();
        assert!(presenter.overlay.as_ref().unwrap().pause_rect.is_some());
        assert!((presenter.overlay.as_ref().unwrap().volume - 0.4).abs() < 0.001);
        let key_event = |key| sdl2::event::Event::KeyDown {
            timestamp: 0,
            window_id: id,
            keycode: Some(key),
            scancode: None,
            keymod: sdl2::keyboard::Mod::NOMOD,
            repeat: false,
        };
        assert!(presenter.intercept_overlay_shortcut(&key_event(sdl2::keyboard::Keycode::F7)));
        presenter.redraw_inner(false).unwrap();
        assert!(presenter.overlay.as_ref().unwrap().confirm_reset);
        assert!(!presenter
            .overlay
            .as_ref()
            .unwrap()
            .actions
            .iter()
            .any(|action| matches!(action, Action::Reset)));
        assert!(presenter.intercept_overlay_shortcut(&key_event(sdl2::keyboard::Keycode::Escape)));
        assert!(!presenter.overlay.as_ref().unwrap().confirm_reset);
        sdl.mouse().show_cursor(true);
    }
    #[test]
    #[ignore = "requires desktop GL"]
    fn live_overlay_clicks_pause_and_preserves_image() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        let (mut commands, _worker) = emuluna_playback_control::channel();
        for theme in ["classic", "daylight", "luna"] {
            presenter.enable_overlay(theme).unwrap();
            assert!(!presenter
                .overlay
                .as_ref()
                .unwrap()
                .filter_choices
                .is_empty());
            presenter.overlay.as_mut().unwrap().filter_request = Some("linear".into());
            presenter.service_overlay(&mut commands).unwrap();
            assert!(matches!(presenter.filter(), Filter::Linear));
            presenter.service_overlay(&mut commands).unwrap();
            presenter
                .draw_inner(2, 2, 4. / 3., &[128; 16], false)
                .unwrap();
            presenter.redraw_inner(false).unwrap();
            let rect = presenter.overlay.as_ref().unwrap().pause_rect.unwrap();
            let p = rect.center();
            assert!(p.x > 0. && p.y > 0. && p.x < 960. && p.y < 720.);
            for pressed in [true, false] {
                let overlay = presenter.overlay.as_mut().unwrap();
                overlay.events.push(egui::Event::PointerMoved(p));
                overlay.events.push(egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
                presenter.redraw_inner(false).unwrap();
            }
            let before = commands.paused();
            assert!(!presenter.service_overlay(&mut commands).unwrap());
            assert_ne!(commands.paused(), before, "{theme}: pause button failed");
            presenter.redraw_inner(false).unwrap();
            presenter.set_filter(Filter::Linear);
            presenter.redraw_inner(false).unwrap();
            assert!(
                presenter.cached.is_some(),
                "Changing filters while paused must keep the controls usable"
            );
            let screenshots = tempfile::tempdir().unwrap();
            presenter.configure_screenshots(screenshots.path().to_path_buf());
            presenter
                .overlay
                .as_mut()
                .unwrap()
                .actions
                .push(Action::Screenshot);
            presenter.service_overlay(&mut commands).unwrap();
            let path = std::fs::read_dir(screenshots.path())
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let image = image::open(path).unwrap().to_rgba8();
            let pixel = image.get_pixel(image.width() / 2, image.height() - 40).0;
            assert_eq!(
                &pixel[..3],
                &[128, 128, 128],
                "Screenshot must exclude overlay controls"
            );
            if let Some(root) = std::env::var_os("EMULUNA_TEST_SHADER_PACK") {
                presenter
                    .configure_overlay_filters(std::path::Path::new(&root))
                    .unwrap();
                presenter.overlay.as_mut().unwrap().filter_request =
                    Some("slang:crt-geom-deluxe".into());
                presenter.service_overlay(&mut commands).unwrap();
                assert_eq!(presenter.preset_key(), Some("slang:crt-geom-deluxe"));
                presenter.redraw_inner(false).unwrap();
                presenter.overlay.as_mut().unwrap().filter_request = Some("missing-preset".into());
                presenter.service_overlay(&mut commands).unwrap();
                assert_eq!(presenter.preset_key(), Some("slang:crt-geom-deluxe"));
                presenter.redraw_inner(false).unwrap();
            }
            if let Some(root) = std::env::var_os("EMULUNA_TEST_OVERLAY_CAPTURE_DIR") {
                presenter
                    .capture_diagnostic(
                        &std::path::Path::new(&root).join(format!("overlay-{theme}.ppm")),
                    )
                    .unwrap();
            }
            unsafe {
                assert_eq!(presenter.gl.get_error(), glow::NO_ERROR);
            }
        }
        // An invalid theme cannot discard the currently functioning painter.
        assert!(presenter.enable_overlay("missing").is_err());
        assert!(presenter.overlay.is_some());
    }
}
