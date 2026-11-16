use super::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
const SPECS: &str = include_str!("../../../../data/controller_profiles.json");
const LAYOUTS: &str = include_str!("../../../../data/controls.json");
const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(500);
const SYSTEMS: &str = include_str!("../../../../data/systems.json");
const BUTTONS: [(&str, u32); 16] = [
    ("Up", 64),
    ("Down", 128),
    ("Left", 32),
    ("Right", 16),
    ("Start", 8),
    ("Select", 4),
    ("A", 1),
    ("B", 2),
    ("X", 1024),
    ("Y", 2048),
    ("L", 512),
    ("R", 256),
    ("L2", 4096),
    ("R2", 8192),
    ("L3", 16384),
    ("R3", 32768),
];
const PAD_NAMES: [&str; 21] = [
    "South (A / Cross)",
    "East (B / Circle)",
    "West (X / Square)",
    "North (Y / Triangle)",
    "Back / Select",
    "Guide",
    "Start",
    "Left stick",
    "Right stick",
    "Left shoulder",
    "Right shoulder",
    "D-pad up",
    "D-pad down",
    "D-pad left",
    "D-pad right",
    "Misc",
    "Paddle 1",
    "Paddle 2",
    "Paddle 3",
    "Paddle 4",
    "Touchpad",
];
#[derive(Default)]
pub struct Controls {
    pub close_requested: bool,
    restore_requested: bool,
    system: String,
    player: usize,
    gamepad: bool,
    capture: Option<(String, Instant)>,
    capture_modifiers: egui::Modifiers,
    hovered_action: Option<String>,
    scroll_to_action: Option<String>,
    subsystem: Option<sdl2::GameControllerSubsystem>,
    controller_events: Option<sdl2::EventPump>,
    pads: Vec<(u32, sdl2::controller::GameController)>,
    refreshed: Option<Instant>,
    error: String,
}
impl Controls {
    fn controller_diagram(
        &mut self,
        ui: &mut egui::Ui,
        env: &mut Environment<'_>,
        spec: &Value,
        system: &str,
        height: f32,
    ) {
        let image = spec["diagram"].as_str().unwrap_or(system);
        let webp = env.data.join("controllers").join(format!("{image}.webp"));
        let path = if webp.is_file() {
            webp
        } else {
            env.data.join("controllers").join(format!("{image}.png"))
        };
        let layout = super::diagram::get(image);
        let path = layout
            .and_then(|layout| layout.image.as_ref())
            .map_or(path, |name| env.data.join("controllers").join(name));
        let rect = ui
            .allocate_exact_size(
                egui::vec2(ui.available_width(), height),
                egui::Sense::hover(),
            )
            .0;
        if let Some(texture) = env.artwork.texture(&path) {
            let fitted = env.artwork.fitted_rect(&path, rect);
            ui.painter().image(
                texture,
                fitted,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
            if let Some(layout) = layout {
                let size = egui::vec2(layout.size[0], layout.size[1]);
                let allowed = actions(spec);
                let response = ui.interact(
                    fitted,
                    ui.id().with("controller-artwork"),
                    egui::Sense::click(),
                );
                for part in &layout.controls {
                    if !allowed.iter().any(|(action, _)| action == &part.action) {
                        continue;
                    }
                    let hovered = response.hover_pos().is_some_and(|point| {
                        fitted.contains(point)
                            && part
                                .contains(((point - fitted.min) / fitted.size() * size).to_pos2())
                    });
                    if hovered {
                        self.hovered_action = Some(part.action.clone());
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        response.clone().on_hover_text(
                            allowed
                                .iter()
                                .find(|(action, _)| action == &part.action)
                                .map_or("", |(_, label)| label),
                        );
                        if response.clicked() {
                            self.capture = Some((part.action.clone(), Instant::now()));
                            self.capture_modifiers = ui.input(|input| input.modifiers);
                            self.scroll_to_action = Some(part.action.clone());
                            ui.ctx().request_repaint();
                        }
                    }
                }
                let selected = self
                    .capture
                    .as_ref()
                    .map(|(action, _)| action)
                    .or(self.hovered_action.as_ref());
                for part in &layout.controls {
                    if selected == Some(&part.action) {
                        part.highlight(
                            ui.painter(),
                            fitted,
                            size,
                            env.theme
                                .color("accent", ui.visuals().selection.stroke.color),
                        );
                    }
                }
            }
        }
        ui.menu_button("Keyboard defaults", |ui| {
            let record = player_defaults(system, self.player);
            egui::Grid::new("default-keyboard-controls").show(ui, |ui| {
                for (action, label) in actions(spec) {
                    ui.label(label);
                    ui.label(binding_label(&record["keyboard"][&action]));
                    ui.end_row();
                }
            });
            ui.separator();
            if ui.button("Restore default bindings").clicked() {
                self.restore_requested = true;
                ui.close_menu();
            }
        });
    }
}
fn newly_pressed_modifier(before: egui::Modifiers, now: egui::Modifiers) -> Option<i64> {
    if now.shift && !before.shift {
        Some(0x01000020)
    } else if now.ctrl && !before.ctrl {
        Some(0x01000021)
    } else if now.alt && !before.alt {
        Some(0x01000023)
    } else if now.mac_cmd && !before.mac_cmd {
        Some(0x01000022)
    } else {
        None
    }
}
fn qt_code(name: &str) -> Option<i64> {
    let name = name.strip_prefix("Key_").unwrap_or(name);
    Some(match name {
        "Escape" => 0x01000000,
        "Tab" => 0x01000001,
        "Backspace" => 0x01000003,
        "Return" => 0x01000004,
        "Enter" => 0x01000005,
        "Delete" => 0x01000007,
        "Left" => 0x01000012,
        "Up" => 0x01000013,
        "Right" => 0x01000014,
        "Down" => 0x01000015,
        "Space" => 32,
        "Home" => 0x01000010,
        "End" => 0x01000011,
        "PageUp" => 0x01000016,
        "PageDown" => 0x01000017,
        "Insert" => 0x01000006,
        "Comma" => 44,
        "Period" => 46,
        "Slash" => 47,
        "Backslash" => 92,
        "Semicolon" => 59,
        "Quote" => 39,
        "Backtick" => 96,
        "Minus" => 45,
        "Equals" => 61,
        "Plus" => 43,
        "OpenBracket" => 91,
        "CloseBracket" => 93,
        _ => {
            if let Some(number) = name
                .strip_prefix('F')
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|n| (1..=24).contains(n))
            {
                return Some(0x01000030 + number - 1);
            }
            if name.len() == 1 {
                name.as_bytes()[0] as i64
            } else {
                return None;
            }
        }
    })
}
fn key_token(key: egui::Key) -> Option<String> {
    let name = format!("{key:?}");
    let code = match name.as_str() {
        "ArrowUp" => 0x01000013,
        "ArrowDown" => 0x01000015,
        "ArrowLeft" => 0x01000012,
        "ArrowRight" => 0x01000014,
        "Enter" => 0x01000004,
        "Backspace" => 0x01000003,
        "Space" => 32,
        "Tab" => 0x01000001,
        _ => qt_code(name.strip_prefix("Num").unwrap_or(&name))?,
    };
    Some(format!("key:{code}"))
}
fn actions(spec: &Value) -> Vec<(String, String)> {
    let mut list = Vec::new();
    for bit in spec["primary"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
    {
        let label = spec["labels"][bit.to_string()]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                BUTTONS
                    .iter()
                    .find(|(_, b)| u64::from(*b) == bit)
                    .map_or("Button", |(label, _)| *label)
                    .to_owned()
            });
        list.push((format!("button:{bit}"), label));
    }
    if spec["analog"].as_bool() == Some(true) {
        let sticks = spec["sticks"].as_array().map_or(1, Vec::len).min(2);
        for index in 0..sticks * 2 {
            for (sign, direction) in [
                (-1, if index % 2 == 0 { "left" } else { "up" }),
                (1, if index % 2 == 0 { "right" } else { "down" }),
            ] {
                list.push((
                    format!("axis:{index}:{sign}"),
                    format!(
                        "{} {direction}",
                        spec["sticks"][index / 2].as_str().unwrap_or("Analog stick")
                    ),
                ));
            }
        }
    }
    if let Some(extra) = spec["extra_actions"].as_object() {
        for (key, label) in extra {
            if let Some(label) = label.as_str() {
                list.push((key.clone(), label.into()));
            }
        }
    }
    list
}
fn player_defaults(system: &str, player: usize) -> Value {
    let mut record = defaults(system);
    if player > 0 {
        record["keyboard"] = json!({});
    }
    record
}

fn defaults(system: &str) -> Value {
    let systems: Value = serde_json::from_str(SYSTEMS).expect("systems");
    let layouts: Value = serde_json::from_str(LAYOUTS).expect("layouts");
    let specs: Value = serde_json::from_str(SPECS).expect("specs");
    let spec = &specs[system];
    let layout = &layouts[systems[system]["input_profile"]
        .as_str()
        .unwrap_or("default")];
    let mut record = json!({"device":"auto","keyboard":{},"gamepad":{}});
    let mut add = |source: &str, action: String, token: String| {
        let entry = record[source]
            .as_object_mut()
            .unwrap()
            .entry(action)
            .or_insert_with(|| json!([]));
        entry.as_array_mut().unwrap().push(json!(token));
    };
    for (key, bit) in layout["keys"].as_object().into_iter().flatten() {
        if let (Some(bit), Some(code)) = (bit.as_u64(), qt_code(key)) {
            add("keyboard", format!("button:{bit}"), format!("key:{code}"));
        }
    }
    for (key, axis) in layout["axes"].as_object().into_iter().flatten() {
        if let (Some(index), Some(value), Some(code)) =
            (axis[0].as_u64(), axis[1].as_i64(), qt_code(key))
        {
            add(
                "keyboard",
                format!("axis:{index}:{}", if value > 0 { 1 } else { -1 }),
                format!("key:{code}"),
            );
        }
    }
    for (button, bit) in layout["gamepad"].as_object().into_iter().flatten() {
        if let Some(bit) = bit.as_u64() {
            if !spec["unbound_gamepad"]
                .as_array()
                .is_some_and(|a| a.iter().any(|b| b.as_u64() == Some(bit)))
            {
                add(
                    "gamepad",
                    format!("button:{bit}"),
                    format!("button:{button}"),
                );
            }
        }
    }
    for (axis, bit) in layout["triggers"].as_object().into_iter().flatten() {
        if let Some(bit) = bit.as_u64() {
            add("gamepad", format!("button:{bit}"), format!("axis:{axis}:1"));
        }
    }
    for axis in 0..4 {
        for sign in [-1, 1] {
            add(
                "gamepad",
                format!("axis:{axis}:{sign}"),
                format!("axis:{axis}:{sign}"),
            );
        }
    }
    if layout["stick_as_dpad"].as_bool() == Some(true) && spec["analog"].as_bool() != Some(true) {
        for (bit, axis, sign) in [(32, 0, -1), (16, 0, 1), (64, 1, -1), (128, 1, 1)] {
            add(
                "gamepad",
                format!("button:{bit}"),
                format!("axis:{axis}:{sign}"),
            );
        }
    }
    for (bit, token) in [
        (4096, "axis:4:1"),
        (8192, "axis:5:1"),
        (16384, "button:7"),
        (32768, "button:8"),
    ] {
        if !spec["unbound_gamepad"]
            .as_array()
            .is_some_and(|a| a.iter().any(|b| b.as_u64() == Some(bit)))
        {
            record["gamepad"]
                .as_object_mut()
                .unwrap()
                .entry(format!("button:{bit}"))
                .or_insert_with(|| json!([token]));
        }
    }
    for (action, token) in spec["gamepad_defaults"].as_object().into_iter().flatten() {
        record["gamepad"][action] = token.clone();
    }
    let allowed = actions(spec);
    for source in ["keyboard", "gamepad"] {
        record[source]
            .as_object_mut()
            .unwrap()
            .retain(|key, _| allowed.iter().any(|(action, _)| action == key));
    }
    record
}
fn binding_label(value: &Value) -> String {
    let mut tokens: Vec<_> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|token| {
            if let Some(code) = token
                .strip_prefix("key:")
                .and_then(|s| s.parse::<i64>().ok())
            {
                return match code {
                    0x01000012 => "Left".into(),
                    0x01000013 => "Up".into(),
                    0x01000014 => "Right".into(),
                    0x01000015 => "Down".into(),
                    0x01000004 | 0x01000005 => "Enter".into(),
                    0x01000003 => "Backspace".into(),
                    32 => "Space".into(),
                    0x01000020 => "Shift".into(),
                    0x01000021 => "Ctrl".into(),
                    0x01000022 => "Meta".into(),
                    0x01000023 => "Alt".into(),
                    0x01000030..=0x01000047 => format!("F{}", code - 0x01000030 + 1),
                    _ => char::from_u32(code as u32)
                        .filter(|c| !c.is_control())
                        .map_or(format!("Key {code}"), |c| c.to_string()),
                };
            }
            if let Some(index) = token
                .strip_prefix("button:")
                .and_then(|s| s.parse::<usize>().ok())
            {
                return PAD_NAMES.get(index).unwrap_or(&"Button").to_string();
            }
            if let Some(axis) = token.strip_prefix("axis:") {
                return format!("Axis {axis}");
            }
            token.to_owned()
        })
        .collect();
    tokens.dedup();
    if tokens.is_empty() {
        "Unassigned".into()
    } else {
        tokens.join(" / ")
    }
}
impl Controls {
    pub fn select_console(&mut self, id: &str) {
        self.system = id.to_owned();
        self.player = 0;
        self.capture = None;
    }
    fn refresh(&mut self) {
        self.refreshed = Some(Instant::now());
        match self.poll_controllers() {
            Ok(()) => self.error.clear(),
            Err(error) => {
                self.error = error;
                self.pads.clear();
                self.controller_events = None;
                self.subsystem = None;
            }
        }
    }
    fn poll_controllers(&mut self) -> Result<(), String> {
        use sdl2::event::Event;

        let first_poll = self.controller_events.is_none();
        if first_poll {
            let sdl = sdl2::init()?;
            self.subsystem = Some(sdl.game_controller()?);
            self.controller_events = Some(sdl.event_pump()?);
        }

        let mut devices_changed = first_poll;
        for event in self.controller_events.as_mut().unwrap().poll_iter() {
            devices_changed |= matches!(
                event,
                Event::ControllerDeviceAdded { .. }
                    | Event::ControllerDeviceRemoved { .. }
                    | Event::ControllerDeviceRemapped { .. }
            );
        }
        if devices_changed || self.pads.iter().any(|(_, pad)| !pad.attached()) {
            let controllers = self.subsystem.as_ref().unwrap();
            self.pads = (0..controllers.num_joysticks()?)
                .filter(|&index| controllers.is_game_controller(index))
                .filter_map(|index| controllers.open(index).ok().map(|pad| (index, pad)))
                .collect();
        }
        Ok(())
    }
    pub fn cancel_capture(&mut self) {
        self.capture = None;
    }
    fn binding_grid(&mut self, ui: &mut egui::Ui, spec: &Value, record: &mut Value, source: &str) {
        ui.spacing_mut().button_padding.y = 1.;
        ui.spacing_mut().interact_size.y = 18.;
        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let visuals = ui.visuals_mut();
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            widget.corner_radius = egui::CornerRadius::same(5);
            widget.bg_stroke = egui::Stroke::new(1_f32, border);
        }
        ui.style_mut().override_font_id = Some(egui::FontId::proportional(13.));
        let bindings = actions(spec);
        let columns = if bindings.len() > 12 && ui.available_width() > 520. {
            2
        } else {
            1
        };
        let binding_width = if columns == 2 {
            128.
        } else {
            (ui.available_width() - 106.).clamp(60., 240.)
        };
        let row_height = 16.;
        let mut previous_group = "";
        egui::Grid::new("controller-binding-grid")
            .spacing([6., 2.])
            .show(ui, |ui| {
                for (index, (action, label)) in bindings.into_iter().enumerate() {
                    if columns == 1 {
                        let group = match action.as_str() {
                            "button:64" | "button:128" | "button:32" | "button:16" => {
                                "DIRECTIONAL PAD"
                            }
                            "button:8" | "button:4" => "MENU",
                            _ => "ACTION BUTTONS",
                        };
                        if group != previous_group {
                            let r = ui
                                .allocate_exact_size(egui::vec2(70., 14.), egui::Sense::hover())
                                .0;
                            ui.painter().text(
                                r.min,
                                egui::Align2::LEFT_TOP,
                                group,
                                fonts::bold(11.),
                                ui.visuals().hyperlink_color,
                            );
                            ui.end_row();
                            previous_group = group;
                        }
                    }
                    ui.add_sized(
                        [70., row_height],
                        egui::Label::new(RichText::new(&label).size(12.)).truncate(),
                    );
                    let listening = self.capture.as_ref().is_some_and(|(a, _)| a == &action);
                    let response = ui.add_sized(
                        [binding_width, row_height],
                        egui::Button::new(if listening {
                            "Press a control…".into()
                        } else {
                            binding_label(&record[source][&action])
                        }),
                    );
                    if response.hovered() || response.has_focus() {
                        self.hovered_action = Some(action.clone());
                    }
                    if self.scroll_to_action.as_ref() == Some(&action) {
                        response.scroll_to_me(Some(egui::Align::Center));
                        response.request_focus();
                        self.scroll_to_action = None;
                    }
                    if response.clicked() {
                        self.capture = Some((action.clone(), Instant::now()));
                        self.capture_modifiers = ui.input(|input| input.modifiers);
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Clear binding")
                        .clicked()
                    {
                        record[source][&action] = json!([]);
                        self.capture = None;
                    }
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    }
    pub fn show(&mut self, ui: &mut egui::Ui, prefs: &mut Preferences, env: &mut Environment<'_>) {
        if self.system.is_empty() {
            self.system = if env.library.consoles.iter().any(|c| c.id == "snes") {
                "snes".into()
            } else {
                env.library
                    .consoles
                    .first()
                    .map_or(String::new(), |c| c.id.clone())
            };
        }
        if self
            .refreshed
            .is_none_or(|t| t.elapsed() >= CONTROLLER_POLL_INTERVAL)
        {
            self.refresh();
        }
        if let Some(last_poll) = self.refreshed {
            ui.ctx().request_repaint_after(
                CONTROLLER_POLL_INTERVAL.saturating_sub(last_poll.elapsed()),
            );
        }
        self.hovered_action = None;
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = 30.;
            ui.style_mut().override_font_id = Some(egui::FontId::proportional(14.));
            self.selectors(ui, env);
        });
        ui.add_space(12.);
        let specs: Value = serde_json::from_str(SPECS).expect("specs");
        let gap = 16.;
        let mapping_width =
            (if self.gamepad { 300_f32 } else { 260_f32 }).min((ui.available_width() - gap) * 0.46);
        let width = (ui.available_width() - gap - mapping_width).max(120.);
        let footer_height =
            ui.text_style_height(&egui::TextStyle::Small) + 2. * ui.spacing().item_spacing.y + 8.;
        let height = (ui.available_height() - footer_height).max(0.);
        let body_rect = ui
            .allocate_exact_size(
                egui::vec2(ui.available_width(), height),
                egui::Sense::hover(),
            )
            .0;
        let left = egui::Rect::from_min_size(body_rect.min, egui::vec2(width, height));
        let right = egui::Rect::from_min_max(
            egui::pos2(left.right() + gap, body_rect.top()),
            body_rect.right_bottom(),
        );
        for rect in [left, right] {
            ui.painter()
                .rect_filled(rect, 12., ui.visuals().window_fill.gamma_multiply(1.06));
            ui.painter().rect_stroke(
                rect,
                12.,
                egui::Stroke::new(1_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        let preview_content = left.shrink(16.);
        let status_height = ui
            .text_style_height(&egui::TextStyle::Body)
            .max(ui.spacing().interact_size.y);
        let status_top = preview_content.bottom() - status_height;
        let preview_rect = egui::Rect::from_min_max(
            preview_content.min,
            egui::pos2(preview_content.right(), status_top - 8.),
        );
        let mut preview = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("controller-preview")
                .max_rect(preview_rect),
        );
        preview.set_clip_rect(preview_rect);
        let diagram_height = (preview_rect.height()
            - preview.spacing().interact_size.y
            - 2. * preview.spacing().button_padding.y
            - preview.spacing().item_spacing.y)
            .min(height - 112.)
            .max(0.);
        self.controller_diagram(
            &mut preview,
            env,
            &specs[&self.system.clone()],
            &self.system.clone(),
            diagram_height,
        );
        let mut status = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("controller-status")
                .max_rect(egui::Rect::from_min_max(
                    egui::pos2(preview_content.left(), status_top),
                    preview_content.max,
                )),
        );
        status.set_clip_rect(left.shrink(8.));
        status.horizontal(|ui| {
            let connected = self.pads.iter().any(|(_, pad)| pad.attached());
            let color = if connected {
                Color32::from_rgb(90, 210, 120)
            } else {
                ui.visuals().weak_text_color()
            };
            let (r, _) = ui.allocate_exact_size(egui::vec2(10., 10.), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 4., color);
            ui.label(if connected {
                "Controller connected"
            } else {
                "No controller connected"
            });
        });
        let mut mapping = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("controller-mapping")
                .max_rect(right.shrink(16.)),
        );
        mapping.set_clip_rect(right.shrink(8.));
        mapping.label(RichText::new("Button mapping").font(fonts::bold(18.)));
        egui::ScrollArea::vertical()
            .id_salt("controls-bindings")
            .auto_shrink([false, false])
            .max_height(mapping.available_height())
            .show(&mut mapping, |ui| {
                self.binding_settings(ui, prefs, env, &specs)
            });
        ui.add_space(8.);
        ui.small("Bindings apply when the next game starts.");
        if self.restore_requested {
            self.restore_requested = false;
            prefs.set(
                &format!("controls.{}.{}", self.system, self.player),
                player_defaults(&self.system, self.player).to_string(),
            );
            self.capture = None;
        }
    }

    fn selectors(&mut self, ui: &mut egui::Ui, env: &Environment<'_>) {
        ui.columns(3, |columns| {
            columns[0].label("Console");
            let ui = &mut columns[0];
            let previous = self.system.clone();
            egui::ComboBox::from_id_salt("controls-console")
                .width(ui.available_width())
                .selected_text(
                    env.library
                        .consoles
                        .iter()
                        .find(|c| c.id == self.system)
                        .map_or(self.system.as_str(), |c| c.name.as_str()),
                )
                .show_ui(ui, |ui| {
                    for console in &env.library.consoles {
                        ui.selectable_value(&mut self.system, console.id.clone(), &console.name);
                    }
                });

            let previous_player = self.player;
            if previous != self.system {
                self.player = 0;
            }
            let count = emuluna_native_input::player_count(&self.system);
            columns[1].label("Player");
            let player_width = columns[1].available_width();
            egui::ComboBox::from_id_salt("controls-player")
                .width(player_width)
                .selected_text(format!("Player {}", self.player + 1))
                .show_ui(&mut columns[1], |ui| {
                    for player in 0..count {
                        ui.selectable_value(
                            &mut self.player,
                            player,
                            format!("Player {}", player + 1),
                        );
                    }
                });
            columns[2].label("Input");
            let ui = &mut columns[2];
            let previous_gamepad = self.gamepad;
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("controls-input")
                    .width(ui.available_width())
                    .selected_text(if self.gamepad {
                        "Controller"
                    } else {
                        "Keyboard"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.gamepad, false, "Keyboard");
                        ui.selectable_value(&mut self.gamepad, true, "Controller");
                    });
            });
            if previous_gamepad != self.gamepad {
                self.capture = None;
            }
            if previous != self.system || previous_player != self.player {
                self.capture = None;
                self.scroll_to_action = None;
            }
        });
    }

    fn binding_settings(
        &mut self,
        ui: &mut egui::Ui,
        prefs: &mut Preferences,
        env: &Environment<'_>,
        specs: &Value,
    ) {
        ui.spacing_mut().item_spacing.y = 4.;
        let spec = &specs[&self.system];
        let key = format!("controls.{}.{}", self.system, self.player);
        let original = value(prefs, env.library, &key, "{}");
        let mut record = player_defaults(&self.system, self.player);
        if let Ok(saved) = serde_json::from_str::<Value>(original) {
            for source in ["keyboard", "gamepad"] {
                if let Some(bindings) = saved[source].as_object() {
                    for (action, tokens) in bindings {
                        record[source][action] = tokens.clone();
                    }
                }
            }
            if let Some(device) = saved["device"].as_str() {
                record["device"] = json!(device);
            }
        }
        let before = record.clone();
        let source = if self.gamepad { "gamepad" } else { "keyboard" };
        if self.gamepad {
            let mut device = record["device"].as_str().unwrap_or("auto").to_owned();
            let device_label = if device == "auto" {
                "Automatic".to_owned()
            } else {
                self.pads
                    .iter()
                    .find(|(index, _)| device == format!("index:{index}"))
                    .map_or("Disconnected controller".to_owned(), |(_, pad)| pad.name())
            };
            egui::ComboBox::from_id_salt("controls-device")
                .width(264.)
                .selected_text(device_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut device, "auto".into(), "Automatic");
                    for (index, pad) in &self.pads {
                        ui.selectable_value(&mut device, format!("index:{index}"), pad.name());
                    }
                });
            record["device"] = json!(device);
        }
        ui.add_space(4.);
        if actions(spec).len() > 12 {
            ui.label(RichText::new("Gameplay Buttons").strong());
        }
        self.binding_grid(ui, spec, &mut record, source);
        if self.capture.is_some() {
            ui.add_space(10.);
            ui.horizontal_wrapped(|ui| {
                ui.label("Waiting for input… Escape cancels.");
                if ui.button("Cancel binding").clicked() {
                    self.capture = None;
                }
            });
        }
        if let Some((action, start)) = self.capture.clone() {
            let mut token = None;
            let mut clear = false;
            // Escape must cancel both controller and keyboard capture.
            let mut cancel =
                ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if !self.gamepad && !cancel {
                ui.input(|input| {
                    for event in &input.events {
                        if let egui::Event::Key {
                            key,
                            pressed: true,
                            repeat: false,
                            ..
                        } = event
                        {
                            if *key == egui::Key::Escape {
                                cancel = true;
                            } else if *key == egui::Key::Delete {
                                clear = true;
                            } else {
                                token = key_token(*key);
                            }
                        }
                    }
                });
                if token.is_none() && !cancel && !clear {
                    ui.input(|input| {
                        let code = newly_pressed_modifier(self.capture_modifiers, input.modifiers);
                        if let Some(code) = code {
                            token = Some(format!("key:{code}"));
                        }
                    });
                }
            } else if !cancel && start.elapsed() > Duration::from_millis(250) {
                if let Some(subsystem) = &self.subsystem {
                    subsystem.update();
                }
                let requested: Vec<_> = (0..emuluna_native_input::player_count(&self.system))
                    .map(|player| {
                        let saved;
                        let device = if player == self.player {
                            record["device"].as_str()
                        } else {
                            saved = serde_json::from_str::<Value>(value(
                                prefs,
                                env.library,
                                &format!("controls.{}.{player}", self.system),
                                "{}",
                            ))
                            .unwrap_or_default();
                            saved["device"].as_str()
                        };
                        device.and_then(emuluna_native_input::controller_index)
                    })
                    .collect();
                let available: Vec<_> = self
                    .pads
                    .iter()
                    .filter(|(_, pad)| pad.attached())
                    .map(|(index, _)| *index)
                    .collect();
                let selected = emuluna_native_input::controller_assignments(&requested, &available)
                    [self.player];
                if let Some((_, pad)) = self.pads.iter().find(|(index, _)| Some(*index) == selected)
                {
                    for (b, name) in [
                        "a",
                        "b",
                        "x",
                        "y",
                        "back",
                        "guide",
                        "start",
                        "leftstick",
                        "rightstick",
                        "leftshoulder",
                        "rightshoulder",
                        "dpup",
                        "dpdown",
                        "dpleft",
                        "dpright",
                        "misc1",
                        "paddle1",
                        "paddle2",
                        "paddle3",
                        "paddle4",
                        "touchpad",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if let Some(button) = sdl2::controller::Button::from_string(name) {
                            if pad.button(button) {
                                token = Some(format!("button:{b}"));
                            }
                        }
                    }
                    for (a, name) in [
                        "leftx",
                        "lefty",
                        "rightx",
                        "righty",
                        "lefttrigger",
                        "righttrigger",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if let Some(axis) = sdl2::controller::Axis::from_string(name) {
                            let amount = pad.axis(axis);
                            if amount.unsigned_abs() > 24000 {
                                token =
                                    Some(format!("axis:{a}:{}", if amount > 0 { 1 } else { -1 }));
                            }
                        }
                    }
                }
            }
            if cancel {
                self.capture = None;
            } else if clear {
                record[source][&action] = json!([]);
                self.capture = None;
            } else if let Some(token) = token {
                record[source][&action] = json!([token]);
                self.capture = None;
            } else if start.elapsed() > Duration::from_secs(15) {
                self.capture = None;
            }
            self.capture_modifiers = ui.input(|input| input.modifiers);
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        if record != before {
            prefs.set(&key, record.to_string());
            prefs.set(
                &format!(
                    "controls_migration.safe_special_buttons.{}.{}",
                    self.system, self.player
                ),
                "1",
            );
        }
        if !self.error.is_empty() {
            ui.label(&self.error);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escape_cancels_keyboard_and_controller_capture_without_changing_bindings() {
        let folder = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        let theme = Theme::builtin("luna").unwrap();
        let library = Library {
            root: folder.path().into(),
            games: Vec::new(),
            consoles: vec![emuluna_library_model::Console {
                id: "snes".into(),
                name: "Super Nintendo".into(),
                icon: String::new(),
                cover_size: [300, 220],
            }],
            collections: Vec::new(),
            theme: "luna".into(),
            settings: Default::default(),
        };
        let mut prefs = Preferences::default();
        let originals = HashMap::new();
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let preferences = folder.path().join("settings.json");
        let mut art = Artwork::new(ctx.clone());
        for gamepad in [false, true] {
            let mut controls = Controls {
                system: "snes".into(),
                gamepad,
                capture: Some(("button:1".into(), Instant::now())),
                refreshed: Some(Instant::now()),
                ..Default::default()
            };
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    controls.show(
                        ui,
                        &mut prefs,
                        &mut Environment {
                            library: &library,
                            presets: &[],
                            themes: &[],
                            data: &data,
                            theme: &theme,
                            artwork: &mut art,
                            original_covers: &originals,
                            preferences_path: &preferences,
                            playing: false,
                        },
                    );
                });
            });
            assert!(controls.capture.is_none());
            assert!(prefs.values.is_empty());
        }
        let image = data.join("controllers/snes.webp");
        art.texture(&image);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !art.idle() && Instant::now() < deadline {
            art.update(&ctx);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(art.size(&image).is_some());
        let specs: Value = serde_json::from_str(SPECS).unwrap();
        let diagram = super::super::diagram::get("snes").unwrap();
        let part = diagram
            .controls
            .iter()
            .find(|part| part.action == "button:1")
            .unwrap();
        let bounds = egui::Rect::from_min_size(egui::pos2(8., 8.), egui::vec2(500., 260.));
        let fitted = art.fitted_rect(&image, bounds);
        let point = fitted.min
            + egui::vec2(
                part.rect[0] + part.rect[2] * 0.5,
                part.rect[1] + part.rect[3] * 0.5,
            ) / egui::vec2(diagram.size[0], diagram.size[1])
                * fitted.size();
        let mut controls = Controls {
            system: "snes".into(),
            ..Default::default()
        };
        for pressed in [None, Some(true), Some(false)] {
            let mut events = vec![egui::Event::PointerMoved(point)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960., 720.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let mut controller_ui =
                            ui.new_child(egui::UiBuilder::new().max_rect(bounds));
                        controls.controller_diagram(
                            &mut controller_ui,
                            &mut Environment {
                                library: &library,
                                presets: &[],
                                themes: &[],
                                data: &data,
                                theme: &theme,
                                artwork: &mut art,
                                original_covers: &originals,
                                preferences_path: &preferences,
                                playing: false,
                            },
                            &specs["snes"],
                            "snes",
                            260.,
                        );
                    });
                },
            );
        }
        assert_eq!(controls.hovered_action.as_deref(), Some("button:1"));
        assert_eq!(
            controls.capture.as_ref().map(|(action, _)| action.as_str()),
            Some("button:1")
        );
        assert_eq!(
            newly_pressed_modifier(egui::Modifiers::SHIFT, egui::Modifiers::SHIFT),
            None
        );
        assert_eq!(
            newly_pressed_modifier(egui::Modifiers::NONE, egui::Modifiers::SHIFT),
            Some(0x01000020)
        );
    }
    #[test]
    fn defaults_and_edited_bindings_are_accepted_by_native_input() {
        let specs: Value = serde_json::from_str(SPECS).unwrap();
        for system in specs.as_object().unwrap().keys() {
            let record = defaults(system);
            emuluna_native_input::library_profile(system, &record.to_string(), true).unwrap();
            emuluna_native_input::Profile::for_system_saved(
                Some(system),
                Some(&record.to_string()),
            )
            .unwrap();
        }
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        for spec in specs.as_object().unwrap().values() {
            let mut controls = Controls::default();
            for source in ["keyboard", "gamepad"] {
                let mut record = json!({"keyboard": {}, "gamepad": {}});
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let before = ui.cursor().top();
                        controls.binding_grid(ui, spec, &mut record, source);
                        assert!(
                            ui.cursor().top() - before <= 360.,
                            "Bindings exceeded available height"
                        );
                    });
                });
            }
        }
        let nes = defaults("nes");
        assert_eq!(binding_label(&nes["keyboard"]["button:1"]), "X");
        assert_eq!(binding_label(&nes["keyboard"]["button:2"]), "Z");
        assert_eq!(binding_label(&nes["keyboard"]["button:8"]), "Enter");
        assert_eq!(binding_label(&nes["keyboard"]["button:64"]), "Up");
        let n64 = defaults("n64");
        assert_eq!(binding_label(&n64["keyboard"]["axis:0:-1"]), "Left");
        assert_eq!(binding_label(&n64["keyboard"]["button:4096"]), "Space");
        assert_eq!(key_token(egui::Key::ArrowUp), Some("key:16777235".into()));
    }
}
