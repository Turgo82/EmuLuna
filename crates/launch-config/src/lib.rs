//! Reuse the existing catalog instead of maintaining divergent Rust defaults.
#![forbid(unsafe_code)]
mod installed;
pub use installed::{
    explicit_core, installed_choices, recommended_core, resolve_installed, resolve_state_core,
    CoreChoice, InstalledCore,
};
use serde_json::Value;
use std::collections::BTreeMap;
pub type Options = BTreeMap<String, String>;
#[derive(Clone, Copy)]
pub enum Renderer {
    Software,
    OpenGl,
}
const CATALOG: &str = include_str!("../../../data/cores.json");
fn merge(target: &mut Options, value: Option<&Value>) -> Result<(), String> {
    if let Some(value) = value {
        let object = value.as_object().ok_or("Core options must be an object")?;
        for (key, value) in object {
            let value = value.as_str().ok_or("Core option values must be strings")?;
            if key.is_empty() || key.contains('\0') || value.contains('\0') {
                return Err("Invalid core option key or value".into());
            }
            target.insert(key.clone(), value.to_owned());
        }
    }
    Ok(())
}
pub fn parse_options(text: &str) -> Result<Options, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("Invalid core options JSON: {e}"))?;
    let mut options = Options::new();
    merge(&mut options, Some(&value))?;
    Ok(options)
}
pub fn launch_options(
    core: &str,
    system: &str,
    renderer: Renderer,
    overrides: &Options,
) -> Result<Options, String> {
    let catalog: Value =
        serde_json::from_str(CATALOG).map_err(|e| format!("Invalid built-in core catalog: {e}"))?;
    let record = catalog
        .get(core)
        .ok_or_else(|| format!("Unknown core: {core}"))?;
    let supported = record
        .get("systems")
        .and_then(Value::as_array)
        .is_some_and(|systems| systems.iter().any(|value| value.as_str() == Some(system)));
    if !supported {
        return Err(format!("Core {core} does not support system {system}"));
    }
    if matches!(renderer, Renderer::Software)
        && record.get("hardware_required").and_then(Value::as_bool) == Some(true)
    {
        return Err(format!("Core {core} requires hardware rendering"));
    }
    let mut options = Options::new();
    merge(&mut options, record.get("options"))?;
    merge(
        &mut options,
        record.get("system_options").and_then(|v| v.get(system)),
    )?;
    if let Some(definitions) = record
        .get("runtime_options")
        .and_then(|v| v.get(system))
        .and_then(Value::as_array)
    {
        for definition in definitions {
            let key = definition
                .get("key")
                .and_then(Value::as_str)
                .ok_or("Runtime option has no key")?;
            let value = definition
                .get("default")
                .and_then(Value::as_str)
                .ok_or("Runtime option has no default")?;
            options.insert(key.into(), value.into());
        }
    }
    if let Some(accessories) = record.get("accessories").and_then(Value::as_array) {
        for accessory in accessories {
            let key = accessory
                .get("key")
                .and_then(Value::as_str)
                .ok_or("Accessory has no key")?;
            let count = accessory
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 4) as usize;
            let defaults = accessory.get("defaults").and_then(Value::as_array);
            for player in 0..count {
                let value = defaults
                    .and_then(|values| values.get(player.min(values.len().saturating_sub(1))))
                    .and_then(Value::as_str)
                    .or_else(|| accessory.get("default").and_then(Value::as_str))
                    .ok_or("Accessory has no default")?;
                options.insert(
                    key.replace("{player}", &(player + 1).to_string()),
                    value.into(),
                );
            }
        }
    }
    merge(
        &mut options,
        record.get(match renderer {
            Renderer::Software => "software_options",
            Renderer::OpenGl => "hardware_options",
        }),
    )?;
    for (key, value) in overrides {
        options.insert(key.clone(), value.clone());
    }
    Ok(options)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_console_specific_audio_defaults() {
        let genesis = launch_options(
            "genesis_plus_gx",
            "genesis",
            Renderer::Software,
            &Options::new(),
        )
        .unwrap();
        let cd = launch_options(
            "genesis_plus_gx",
            "segacd",
            Renderer::Software,
            &Options::new(),
        )
        .unwrap();
        assert_eq!(genesis["genesis_plus_gx_audio_filter"], "low-pass");
        assert_eq!(cd["genesis_plus_gx_audio_filter"], "low-pass");
    }
    #[test]
    fn flycast_uses_existing_profile_and_explicit_overrides() {
        let overrides = parse_options(r#"{"reicast_internal_resolution":"1280x960"}"#).unwrap();
        let options = launch_options("flycast", "dreamcast", Renderer::OpenGl, &overrides).unwrap();
        assert_eq!(options["reicast_threaded_rendering"], "disabled");
        assert_eq!(options["reicast_internal_resolution"], "1280x960");
        assert_eq!(options["reicast_alpha_sorting"], "per-triangle (normal)");
        assert!(launch_options("flycast", "dreamcast", Renderer::Software, &overrides).is_err());
        assert!(launch_options("flycast", "nes", Renderer::OpenGl, &overrides).is_err());
        assert!(parse_options(r#"{"bad":42}"#).is_err());
    }
    #[test]
    fn controller_accessory_defaults_match_the_existing_frontend() {
        let options =
            launch_options("parallel_n64", "n64", Renderer::Software, &Options::new()).unwrap();
        assert_eq!(options["parallel-n64-pak1"], "memory");
        assert_eq!(options["parallel-n64-pak2"], "none");
        assert_eq!(options["parallel-n64-pak4"], "none");
    }
}
