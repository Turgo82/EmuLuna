//! Map EmuLuna console IDs to the metadata provider's system names.
use serde_json::Value;

pub fn system_names(catalog: &Value, system: &str) -> Option<String> {
    let names = catalog[system]["openvgdb_systems"].as_array()?;
    (!names.is_empty()).then_some(())?;
    serde_json::to_string(names).ok()
}
