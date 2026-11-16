use eframe::egui::{self, TextureHandle};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};

fn reader(path: &Path) -> Result<image::ImageReader<std::io::BufReader<std::fs::File>>, String> {
    let meta = path.metadata().map_err(|error| error.to_string())?;
    if !meta.is_file() || meta.len() > 32 * 1024 * 1024 {
        return Err("Artwork file exceeds preview limits".into());
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}
pub fn dimensions(path: &Path) -> Option<[u32; 2]> {
    reader(path)
        .ok()?
        .into_dimensions()
        .ok()
        .filter(|(width, height)| *width > 0 && *height > 0 && *width <= 8192 && *height <= 8192)
        .map(|(width, height)| [width, height])
}
fn color_image(image: image::DynamicImage) -> egui::ColorImage {
    let pixels = image.into_rgba8();
    egui::ColorImage::from_rgba_unmultiplied(
        [pixels.width() as usize, pixels.height() as usize],
        pixels.as_raw(),
    )
}
fn is_interface_artwork(path: &Path) -> bool {
    path.components()
        .any(|part| part.as_os_str() == "branding" || part.as_os_str() == "controllers")
}

pub(crate) const SETTINGS_ICON_POINTS: f32 = 40.;

fn is_settings_icon(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "svg")
        && path
            .parent()
            .is_some_and(|folder| folder.ends_with("icons/settings"))
}

fn decode_artwork(path: &Path) -> Option<egui::ColorImage> {
    if is_settings_icon(path) {
        // Rasterize at four times the displayed size for clean mip levels.
        let svg = std::fs::read(path).ok()?;
        let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).ok()?;
        let size = tree.size();
        let scale = (SETTINGS_ICON_POINTS * 4.) / size.width().max(size.height());
        let width = (size.width() * scale).round().max(1.) as u32;
        let height = (size.height() * scale).round().max(1.) as u32;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        return Some(egui::ColorImage::from_rgba_premultiplied(
            [width as usize, height as usize],
            pixmap.data(),
        ));
    }

    let image = reader(path).ok()?.decode().ok()?;
    let limit = if is_interface_artwork(path) {
        2048
    } else {
        512
    };
    Some(color_image(if image.width().max(image.height()) <= limit {
        image
    } else {
        image.thumbnail(limit, limit)
    }))
}

fn texture_options(path: &Path) -> egui::TextureOptions {
    if is_interface_artwork(path) || is_settings_icon(path) {
        // Blend between downsampled levels as the artwork scales or rotates.
        // ColorImage already premultiplies alpha, so transparent edges blend cleanly.
        egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear))
    } else {
        egui::TextureOptions::LINEAR
    }
}

pub(crate) fn preview_path(source: &Path, cache: &Path) -> Option<PathBuf> {
    let metadata = source.metadata().ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let mut hash = Sha256::new();
    hash.update(source.as_os_str().as_encoded_bytes());
    hash.update(metadata.len().to_le_bytes());
    hash.update(modified.as_nanos().to_le_bytes());
    Some(cache.join(format!("{:x}-96px.png", hash.finalize())))
}
fn small_preview(source: &Path, cache: Option<&Path>) -> Option<image::DynamicImage> {
    let cached = cache.and_then(|cache| preview_path(source, cache));
    if let Some(path) = &cached {
        if let Some(image) = reader(path)
            .ok()
            .and_then(|reader| reader.decode().ok())
            .filter(|image| image.width() <= 96 && image.height() <= 96)
        {
            return Some(image);
        }
    }
    let image = reader(source).ok()?.decode().ok()?.thumbnail(96, 96);
    if let Some(path) = cached {
        let _ = (|| -> Result<(), String> {
            let parent = path.parent().ok_or("Preview has no cache directory")?;
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            let mut temporary =
                tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
            image
                .write_to(&mut temporary, image::ImageFormat::Png)
                .map_err(|error| error.to_string())?;
            temporary.persist(path).map_err(|error| error.to_string())?;
            Ok(())
        })();
    }
    Some(image)
}
struct Entry {
    texture: Option<TextureHandle>,
    bytes: usize,
    used: u64,
}
struct Decoded {
    path: PathBuf,
    image: Option<egui::ColorImage>,
    version: u64,
    complete: bool,
}
struct Request {
    path: PathBuf,
    version: u64,
    box_art: bool,
}
pub struct Artwork {
    entries: HashMap<PathBuf, Entry>,
    previews: HashMap<PathBuf, Entry>,
    pending: HashMap<PathBuf, u64>,
    versions: HashMap<PathBuf, u64>,
    request: Option<mpsc::SyncSender<Request>>,
    result: Option<mpsc::Receiver<Decoded>>,
    workers: Vec<thread::JoinHandle<()>>,
    warm_request: Option<mpsc::Sender<Vec<PathBuf>>>,
    warm_signature: Vec<PathBuf>,
    cache: Option<PathBuf>,
    ctx: egui::Context,
    stop: Arc<AtomicBool>,
    bytes: usize,
    epoch: u64,
}
impl Artwork {
    #[cfg(test)]
    pub fn new(ctx: egui::Context) -> Self {
        Self::with_cache(ctx, None)
    }
    pub fn with_cache(ctx: egui::Context, cache: Option<PathBuf>) -> Self {
        let (request, requests) = mpsc::sync_channel::<Request>(48);
        let (results, result) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(requests));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let requests = requests.clone();
            let results = results.clone();
            let stop = stop.clone();
            let cache = cache.clone();
            let ctx = ctx.clone();
            workers.push(thread::spawn(move || loop {
                let request = requests.lock().unwrap().recv();
                let Ok(Request {
                    path,
                    version,
                    box_art,
                }) = request
                else {
                    break;
                };
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if box_art {
                    let preview = small_preview(&path, cache.as_deref()).map(color_image);
                    if results
                        .send(Decoded {
                            path: path.clone(),
                            image: preview,
                            version,
                            complete: false,
                        })
                        .is_err()
                    {
                        break;
                    }
                    ctx.request_repaint();
                }
                let image = decode_artwork(&path);
                if results
                    .send(Decoded {
                        path,
                        image,
                        version,
                        complete: true,
                    })
                    .is_err()
                {
                    break;
                }
                ctx.request_repaint();
            }));
        }
        let (warm_request, warm_requests) = mpsc::channel::<Vec<PathBuf>>();
        let warm_cache = cache.clone();
        let warm_stop = stop.clone();
        workers.push(thread::spawn(move || {
            let mut paths = VecDeque::new();
            while !warm_stop.load(Ordering::Acquire) {
                if paths.is_empty() {
                    match warm_requests.recv() {
                        Ok(latest) => paths = latest.into(),
                        Err(_) => break,
                    }
                }
                while let Ok(latest) = warm_requests.try_recv() {
                    paths = latest.into();
                }
                if let (Some(path), Some(cache)) = (paths.pop_front(), warm_cache.as_deref()) {
                    if preview_path(&path, cache).is_some_and(|path| !path.is_file()) {
                        let _ = small_preview(&path, Some(cache));
                    }
                }
            }
        }));

        Self {
            entries: HashMap::new(),
            previews: HashMap::new(),
            pending: HashMap::new(),
            versions: HashMap::new(),
            request: Some(request),
            result: Some(result),
            workers,
            warm_request: Some(warm_request),
            warm_signature: Vec::new(),
            cache,
            ctx,
            stop,
            bytes: 0,
            epoch: 0,
        }
    }
    pub fn warm_box_art_previews(&mut self, paths: Vec<PathBuf>) {
        if self.warm_signature == paths {
            return;
        }
        self.warm_signature = paths.clone();
        if let Some(sender) = &self.warm_request {
            let _ = sender.send(paths);
        }
    }
    pub fn invalidate(&mut self, path: &Path) {
        if let Some(entry) = self.entries.remove(path) {
            self.bytes = self.bytes.saturating_sub(entry.bytes);
        }
        self.previews.remove(path);
        let version = self.versions.entry(path.to_owned()).or_default();
        *version = version.wrapping_add(1);
        self.pending.remove(path);
    }
    pub fn size(&self, path: &Path) -> Option<egui::Vec2> {
        self.entries
            .get(path)
            .and_then(|entry| entry.texture.as_ref())
            .or_else(|| {
                self.previews
                    .get(path)
                    .and_then(|entry| entry.texture.as_ref())
            })
            .map(TextureHandle::size_vec2)
    }
    pub fn update(&mut self, ctx: &egui::Context) {
        self.epoch += 1;
        for _ in 0..8 {
            let Some(decoded) = self
                .result
                .as_ref()
                .and_then(|receiver| receiver.try_recv().ok())
            else {
                break;
            };
            if decoded.version != self.versions.get(&decoded.path).copied().unwrap_or(0) {
                continue;
            }
            let options = texture_options(&decoded.path);
            let bytes = decoded.image.as_ref().map_or(0, |image| {
                let [mut width, mut height] = image.size;
                let mut pixels = width * height;
                if options.mipmap_mode.is_some() {
                    while width > 1 || height > 1 {
                        width = (width / 2).max(1);
                        height = (height / 2).max(1);
                        pixels += width * height;
                    }
                }
                pixels * 4
            });
            let texture = decoded
                .image
                .map(|image| ctx.load_texture(decoded.path.to_string_lossy(), image, options));
            let entry = Entry {
                texture,
                bytes,
                used: self.epoch,
            };
            if decoded.complete {
                self.pending.remove(&decoded.path);
                if let Some(previous) = self.entries.insert(decoded.path, entry) {
                    self.bytes = self.bytes.saturating_sub(previous.bytes);
                }
                self.bytes += bytes;
            } else {
                self.previews.insert(decoded.path, entry);
            }
        }
        while self.entries.len() > 256 || self.bytes > 256 * 1024 * 1024 {
            let key = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(path, _)| path.clone());
            let Some(key) = key else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes -= entry.bytes;
            }
        }
        while self.previews.len() > 1024 {
            let Some(key) = self
                .previews
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            self.previews.remove(&key);
        }
    }
    pub fn fitted_rect(&self, path: &Path, bounds: egui::Rect) -> egui::Rect {
        if let Some(size) = self.size(path) {
            let scale = (bounds.width() / size.x).min(bounds.height() / size.y);
            egui::Rect::from_center_size(bounds.center(), size * scale)
        } else {
            bounds
        }
    }
    pub fn texture(&mut self, path: &Path) -> Option<egui::TextureId> {
        self.load_texture(path, false)
    }
    pub fn box_art_texture(&mut self, path: &Path) -> Option<egui::TextureId> {
        self.load_texture(path, true)
    }
    fn load_texture(&mut self, path: &Path, box_art: bool) -> Option<egui::TextureId> {
        if let Some(entry) = self.entries.get_mut(path) {
            entry.used = self.epoch;
            if let Some(texture) = &entry.texture {
                return Some(texture.id());
            }
            return if box_art {
                self.box_art_preview(path)
            } else {
                None
            };
        }
        if box_art && !self.previews.contains_key(path) {
            let cached = self
                .cache
                .as_deref()
                .and_then(|cache| preview_path(path, cache));
            if let Some(image) = cached
                .and_then(|path| reader(&path).ok()?.decode().ok())
                .filter(|image| image.width() <= 96 && image.height() <= 96)
            {
                let image = color_image(image);
                let bytes = image.pixels.len() * 4;
                let texture = self.ctx.load_texture(
                    path.to_string_lossy(),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                self.previews.insert(
                    path.to_owned(),
                    Entry {
                        texture: Some(texture),
                        bytes,
                        used: self.epoch,
                    },
                );
            }
        }
        if !self.pending.contains_key(path) && self.pending.len() < 48 {
            let version = self.versions.get(path).copied().unwrap_or(0);
            if self.request.as_ref().is_some_and(|sender| {
                sender
                    .try_send(Request {
                        path: path.to_owned(),
                        version,
                        box_art,
                    })
                    .is_ok()
            }) {
                self.pending.insert(path.to_owned(), version);
            }
        }
        if box_art {
            self.box_art_preview(path)
        } else {
            None
        }
    }
    fn box_art_preview(&mut self, path: &Path) -> Option<egui::TextureId> {
        self.previews.get_mut(path).and_then(|entry| {
            entry.used = self.epoch;
            entry.texture.as_ref().map(TextureHandle::id)
        })
    }
    pub fn idle(&self) -> bool {
        self.pending.is_empty()
    }
    pub fn bytes(&self) -> usize {
        self.bytes
            + self
                .previews
                .values()
                .map(|entry| entry.bytes)
                .sum::<usize>()
    }
}
impl Drop for Artwork {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.request.take();
        self.result.take();
        self.warm_request.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interface_artwork_skips_cached_thumbnails_and_uses_mipmaps() {
        let directory = tempfile::tempdir().unwrap();
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let ctx = egui::Context::default();
        let mut artwork = Artwork::with_cache(ctx, Some(directory.path().to_owned()));
        for file in ["branding/emuluna-logo.png", "controllers/snes.webp"] {
            let source = data.join(file);
            // Older builds could have written a tiny preview for these assets.
            image::RgbaImage::new(96, 96)
                .save(preview_path(&source, directory.path()).unwrap())
                .unwrap();
            assert!(artwork.texture(&source).is_none());
            let full = artwork
                .result
                .as_ref()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
            assert!(full.complete);
            assert_eq!(
                full.image.unwrap().size,
                dimensions(&source).unwrap().map(|value| value as usize)
            );
            assert_eq!(
                texture_options(&source).mipmap_mode,
                Some(egui::TextureFilter::Linear)
            );
        }
        assert_eq!(
            texture_options(Path::new("covers/game.webp")).mipmap_mode,
            None
        );
    }
    #[test]
    fn regular_textures_do_not_read_or_write_box_art_thumbnails() {
        let directory = tempfile::tempdir().unwrap();
        let cache = directory.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        let screenshot = directory.path().join("screenshot.png");
        image::RgbaImage::new(640, 480).save(&screenshot).unwrap();
        let icon = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/icons/snes.png");
        let old_icon_preview = preview_path(&icon, &cache).unwrap();
        image::RgbaImage::new(96, 96)
            .save(&old_icon_preview)
            .unwrap();
        let mut artwork = Artwork::with_cache(egui::Context::default(), Some(cache.clone()));
        let settings_icon =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/icons/settings/general.svg");
        for source in [&screenshot, &icon, &settings_icon] {
            assert!(artwork.texture(source).is_none());
            let full = artwork
                .result
                .as_ref()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
            assert!(full.complete);
            assert!(full.image.is_some());
        }
        assert!(!preview_path(&screenshot, &cache).unwrap().exists());
        assert_eq!(std::fs::read_dir(cache).unwrap().count(), 1);
    }
    #[test]
    fn small_preview_precedes_full_artwork_and_is_cached_without_changing_source() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("cover.png");
        image::RgbaImage::from_pixel(800, 1200, image::Rgba([40, 120, 210, 255]))
            .save(&source)
            .unwrap();
        let original = std::fs::read(&source).unwrap();
        let cache = directory.path().join("previews");
        let ctx = egui::Context::default();
        let mut artwork = Artwork::with_cache(ctx, Some(cache.clone()));
        assert!(artwork.box_art_texture(&source).is_none());
        let receiver = artwork.result.as_ref().unwrap();
        let small = receiver
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert!(!small.complete);
        assert_eq!(small.image.unwrap().size, [64, 96]);
        let full = receiver
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert!(full.complete);
        assert_eq!(full.image.unwrap().size, [341, 512]);
        assert_eq!(
            dimensions(&preview_path(&source, &cache).unwrap()),
            Some([64, 96])
        );
        assert_eq!(std::fs::read(&source).unwrap(), original);
        let mut reopened = Artwork::with_cache(egui::Context::default(), Some(cache.clone()));
        assert!(
            reopened.box_art_texture(&source).is_some(),
            "A cached small preview must be available on the first request"
        );
        assert!(reopened.pending.len() <= 48);
        drop(reopened);
        let old_path = preview_path(&source, &cache).unwrap();
        image::RgbaImage::from_pixel(400, 400, image::Rgba([230, 40, 20, 255]))
            .save(&source)
            .unwrap();
        assert_ne!(preview_path(&source, &cache).unwrap(), old_path);
        assert_eq!(
            small_preview(&source, Some(&cache))
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [230, 40, 20, 255]
        );
    }
}
