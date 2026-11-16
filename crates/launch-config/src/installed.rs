//! Read-only core selection and integrity checks for native library launching.
use serde_json::Value;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct InstalledCore {
    /// Installed build identity, including locally imported variants.
    pub id: String,
    /// Defaults come from this catalog entry; it may differ from the build ID.
    pub catalog_id: Option<String>,
    pub name: String,
    pub path: PathBuf,
    pub sha256: String,
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
}
fn read_manifest(path: &Path) -> Result<Value, String> {
    let file = File::open(path).map_err(|e| format!("Could not read installed cores: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("The installed core list is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("The installed core list exceeds its size limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("The installed core list is damaged: {e}"))
}
#[derive(Clone, Debug)]
pub struct CoreChoice {
    pub id: String,
    pub name: String,
    pub catalog_id: String,
}
pub fn recommended_core(system: &str) -> Option<&'static str> {
    static SYSTEMS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    SYSTEMS
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../data/systems.json"))
                .expect("built-in system catalog")
        })
        .get(system)?["default_core"]
        .as_str()
}

/// Interpret saved selections from older builds.
pub fn explicit_core(selection: &str) -> Option<&str> {
    (!matches!(selection, "" | "auto" | "builtin")).then_some(selection)
}

/// Read-only choices for settings. Binary integrity is checked when launching.
/// The recommended build is first when installed.
pub fn installed_choices(root: &Path, system: &str) -> Result<Vec<CoreChoice>, String> {
    let manifest = read_manifest(&root.join("cores/installed.json"))?;
    let records = manifest
        .as_object()
        .ok_or("The installed core list is damaged")?;
    let default = recommended_core(system);
    let mut choices: Vec<_> = records
        .iter()
        .filter(|(id, record)| {
            valid_id(id)
                && record["id"].as_str() == Some(id.as_str())
                && record["systems"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|s| s.as_str() == Some(system)))
        })
        .map(|(id, record)| CoreChoice {
            id: id.clone(),
            name: record["name"].as_str().unwrap_or(id).to_owned(),
            catalog_id: record["catalog_id"].as_str().unwrap_or(id).to_owned(),
        })
        .collect();
    if let Some(index) = choices.iter().position(|c| Some(c.id.as_str()) == default) {
        let preferred = choices.remove(index);
        choices.insert(0, preferred);
    }
    Ok(choices)
}
/// Match the current application's preferred/explicit selection without
/// downloading, migrating, creating directories, or modifying the manifest.
/// JSON insertion order preserves the existing fallback choice.
pub fn resolve_installed(
    root: &Path,
    system: &str,
    selection: &str,
) -> Result<InstalledCore, String> {
    let default = recommended_core(system).ok_or("Unknown console")?;
    let root = root
        .canonicalize()
        .map_err(|e| format!("Library folder is unavailable: {e}"))?;
    let manifest = read_manifest(&root.join("cores/installed.json"))?;
    let records = manifest
        .as_object()
        .ok_or("The installed core list is damaged")?;
    let (key, record) = if explicit_core(selection).is_none() {
        records
            .get_key_value(default)
            .filter(|(_, r)| supports(r, system))
            .or_else(|| records.iter().find(|(_, r)| supports(r, system)))
            .ok_or("No compatible core is installed. Download one in Settings → Cores.")?
    } else {
        records.get_key_value(selection).filter(|(_,r)|supports(r, system))
            .ok_or("The selected core is unavailable for this console. Choose another core in Settings.")?
    };
    validate_record(&root, key, record)
}

/// Resolve the exact core recorded in a state, including builds retained after an update.
pub fn resolve_state_core(
    root: &Path,
    system: &str,
    id: &str,
    digest: &str,
) -> Result<InstalledCore, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Library folder is unavailable: {e}"))?;
    let matches =
        |record: &Value| record["sha256"].as_str() == Some(digest) && supports(record, system);
    if let Some(record) = read_manifest(&root.join("cores/installed.json"))
        .ok()
        .as_ref()
        .and_then(|manifest| manifest.get(id))
        .filter(|record| matches(record))
    {
        return validate_record(&root, id, record);
    }
    let history = read_manifest(&root.join("cores/history.json")).ok();
    let record = history
        .as_ref()
        .and_then(|manifest| manifest[id].as_array())
        .and_then(|records| records.iter().find(|record| matches(record)))
        .ok_or(
            "This save requires an older core build that is no longer available in this library.",
        )?;
    validate_record(&root, id, record)
}

fn supports(record: &Value, system: &str) -> bool {
    record["systems"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(system)))
}

fn validate_record(root: &Path, key: &str, record: &Value) -> Result<InstalledCore, String> {
    let id = record["id"]
        .as_str()
        .filter(|id| valid_id(id) && *id == key)
        .ok_or("The selected core has an invalid identity")?;
    let catalog_id = record
        .get("catalog_id")
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_str()
                .filter(|id| valid_id(id))
                .map(str::to_owned)
                .ok_or("The selected core has an invalid catalog identity")
        })
        .transpose()?;
    let digest = record["sha256"]
        .as_str()
        .filter(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or("The selected core has an invalid checksum")?;
    let relative = record["path"]
        .as_str()
        .ok_or("The selected core has no file path")?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|_| "The selected core is missing. Download it again in Settings.")?;
    if !path.starts_with(root.join("cores")) {
        return Err("The selected core file is outside the installed cores folder".into());
    }
    let metadata = path.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 128 * 1024 * 1024 {
        return Err("The selected core file is invalid or exceeds its size limit".into());
    }
    if emuluna_state_file::digest_file(&path)? != digest {
        return Err("The selected core file is damaged. Download it again in Settings.".into());
    }
    Ok(InstalledCore {
        id: id.into(),
        catalog_id,
        name: record["name"].as_str().unwrap_or(id).into(),
        path,
        sha256: digest.into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn install(root: &Path, id: &str, system: &str) -> Value {
        let path = root.join("cores").join(format!("{id}.so"));
        std::fs::write(&path, b"original diagnostic binary").unwrap();
        json!({"id":id,"catalog_id":"nestopia","name":id,"systems":[system],"path":format!("cores/{id}.so"),"sha256":emuluna_state_file::digest_file(&path).unwrap()})
    }
    #[test]
    fn state_can_resume_with_a_verified_retained_core() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("cores");
        std::fs::create_dir(&folder).unwrap();
        let mut retained = install(root.path(), "nestopia", "nes");
        std::fs::rename(folder.join("nestopia.so"), folder.join("retained.so")).unwrap();
        retained["path"] = json!("cores/retained.so");
        let digest = retained["sha256"].as_str().unwrap().to_owned();
        let mut active = install(root.path(), "nestopia", "nes");
        std::fs::write(folder.join("nestopia.so"), b"new binary").unwrap();
        active["sha256"] =
            json!(emuluna_state_file::digest_file(&folder.join("nestopia.so")).unwrap());
        std::fs::write(
            folder.join("installed.json"),
            json!({"nestopia": active}).to_string(),
        )
        .unwrap();
        std::fs::write(
            folder.join("history.json"),
            json!({"nestopia": [retained]}).to_string(),
        )
        .unwrap();
        assert!(
            resolve_state_core(root.path(), "nes", "nestopia", &digest)
                .unwrap()
                .path
                .ends_with("retained.so")
        );
        assert!(
            resolve_installed(root.path(), "nes", "nestopia")
                .unwrap()
                .path
                .ends_with("nestopia.so")
        );
        std::fs::write(folder.join("retained.so"), b"damaged").unwrap();
        assert!(resolve_state_core(root.path(), "nes", "nestopia", &digest).is_err());
    }

    #[test]
    fn selection_preserves_preference_explicit_choice_and_manifest_order() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("cores")).unwrap();
        let mut manifest = serde_json::Map::new();
        manifest.insert("z_first".into(), install(root.path(), "z_first", "nes"));
        manifest.insert("a_second".into(), install(root.path(), "a_second", "nes"));
        let write = |m: &serde_json::Map<String, Value>| {
            std::fs::write(
                root.path().join("cores/installed.json"),
                serde_json::to_vec(m).unwrap(),
            )
            .unwrap()
        };
        write(&manifest);
        assert_eq!(
            resolve_installed(root.path(), "nes", "auto").unwrap().id,
            "z_first"
        );
        assert_eq!(
            resolve_installed(root.path(), "nes", "a_second")
                .unwrap()
                .id,
            "a_second"
        );
        assert!(resolve_installed(root.path(), "snes", "a_second").is_err());
        assert!(resolve_installed(root.path(), "nes", "missing").is_err());
        manifest.insert("nestopia".into(), install(root.path(), "nestopia", "nes"));
        write(&manifest);
        for selection in ["auto", "builtin", ""] {
            assert_eq!(
                resolve_installed(root.path(), "nes", selection).unwrap().id,
                "nestopia"
            );
        }
        let choices = installed_choices(root.path(), "nes").unwrap();
        assert_eq!(choices[0].id, "nestopia");
        assert_eq!(
            choices
                .iter()
                .find(|c| c.id == "z_first")
                .unwrap()
                .catalog_id,
            "nestopia"
        );
        assert!(installed_choices(root.path(), "snes").unwrap().is_empty());
        let before = std::fs::read(root.path().join("cores/installed.json")).unwrap();
        resolve_installed(root.path(), "nes", "auto").unwrap();
        assert_eq!(
            std::fs::read(root.path().join("cores/installed.json")).unwrap(),
            before
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn corrupt_missing_and_escaping_core_paths_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("cores")).unwrap();
        let mut record = install(root.path(), "nestopia", "nes");
        let manifest = root.path().join("cores/installed.json");
        let write = |record: &Value| {
            std::fs::write(
                &manifest,
                serde_json::to_vec(&json!({"nestopia":record})).unwrap(),
            )
            .unwrap()
        };
        write(&record);
        std::fs::write(root.path().join("cores/nestopia.so"), b"changed").unwrap();
        assert!(
            resolve_installed(root.path(), "nes", "auto")
                .unwrap_err()
                .contains("damaged")
        );
        std::fs::remove_file(root.path().join("cores/nestopia.so")).unwrap();
        assert!(
            resolve_installed(root.path(), "nes", "auto")
                .unwrap_err()
                .contains("missing")
        );
        let outside = root.path().join("outside.so");
        std::fs::write(&outside, b"diagnostic").unwrap();
        record["path"] = json!("cores/../outside.so");
        write(&record);
        assert!(
            resolve_installed(root.path(), "nes", "auto")
                .unwrap_err()
                .contains("outside")
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.path().join("cores/link.so")).unwrap();
            record["path"] = json!("cores/link.so");
            write(&record);
            assert!(
                resolve_installed(root.path(), "nes", "auto")
                    .unwrap_err()
                    .contains("outside")
            );
        }
        std::fs::write(&manifest, vec![b' '; 4 * 1024 * 1024 + 1]).unwrap();
        assert!(
            resolve_installed(root.path(), "nes", "auto")
                .unwrap_err()
                .contains("size limit")
        );
    }
}
