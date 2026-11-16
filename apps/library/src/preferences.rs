//! Native preferences are independent of the existing application's database.
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
#[derive(Default)]
pub struct Preferences {
    pub values: BTreeMap<String, String>,
}
impl Preferences {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("Could not read native preferences: {e}")),
        };
        let mut bytes = Vec::new();
        file.take(4194305)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 4194304 {
            return Err("Native preferences exceed their size limit".into());
        }
        let json: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Native preferences are damaged: {e}"))?;
        if json["version"].as_u64() != Some(1) {
            return Err("Unsupported native preferences version".into());
        }
        let values: BTreeMap<String, String> =
            serde_json::from_value(json["values"].clone()).map_err(|e| e.to_string())?;
        let result = Self { values };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        if self.values.len() > 16384
            || self.values.iter().any(|(k, v)| {
                k.is_empty()
                    || k.len() > 256
                    || v.len() > 4096
                    || k.contains('\0')
                    || v.contains('\0')
            })
        {
            return Err("Invalid native preference value".into());
        }
        Ok(())
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let bytes =
            serde_json::to_vec_pretty(&serde_json::json!({"version":1,"values":self.values}))
                .map_err(|e| e.to_string())?;
        if bytes.len() > 4194304 {
            return Err("Native preferences exceed their size limit".into());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.values.insert(key.into(), value.into());
    }
}
pub fn library_progress_root(root: &Path, progress: &Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    progress.join(format!(
        "{:x}",
        Sha256::digest(root.to_string_lossy().as_bytes())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_round_trip_preserves_unknown_keys_and_rejects_damage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let mut prefs = Preferences::load(&path).unwrap();
        prefs.set("theme", "luna");
        prefs.set("future.key", "future value");
        prefs.save(&path).unwrap();
        let mut loaded = Preferences::load(&path).unwrap();
        assert_eq!(loaded.get("theme"), Some("luna"));
        loaded.set("theme", "daylight");
        loaded.save(&path).unwrap();
        assert_eq!(
            Preferences::load(&path).unwrap().get("future.key"),
            Some("future value")
        );
        let before = std::fs::read(&path).unwrap();
        loaded.set("bad", "has\0nul");
        assert!(loaded.save(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::write(&path, br#"{"version":999,"values":{}}"#).unwrap();
        assert!(Preferences::load(&path).is_err());
        std::fs::write(&path, vec![b' '; 4194305]).unwrap();
        assert!(Preferences::load(&path).is_err());
    }
}
