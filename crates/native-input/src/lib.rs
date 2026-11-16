//! Window-thread input using the existing EmuLuna console catalogs.
#![forbid(unsafe_code)]
use emuluna_session::input::Snapshot;
mod saved;
pub use saved::library_profile;
use sdl2::{
    controller::{Axis, Button, GameController},
    event::{Event, WindowEvent},
    keyboard::Keycode,
    GameControllerSubsystem, Sdl,
};
use serde_json::Value;
use std::collections::HashSet;
const CONTROLS: &str = include_str!("../../../data/controls.json");
const SYSTEMS: &str = include_str!("../../../data/systems.json");
const SPECS: &str = include_str!("../../../data/controller_profiles.json");
const BUTTONS: [Button; 21] = [
    Button::A,
    Button::B,
    Button::X,
    Button::Y,
    Button::Back,
    Button::Guide,
    Button::Start,
    Button::LeftStick,
    Button::RightStick,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::Misc1,
    Button::Paddle1,
    Button::Paddle2,
    Button::Paddle3,
    Button::Paddle4,
    Button::Touchpad,
];
const AXES: [Axis; 6] = [
    Axis::LeftX,
    Axis::LeftY,
    Axis::RightX,
    Axis::RightY,
    Axis::TriggerLeft,
    Axis::TriggerRight,
];
pub struct Profile {
    keys: Vec<(Keycode, u32)>,
    key_axes: Vec<(Keycode, usize, i16)>,
    pad_buttons: Vec<(usize, u32)>,
    pad_triggers: Vec<(usize, u32)>,
    axis_signs: [u8; 4],
    stick_as_dpad: bool,
    allowed_buttons: u32,
    custom_pad: Vec<saved::PadBinding>,
    custom_pad_buttons: u32,
    custom_pad_axes: [u8; 4],
    requested_device: Option<u32>,
}
impl Profile {
    pub fn for_system(system: Option<&str>) -> Result<Self, String> {
        let systems: Value = serde_json::from_str(SYSTEMS).map_err(|e| e.to_string())?;
        let specs: Value = serde_json::from_str(SPECS).map_err(|e| e.to_string())?;
        let layouts: Value = serde_json::from_str(CONTROLS).map_err(|e| e.to_string())?;
        if let Some(id) = system {
            if systems.get(id).is_none() {
                return Err(format!("Unknown input system: {id}"));
            }
        }
        let spec = system.and_then(|id| specs.get(id));
        let layout_name = system
            .and_then(|id| systems[id]["input_profile"].as_str())
            .unwrap_or("default");
        let layout = &layouts[layout_name];
        let mut allowed = spec
            .map(|s| {
                s["primary"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_u64)
                    .fold(0u32, |mask, bit| mask | bit as u32)
            })
            .unwrap_or(65535);
        let analog = spec.is_some_and(|s| s["analog"].as_bool() == Some(true));
        let axis_count = if analog {
            spec.and_then(|s| s["sticks"].as_array())
                .map_or(2, |v| v.len() * 2)
                .min(4)
        } else {
            0
        };
        let mut axis_signs = std::array::from_fn(|index| if index < axis_count { 3 } else { 0 });
        if let Some(extra) = spec.and_then(|s| s["extra_actions"].as_object()) {
            for action in extra.keys() {
                let parts: Vec<_> = action.split(':').collect();
                match parts.as_slice() {
                    ["axis", index, sign] => {
                        let index: usize = index.parse().map_err(|_| "Invalid extra axis")?;
                        if index < 4 {
                            axis_signs[index] |= if *sign == "-1" { 1 } else { 2 };
                        }
                    }
                    ["button", bit] => {
                        allowed |= bit.parse::<u32>().map_err(|_| "Invalid extra button")?;
                    }
                    _ => return Err("Invalid extra input action".into()),
                }
            }
        }
        let mut profile = Self {
            keys: Vec::new(),
            key_axes: Vec::new(),
            pad_buttons: Vec::new(),
            pad_triggers: Vec::new(),
            axis_signs,
            allowed_buttons: allowed,
            custom_pad: Vec::new(),
            custom_pad_buttons: 0,
            custom_pad_axes: [0; 4],
            requested_device: None,
            stick_as_dpad: !analog && layout["stick_as_dpad"].as_bool() == Some(true),
        };
        for (name, bit) in layout["keys"].as_object().ok_or("Invalid key catalog")? {
            let bit = bit.as_u64().ok_or("Invalid key bit")? as u32;
            if bit & allowed != 0 {
                profile.keys.push((keycode(name)?, bit));
            }
        }
        if let Some(axes) = layout["axes"].as_object() {
            for (name, binding) in axes {
                let index = binding[0].as_u64().ok_or("Invalid key axis")? as usize;
                if index < 4 && axis_signs[index] != 0 {
                    profile.key_axes.push((
                        keycode(name)?,
                        index,
                        binding[1].as_i64().ok_or("Invalid key axis value")? as i16,
                    ));
                }
            }
        }
        for (index, bit) in layout["gamepad"].as_object().ok_or("Invalid pad catalog")? {
            let bit = bit.as_u64().ok_or("Invalid pad bit")? as u32;
            let index = index.parse::<usize>().map_err(|e| e.to_string())?;
            if bit & allowed != 0 && index < BUTTONS.len() {
                profile.pad_buttons.push((index, bit));
            }
        }
        if let Some(triggers) = layout["triggers"].as_object() {
            for (index, bit) in triggers {
                let index = index.parse::<usize>().map_err(|e| e.to_string())?;
                let bit = bit.as_u64().ok_or("Invalid trigger bit")? as u32;
                if bit & allowed != 0 && index < 6 {
                    profile.pad_triggers.push((index, bit));
                }
            }
        }
        // Match controller_profiles.defaults: add standard extra inputs only
        // when that action has no console-specific binding and is permitted.
        let unbound = spec.and_then(|s| s["unbound_gamepad"].as_array());
        for (bit, index, trigger) in [
            (4096, 4, true),
            (8192, 5, true),
            (16384, 7, false),
            (32768, 8, false),
        ] {
            if bit & allowed == 0
                || unbound.is_some_and(|v| v.iter().any(|n| n.as_u64() == Some(bit as u64)))
                || profile.pad_buttons.iter().any(|(_, b)| *b == bit)
                || profile.pad_triggers.iter().any(|(_, b)| *b == bit)
            {
                continue;
            }
            if trigger {
                profile.pad_triggers.push((index, bit));
            } else {
                profile.pad_buttons.push((index, bit));
            }
        }
        Ok(profile)
    }
    pub fn sample(
        &self,
        pressed: &HashSet<Keycode>,
        pad_buttons: u32,
        pad_axes: [i16; 6],
    ) -> Snapshot {
        let mut result = Snapshot::default();
        for &(key, bit) in &self.keys {
            if pressed.contains(&key) {
                result.buttons |= bit;
            }
        }
        for &(index, bit) in &self.pad_buttons {
            if pad_buttons & (1 << index) != 0 {
                result.buttons |= bit;
            }
        }
        for &(index, bit) in &self.pad_triggers {
            if pad_axes[index] >= 8000 {
                result.buttons |= bit;
            }
        }
        for (index, &value) in pad_axes[..4].iter().enumerate() {
            if i32::from(value).abs() >= 8000
                && self.axis_signs[index] & (if value < 0 { 1 } else { 2 }) != 0
                && self.custom_pad_axes[index] & (if value < 0 { 1 } else { 2 }) == 0
            {
                result.axes[index] = value;
            }
        }
        if self.stick_as_dpad {
            for (index, negative, positive) in [(0, 32, 16), (1, 64, 128)] {
                if pad_axes[index] <= -8000 && self.custom_pad_buttons & negative == 0 {
                    result.buttons |= negative;
                }
                if pad_axes[index] >= 8000 && self.custom_pad_buttons & positive == 0 {
                    result.buttons |= positive;
                }
            }
        }
        saved::apply_pad(&self.custom_pad, pad_buttons, pad_axes, &mut result);
        let mut key_axes = [0i32; 4];
        let mut overridden = [false; 4];
        for &(key, index, value) in &self.key_axes {
            if pressed.contains(&key) {
                key_axes[index] += i32::from(value);
                overridden[index] = true;
            }
        }
        for index in 0..4 {
            if overridden[index] {
                result.axes[index] = key_axes[index].clamp(-32768, 32767) as i16;
            }
        }
        result
    }
}
fn keycode(name: &str) -> Result<Keycode, String> {
    let name = name.strip_prefix("Key_").ok_or("Invalid key name")?;
    Keycode::from_name(if name == "Enter" {
        "Keypad Enter"
    } else {
        name
    })
    .ok_or_else(|| format!("Unsupported key: {name}"))
}
pub const MAX_PLAYERS: usize = 4;

pub fn player_count(system: &str) -> usize {
    serde_json::from_str::<Value>(SPECS).expect("controller profiles")[system]["players"]
        .as_u64()
        .unwrap_or(1)
        .clamp(1, MAX_PLAYERS as u64) as usize
}

pub fn controller_index(device: &str) -> Option<u32> {
    device
        .strip_prefix("index:")
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse().ok())
        .filter(|i| *i < 256)
}

/// Reserve explicitly selected devices before assigning automatic controllers.
pub fn controller_assignments(
    requested: &[Option<u32>],
    available: &[u32],
) -> [Option<u32>; MAX_PLAYERS] {
    let mut assigned = [None; MAX_PLAYERS];
    for (slot, device) in requested.iter().take(MAX_PLAYERS).enumerate() {
        if let Some(device) = device.filter(|index| available.contains(index)) {
            if !assigned.contains(&Some(device)) {
                assigned[slot] = Some(device);
            }
        }
    }
    for (slot, device) in requested.iter().take(MAX_PLAYERS).enumerate() {
        if device.is_none() {
            assigned[slot] = available
                .iter()
                .copied()
                .find(|index| !assigned.contains(&Some(*index)));
        }
    }
    assigned
}

pub struct Players {
    ports: Vec<Driver>,
}

impl Players {
    pub fn new(
        sdl: &Sdl,
        system: Option<&str>,
        window_id: u32,
        saved: Option<&str>,
    ) -> Result<Self, String> {
        let count = system.map_or(1, player_count);
        let record = saved.and_then(|text| serde_json::from_str::<Value>(text).ok());
        let mut ports = Vec::with_capacity(count);
        for port in 0..count {
            let record = record.as_ref().and_then(|record| {
                if record.is_array() {
                    record.get(port)
                } else {
                    (port == 0).then_some(record)
                }
            });
            let profile =
                Profile::for_player_saved(system, port, record.map(Value::to_string).as_deref())?;
            ports.push(Driver::from_profile(sdl, window_id, profile)?);
        }
        let mut players = Self { ports };
        players.refresh_controllers();
        Ok(players)
    }

    pub fn port_count(&self) -> usize {
        self.ports.len()
    }

    fn refresh_controllers(&mut self) {
        let subsystem = &self.ports[0].subsystem;
        let available: Vec<_> = (0..subsystem.num_joysticks().unwrap_or(0))
            .filter(|index| subsystem.is_game_controller(*index))
            .filter_map(|index| subsystem.open(index).ok().map(|pad| (index, pad)))
            .collect();
        let indices: Vec<_> = available.iter().map(|(index, _)| *index).collect();
        let requested: Vec<_> = self
            .ports
            .iter()
            .map(|port| port.profile.requested_device)
            .collect();
        let assignments = controller_assignments(&requested, &indices);
        for port in &mut self.ports {
            port.controller = None;
        }
        for (index, pad) in available {
            if let Some(slot) = assignments
                .iter()
                .position(|assigned| *assigned == Some(index))
            {
                self.ports[slot].controller = Some(pad);
            }
        }
        for port in &mut self.ports {
            port.sync_controller();
        }
    }

    pub fn event(&mut self, event: &Event) {
        match event {
            Event::ControllerDeviceAdded { .. } | Event::ControllerDeviceRemoved { .. } => {
                self.refresh_controllers();
            }
            _ => {
                for port in &mut self.ports {
                    port.event(event);
                }
            }
        }
    }

    pub fn binds_key(&self, key: Keycode) -> bool {
        self.ports.iter().any(|port| {
            port.profile.keys.iter().any(|&(bound, _)| bound == key)
                || port
                    .profile
                    .key_axes
                    .iter()
                    .any(|&(bound, _, _)| bound == key)
        })
    }

    pub fn snapshot(&self, port: usize) -> Snapshot {
        self.ports[port].snapshot()
    }
}

/// First available standard controller, with keyboard priority for bound axes.
/// Device polling and SDL calls stay on the window thread, never the core/audio.
pub struct Driver {
    profile: Profile,
    pressed: HashSet<Keycode>,
    subsystem: GameControllerSubsystem,
    controller: Option<GameController>,
    focused: bool,
    window_id: u32,
    active_instance: Option<u32>,
    pad_buttons: u32,
    pad_axes: [i16; 6],
}
impl Driver {
    pub fn new(sdl: &Sdl, system: Option<&str>, window_id: u32) -> Result<Self, String> {
        Self::new_saved(sdl, system, window_id, None)
    }
    pub fn new_saved(
        sdl: &Sdl,
        system: Option<&str>,
        window_id: u32,
        record: Option<&str>,
    ) -> Result<Self, String> {
        let profile = Profile::for_system_saved(system, record)?;
        let mut driver = Self::from_profile(sdl, window_id, profile)?;
        driver.find_controller();
        Ok(driver)
    }
    fn from_profile(sdl: &Sdl, window_id: u32, profile: Profile) -> Result<Self, String> {
        Ok(Self {
            profile,
            pressed: HashSet::with_capacity(32),
            subsystem: sdl.game_controller()?,
            controller: None,
            focused: true,
            window_id,
            active_instance: None,
            pad_buttons: 0,
            pad_axes: [0; 6],
        })
    }
    fn find_controller(&mut self) {
        if self
            .controller
            .as_ref()
            .is_some_and(GameController::attached)
        {
            return;
        }
        self.controller = None;
        self.active_instance = None;
        self.pad_buttons = 0;
        self.pad_axes = [0; 6];
        for index in 0..self.subsystem.num_joysticks().unwrap_or(0) {
            if self
                .profile
                .requested_device
                .is_some_and(|requested| requested != index)
            {
                continue;
            }
            if self.subsystem.is_game_controller(index) {
                if let Ok(controller) = self.subsystem.open(index) {
                    self.controller = Some(controller);
                    self.sync_controller();
                    break;
                }
            }
        }
    }
    fn sync_controller(&mut self) {
        self.active_instance = None;
        self.pad_buttons = 0;
        self.pad_axes = [0; 6];
        if let Some(pad) = self.controller.as_ref().filter(|pad| pad.attached()) {
            self.active_instance = Some(pad.instance_id());
            for (index, button) in BUTTONS.iter().enumerate() {
                if pad.button(*button) {
                    self.pad_buttons |= 1 << index;
                }
            }
            for (index, axis) in AXES.iter().enumerate() {
                self.pad_axes[index] = pad.axis(*axis);
            }
        }
    }
    pub fn event(&mut self, event: &Event) {
        match *event {
            Event::KeyDown { window_id, .. }
            | Event::KeyUp { window_id, .. }
            | Event::Window { window_id, .. }
                if window_id != self.window_id =>
            {
                return
            }
            _ => {}
        }
        match *event {
            Event::KeyDown {
                keycode: Some(key),
                repeat: false,
                ..
            } if self.focused => {
                self.pressed.insert(key);
            }
            Event::KeyUp {
                keycode: Some(key), ..
            } => {
                self.pressed.remove(&key);
            }
            Event::Window {
                win_event: WindowEvent::FocusLost,
                ..
            } => {
                self.focused = false;
                self.pressed.clear();
            }
            Event::Window {
                win_event: WindowEvent::FocusGained,
                ..
            } => {
                self.focused = true;
                self.sync_controller();
            }
            Event::ControllerDeviceAdded { .. } => self.find_controller(),
            Event::ControllerDeviceRemoved { which, .. } if self.active_instance == Some(which) => {
                self.controller = None;
                self.active_instance = None;
                self.pad_buttons = 0;
                self.pad_axes = [0; 6];
                self.find_controller();
            }
            Event::ControllerDeviceRemapped { which, .. }
                if self.active_instance == Some(which) =>
            {
                self.sync_controller()
            }
            Event::ControllerAxisMotion {
                which, axis, value, ..
            } if self.active_instance == Some(which) => {
                let index = axis as usize;
                if index < 6 {
                    self.pad_axes[index] = value;
                }
            }
            Event::ControllerButtonDown { which, button, .. }
                if self.active_instance == Some(which) =>
            {
                let index = button as usize;
                if index < BUTTONS.len() {
                    self.pad_buttons |= 1 << index;
                }
            }
            Event::ControllerButtonUp { which, button, .. }
                if self.active_instance == Some(which) =>
            {
                let index = button as usize;
                if index < BUTTONS.len() {
                    self.pad_buttons &= !(1 << index);
                }
            }
            _ => {}
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        if !self.focused {
            return Snapshot::default();
        }
        self.profile
            .sample(&self.pressed, self.pad_buttons, self.pad_axes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    static SDL_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn keys(keys: &[Keycode]) -> HashSet<Keycode> {
        keys.iter().copied().collect()
    }
    #[test]
    #[ignore = "requires reference exported with tools/export_input_reference.py"]
    fn all_system_defaults_match_current_application() {
        let path =
            std::env::var_os("EMULUNA_TEST_INPUT_REFERENCE").expect("EMULUNA_TEST_INPUT_REFERENCE");
        let reference: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(reference["format"].as_u64(), Some(1));
        for group in reference["profiles"].as_array().unwrap() {
            let record = group
                .get("saved")
                .filter(|v| !v.is_null())
                .map(|v| serde_json::to_string(v).unwrap());
            let profile =
                Profile::for_system_saved(group["system"].as_str(), record.as_deref()).unwrap();
            assert_eq!(
                profile.requested_device(),
                group["requested_device"].as_u64().map(|v| v as u32)
            );
            for case in group["cases"].as_array().unwrap() {
                let pressed = case["keys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|name| keycode(name.as_str().unwrap()).unwrap())
                    .collect();
                let axes =
                    std::array::from_fn(|index| case["pad_axes"][index].as_i64().unwrap() as i16);
                let state =
                    profile.sample(&pressed, case["pad_buttons"].as_u64().unwrap() as u32, axes);
                assert_eq!(
                    state.buttons,
                    case["buttons"].as_u64().unwrap() as u32,
                    "system={} record={record:?} case={case}",
                    group["system"]
                );
                assert_eq!(
                    state.axes,
                    std::array::from_fn(|index| case["axes"][index].as_i64().unwrap() as i16),
                    "system={} record={record:?} case={case}",
                    group["system"]
                );
            }
        }
    }
    #[test]
    fn all_console_catalogs_load() {
        let systems: Value = serde_json::from_str(SYSTEMS).unwrap();
        let catalog: Value = serde_json::from_str(CONTROLS).unwrap();
        for id in systems.as_object().unwrap().keys() {
            let profile = Profile::for_system(Some(id)).unwrap();
            let layout = &catalog[systems[id]["input_profile"].as_str().unwrap_or("default")];
            for (name, bit) in layout["keys"].as_object().into_iter().flatten() {
                let bit = bit.as_u64().unwrap() as u32;
                if profile.allowed_buttons & bit != 0 {
                    let state = profile.sample(&keys(&[keycode(name).unwrap()]), 0, [0; 6]);
                    assert_ne!(state.buttons & bit, 0, "{id}: {name}");
                }
            }
            for (name, binding) in layout["axes"].as_object().into_iter().flatten() {
                let axis = binding[0].as_u64().unwrap() as usize;
                if profile.axis_signs[axis] != 0 {
                    let state = profile.sample(&keys(&[keycode(name).unwrap()]), 0, [0; 6]);
                    assert_eq!(
                        i64::from(state.axes[axis]),
                        binding[1].as_i64().unwrap(),
                        "{id}: {name}"
                    );
                }
            }
        }
        assert!(Profile::for_system(Some("unknown")).is_err());
    }
    #[test]
    fn n64_and_dreamcast_keep_their_analog_and_button_mappings() {
        for system in ["n64", "dreamcast"] {
            let profile = Profile::for_system(Some(system)).unwrap();
            let state = profile.sample(
                &keys(&[
                    Keycode::X,
                    Keycode::Left,
                    Keycode::Up,
                    Keycode::Return,
                    Keycode::KpEnter,
                ]),
                0,
                [0; 6],
            );
            assert_eq!(state.buttons, 2 | 8);
            assert_eq!(state.axes[..2], [-32767, -32767]);
            let state = profile.sample(
                &keys(&[Keycode::Left, Keycode::Right]),
                0,
                [12000, 16000, 0, 0, 0, 0],
            );
            assert_eq!(state.axes[..2], [0, 16000]);
        }
        let profile = Profile::for_system(Some("n64")).unwrap();
        assert_eq!(
            profile
                .sample(&keys(&[]), 0, [0, 0, 0, 0, 10000, 10000])
                .buttons,
            4096
        );
    }
    #[test]
    fn digital_console_stick_deadzone_and_duplicate_start_keys() {
        let profile = Profile::for_system(Some("nes")).unwrap();
        assert_eq!(
            profile
                .sample(&keys(&[Keycode::Return]), 0, [7999, 0, 0, 0, 0, 0])
                .buttons,
            8
        );
        assert_eq!(
            profile
                .sample(&keys(&[]), 0, [-8000, 8000, 0, 0, 0, 0])
                .buttons,
            32 | 128
        );
        assert_eq!(profile.sample(&keys(&[Keycode::W]), 0, [0; 6]).buttons, 0);
    }
    #[test]
    fn four_player_defaults_and_saved_keys_are_independent() {
        let _guard = SDL_TEST.lock().unwrap();
        let sdl = sdl2::init().unwrap();
        let mut players = Players::new(
            &sdl, Some("n64"), 7,
            Some(r#"[{},{"keyboard":{"button:64":["key:74"]}},{},{"keyboard":{"button:128":["key:75"]}}]"#),
        ).unwrap();
        assert_eq!(players.port_count(), 4);
        assert_eq!(player_count("dreamcast"), 4);
        assert_eq!(player_count("snes"), 2);
        assert_eq!(player_count("gba"), 1);

        for keycode in [Keycode::J, Keycode::K] {
            players.event(&Event::KeyDown {
                timestamp: 0,
                window_id: 7,
                keycode: Some(keycode),
                scancode: None,
                keymod: sdl2::keyboard::Mod::NOMOD,
                repeat: false,
            });
        }
        assert_eq!(players.snapshot(1).buttons, 64);
        assert_eq!(players.snapshot(2), Snapshot::default());
        assert_eq!(players.snapshot(3).buttons, 128);
    }

    #[test]
    fn automatic_controllers_use_distinct_devices_after_explicit_assignments() {
        assert_eq!(
            controller_assignments(&[None, None, Some(0), None], &[0, 2, 4, 6]),
            [Some(2), Some(4), Some(0), Some(6)],
        );
        assert_eq!(
            controller_assignments(&[None, None, Some(0), Some(0)], &[0, 2]),
            [Some(2), None, Some(0), None],
        );
    }

    #[test]
    fn losing_focus_clears_keys_and_suppresses_gamepad() {
        let _guard = SDL_TEST.lock().unwrap();
        let sdl = sdl2::init().unwrap();
        let mut driver = Driver::new(&sdl, Some("n64"), 0).unwrap();
        driver.pressed.insert(Keycode::Left);
        driver.event(&Event::Window {
            timestamp: 0,
            window_id: 0,
            win_event: WindowEvent::FocusLost,
        });
        assert_eq!(driver.snapshot(), Snapshot::default());
        driver.event(&Event::Window {
            timestamp: 0,
            window_id: 0,
            win_event: WindowEvent::FocusGained,
        });
        assert!(driver.pressed.is_empty());
    }
    #[test]
    fn controller_events_route_only_selected_instance_and_disconnect_releases_input() {
        let _guard = SDL_TEST.lock().unwrap();
        let sdl = sdl2::init().unwrap();
        let mut driver =
            Driver::new_saved(&sdl, Some("n64"), 0, Some(r#"{"device":"index:255"}"#)).unwrap();
        driver.active_instance = Some(7);
        driver.pad_buttons = 0;
        driver.pad_axes = [0; 6];
        driver.event(&Event::ControllerButtonDown {
            timestamp: 0,
            which: 7,
            button: Button::A,
        });
        driver.event(&Event::ControllerAxisMotion {
            timestamp: 0,
            which: 7,
            axis: Axis::LeftX,
            value: 20000,
        });
        driver.event(&Event::ControllerAxisMotion {
            timestamp: 0,
            which: 8,
            axis: Axis::LeftY,
            value: 20000,
        });
        assert_eq!(
            driver.snapshot(),
            Snapshot {
                buttons: 2,
                axes: [20000, 0, 0, 0]
            }
        );
        driver.event(&Event::ControllerButtonUp {
            timestamp: 0,
            which: 7,
            button: Button::A,
        });
        assert_eq!(driver.snapshot().buttons, 0);
        driver.event(&Event::ControllerDeviceRemoved {
            timestamp: 0,
            which: 7,
        });
        assert_eq!(driver.snapshot(), Snapshot::default());
    }
}
