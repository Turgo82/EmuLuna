//! Content-addressed WebP copies. Never overwrites the legacy artwork or index.
use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub struct Entry {
    pub file: String,
    pub source_sha: String,
    pub quality: u8,
}
pub type Index = BTreeMap<String, Entry>;
pub fn directory(library: &Path, preferences: &Path) -> PathBuf {
    crate::storage::converted_artwork(library, preferences)
}
fn valid_digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn load_index(root: &Path) -> Result<Index, String> {
    let file = match File::open(root.join("index.json")) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Index::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Artwork index exceeds its size limit".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["format"].as_u64() != Some(1) {
        return Err("Unsupported artwork index format".into());
    }
    let entries = value["entries"]
        .as_object()
        .ok_or("Invalid artwork index")?;
    if entries.len() > 100000 {
        return Err("Artwork index has too many entries".into());
    }
    let mut index = Index::new();
    for (id, value) in entries {
        let file = value["file"]
            .as_str()
            .ok_or("Artwork entry has no filename")?;
        let source = value["source_sha256"]
            .as_str()
            .ok_or("Artwork entry has no source checksum")?;
        let quality = value["quality"]
            .as_u64()
            .ok_or("Artwork entry has no quality")?;
        if !valid_digest(id)
            || !valid_digest(source)
            || ![75, 85, 95, 100].contains(&quality)
            || file != format!("{source}-q{quality}.webp")
        {
            return Err("Invalid artwork index entry".into());
        }
        index.insert(
            id.clone(),
            Entry {
                file: file.into(),
                source_sha: source.into(),
                quality: quality as u8,
            },
        );
    }
    Ok(index)
}
fn conversion_lock(root: &Path) -> Result<File, String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".conversion.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|_| "Artwork conversion is already running for this library")?;
    Ok(lock)
}
fn save_index(root: &Path, index: &Index) -> Result<(), String> {
    let entries: serde_json::Map<_, _> = index
        .iter()
        .map(|(id, entry)| {
            (
                id.clone(),
                json!({"file":entry.file,"source_sha256":entry.source_sha,"quality":entry.quality}),
            )
        })
        .collect();
    super::tools::atomic_json(
        &root.join("index.json"),
        &json!({"format":1,"entries":entries}),
    )
}
pub fn apply(
    library: &mut Library,
    originals: &HashMap<String, PathBuf>,
    root: &Path,
    index: &Index,
) {
    for game in &mut library.games {
        game.cover = index
            .get(&game.id)
            .map(|entry| root.join(&entry.file))
            .filter(|path| path.is_file())
            .or_else(|| originals.get(&game.id).cloned());
    }
}
enum Event {
    Progress(usize, usize, usize),
    Finished(Result<(Index, bool, usize), String>),
}
#[derive(Default)]
pub struct Conversion {
    receiver: Option<mpsc::Receiver<Event>>,
    cancel: Arc<AtomicBool>,
    progress: (usize, usize, usize),
    pub changed: Option<(PathBuf, Index)>,
    root: PathBuf,
}
impl Drop for Conversion {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Conversion {
    pub fn busy(&self) -> bool {
        self.receiver.is_some()
    }
    pub fn sidebar(&mut self, ui: &mut egui::Ui) -> bool {
        if self.receiver.is_none() {
            return false;
        }
        let (done, total, failed) = self.progress;
        let cancelling = self.cancel.load(Ordering::Acquire);
        let detail = if cancelling {
            "Cancelling…".into()
        } else {
            format!("{done} / {total} covers · {failed} skipped")
        };
        if crate::activity::sidebar(
            ui,
            "Converting cover art",
            &detail,
            (done as u64, total as u64),
            Some(!cancelling),
        ) {
            self.cancel.store(true, Ordering::Release);
        }
        true
    }
    pub fn poll(&mut self, ctx: &egui::Context, message: &mut String) {
        let mut finished = false;
        if let Some(receiver) = &self.receiver {
            loop {
                match receiver.try_recv() {
                    Ok(Event::Progress(done, total, failed)) => {
                        self.progress = (done, total, failed);
                        ctx.request_repaint();
                        ctx.request_repaint_of(egui::ViewportId::ROOT);
                    }
                    Ok(Event::Finished(result)) => {
                        finished = true;
                        match result {
                            Ok((index, cancelled, failed)) => {
                                self.changed = Some((self.root.clone(), index));
                                *message=format!("{} {} covers checked; {failed} could not be converted. Original artwork was kept.",if cancelled{"Conversion stopped."}else{"Conversion complete."},self.progress.0);
                            }
                            Err(error) => *message = error,
                        }
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        finished = true;
                        *message =
                            "The conversion worker stopped. Original artwork was kept.".into();
                        break;
                    }
                }
            }
        }
        if finished {
            self.receiver = None;
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        prefs: &mut Preferences,
        env: &Environment<'_>,
        message: &mut String,
    ) {
        heading(ui, "Artwork conversion");
        let mut quality = value(prefs, env.library, "artwork.webp_quality", "85")
            .parse::<u8>()
            .ok()
            .filter(|q| [75, 85, 95, 100].contains(q))
            .unwrap_or(85);
        let old = quality;
        ui.add_enabled_ui(self.receiver.is_none(), |ui| {
            egui::ComboBox::from_id_salt("webp-quality")
                .width(280.)
                .selected_text(quality_label(quality))
                .show_ui(ui, |ui| {
                    for q in [75, 85, 95, 100] {
                        ui.selectable_value(&mut quality, q, quality_label(q));
                    }
                });
        });
        if quality != old {
            prefs.set("artwork.webp_quality", quality.to_string());
        }
        ui.label("Convert artwork to WebP for the native library. Copies keep their full dimensions; the existing library artwork stays untouched.");
        if self.receiver.is_none() {
            if ui.button("Convert existing covers to WebP").clicked() {
                let sources: Vec<_> = env
                    .original_covers
                    .iter()
                    .map(|(id, path)| (id.clone(), path.clone()))
                    .collect();
                let root = directory(&env.library.root, env.preferences_path);
                self.root = root.clone();
                self.cancel = Arc::new(AtomicBool::new(false));
                let cancel = self.cancel.clone();
                let (tx, rx) = mpsc::channel();
                self.receiver = Some(rx);
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                self.progress = (0, sources.len(), 0);
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let result =
                        convert_all(&root, &sources, quality, &cancel, |done, total, failed| {
                            let _ = tx.send(Event::Progress(done, total, failed));
                            ctx.request_repaint();
                            ctx.request_repaint_of(egui::ViewportId::ROOT);
                        });
                    let _ = tx.send(Event::Finished(result));
                    ctx.request_repaint();
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                });
                *message = "Converting artwork in the background…".into();
            }
        } else {
            let (done, total, failed) = self.progress;
            ui.add(
                egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                    .text(format!("{done} / {total} covers · {failed} skipped"))
                    .animate(true),
            );
            if ui.button("Stop conversion").clicked() {
                self.cancel.store(true, Ordering::Release);
                *message = "Stopping after the current file…".into();
            }
        }
        ui.add_enabled_ui(self.receiver.is_none(),|ui| {
            if ui.button("Use original artwork").clicked() {
                let root=directory(&env.library.root,env.preferences_path);let index=Index::new();
                match save_index(&root,&index) {Ok(())=>{self.changed=Some((root,index));*message="Original artwork restored for the native library. Converted copies are kept for reuse.".into();},Err(error)=>*message=error}
            }
        });
    }
}
fn quality_label(quality: u8) -> &'static str {
    match quality {
        75 => "75 — Smaller files",
        95 => "95 — Higher detail",
        100 => "100 — Lossless",
        _ => "85 — Balanced (recommended)",
    }
}
fn read_source(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
        return Err("Artwork exceeds conversion limits".into());
    }
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Artwork exceeds conversion limits".into());
    }
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
}
pub(crate) fn encode(
    root: &Path,
    source: &[u8],
    quality: u8,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let digest = format!("{:x}", Sha256::digest(source));
    let filename = format!("{digest}-q{quality}.webp");
    let destination = root.join(&filename);
    let decoded = decode(source)?;
    if destination.is_file() {
        if let Ok(bytes) = read_source(&destination) {
            if decode(&bytes).is_ok_and(|image| {
                image.width() == decoded.width() && image.height() == decoded.height()
            }) {
                return Ok(filename);
            }
        }
    }
    let work = tempfile::tempdir_in(root).map_err(|e| e.to_string())?;
    let input = work.path().join("source.png");
    let output = work.path().join("converted.webp");
    decoded
        .save_with_format(&input, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let mut child = Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&input)
        .args([
            "-frames:v",
            "1",
            "-c:v",
            "libwebp",
            "-quality",
            &quality.to_string(),
            "-lossless",
            if quality == 100 { "1" } else { "0" },
            "-threads",
            "1",
            "-pix_fmt",
            "bgra",
        ])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("WebP conversion requires FFmpeg with the libwebp encoder: {e}"))?;
    let start = Instant::now();
    loop {
        if cancel.load(Ordering::Acquire) || start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Conversion stopped".into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                return Err("The WebP encoder could not convert this cover".into());
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let bytes = read_source(&output)?;
    let converted = decode(&bytes)?;
    if converted.width() != decoded.width() || converted.height() != decoded.height() {
        return Err("Converted cover dimensions did not match".into());
    }
    fs::rename(&output, &destination).map_err(|e| e.to_string())?;
    Ok(filename)
}
fn require_encoder() -> Result<(), String> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-h", "encoder=libwebp"])
        .output()
        .map_err(|e| format!("WebP conversion requires FFmpeg with the libwebp encoder: {e}"))?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout).contains("Encoder libwebp")
    {
        return Err("This FFmpeg build does not include the WebP encoder.".into());
    }
    Ok(())
}
fn convert_all(
    root: &Path,
    sources: &[(String, PathBuf)],
    quality: u8,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize, usize),
) -> Result<(Index, bool, usize), String> {
    if !sources.is_empty() && !cancel.load(Ordering::Acquire) {
        require_encoder()?;
    }
    let _lock = conversion_lock(root)?;
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut index = load_index(root)?;
    let mut failed = 0;
    let mut done = 0;
    for (id, path) in sources {
        if cancel.load(Ordering::Acquire) {
            break;
        }
        if !valid_digest(id) {
            failed += 1;
            done += 1;
            progress(done, sources.len(), failed);
            continue;
        }
        match read_source(path).and_then(|bytes| {
            let digest = format!("{:x}", Sha256::digest(&bytes));
            encode(root, &bytes, quality, cancel).map(|file| Entry {
                file,
                source_sha: digest,
                quality,
            })
        }) {
            Ok(entry) => {
                index.insert(id.clone(), entry);
            }
            Err(_) if cancel.load(Ordering::Acquire) => break,
            Err(_) => failed += 1,
        }
        done += 1;
        progress(done, sources.len(), failed);
        if done.is_multiple_of(32) {
            save_index(root, &index)?;
        }
    }
    save_index(root, &index)?;
    progress(done, sources.len(), failed);
    Ok((index, cancel.load(Ordering::Acquire), failed))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artwork_index_rejects_paths_outside_managed_copies() {
        let root = tempfile::tempdir().unwrap();
        super::super::tools::atomic_json(&root.path().join("index.json"),&json!({"format":1,"entries":{"a":{"file":"../outside.webp","source_sha256":"b","quality":85}}})).unwrap();
        assert!(load_index(root.path()).is_err());
    }
    #[test]
    #[ignore = "requires FFmpeg with libwebp"]
    fn conversion_keeps_sources_and_dimensions_at_each_quality() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("original.png");
        let image = image::RgbaImage::from_fn(64, 96, |x, y| {
            image::Rgba([
                (x * 3) as u8,
                (y * 2) as u8,
                140,
                if x < 16 { 100 } else { 255 },
            ])
        });
        image.save(&source).unwrap();
        let before = fs::read(&source).unwrap();
        let copies = root.path().join("copies");
        let id = "a".repeat(64);
        for quality in [75, 85, 95, 100] {
            let (index, cancelled, failed) = convert_all(
                &copies,
                &[(id.clone(), source.clone())],
                quality,
                &AtomicBool::new(false),
                |_, _, _| {},
            )
            .unwrap();
            assert!(!cancelled);
            assert_eq!(failed, 0);
            let output = image::open(copies.join(&index[&id].file)).unwrap();
            assert_eq!((output.width(), output.height()), (64, 96));
            if quality == 100 {
                assert_eq!(output.into_rgba8(), image);
            }
            assert_eq!(fs::read(&source).unwrap(), before);
            assert_eq!(load_index(&copies).unwrap()[&id].quality, quality);
        }
        let (index, cancelled, _) = convert_all(
            &copies,
            &[(id.clone(), source)],
            85,
            &AtomicBool::new(true),
            |_, _, _| {},
        )
        .unwrap();
        assert!(cancelled);
        assert_eq!(index[&id].quality, 100);
    }
}
