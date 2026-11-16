//! Existing per-console/player JSON bindings, translated once at session start.
use super::*;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Button(u32),
    Axis(usize, i32),
}
#[derive(Clone, Copy)]
enum Token {
    Button(usize),
    Axis(usize, i32),
}
pub(super) struct PadBinding {
    action: Action,
    tokens: Vec<Token>,
}
fn sign_bit(sign: i32) -> u8 {
    if sign < 0 {
        1
    } else {
        2
    }
}
fn action(text: &str, profile: &Profile) -> Option<Action> {
    let parts: Vec<_> = text.split(':').collect();
    match parts.as_slice() {
        ["button", bit] => {
            let bit: u32 = bit.parse().ok()?;
            (bit.is_power_of_two() && profile.allowed_buttons & bit != 0)
                .then_some(Action::Button(bit))
        }
        ["axis", index, sign] => {
            let index: usize = index.parse().ok()?;
            let sign: i32 = sign.parse().ok()?;
            (index < 4
                && [-1, 1].contains(&sign)
                && profile.axis_signs[index] & sign_bit(sign) != 0)
                .then_some(Action::Axis(index, sign))
        }
        _ => None,
    }
}
fn token(text: &str) -> Option<Token> {
    let parts: Vec<_> = text.split(':').collect();
    match parts.as_slice() {
        ["button", index] => {
            let index: usize = index.parse().ok()?;
            (index < BUTTONS.len()).then_some(Token::Button(index))
        }
        ["axis", index, sign] => {
            let index: usize = index.parse().ok()?;
            let sign: i32 = sign.parse().ok()?;
            (index < 6 && [-1, 1].contains(&sign)).then_some(Token::Axis(index, sign))
        }
        _ => None,
    }
}
/// Qt's persisted scalar key codes are data, not a Qt runtime dependency.
fn qt_keys(value: i64) -> Option<Vec<Keycode>> {
    let single = match value {
        0x01000000 => Keycode::Escape,
        0x01000001 => Keycode::Tab,
        0x01000003 => Keycode::Backspace,
        0x01000004 => Keycode::Return,
        0x01000005 => Keycode::KpEnter,
        0x01000006 => Keycode::Insert,
        0x01000007 => Keycode::Delete,
        0x01000008 => Keycode::Pause,
        0x01000009 => Keycode::PrintScreen,
        0x01000010 => Keycode::Home,
        0x01000011 => Keycode::End,
        0x01000012 => Keycode::Left,
        0x01000013 => Keycode::Up,
        0x01000014 => Keycode::Right,
        0x01000015 => Keycode::Down,
        0x01000016 => Keycode::PageUp,
        0x01000017 => Keycode::PageDown,
        0x01000020 => return Some(vec![Keycode::LShift, Keycode::RShift]),
        0x01000021 => return Some(vec![Keycode::LCtrl, Keycode::RCtrl]),
        0x01000022 => return Some(vec![Keycode::LGui, Keycode::RGui]),
        0x01000023 => return Some(vec![Keycode::LAlt, Keycode::RAlt]),
        0x01000024 => Keycode::CapsLock,
        0x01000025 => Keycode::NumLockClear,
        0x01000026 => Keycode::ScrollLock,
        0x01000030..=0x01000047 => Keycode::from_name(&format!("F{}", value - 0x01000030 + 1))?,
        48..=57 => {
            return Some(vec![
                Keycode::from_name(&char::from_u32(value as u32)?.to_string())?,
                [
                    Keycode::Kp0,
                    Keycode::Kp1,
                    Keycode::Kp2,
                    Keycode::Kp3,
                    Keycode::Kp4,
                    Keycode::Kp5,
                    Keycode::Kp6,
                    Keycode::Kp7,
                    Keycode::Kp8,
                    Keycode::Kp9,
                ][(value - 48) as usize],
            ])
        }
        1..=0x10ffff => Keycode::from_name(&char::from_u32(value as u32)?.to_string())?,
        _ => return None,
    };
    let keypad = match single {
        Keycode::Plus => Some(Keycode::KpPlus),
        Keycode::Minus => Some(Keycode::KpMinus),
        Keycode::Equals => Some(Keycode::KpEquals),
        Keycode::Period => Some(Keycode::KpPeriod),
        Keycode::Slash => Some(Keycode::KpDivide),
        Keycode::Asterisk => Some(Keycode::KpMultiply),
        _ => None,
    };
    Some(std::iter::once(single).chain(keypad).collect())
}
fn keyboard_tokens(value: &Value) -> Option<Vec<Keycode>> {
    let tokens = value.as_array().filter(|v| v.len() <= 4)?;
    let mut keys = Vec::new();
    for token in tokens {
        let parts: Vec<_> = token.as_str()?.split(':').collect();
        let ["key", code] = parts.as_slice() else {
            return None;
        };
        let code: i64 = code.parse().ok()?;
        if !(1..0x02000000).contains(&code) {
            return None;
        }
        keys.extend(qt_keys(code)?);
    }
    keys.sort_unstable_by_key(|key| i32::from(*key));
    keys.dedup();
    Some(keys)
}
impl Profile {
    /// Invalid entries retain defaults, matching the current app. A valid empty
    /// token array deliberately unbinds that action. No library writes/migrations.
    pub fn for_system_saved(system: Option<&str>, text: Option<&str>) -> Result<Self, String> {
        Self::for_player_saved(system, 0, text)
    }
    pub(super) fn for_player_saved(
        system: Option<&str>,
        player: usize,
        text: Option<&str>,
    ) -> Result<Self, String> {
        let mut profile = Self::for_system(system)?;
        if player > 0 {
            profile.keys.clear();
            profile.key_axes.clear();
        }
        let Some(text) = text else {
            return Ok(profile);
        };
        if text.len() > 65536 {
            return Err("Controller profile exceeds its size limit".into());
        }
        let Ok(record) = serde_json::from_str::<Value>(text) else {
            return Ok(profile);
        };
        if let Some(device) = record["device"].as_str() {
            if device == "auto" {
                profile.requested_device = None;
            } else if let Some(index) = controller_index(device) {
                profile.requested_device = Some(index);
            }
        }
        if let Some(entries) = record["keyboard"].as_object() {
            for (name, tokens) in entries {
                let Some(action) = action(name, &profile) else {
                    continue;
                };
                let Some(keys) = keyboard_tokens(tokens) else {
                    continue;
                };
                match action {
                    Action::Button(bit) => {
                        profile.keys.retain(|(_, old)| *old != bit);
                        profile.keys.extend(keys.into_iter().map(|key| (key, bit)));
                    }
                    Action::Axis(index, sign) => {
                        profile.key_axes.retain(|(_, old, value)| {
                            !(*old == index && i32::from(*value).signum() == sign)
                        });
                        for key in keys {
                            profile.key_axes.push((key, index, (sign * 32767) as i16));
                        }
                    }
                }
            }
        }
        if let Some(entries) = record["gamepad"].as_object() {
            for (name, values) in entries {
                let Some(action) = action(name, &profile) else {
                    continue;
                };
                let Some(values) = values.as_array().filter(|v| v.len() <= 4) else {
                    continue;
                };
                let Some(tokens) = values
                    .iter()
                    .map(|v| v.as_str().and_then(token))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                match action {
                    Action::Button(bit) => {
                        profile.pad_buttons.retain(|(_, old)| *old != bit);
                        profile.pad_triggers.retain(|(_, old)| *old != bit);
                        profile.custom_pad_buttons |= bit;
                    }
                    Action::Axis(index, sign) => profile.custom_pad_axes[index] |= sign_bit(sign),
                }
                profile.custom_pad.push(PadBinding { action, tokens });
            }
        }
        Ok(profile)
    }
    pub fn requested_device(&self) -> Option<u32> {
        self.requested_device
    }
}
pub(super) fn apply_pad(
    bindings: &[PadBinding],
    buttons: u32,
    axes: [i16; 6],
    result: &mut Snapshot,
) {
    if bindings.is_empty() {
        return;
    }
    let mut output = result.axes.map(i32::from);
    for binding in bindings {
        let strength = binding
            .tokens
            .iter()
            .map(|token| match *token {
                Token::Button(index) => {
                    if buttons & (1 << index) != 0 {
                        32767
                    } else {
                        0
                    }
                }
                Token::Axis(index, sign) => (i32::from(axes[index]) * sign).max(0),
            })
            .max()
            .unwrap_or(0);
        if strength < 8000 {
            continue;
        }
        match binding.action {
            Action::Button(bit) => result.buttons |= bit,
            Action::Axis(index, sign) => output[index] += strength * sign,
        }
    }
    result.axes = output.map(|n| n.clamp(-32768, 32767) as i16);
}
#[cfg(test)]
mod tests {
    use super::*;
    fn keys(keys: &[Keycode]) -> HashSet<Keycode> {
        keys.iter().copied().collect()
    }
    #[test]
    fn saved_remap_replaces_defaults_and_empty_arrays_unbind() {
        let profile=Profile::for_system_saved(Some("nes"),Some(r#"{"device":"index:3","keyboard":{"button:1":["key:67"],"button:2":[]},"gamepad":{"button:1":["axis:2:-1"],"button:2":[]}}"#)).unwrap();
        assert_eq!(profile.requested_device(), Some(3));
        assert_eq!(
            profile
                .sample(&keys(&[Keycode::X, Keycode::Z]), 1, [0; 6])
                .buttons,
            0
        );
        assert_eq!(profile.sample(&keys(&[Keycode::C]), 0, [0; 6]).buttons, 1);
        assert_eq!(
            profile
                .sample(&keys(&[]), 0, [0, 0, -8000, 0, 0, 0])
                .buttons,
            1
        );
    }
    #[test]
    fn shared_key_can_drive_multiple_axes_and_keypad_operator_can_drive_button() {
        let profile = Profile::for_system_saved(
            Some("n64"),
            Some(r#"{"keyboard":{"axis:0:-1":["key:67"],"axis:1:-1":["key:67"],"button:4096":["key:45"]}}"#),
        ).unwrap();
        let state = profile.sample(&keys(&[Keycode::C, Keycode::KpMinus]), 0, [0; 6]);
        assert_eq!(state.axes, [-32767, -32767, 0, 0]);
        assert_eq!(state.buttons, 4096);
    }
    #[test]
    fn analog_remaps_override_only_the_selected_direction() {
        let profile=Profile::for_system_saved(Some("n64"),Some(r#"{"keyboard":{"axis:0:-1":["key:67"]},"gamepad":{"axis:0:-1":["button:15"],"button:4096":["axis:5:1"]}}"#)).unwrap();
        assert_eq!(
            profile
                .sample(&keys(&[]), 0, [-20000, 0, 0, 0, 10000, 0])
                .axes[0],
            0
        );
        assert_eq!(
            profile.sample(&keys(&[]), 0, [20000, 0, 0, 0, 0, 0]).axes[0],
            20000
        );
        assert_eq!(profile.sample(&keys(&[]), 1 << 15, [0; 6]).axes[0], -32767);
        assert_eq!(
            profile.sample(&keys(&[Keycode::Left]), 0, [0; 6]).axes[0],
            0
        );
        assert_eq!(
            profile
                .sample(&keys(&[Keycode::C]), 0, [12000, 0, 0, 0, 0, 0])
                .axes[0],
            -32767
        );
        assert_eq!(
            profile
                .sample(&keys(&[]), 0, [0, 0, 0, 0, 0, 10000])
                .buttons,
            4096
        );
    }
    #[test]
    fn invalid_entries_keep_defaults_and_modifier_keys_translate() {
        let profile=Profile::for_system_saved(Some("nes"),Some(r#"{"keyboard":{"button:1":["key:16777248"],"button:2":["bad"]},"gamepad":{"button:2":["button:9999"]},"device":"index:256"}"#)).unwrap();
        assert_eq!(profile.requested_device(), None);
        assert_eq!(
            profile
                .sample(&keys(&[Keycode::LShift, Keycode::Z]), 0, [0; 6])
                .buttons,
            3
        );
        assert_eq!(
            profile.sample(&keys(&[Keycode::RShift]), 0, [0; 6]).buttons,
            1
        );
    }
    #[test]
    fn persisted_number_key_accepts_main_and_keypad_keys() {
        let profile =
            Profile::for_system_saved(Some("nes"), Some(r#"{"keyboard":{"button:1":["key:49"]}}"#))
                .unwrap();
        for key in [Keycode::Num1, Keycode::Kp1] {
            assert_eq!(profile.sample(&keys(&[key]), 0, [0; 6]).buttons, 1);
        }
        assert_eq!(profile.sample(&keys(&[Keycode::Kp0]), 0, [0; 6]).buttons, 0);
    }
}

/// Apply the existing safe-special-button migration in memory. The read-only
/// native preview never writes migration flags back to the user's database.
pub fn library_profile(
    system: &str,
    text: &str,
    migration_complete: bool,
) -> Result<String, String> {
    Profile::for_system(Some(system))?;
    if text.len() > 65536 {
        return Err("Controller profile exceeds its size limit".into());
    }
    let mut record = serde_json::from_str::<Value>(text)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let specs: Value = serde_json::from_str(SPECS).map_err(|e| e.to_string())?;
    let unbound = specs[system]["unbound_gamepad"]
        .as_array()
        .is_some_and(|v| v.iter().any(|v| v.as_u64() == Some(4096)));
    if !migration_complete && unbound {
        if let Some(pad) = record.get_mut("gamepad").and_then(Value::as_object_mut) {
            if pad.get("button:4096") == Some(&serde_json::json!(["axis:4:1"])) {
                pad.remove("button:4096");
            }
        }
    }
    serde_json::to_string(&record).map_err(|e| e.to_string())
}
#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn obsolete_turbo_trigger_is_removed_only_when_old_profile_needs_migration() {
        let old = r#"{"gamepad":{"button:4096":["axis:4:1"],"button:8192":["axis:5:1"]}}"#;
        let changed: Value =
            serde_json::from_str(&library_profile("pce", old, false).unwrap()).unwrap();
        assert!(changed["gamepad"].get("button:4096").is_none());
        assert_eq!(
            changed["gamepad"]["button:8192"],
            serde_json::json!(["axis:5:1"])
        );
        for (system, complete) in [("pce", true), ("nes", false)] {
            let kept: Value =
                serde_json::from_str(&library_profile(system, old, complete).unwrap()).unwrap();
            assert_eq!(
                kept["gamepad"]["button:4096"],
                serde_json::json!(["axis:4:1"])
            );
        }
        assert_eq!(library_profile("pce", "broken", false).unwrap(), "{}");
    }
}
