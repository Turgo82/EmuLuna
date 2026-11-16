//! Explicit screenshots/state previews only; no readback during normal playback.
use super::*;
pub struct Preview {
    image: image::RgbaImage,
}
impl Preview {
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        use image::ImageEncoder;
        let parent = path.parent().ok_or("Preview has no containing folder")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut output = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        image::codecs::png::PngEncoder::new(&mut output)
            .write_image(
                self.image.as_raw(),
                self.image.width(),
                self.image.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
        output.as_file().sync_all().map_err(|e| e.to_string())?;
        output.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
}
impl Presenter {
    /// Caller must retain the current hardware source lease. Redraws only the
    /// completed cached image: no shader history advance and no overlay in PNGs.
    pub fn capture_game_preview(&mut self) -> Result<Option<Preview>, String> {
        self.ensure_current()?;
        let Some((view, filter)) = self.cached else {
            return Ok(None);
        };
        let (width, height) = self.window.drawable_size();
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_777_216 {
            return Err("The preview exceeds its image size limit".into());
        }
        self.paint_image(
            view.image,
            view.top_left,
            filter,
            viewport(width, height, view.aspect),
        )?;
        let mut pixels = vec![0; width as usize * height as usize * 4];
        // SAFETY: owning display context is current, default framebuffer has
        // just been painted, and the slice holds the complete RGBA output.
        unsafe {
            self.gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            let error = self.gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!("Preview capture failed: {error:#x}"));
            }
        }
        let mut image = image::RgbaImage::from_raw(width, height, pixels)
            .ok_or("Invalid preview dimensions")?;
        image::imageops::flip_vertical_in_place(&mut image);
        Ok(Some(Preview { image }))
    }
    pub fn save_game_preview(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.capture_game_preview()?
            .ok_or("No game image is available yet")?
            .save(path)
    }
}
