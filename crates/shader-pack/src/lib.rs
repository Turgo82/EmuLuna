//! Native metadata for build-time-prepared bundled shader stages.
#![forbid(unsafe_code)]
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[derive(Deserialize)]
struct Index {
    version: u32,
    presets: BTreeMap<String, Entry>,
}
#[derive(Deserialize)]
struct Entry {
    name: String,
    file: String,
}
pub struct Pack {
    root: PathBuf,
    index: Index,
}
#[derive(Deserialize)]
pub struct Preset {
    pub name: String,
    pub passes: Vec<Pass>,
    pub textures: BTreeMap<String, Lookup>,
    pub parameters: Vec<Parameter>,
    pub values: BTreeMap<String, f32>,
    pub minimum_viewport: [u32; 2],
}
#[derive(Deserialize)]
pub struct Pass {
    pub vertex: String,
    pub fragment: String,
    pub linear: bool,
    pub wrap: String,
    pub mipmap: bool,
    #[serde(rename = "float")]
    pub float_buffer: bool,
    pub srgb: bool,
    pub alias: String,
    #[serde(rename = "mod")]
    pub frame_count_mod: u32,
    pub scale_x: f64,
    pub scale_y: f64,
    #[serde(rename = "type_x")]
    pub scale_type_x: String,
    #[serde(rename = "type_y")]
    pub scale_type_y: String,
}
#[derive(Deserialize)]
pub struct Lookup {
    pub path: String,
    pub linear: bool,
    pub mipmap: bool,
    pub wrap: String,
}
#[derive(Clone, Deserialize)]
pub struct Parameter {
    pub key: String,
    pub label: String,
    pub default: f32,
    pub minimum: f32,
    pub maximum: f32,
    pub step: f32,
}
impl Pack {
    pub fn open(root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("Shader pack not found: {e}"))?;
        let text = std::fs::read_to_string(root.join("pack.json")).map_err(|e| e.to_string())?;
        let index: Index =
            serde_json::from_str(&text).map_err(|e| format!("Invalid shader pack index: {e}"))?;
        if index.version != 1 {
            return Err("Unsupported shader pack version".into());
        }
        Ok(Self { root, index })
    }
    pub fn keys(&self) -> impl Iterator<Item = (&str, &str)> {
        self.index
            .presets
            .iter()
            .map(|(key, entry)| (key.as_str(), entry.name.as_str()))
    }
    pub fn asset(&self, path: &str) -> Result<PathBuf, String> {
        let path = self
            .root
            .join(path)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path.starts_with(&self.root) || !path.is_file() {
            return Err("Shader asset is outside its pack".into());
        }
        Ok(path)
    }
    pub fn preset(&self, key: &str) -> Result<Preset, String> {
        let entry = self
            .index
            .presets
            .get(key)
            .ok_or_else(|| format!("Unknown shader preset: {key}"))?;
        let preset: Preset = serde_json::from_str(
            &std::fs::read_to_string(self.asset(&entry.file)?).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("Invalid shader preset: {e}"))?;
        if preset.passes.is_empty() || preset.passes.len() > 64 {
            return Err("Invalid shader pass count".into());
        }
        if preset
            .parameters
            .iter()
            .any(|parameter| parameter.minimum > parameter.maximum)
        {
            return Err("Invalid shader parameter range".into());
        }
        for pass in &preset.passes {
            if !pass.scale_x.is_finite()
                || !pass.scale_y.is_finite()
                || pass.scale_x <= 0.
                || pass.scale_y <= 0.
            {
                return Err("Invalid shader scale".into());
            }
        }
        Ok(preset)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires exported assets and EMULUNA_TEST_SHADER_PACK"]
    fn shipped_pack_loads_when_explicitly_supplied() {
        let root = std::env::var("EMULUNA_TEST_SHADER_PACK").expect("Set EMULUNA_TEST_SHADER_PACK");
        let pack = Pack::open(Path::new(&root)).unwrap();
        let keys: Vec<_> = pack.keys().collect();
        assert_eq!(keys.len(), 23);
        for (key, _) in keys {
            let preset = pack.preset(key).unwrap();
            for pass in preset.passes {
                assert!(pass.vertex.starts_with("#version"));
                assert!(pass.fragment.starts_with("#version"));
            }
            for lookup in preset.textures.values() {
                assert!(pack.asset(&lookup.path).unwrap().is_file());
            }
        }
        assert!(pack.asset("../absent").is_err());
    }
}
