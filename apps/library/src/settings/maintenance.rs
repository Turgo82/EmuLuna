use super::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    sync::mpsc,
};
#[derive(Clone)]
struct Firmware {
    path: String,
    used_by: String,
    requirement: String,
    digests: Vec<String>,
    status: String,
    verified: bool,
}
enum ResultData {
    Firmware(Vec<Firmware>),
    Audit(Vec<(String, String, PathBuf)>),
    Preference(String, String),
    Message(String),
}
#[derive(Default)]
pub struct Maintenance {
    pub close_requested: bool,
    task: Option<mpsc::Receiver<Result<ResultData, String>>>,
    firmware: Vec<Firmware>,
    audit: Vec<(String, String, PathBuf)>,
    firmware_scope: String,
    optional: bool,
    all_cores: bool,
    refresh: bool,
    firmware_checked: bool,
    selected: Option<String>,
}
fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("File exceeds its size limit".into());
    }
    Ok(bytes)
}
fn bios_directory(prefs: &Preferences, library: &Library) -> PathBuf {
    PathBuf::from(value(
        prefs,
        library,
        "bios_directory",
        library.root.join("system").to_str().unwrap_or("system"),
    ))
}
impl Maintenance {
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce() -> Result<ResultData, String> + Send + 'static,
    ) {
        if self.task.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.task = Some(rx);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = work();
            let _ = tx.send(result);
            ctx.request_repaint();
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        });
    }
    pub fn poll(&mut self, _ctx: &egui::Context, prefs: &mut Preferences, message: &mut String) {
        if let Some(task) = &self.task {
            match task.try_recv() {
                Ok(result) => {
                    self.task = None;
                    match result {
                        Ok(ResultData::Firmware(rows)) => {
                            if !rows
                                .iter()
                                .any(|row| self.selected.as_ref() == Some(&row.path))
                            {
                                self.selected = rows.first().map(|row| row.path.clone());
                            }
                            self.firmware = rows;
                            *message = "System file check complete.".into();
                        }
                        Ok(ResultData::Audit(rows)) => {
                            *message =
                                format!("Audit complete: {} games need attention.", rows.len());
                            self.audit = rows;
                        }
                        Ok(ResultData::Preference(key, value)) => {
                            prefs.set(&key, value);
                            self.refresh = true;
                            if let Some(id) = key
                                .strip_prefix("game.")
                                .and_then(|key| key.strip_suffix(".rom"))
                            {
                                self.audit.retain(|(game, _, _)| game != id);
                                *message = "ROM reconnected.".into();
                            } else {
                                self.refresh_firmware();
                                *message = "Saved.".into();
                            }
                        }
                        Ok(ResultData::Message(text)) => {
                            self.refresh = true;
                            *message = text;
                        }
                        Err(e) => *message = e,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.task = None;
                    *message = "The background task stopped unexpectedly.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }
    pub fn restore_firmware_defaults(&mut self) {
        self.firmware_scope.clear();
        self.optional = false;
        self.all_cores = false;
        self.selected = None;
        self.refresh_firmware();
    }
    pub fn refresh_firmware(&mut self) {
        self.refresh = true;
        self.firmware_checked = false;
        self.firmware.clear();
    }
    pub fn export_inventory(&mut self, ui: &mut egui::Ui, env: &Environment<'_>) {
        if ui
            .add_enabled(self.task.is_none(), egui::Button::new("Export inventory…"))
            .clicked()
        {
            let games = env.library.games.clone();
            self.start(ui.ctx(), move || {
                let Some(path) =
                    super::tools::pick_path(super::tools::Picker::Save, "Export game inventory")?
                else {
                    return Ok(ResultData::Message("Export cancelled.".into()));
                };
                let records: Vec<_> = games
                    .iter()
                    .map(|g| json!({"id":g.id,"title":g.title,"console":g.system,"rom":g.rom}))
                    .collect();
                super::tools::atomic_json(&path, &json!({"format":1,"games":records}))?;
                Ok(ResultData::Message(format!(
                    "Inventory exported to {}",
                    path.display()
                )))
            });
        }
    }
    pub fn firmware(
        &mut self,
        ui: &mut egui::Ui,
        prefs: &mut Preferences,
        env: &Environment<'_>,
        message: &mut String,
    ) {
        let folder = bios_directory(prefs, env.library);
        ui.horizontal(|ui| {
            ui.label("BIOS / system folder");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Open folder").clicked() {
                    if let Err(error) = std::fs::create_dir_all(&folder)
                        .map_err(|e| e.to_string())
                        .and_then(|_| crate::actions::open_path(folder.clone()))
                    {
                        *message = error;
                    }
                }
                let mut path = folder.display().to_string();
                ui.add(
                    egui::TextEdit::singleline(&mut path)
                        .interactive(false)
                        .desired_width(f32::INFINITY),
                );
            });
        });
        ui.label("System files for the cores you use. Regional BIOS files are alternatives: add the regions you play. Import your own files; originals are kept.");
        let old = (self.firmware_scope.clone(), self.optional, self.all_cores);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("firmware-scope")
                .width((ui.available_width() - 210.).max(120.))
                .selected_text(if self.firmware_scope.is_empty() {
                    "My consoles"
                } else if self.firmware_scope == "all" {
                    "All consoles"
                } else {
                    env.library
                        .consoles
                        .iter()
                        .find(|c| c.id == self.firmware_scope)
                        .map_or(self.firmware_scope.as_str(), |c| c.name.as_str())
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.firmware_scope, String::new(), "My consoles");
                    ui.selectable_value(&mut self.firmware_scope, "all".into(), "All consoles");
                    for c in &env.library.consoles {
                        ui.selectable_value(&mut self.firmware_scope, c.id.clone(), &c.name);
                    }
                });
            egui::ComboBox::from_id_salt("firmware-core-scope")
                .width(190.)
                .selected_text(if self.all_cores {
                    "All compatible cores"
                } else {
                    "Selected cores"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.all_cores, false, "Selected cores");
                    ui.selectable_value(&mut self.all_cores, true, "All compatible cores");
                });
        });
        ui.checkbox(&mut self.optional, "Show optional files and accessories");
        let refresh = self.refresh;
        if (refresh
            || self.refresh
            || old != (self.firmware_scope.clone(), self.optional, self.all_cores)
            || !self.firmware_checked)
            && self.task.is_none()
        {
            self.refresh = false;
            self.firmware_checked = true;
            let root = env.library.root.clone();
            let folder = folder.clone();
            let scope = self.firmware_scope.clone();
            let optional = self.optional;
            let all = self.all_cores;
            let systems: Vec<_> = env
                .library
                .consoles
                .iter()
                .filter(|c| {
                    if scope == "all" {
                        true
                    } else if scope.is_empty() {
                        env.library.games.iter().any(|g| g.system == c.id)
                    } else {
                        scope == c.id
                    }
                })
                .map(|c| c.id.clone())
                .collect();
            let mut settings = env.library.settings.clone();
            settings.extend(prefs.values.clone());
            self.start(ui.ctx(), move || {
                Ok(ResultData::Firmware(check_firmware(
                    &root, &folder, &systems, optional, all, &settings,
                )?))
            });
        }
        if self.task.is_some() {
            ui.spinner();
            ui.label("Checking system files…");
        }
        let green = VERIFIED_TEXT;
        let widths = [0.21, 0.30, 0.39, 0.10];
        let table_width = ui.available_width();
        let line = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let header = ui
            .allocate_exact_size(egui::vec2(table_width, 30.), egui::Sense::hover())
            .0;
        ui.painter()
            .rect_filled(header, 0., ui.visuals().widgets.inactive.bg_fill);
        let mut x = header.left();
        for (label, fraction) in ["File", "Used by", "Requirement", "Status"]
            .into_iter()
            .zip(widths)
        {
            ui.painter().text(
                egui::pos2(x + table_width * fraction * 0.5, header.center().y),
                egui::Align2::CENTER_CENTER,
                label,
                fonts::bold(13.),
                ui.visuals().text_color(),
            );
            x += table_width * fraction;
        }
        egui::Frame::new()
            .stroke(egui::Stroke::new(1_f32, line))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("system-files-rows")
                    .max_height(250.)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        ui.spacing_mut().item_spacing.y = 0.;
                        for row in &self.firmware {
                            let selected = self.selected.as_deref() == Some(&row.path);
                            let color = if selected {
                                ui.visuals().selection.stroke.color
                            } else {
                                ui.visuals().text_color()
                            };
                            let values = [&row.path, &row.used_by, &row.requirement, &row.status];
                            let mut galleys = Vec::new();
                            for (index, value) in values.into_iter().enumerate() {
                                galleys.push(
                                    ui.painter().layout(
                                        value.clone(),
                                        egui::FontId::proportional(13.),
                                        if index == 3 && row.verified {
                                            green
                                        } else {
                                            color
                                        },
                                        (width * widths[index]
                                            - if index == 0 { 32. } else { 12. })
                                        .max(35.),
                                    ),
                                );
                            }
                            let height = galleys
                                .iter()
                                .map(|galley| galley.size().y + 10.)
                                .fold(34_f32, f32::max);
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(width, height),
                                egui::Sense::click(),
                            );
                            if selected || response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    0.,
                                    if selected {
                                        ui.visuals().selection.bg_fill
                                    } else {
                                        ui.visuals().widgets.hovered.weak_bg_fill
                                    },
                                );
                            }
                            if response.clicked() {
                                self.selected = Some(row.path.clone());
                            }
                            let check = egui::Rect::from_center_size(
                                rect.left_center() + egui::vec2(12., 0.),
                                egui::vec2(15., 15.),
                            );
                            paint_firmware_check(ui, check, row.verified, color);
                            let mut x = rect.left();
                            for (index, galley) in galleys.into_iter().enumerate() {
                                let offset = if index == 0 { 26. } else { 6. };
                                ui.painter().galley(
                                    egui::pos2(x + offset, rect.center().y - galley.size().y * 0.5),
                                    galley,
                                    color,
                                );
                                x += width * widths[index];
                                ui.painter().line_segment(
                                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                                    egui::Stroke::new(1_f32, line),
                                );
                            }
                            ui.painter().line_segment(
                                [rect.left_bottom(), rect.right_bottom()],
                                egui::Stroke::new(1_f32, line),
                            );
                        }
                    });
            });
        let present = self
            .firmware
            .iter()
            .filter(|row| row.status != "Missing")
            .count();
        ui.label(format!(
            "{present} of {} listed files present.",
            self.firmware.len()
        ));
        if ui
            .add_enabled(self.task.is_none(), egui::Button::new("Check again"))
            .clicked()
        {
            self.refresh = true;
        }
        if let Some(row) = self
            .firmware
            .iter()
            .find(|r| Some(&r.path) == self.selected.as_ref())
            .cloned()
        {
            ui.label(&row.path);
            ui.label(format!(
                "Expected MD5: {}",
                if row.digests.is_empty() {
                    "Not provided".to_owned()
                } else {
                    row.digests.join(" / ")
                }
            ));
            if ui
                .add_enabled(
                    self.task.is_none(),
                    egui::Button::new("Import selected BIOS…"),
                )
                .clicked()
            {
                self.start(ui.ctx(),move||{
            let Some(source)=super::tools::pick_path(super::tools::Picker::File,"Import system file")?else{return Ok(ResultData::Message("Import cancelled.".into()));};
            let bytes=bounded_read(&source,128*1024*1024)?;
            if !row.digests.is_empty(){let digest=md5(&source)?;if !row.digests.iter().any(|d|d==&digest){return Err("This file does not match the selected BIOS checksum. The original file was kept.".into());}}
            let relative=Path::new(&row.path);if relative.is_absolute() || relative.components().any(|c|!matches!(c,std::path::Component::Normal(_))){return Err("Invalid system file path".into());}
            let destination=folder.join(relative);if destination.exists(){return Err("A file already exists at this location. Move it aside before importing a replacement.".into());}
            let parent=destination.parent().ok_or("Invalid destination")?;std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
            let canonical=parent.canonicalize().map_err(|e|e.to_string())?;if !canonical.starts_with(folder.canonicalize().map_err(|e|e.to_string())?){return Err("The destination leaves the system folder".into());}
            let mut temp=tempfile::NamedTempFile::new_in(parent).map_err(|e|e.to_string())?;temp.write_all(&bytes).map_err(|e|e.to_string())?;temp.persist_noclobber(&destination).map_err(|e|e.to_string())?;
            Ok(ResultData::Message(format!("Imported {}. Choose Check system files to verify it.",row.path)))
        });
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Done").clicked() {
                self.close_requested = true;
            }
        });
    }
    pub fn audit(
        &mut self,
        ui: &mut egui::Ui,
        _prefs: &mut Preferences,
        env: &Environment<'_>,
        _message: &mut String,
    ) {
        ui.label("Find missing ROMs and missing disc-image files. Reconnect a game without changing the original library index.");
        if ui
            .add_enabled(self.task.is_none(), egui::Button::new("Audit library"))
            .clicked()
        {
            let games = env.library.games.clone();
            self.start(ui.ctx(), move || {
                let mut rows = Vec::new();
                for game in games {
                    let mut problem = None;
                    if !game.rom.is_file() {
                        problem = Some("ROM file is missing".to_owned());
                    } else if game
                        .rom
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("cue"))
                    {
                        if let Ok(bytes) = bounded_read(&game.rom, 1024 * 1024) {
                            let text = String::from_utf8_lossy(&bytes);
                            for line in text.lines() {
                                if line.trim_start().to_uppercase().starts_with("FILE ") {
                                    if let Some(file) = line.split('"').nth(1) {
                                        if !game
                                            .rom
                                            .parent()
                                            .unwrap_or(Path::new("."))
                                            .join(file)
                                            .is_file()
                                        {
                                            problem =
                                                Some(format!("Disc track is missing: {file}"));
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(problem) = problem {
                        rows.push((game.id, format!("{} — {problem}", game.title), game.rom));
                    }
                }
                Ok(ResultData::Audit(rows))
            });
        }
        if self.task.is_some() {
            ui.spinner();
        }
        for (id, label, path) in self.audit.clone() {
            ui.group(|ui| {
                ui.label(label);
                ui.weak(path.display().to_string());
                if ui
                    .add_enabled(self.task.is_none(), egui::Button::new("Locate ROM…"))
                    .clicked()
                {
                    self.start(ui.ctx(), move || {
                        Ok(
                            match super::tools::pick_path(
                                super::tools::Picker::File,
                                "Locate game ROM",
                            )? {
                                Some(path) => {
                                    if emuluna_state_file::digest_file(&path)?!=id {return Err("The selected file does not match the original game's checksum. Choose the matching ROM; multi-file disc repair still needs the original library tools.".into());}
                                    ResultData::Preference(format!("game.{id}.rom"),path.display().to_string())
                                },
                                None => ResultData::Message("Reconnect cancelled.".into()),
                            },
                        )
                    });
                }
            });
        }
    }
}
fn md5(path: &Path) -> Result<String, String> {
    let result = std::process::Command::new("md5sum")
        .arg("--")
        .arg(path)
        .output()
        .map_err(|e| format!("Could not verify the system file: {e}"))?;
    if !result.status.success() {
        return Err("The system file could not be read".into());
    }
    String::from_utf8_lossy(&result.stdout)
        .split_whitespace()
        .next()
        .filter(|s| s.len() == 32)
        .map(str::to_owned)
        .ok_or("Invalid checksum result".into())
}
fn check_firmware(
    root: &Path,
    folder: &Path,
    systems: &[String],
    optional: bool,
    all: bool,
    settings: &BTreeMap<String, String>,
) -> Result<Vec<Firmware>, String> {
    let catalog: Value = serde_json::from_str(include_str!("../../../../data/cores.json"))
        .map_err(|e| e.to_string())?;
    let system_catalog: Value = serde_json::from_str(include_str!("../../../../data/systems.json"))
        .map_err(|e| e.to_string())?;
    let mut rows: BTreeMap<String, Firmware> = BTreeMap::new();
    for system in systems {
        let choices = emuluna_launch_config::installed_choices(root, system).unwrap_or_default();
        let selection = settings
            .get(&format!("core.{system}"))
            .map_or("auto", String::as_str);
        let selected = if matches!(selection, "auto" | "builtin" | "") {
            choices.first()
        } else {
            choices.iter().find(|c| c.id == selection)
        };
        for (id, core) in catalog.as_object().ok_or("Invalid catalog")? {
            if !core["systems"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(system)))
                || (!all && selected.is_some_and(|c| c.catalog_id != *id))
            {
                continue;
            }
            for file in core["firmware"].as_array().into_iter().flatten() {
                if file["systems"]
                    .as_array()
                    .is_some_and(|a| !a.iter().any(|v| v.as_str() == Some(system)))
                    || (!optional
                        && file["optional"].as_bool() == Some(true)
                        && !file["required_systems"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(system))))
                {
                    continue;
                }
                let Some(path) = file["path"].as_str() else {
                    continue;
                };
                let row = rows.entry(path.into()).or_insert_with(|| Firmware {
                    path: path.into(),
                    used_by: String::new(),
                    requirement: file["requirement"].as_str().unwrap_or("Required").into(),
                    digests: Vec::new(),
                    status: String::new(),
                    verified: false,
                });
                let system_name = system_catalog[system]["name"].as_str().unwrap_or(system);
                if !row.used_by.split(", ").any(|s| s == system_name) {
                    if !row.used_by.is_empty() {
                        row.used_by.push_str(", ");
                    }
                    row.used_by.push_str(system_name);
                }
                if let Some(digest) = file["md5"].as_str() {
                    row.digests.push(digest.to_lowercase());
                }
            }
        }
    }
    for row in rows.values_mut() {
        verify_file(row, folder);
    }
    Ok(rows.into_values().collect())
}
fn verify_file(row: &mut Firmware, folder: &Path) {
    let path = folder.join(&row.path);
    row.status = if !path.is_file() {
        "Missing".into()
    } else if row.digests.is_empty() {
        "Present (no checksum)".into()
    } else {
        match md5(&path) {
            Ok(actual) if row.digests.contains(&actual) => {
                row.verified = true;
                "Verified".into()
            }
            Ok(_) => "Checksum mismatch".into(),
            Err(_) => "Could not verify".into(),
        }
    };
}

// Firmware checkmark is shared with the theme preview.
pub(super) const VERIFIED_TEXT: Color32 = Color32::from_rgb(71, 213, 137);

pub(super) fn paint_firmware_check(
    ui: &egui::Ui,
    check: egui::Rect,
    verified: bool,
    color: Color32,
) {
    if verified {
        ui.painter()
            .rect_filled(check, 3., Color32::from_rgb(21, 132, 78));
        ui.painter().add(egui::Shape::line(
            vec![
                check.min + egui::vec2(3., 7.),
                check.min + egui::vec2(6., 10.),
                check.min + egui::vec2(12., 4.),
            ],
            egui::Stroke::new(1.8_f32, Color32::WHITE),
        ));
    } else {
        ui.painter().rect_stroke(
            check,
            3.,
            egui::Stroke::new(1_f32, color),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_requires_a_matching_checksum_and_keeps_originals() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("bios.bin");
        std::fs::write(&path, b"abc").unwrap();
        let mut row = Firmware {
            path: "bios.bin".into(),
            used_by: "test".into(),
            requirement: "Required".into(),
            digests: vec!["900150983cd24fb0d6963f7d28e17f72".into()],
            status: String::new(),
            verified: false,
        };
        verify_file(&mut row, root.path());
        assert!(row.verified);
        assert_eq!(row.status, "Verified");
        std::fs::write(&path, b"changed").unwrap();
        row.verified = false;
        verify_file(&mut row, root.path());
        assert!(!row.verified);
        assert_eq!(row.status, "Checksum mismatch");
        assert_eq!(std::fs::read(&path).unwrap(), b"changed");
        row.digests.clear();
        verify_file(&mut row, root.path());
        assert!(!row.verified);
        assert_eq!(row.status, "Present (no checksum)");
    }
}
