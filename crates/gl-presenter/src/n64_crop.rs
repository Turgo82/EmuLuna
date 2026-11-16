//! Conservative N64 black-border detection and GPU-only cropped output.
use super::*;

#[derive(Default)]
struct Detector {
    size: (u32, u32),
    pending: Option<([u32; 4], Instant)>,
    locked: Option<[u32; 4]>,
}
impl Detector {
    fn observe(&mut self, width: u32, height: u32, rgba: &[u8], now: Instant) {
        if self.locked.is_some() {
            return;
        }
        if self.size != (width, height) {
            *self = Self {
                size: (width, height),
                ..Self::default()
            };
        }
        let Some(margins) = margins(width, height, rgba) else {
            self.pending = None;
            return;
        };
        if !significant(margins, width, height) {
            self.pending = None;
            return;
        }
        if let Some((old, since)) = self.pending.as_mut() {
            for i in 0..4 {
                old[i] = old[i].min(margins[i]);
            }
            if !significant(*old, width, height) {
                self.pending = None;
            } else if now.duration_since(*since) >= Duration::from_secs(3) {
                self.locked = Some(*old);
                self.pending = None;
            }
        } else {
            self.pending = Some((margins, now));
        }
    }
    fn scaled(&self, width: u32, height: u32) -> Option<[u32; 4]> {
        let old = self.locked?;
        let (w, h) = self.size;
        Some([
            ((old[0] as u64 * width as u64 + w as u64 / 2) / w as u64) as u32,
            ((old[1] as u64 * height as u64 + h as u64 / 2) / h as u64) as u32,
            ((old[2] as u64 * width as u64 + w as u64 / 2) / w as u64) as u32,
            ((old[3] as u64 * height as u64 + h as u64 / 2) / h as u64) as u32,
        ])
    }
}
// Tiny overscan on a startup logo must not lock out larger in-game borders.
fn significant(edges: [u32; 4], width: u32, height: u32) -> bool {
    u64::from(edges[0] + edges[2]) * 100 >= u64::from(width) * 2
        || u64::from(edges[1] + edges[3]) * 100 >= u64::from(height) * 2
}
fn margins(w: u32, h: u32, rgba: &[u8]) -> Option<[u32; 4]> {
    let (w, h) = (w as usize, h as usize);
    if w < 2 || h < 2 || rgba.len() != w.checked_mul(h)?.checked_mul(4)? {
        return None;
    }
    let black = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        // Some core/filter paths produce 1/255 noise in otherwise black padding.
        rgba[i..i + 3].iter().all(|channel| *channel <= 1)
    };
    let mut dark = 0;
    let mut samples = 0;
    for y in (0..h).step_by((h / 24).max(1)) {
        for x in (0..w).step_by((w / 32).max(1)) {
            let i = (y * w + x) * 4;
            dark += usize::from(rgba[i..i + 3].iter().all(|v| *v <= 24));
            samples += 1;
        }
    }
    if dark * 5 >= samples * 4 {
        return None;
    }
    let mut top = 0;
    while top < h / 2 && (0..w).all(|x| black(x, top)) {
        top += 1;
    }
    let mut bottom = 0;
    while bottom < h / 2 && (0..w).all(|x| black(x, h - bottom - 1)) {
        bottom += 1;
    }
    if top == h / 2 || bottom == h / 2 {
        return None;
    }
    let mut left = 0;
    while left < w / 2 && (top..h - bottom).all(|y| black(left, y)) {
        left += 1;
    }
    let mut right = 0;
    while right < w / 2 && (top..h - bottom).all(|y| black(w - right - 1, y)) {
        right += 1;
    }
    if left == w / 2 || right == w / 2 || left + right > w / 4 || top + bottom > h / 4 {
        return Some([0; 4]);
    }
    Some([left as u32, top as u32, right as u32, bottom as u32])
}

pub(super) struct AutoCrop {
    detector: Detector,
    last: Option<Instant>,
    read: glow::Framebuffer,
    draw: glow::Framebuffer,
    texture: glow::Texture,
    pbo: glow::Buffer,
    fence: Option<glow::Fence>,
    pending: (u32, u32),
    output: (u32, u32),
    gl: Arc<glow::Context>,
}
impl AutoCrop {
    pub fn new(gl: Arc<glow::Context>) -> Result<Self, String> {
        unsafe {
            let read = gl.create_framebuffer()?;
            let draw = gl.create_framebuffer()?;
            let texture = gl.create_texture()?;
            let pbo = gl.create_buffer()?;
            Ok(Self {
                detector: Detector::default(),
                last: None,
                read,
                draw,
                texture,
                pbo,
                fence: None,
                pending: (0, 0),
                output: (0, 0),
                gl,
            })
        }
    }
    pub fn process(&mut self, input: FrameView) -> Result<FrameView, String> {
        let gl = &self.gl;
        let now = Instant::now();
        unsafe {
            if let Some(fence) = self.fence {
                let status = gl.client_wait_sync(fence, 0, 0);
                if status == glow::ALREADY_SIGNALED || status == glow::CONDITION_SATISFIED {
                    gl.delete_sync(fence);
                    self.fence = None;
                    let (w, h) = self.pending;
                    let mut pixels = vec![0; w as usize * h as usize * 4];
                    gl.bind_buffer(glow::PIXEL_PACK_BUFFER, Some(self.pbo));
                    gl.get_buffer_sub_data(glow::PIXEL_PACK_BUFFER, 0, &mut pixels);
                    gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
                    self.detector.observe(w, h, &pixels, now);
                } else if status == glow::WAIT_FAILED {
                    gl.delete_sync(fence);
                    self.fence = None;
                }
            }
            let w = input.image.width;
            let h = input.image.height;
            if self.detector.locked.is_none()
                && self.fence.is_none()
                && self
                    .last
                    .is_none_or(|t| now.duration_since(t) >= Duration::from_millis(250))
                && w > 0
                && h > 0
                && (w as u64 * h as u64) <= 4_194_304
            {
                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.read));
                gl.framebuffer_texture_2d(
                    glow::READ_FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(input.image.texture),
                    0,
                );
                gl.read_buffer(glow::COLOR_ATTACHMENT0);
                if gl.check_framebuffer_status(glow::READ_FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE
                {
                    gl.bind_buffer(glow::PIXEL_PACK_BUFFER, Some(self.pbo));
                    gl.buffer_data_size(
                        glow::PIXEL_PACK_BUFFER,
                        (w * h * 4) as i32,
                        glow::STREAM_READ,
                    );
                    gl.read_pixels(
                        0,
                        0,
                        w as i32,
                        h as i32,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::BufferOffset(0),
                    );
                    self.fence = Some(gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0)?);
                    gl.flush();
                    gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
                    self.pending = (w, h);
                    self.last = Some(now);
                }
                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
            }
            let Some([left, top, right, bottom]) = self.detector.scaled(w, h) else {
                return Ok(input);
            };
            if left + right >= w || top + bottom >= h {
                return Ok(input);
            }
            let dims = (w - left - right, h - top - bottom);
            if self.output != dims {
                gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    dims.0 as i32,
                    dims.1 as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::NEAREST as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::NEAREST as i32,
                );
                self.output = dims;
            }
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.read));
            gl.framebuffer_texture_2d(
                glow::READ_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(input.image.texture),
                0,
            );
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(self.draw));
            gl.framebuffer_texture_2d(
                glow::DRAW_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(self.texture),
                0,
            );
            if gl.check_framebuffer_status(glow::READ_FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
                || gl.check_framebuffer_status(glow::DRAW_FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
            {
                gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                return Err("N64 border adjustment framebuffer unavailable".into());
            }
            gl.disable(glow::SCISSOR_TEST);
            gl.blit_framebuffer(
                left as i32,
                top as i32,
                (w - right) as i32,
                (h - bottom) as i32,
                0,
                0,
                dims.0 as i32,
                dims.1 as i32,
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            Ok(FrameView {
                image: chain::Image {
                    texture: self.texture,
                    width: dims.0,
                    height: dims.1,
                },
                ..input
            })
        }
    }
    pub unsafe fn destroy(&mut self) {
        if let Some(fence) = self.fence.take() {
            self.gl.delete_sync(fence);
        }
        self.gl.delete_buffer(self.pbo);
        self.gl.delete_texture(self.texture);
        self.gl.delete_framebuffer(self.read);
        self.gl.delete_framebuffer(self.draw);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frame(w: usize, h: usize, left: usize, top: usize, right: usize, bottom: usize) -> Vec<u8> {
        let mut p = vec![0; w * h * 4];
        for y in top..h - bottom {
            for x in left..w - right {
                let i = (y * w + x) * 4;
                p[i..i + 4].copy_from_slice(&[150, 80, 30, 255]);
            }
        }
        p
    }
    #[test]
    fn ignores_dark_and_excessive_borders() {
        assert_eq!(margins(64, 48, &vec![0; 64 * 48 * 4]), None);
        assert_eq!(margins(64, 48, &frame(64, 48, 20, 0, 0, 0)), Some([0; 4]));
    }
    #[test]
    fn detects_near_black_padding_without_trimming_darker_picture() {
        let mut pixels = frame(320, 240, 0, 12, 0, 10);
        for y in (0..12).chain(230..240) {
            for x in 0..320 {
                let i = (y * 320 + x) * 4;
                pixels[i..i + 4].copy_from_slice(&[1, 1, 1, 255]);
            }
        }
        assert_eq!(margins(320, 240, &pixels), Some([0, 12, 0, 10]));
        let row = 12 * 320 * 4;
        pixels[row..row + 4].copy_from_slice(&[2, 2, 2, 255]);
        assert_eq!(margins(320, 240, &pixels), Some([0, 12, 0, 10]));
    }
    #[test]
    fn settles_conservatively_and_locks_across_resolution_changes() {
        let start = Instant::now();
        let mut d = Detector::default();
        let a = frame(64, 48, 4, 2, 4, 2);
        d.observe(64, 48, &a, start);
        d.observe(64, 48, &a, start + Duration::from_secs(2));
        assert!(d.locked.is_none());
        d.observe(64, 48, &a, start + Duration::from_secs(3));
        assert_eq!(d.locked, Some([4, 2, 4, 2]));
        d.observe(
            64,
            48,
            &vec![0; 64 * 48 * 4],
            start + Duration::from_secs(4),
        );
        assert_eq!(d.scaled(128, 96), Some([8, 4, 8, 4]));
    }
    #[test]
    fn transient_dark_frame_resets_settling() {
        let start = Instant::now();
        let mut d = Detector::default();
        let a = frame(64, 48, 2, 2, 2, 2);
        d.observe(64, 48, &a, start);
        d.observe(
            64,
            48,
            &vec![0; 64 * 48 * 4],
            start + Duration::from_secs(2),
        );
        d.observe(64, 48, &a, start + Duration::from_secs(4));
        assert!(d.locked.is_none());
    }
    #[test]
    fn tiny_startup_border_does_not_lock_before_larger_game_border() {
        let start = Instant::now();
        let mut d = Detector::default();
        let startup = frame(320, 240, 0, 1, 0, 2);
        d.observe(320, 240, &startup, start);
        d.observe(320, 240, &startup, start + Duration::from_secs(5));
        assert!(d.locked.is_none());
        let game = frame(320, 240, 0, 12, 0, 12);
        d.observe(320, 240, &game, start + Duration::from_secs(6));
        d.observe(320, 240, &game, start + Duration::from_secs(9));
        assert_eq!(d.locked, Some([0, 12, 0, 12]));
    }
    #[test]
    #[ignore = "requires a desktop OpenGL context"]
    fn gpu_crop_preserves_interior_and_asynchronous_readback() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let presenter = Presenter::new(&video).unwrap();
        let gl = &presenter.gl;
        let pixels = frame(64, 48, 4, 2, 4, 2);
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(presenter.texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                64,
                48,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&pixels)),
            );
        }
        let input = FrameView {
            image: chain::Image {
                texture: presenter.texture,
                width: 64,
                height: 48,
            },
            aspect: 4. / 3.,
            top_left: true,
            frame: 0,
        };
        let mut crop = AutoCrop::new(gl.clone()).unwrap();
        crop.process(input).unwrap();
        assert!(crop.fence.is_some());
        unsafe {
            gl.finish();
        }
        crop.process(input).unwrap();
        assert!(crop.detector.pending.is_some());
        crop.detector.locked = Some([4, 2, 4, 2]);
        crop.detector.size = (64, 48);
        let result = crop.process(input).unwrap();
        assert_eq!((result.image.width, result.image.height), (56, 44));
        unsafe {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(crop.read));
            gl.framebuffer_texture_2d(
                glow::READ_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(result.image.texture),
                0,
            );
            let mut pixel = [0_u8; 4];
            gl.read_pixels(
                0,
                0,
                1,
                1,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixel)),
            );
            assert_eq!(pixel, [150, 80, 30, 255]);
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
            crop.destroy();
        }
    }
}
