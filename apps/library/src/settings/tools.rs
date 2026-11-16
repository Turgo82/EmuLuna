use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
};
pub enum Picker {
    File,
    Folder,
    Save,
}
pub fn pick_path(kind: Picker, title: &str) -> Result<Option<PathBuf>, String> {
    let mode = match kind {
        Picker::File => "--getopenfilename",
        Picker::Folder => "--getexistingdirectory",
        Picker::Save => "--getsavefilename",
    };
    let output = Command::new("kdialog")
        .args(["--title", title, mode])
        .output()
        .or_else(|_| {
            let mut cmd = Command::new("zenity");
            cmd.args(["--file-selection", "--title", title]);
            match kind {
                Picker::Folder => {
                    cmd.arg("--directory");
                }
                Picker::Save => {
                    cmd.args(["--save", "--confirm-overwrite"]);
                }
                _ => {}
            }
            cmd.output()
        })
        .map_err(|e| format!("Could not open the file chooser: {e}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "Invalid file path")?;
    let text = text.trim_end_matches(['\n', '\r']);
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(text)))
    }
}
pub fn atomic_json(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid file location")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn read_json(path: &Path) -> Result<Value, String> {
    match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(4194305)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 4194304 {
                return Err("Manifest exceeds its size limit".into());
            }
            let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if !value.is_object() {
                return Err("Invalid core manifest".into());
            }
            Ok(value)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}
pub fn copy_theme(env: &Environment<'_>, name: &str) -> Result<String, String> {
    let id = name.trim().to_lowercase().replace(' ', "-");
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || ["classic", "luna", "daylight", "system"].contains(&id.as_str())
    {
        return Err("Choose a unique theme name using letters, numbers and spaces.".into());
    }
    let root = env
        .preferences_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("themes");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let destination = root.join(&id);
    if destination.exists() {
        return Err("A theme already uses this name.".into());
    }
    let source = theme_root(env.data, env.preferences_path, &env.theme.id).join(&env.theme.id);
    let work = tempfile::tempdir_in(&root).map_err(|e| e.to_string())?;
    copy_theme_files(&source, work.path(), 0)?;
    let mut theme = read_json(&work.path().join("theme.json"))?;
    theme["id"] = json!(id);
    theme["name"] = json!(name.trim());
    atomic_json(&work.path().join("theme.json"), &theme)?;
    fs::rename(work.path(), &destination).map_err(|e| e.to_string())?;
    Ok(id)
}
fn copy_theme_files(source: &Path, target: &Path, depth: usize) -> Result<(), String> {
    if depth > 8 {
        return Err("Theme folders are nested too deeply".into());
    }
    for item in fs::read_dir(source).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let metadata = item.file_type().map_err(|e| e.to_string())?;
        if metadata.is_symlink() {
            return Err("Theme copies cannot include symbolic links".into());
        }
        let destination = target.join(item.file_name());
        if metadata.is_dir() {
            fs::create_dir(&destination).map_err(|e| e.to_string())?;
            copy_theme_files(&item.path(), &destination, depth + 1)?;
        } else if metadata.is_file() {
            if item.metadata().map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
                return Err("A theme asset exceeds its size limit".into());
            }
            fs::copy(item.path(), destination).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
#[derive(Default)]
pub struct Tools {
    task: Option<mpsc::Receiver<Result<String, String>>>,
    pub cores_changed: bool,
    catalog_id: String,
    confirm_remove: bool,
}
impl Tools {
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
    pub fn poll(&mut self, _ctx: &egui::Context, _prefs: &mut Preferences, message: &mut String) {
        if let Some(task) = &self.task {
            match task.try_recv() {
                Ok(result) => {
                    self.task = None;
                    match result {
                        Ok(text) => {
                            *message = text;
                            self.cores_changed = true;
                        }
                        Err(e) => *message = e,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.task = None;
                    *message = "Core task stopped unexpectedly.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        if self.task.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.task = Some(rx);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        });
    }
    pub fn cores(
        &mut self,
        ui: &mut egui::Ui,
        env: &Environment<'_>,
        console: &str,
        message: &mut String,
    ) {
        heading(ui, "Install and manage cores");
        let catalog: Value =
            serde_json::from_str(include_str!("../../../../data/cores.json")).expect("catalog");
        let mut compatible: Vec<_> = catalog
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, core)| {
                core["systems"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|s| s.as_str() == Some(console)))
            })
            .collect();
        let recommended = emuluna_launch_config::recommended_core(console);
        compatible.sort_by_key(|(id, _)| Some(id.as_str()) != recommended);
        let label = |id: &str, core: &Value| {
            let name = core["name"].as_str().unwrap_or(id);
            if Some(id) == recommended {
                format!("{name} (Recommended)")
            } else {
                name.to_owned()
            }
        };
        if !compatible.iter().any(|(id, _)| **id == self.catalog_id) {
            self.catalog_id = compatible
                .first()
                .map_or(String::new(), |(id, _)| (*id).clone());
            self.confirm_remove = false;
        }
        egui::ComboBox::from_id_salt("manage-catalog-core")
            .width(300.)
            .selected_text(if self.catalog_id.is_empty() {
                "Choose a core".into()
            } else {
                label(&self.catalog_id, &catalog[&self.catalog_id])
            })
            .show_ui(ui, |ui| {
                for (id, core) in &compatible {
                    ui.selectable_value(
                        &mut self.catalog_id,
                        (*id).clone(),
                        label(id, core),
                    );
                }
            });
        ui.add_enabled_ui(!env.playing&&self.task.is_none(),|ui|{
            ui.horizontal_wrapped(|ui|{
                if ui.button("Download / update").clicked(){let root=env.library.root.clone();let id=self.catalog_id.clone();self.start(ui.ctx(),move||download_core(&root,&id));*message="Downloading from the official Libretro build server…".into();}
                if ui.button("Import core…").clicked(){let root=env.library.root.clone();let id=self.catalog_id.clone();self.start(ui.ctx(),move||{let Some(path)=pick_path(Picker::File,"Import Linux libretro core")?else{return Ok("Core import cancelled.".into());};let mut data=Vec::new();File::open(path).map_err(|e|e.to_string())?.take(128*1024*1024+1).read_to_end(&mut data).map_err(|e|e.to_string())?;install_core(&root,&id,&data,None)?;Ok("Core imported. Select it above to use it.".into())});}
                if ui.button("Remove installed core…").clicked(){self.confirm_remove=true;}
            });
            if self.confirm_remove{ui.label(format!("Remove {} from the installed list? Retained builds and saved progress are kept.",catalog[&self.catalog_id]["name"].as_str().unwrap_or(&self.catalog_id)));ui.horizontal(|ui|{if ui.button("Cancel").clicked(){self.confirm_remove=false;}
if ui.button("Remove").clicked(){let root=env.library.root.clone();let id=self.catalog_id.clone();self.confirm_remove=false;self.start(ui.ctx(),move||{let _lock=manager_lock(&root)?;let path=root.join("cores/installed.json");let mut manifest=read_json(&path)?;let record=manifest.as_object_mut().unwrap().remove(&id).ok_or("This core is not installed")?;retain_history(&root,&id,&record)?;atomic_json(&path,&manifest)?;Ok("Core removed from the installed list. Its retained binary and saved progress were kept.".into())});}});}
            if ui.button("Restore previous build").clicked(){let root=env.library.root.clone();let id=self.catalog_id.clone();self.start(ui.ctx(),move||{let _lock=manager_lock(&root)?;let path=root.join("cores/installed.json");let mut manifest=read_json(&path)?;let history=read_json(&root.join("cores/history.json"))?;let current=manifest[&id]["sha256"].as_str();let previous=history[&id].as_array().and_then(|a|a.iter().find(|v|v["sha256"].as_str()!=current&&v["path"].as_str().is_some_and(|p|root.join(p).is_file()))).ok_or("No previous build is available")?.clone();let p=previous["path"].as_str().ok_or("Invalid retained build")?;let path_to_core=root.join(p).canonicalize().map_err(|e|e.to_string())?;if !path_to_core.starts_with(root.join("cores").canonicalize().map_err(|e|e.to_string())?){return Err("Retained core leaves its folder".into());}
if emuluna_state_file::digest_file(&path_to_core)?!=previous["sha256"].as_str().ok_or("Missing checksum")?{return Err("The retained core checksum does not match".into());}
if manifest.get(&id).is_some(){retain_history(&root,&id,&manifest[&id])?;}manifest[&id]=previous;atomic_json(&path,&manifest)?;Ok("Previous core build restored.".into())});}
        });
        if self.task.is_some() {
            ui.spinner();
            ui.label("Working in the background…");
        }
        if env.playing {
            ui.weak("Close the running game before changing installed cores.");
        }
        ui.weak("Core and option changes apply the next time a game starts. Retained builds preserve compatibility with existing save states.");
    }
}
fn manager_lock(root: &Path) -> Result<File, String> {
    let directory = root.join("cores");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(".install.lock"))
        .map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|_| "Another native core operation is running")?;
    Ok(file)
}
fn retain_history(root: &Path, id: &str, record: &Value) -> Result<(), String> {
    let path = root.join("cores/history.json");
    let mut history = read_json(&path)?;
    if !history[id].is_array() {
        history[id] = json!([]);
    }
    let records = history[id].as_array_mut().unwrap();
    if !records.iter().any(|r| r["sha256"] == record["sha256"]) {
        records.insert(0, record.clone());
    }
    atomic_json(&path, &history)
}
fn install_core(
    root: &Path,
    id: &str,
    bytes: &[u8],
    build: Option<(&str, &str)>,
) -> Result<(), String> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
    {
        return Err("Invalid core identifier".into());
    }
    let machine = if cfg!(target_arch = "x86_64") {
        62
    } else if cfg!(target_arch = "aarch64") {
        183
    } else {
        return Err("Unsupported core architecture".into());
    };
    if bytes.len() < 64
        || bytes.len() > 128 * 1024 * 1024
        || &bytes[..6] != b"\x7fELF\x02\x01"
        || u16::from_le_bytes([bytes[18], bytes[19]]) != machine
        || u16::from_le_bytes([bytes[16], bytes[17]]) != 3
    {
        return Err("Choose a 64-bit Linux core built for this computer.".into());
    }
    let catalog: Value = serde_json::from_str(include_str!("../../../../data/cores.json"))
        .map_err(|e| e.to_string())?;
    let core = catalog.get(id).ok_or("Unknown catalog core")?;
    let _lock = manager_lock(root)?;
    let digest = format!("{:x}", Sha256::digest(bytes));
    let directory = root.join("cores").join(id);
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    if !directory
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(
            root.join("cores")
                .canonicalize()
                .map_err(|e| e.to_string())?,
        )
    {
        return Err("The core destination leaves the cores folder".into());
    }
    let path = directory.join(format!("{digest}.so"));
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("The managed core file cannot be a symbolic link".into());
    }
    if !path.is_file() || emuluna_state_file::digest_file(&path)? != digest {
        let mut temp = tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
        temp.write_all(bytes).map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        verify_exports(temp.path())?;
        temp.persist(&path).map_err(|e| e.to_string())?;
    }
    if emuluna_state_file::digest_file(&path)? != digest {
        return Err("The retained core file is damaged; it was not overwritten.".into());
    }
    verify_exports(&path)?;
    let manifest_path = root.join("cores/installed.json");
    let mut manifest = read_json(&manifest_path)?;
    if manifest.get(id).is_some() {
        retain_history(root, id, &manifest[id])?;
    }
    let record = json!({"id":id,"catalog_id":id,"name":core["name"],"version":if build.is_some(){"Buildbot"}else{"Imported"},"systems":core["systems"],"path":path.strip_prefix(root).map_err(|e|e.to_string())?,"sha256":digest,"source":if build.is_some(){"buildbot"}else{"import"},"extensions":core["extensions"],"build":build.map(|b|b.0),"crc":build.map(|b|b.1),"installed":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.,|d|d.as_secs_f64())});
    manifest[id] = record;
    atomic_json(&manifest_path, &manifest)
}
fn verify_exports(path: &Path) -> Result<(), String> {
    let mut child = Command::new("nm")
        .args(["-D", "--defined-only"])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not inspect the core's exports (binutils is required): {e}"))?;
    let mut output = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("Could not read core exports")?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut output);
    if read.is_err() || output.len() > 4 * 1024 * 1024 {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Core export table exceeds its size limit".into());
    }
    if !child.wait().map_err(|e| e.to_string())?.success() {
        return Err("The selected file is not a readable Linux shared library".into());
    }
    let text = String::from_utf8_lossy(&output);
    for required in [
        "retro_api_version",
        "retro_init",
        "retro_deinit",
        "retro_get_system_info",
        "retro_get_system_av_info",
        "retro_load_game",
        "retro_unload_game",
        "retro_run",
        "retro_set_environment",
        "retro_set_video_refresh",
        "retro_set_audio_sample",
        "retro_set_audio_sample_batch",
        "retro_set_input_poll",
        "retro_set_input_state",
    ] {
        if !text.lines().any(|line| {
            line.split_whitespace()
                .last()
                .and_then(|name| name.split('@').next())
                == Some(required)
        }) {
            return Err(format!(
                "This file is missing the libretro function {required}"
            ));
        }
    }
    Ok(())
}
fn fetch(url: &str, path: &Path, max: u64) -> Result<(), String> {
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "120",
            "--max-filesize",
            &max.to_string(),
            "--output",
        ])
        .arg(path)
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(
            "The official core download failed. Check your connection and try again.".into(),
        );
    }
    Ok(())
}
pub(crate) fn download_core(root: &Path, id: &str) -> Result<String, String> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err("Automatic downloads currently support Linux x86_64. Import a matching native core instead.".into());
    }
    let work = tempfile::tempdir().map_err(|e| e.to_string())?;
    let base = "https://buildbot.libretro.com/nightly/linux/x86_64/latest/";
    let index = work.path().join("index");
    fetch(&format!("{base}.index-extended"), &index, 4 * 1024 * 1024)?;
    let filename = format!("{id}_libretro.so");
    let text = fs::read_to_string(index).map_err(|e| e.to_string())?;
    let (build, crc) = text
        .lines()
        .find_map(|line| {
            let parts: Vec<_> = line.split_whitespace().collect();
            (parts.len() == 3 && parts[2] == format!("{filename}.zip"))
                .then(|| (parts[0].to_owned(), parts[1].to_owned()))
        })
        .ok_or("This core is unavailable from the official build server")?;
    let archive = work.path().join("core.zip");
    fetch(&format!("{base}{filename}.zip"), &archive, 64 * 1024 * 1024)?;
    let mut child = Command::new("unzip")
        .arg("-p")
        .arg(&archive)
        .arg(&filename)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("Could not read core archive")?
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes);
    if bytes.len() > 128 * 1024 * 1024 || read.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Downloaded core exceeds its size limit".into());
    }
    if !child.wait().map_err(|e| e.to_string())?.success() {
        return Err("The core archive could not be extracted".into());
    }
    if format!("{:08x}", crc32fast::hash(&bytes)) != crc.to_lowercase() {
        return Err("Downloaded core does not match the official index. Try again.".into());
    }
    install_core(root, id, &bytes, Some((&build, &crc)))?;
    Ok("Core downloaded and installed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_cores_without_replacing_the_manifest() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cores/installed.json");
        let original = json!({"keep":{"name":"Existing core"}});
        atomic_json(&path, &original).unwrap();
        assert!(install_core(root.path(), "nestopia", b"not a core", None).is_err());
        assert_eq!(read_json(&path).unwrap(), original);
    }
    #[test]
    #[ignore = "requires the compiled CC0 native input fixture"]
    fn installs_verified_fixture_and_retains_previous_build() {
        let bytes =
            std::fs::read(std::env::var_os("EMULUNA_CORE_FIXTURE").expect("fixture path")).unwrap();
        let root = tempfile::tempdir().unwrap();
        install_core(root.path(), "nestopia", &bytes, None).unwrap();
        let installed =
            emuluna_launch_config::resolve_installed(root.path(), "nes", "nestopia").unwrap();
        assert_eq!(installed.sha256, format!("{:x}", Sha256::digest(&bytes)));
        let mut newer = bytes.clone();
        newer.push(0);
        install_core(root.path(), "nestopia", &newer, None).unwrap();
        let history = read_json(&root.path().join("cores/history.json")).unwrap();
        assert_eq!(history["nestopia"][0]["sha256"], installed.sha256);
        assert!(installed.path.is_file());
    }
}
