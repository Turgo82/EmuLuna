//! Owns a desktop GL context and its objects on the window thread.
//! Software pixels upload once; filtering and presentation have no CPU readback.
use glow::HasContext;
use std::{collections::BTreeMap, rc::Rc, sync::Arc};
mod chain;
mod n64_crop;
mod overlay;
mod preview;
mod window;
pub use preview::Preview;
pub use window::WindowPreferences;
mod shared;
use sdl2::{
    pixels::PixelFormatEnum,
    surface::Surface,
    video::{FullscreenType, GLContext, GLProfile, SwapInterval, Window},
    VideoSubsystem,
};
pub use shared::{PlaybackOptions, SharedProbeStatistics};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    Nearest,
    Linear,
    Scanlines,
}
impl Filter {
    pub fn next(self) -> Self {
        match self {
            Self::Nearest => Self::Linear,
            Self::Linear => Self::Scanlines,
            Self::Scanlines => Self::Nearest,
        }
    }
}
#[derive(Default)]
pub struct Timing {
    pub upload: Duration,
    pub present: Duration,
}

/// Letterbox in physical drawable pixels, including high DPI windows.
fn viewport(width: u32, height: u32, aspect: f64) -> (i32, i32, i32, i32) {
    if width == 0 || height == 0 {
        return (0, 0, 0, 0);
    }
    let w = (height as f64 * aspect).round().clamp(1., width as f64) as u32;
    let h = (w as f64 / aspect).round().clamp(1., height as f64) as u32;
    (
        ((width - w) / 2) as i32,
        ((height - h) / 2) as i32,
        w as i32,
        h as i32,
    )
}
const VERTEX: &str = r#"#version 330 core
out vec2 uv;
uniform int source_top_left;
void main() {
    vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
    uv = vec2(p.x, source_top_left != 0 ? 1.0 - p.y : p.y);
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}"#;
const FRAGMENT: &str = r#"#version 330 core
in vec2 uv;
out vec4 color;
uniform sampler2D source;
uniform int filter_mode;
uniform float source_height;
void main() {
    vec3 rgb = texture(source, uv).rgb;
    if (filter_mode == 2) {
        float beam = 0.82 + 0.18 * cos(6.28318530718 * (uv.y * source_height - 0.5));
        rgb *= beam;
    }
    color = vec4(rgb, 1.0);
}"#;

#[derive(Clone, Copy)]
struct FrameView {
    image: chain::Image,
    aspect: f64,
    top_left: bool,
    frame: u64,
}
pub struct Presenter {
    gl: Arc<glow::Context>,
    program: glow::Program,
    vao: glow::VertexArray,
    texture: glow::Texture,
    nearest_sampler: glow::Sampler,
    linear_sampler: glow::Sampler,
    last_uniforms: Option<(Filter, u32, bool)>,
    filter_mode: Option<glow::UniformLocation>,
    source_height: Option<glow::UniformLocation>,
    source_top_left: Option<glow::UniformLocation>,
    dimensions: (u32, u32),
    filter: Filter,
    preset: Option<chain::Pipeline>,
    preset_key: Option<String>,
    shader_system: String,
    shader_settings: BTreeMap<String, String>,
    shader_pending: BTreeMap<String, String>,
    shader_resolution: u32,
    frames: u64,
    generation: Option<u64>,
    cached: Option<(FrameView, Filter)>,
    last_source: Option<FrameView>,
    overlay: Option<overlay::Overlay>,
    n64_crop: Option<n64_crop::AutoCrop>,
    n64_preserve_aspect: bool,
    window_state: window::WindowState,
    // Neither SDL window nor context is Send. GL handles never escape this owner.
    context: Rc<GLContext>,
    window: Window,
}
impl Presenter {
    pub fn set_title(&mut self, title: &str) -> Result<(), String> {
        self.window.set_title(title).map_err(|e| e.to_string())
    }
    pub fn window_id(&self) -> u32 {
        self.window.id()
    }
    pub fn new(video: &VideoSubsystem) -> Result<Self, String> {
        let attributes = video.gl_attr();
        attributes.set_context_profile(GLProfile::Core);
        attributes.set_context_version(3, 3);
        attributes.set_double_buffer(true);
        attributes.set_depth_size(0);
        let mut window = video
            .window("EmuLuna — Rust GPU playback prototype", 960, 720)
            .opengl()
            .allow_highdpi()
            .position_centered()
            .resizable()
            .build()
            .map_err(|e| e.to_string())?;
        let mut icon = image::load_from_memory(include_bytes!("../../../data/branding/emuluna.png"))
            .map_err(|e| e.to_string())?
            .thumbnail(128, 128)
            .into_rgba8();
        let (icon_width, icon_height) = icon.dimensions();
        let surface = Surface::from_data(
            icon.as_mut(),
            icon_width,
            icon_height,
            icon_width * 4,
            PixelFormatEnum::RGBA32,
        )?;
        window.set_icon(surface);
        let context = window.gl_create_context()?;
        window.gl_make_current(&context)?;
        // The diagnostic keeps presentation from controlling the worker's clock.
        // Report inability to disable vsync rather than silently assuming it.
        if let Err(error) = video.gl_set_swap_interval(SwapInterval::Immediate) {
            eprintln!("Could not disable swap pacing: {error}");
        }
        // SAFETY: SDL created and made this context current on this thread. The
        // window/context remain owned here until all GL objects are deleted.
        let gl = unsafe {
            glow::Context::from_loader_function(|name| video.gl_get_proc_address(name).cast())
        };
        unsafe {
            eprintln!(
                "renderer=rust-opengl gl={} device={}",
                gl.get_parameter_string(glow::VERSION),
                gl.get_parameter_string(glow::RENDERER)
            );
            let program = program(&gl)?;
            let vao = gl.create_vertex_array()?;
            let texture = gl.create_texture()?;
            let nearest_sampler = gl.create_sampler()?;
            let linear_sampler = gl.create_sampler()?;
            for (sampler, sampling) in [
                (nearest_sampler, glow::NEAREST),
                (linear_sampler, glow::LINEAR),
            ] {
                gl.sampler_parameter_i32(sampler, glow::TEXTURE_MIN_FILTER, sampling as i32);
                gl.sampler_parameter_i32(sampler, glow::TEXTURE_MAG_FILTER, sampling as i32);
                gl.sampler_parameter_i32(sampler, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                gl.sampler_parameter_i32(sampler, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.use_program(Some(program));
            gl.uniform_1_i32(gl.get_uniform_location(program, "source").as_ref(), 0);
            let filter_mode = gl.get_uniform_location(program, "filter_mode");
            let source_height = gl.get_uniform_location(program, "source_height");
            let source_top_left = gl.get_uniform_location(program, "source_top_left");
            Ok(Self {
                gl: Arc::new(gl),
                program,
                vao,
                texture,
                nearest_sampler,
                linear_sampler,
                last_uniforms: None,
                filter_mode,
                source_height,
                source_top_left,
                dimensions: (0, 0),
                filter: Filter::Nearest,
                preset: None,
                preset_key: None,
                shader_system: String::new(),
                shader_settings: BTreeMap::new(),
                shader_pending: BTreeMap::new(),
                shader_resolution: 0,
                frames: 0,
                generation: None,
                cached: None,
                last_source: None,
                overlay: None,
                n64_crop: None,
                n64_preserve_aspect: false,
                window_state: window::WindowState::default(),
                context: Rc::new(context),
                window,
            })
        }
    }
    fn ensure_current(&self) -> Result<(), String> {
        if self.context.is_current() {
            Ok(())
        } else {
            self.window.gl_make_current(&self.context)
        }
    }
    pub fn toggle_fullscreen(&mut self) -> Result<(), String> {
        let mode = if self.window.fullscreen_state() == FullscreenType::Off {
            FullscreenType::Desktop
        } else {
            FullscreenType::Off
        };
        self.window.set_fullscreen(mode)
    }
    pub fn output_size(&self) -> (u32, u32) {
        self.window.drawable_size()
    }
    pub fn set_filter(&mut self, filter: Filter) {
        self.cached = None;
        self.preset = None;
        self.preset_key = None;
        self.filter = filter;
        self.refresh_shader_controls();
    }
    pub fn select_preset(&mut self, root: &std::path::Path, key: &str) -> Result<(), String> {
        if key == "nearest" {
            self.set_filter(Filter::Nearest);
            return Ok(());
        }
        if key == "scanlines" {
            self.set_filter(Filter::Scanlines);
            return Ok(());
        }
        if key == "linear" {
            self.set_filter(Filter::Linear);
            return Ok(());
        }
        self.ensure_current()?;
        let mut pipeline = chain::Pipeline::new(
            self.gl.clone(),
            self.context.clone(),
            self.window.clone(),
            root,
            key,
        )?;
        let setting = format!("shader_parameters.{}.{key}", self.shader_system);
        if let Some(values) = self
            .shader_settings
            .get(&setting)
            .and_then(|json| serde_json::from_str(json).ok())
        {
            pipeline.apply_parameters(&values);
        }
        self.cached = None;
        self.preset = Some(pipeline);
        self.preset_key = Some(key.to_owned());
        self.last_uniforms = None;
        self.refresh_shader_controls();
        Ok(())
    }
    pub fn configure_shader_settings(&mut self, system: &str) {
        self.shader_system = system.to_owned();
        self.shader_settings = std::env::var_os("EMULUNA_SHADER_SETTINGS_FILE")
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        self.shader_resolution = self
            .shader_settings
            .get(&format!("filter_resolution.{system}"))
            .and_then(|value| value.parse().ok())
            .filter(|value| matches!(value, 720 | 1080))
            .unwrap_or(0);
        self.refresh_shader_controls();
    }
    pub fn preset_key(&self) -> Option<&str> {
        self.preset_key.as_deref()
    }
    pub fn filter(&self) -> Filter {
        self.filter
    }
    pub fn draw(
        &mut self,
        width: u32,
        height: u32,
        aspect: f64,
        pixels: &[u8],
    ) -> Result<Timing, String> {
        self.draw_inner(width, height, aspect, pixels, true)
    }
    fn draw_inner(
        &mut self,
        width: u32,
        height: u32,
        aspect: f64,
        pixels: &[u8],
        swap: bool,
    ) -> Result<Timing, String> {
        let frame = self.frames;
        self.frames = self.frames.wrapping_add(1);
        self.draw_at(width, height, aspect, pixels, frame, swap)
    }
    fn accept_generation(&mut self, generation: u64) {
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            self.cached = None;
            self.last_source = None;
            if let Some(pipeline) = self.preset.as_mut() {
                pipeline.invalidate();
            }
        }
    }
    pub fn draw_frame(&mut self, frame: &emuluna_libretro_host::Frame) -> Result<Timing, String> {
        self.accept_generation(frame.generation);
        self.draw_at(
            frame.width,
            frame.height,
            frame.aspect,
            &frame.pixels,
            frame.sequence,
            true,
        )
    }
    /// Explicit one-frame developer capture, never used by ordinary playback.
    pub fn draw_frame_captured(
        &mut self,
        frame: &emuluna_libretro_host::Frame,
        path: &std::path::Path,
    ) -> Result<Timing, String> {
        self.accept_generation(frame.generation);
        let timing = self.draw_at(
            frame.width,
            frame.height,
            frame.aspect,
            &frame.pixels,
            frame.sequence,
            false,
        )?;
        self.capture_diagnostic(path)?;
        self.window.gl_swap_window();
        Ok(timing)
    }
    fn draw_at(
        &mut self,
        width: u32,
        height: u32,
        aspect: f64,
        pixels: &[u8],
        frame: u64,
        swap: bool,
    ) -> Result<Timing, String> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || pixels.len() != width as usize * height as usize * 4
            || !aspect.is_finite()
            || !(0.1..=10.).contains(&aspect)
        {
            return Err("Invalid GPU input geometry or pixel storage".into());
        }
        self.ensure_current()?;
        let before = Instant::now();
        // SAFETY: validated packed native ARGB32 storage; live GL objects owned
        // by the current context. No framebuffer is mapped or read back here.
        unsafe {
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            self.gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            if self.dimensions != (width, height) {
                self.gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    width as i32,
                    height as i32,
                    0,
                    glow::BGRA,
                    glow::UNSIGNED_INT_8_8_8_8_REV,
                    glow::PixelUnpackData::Slice(None),
                );
                self.dimensions = (width, height);
            }
            self.gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                width as i32,
                height as i32,
                glow::BGRA,
                glow::UNSIGNED_INT_8_8_8_8_REV,
                glow::PixelUnpackData::Slice(Some(pixels)),
            );
        }
        let upload = before.elapsed();
        let present = self.present_texture(
            FrameView {
                image: chain::Image {
                    texture: self.texture,
                    width,
                    height,
                },
                aspect,
                top_left: true,
                frame,
            },
            swap,
        )?;
        Ok(Timing { upload, present })
    }
    // Caller owns the texture and must establish producer-ready synchronization
    // before sampling it. This private boundary is also used by the shared probe.
    fn present_texture(&mut self, input: FrameView, swap: bool) -> Result<Duration, String> {
        self.ensure_current()?;
        let before = Instant::now();
        self.update_window_aspect(input)?;
        self.last_source = Some(input);
        let (dw, dh) = self.window.drawable_size();
        if dw == 0 || dh == 0 {
            return Ok(before.elapsed());
        }
        self.last_source = Some(input);
        let mut adjusted = match self.n64_crop.as_mut().map(|crop| crop.process(input)) {
            Some(Ok(frame)) => frame,
            Some(Err(error)) => {
                eprintln!("N64 border adjustment disabled: {error}");
                if let Err(cleanup_error) = self.configure_n64_auto_crop(false) {
                    eprintln!("Could not release border adjustment: {cleanup_error}");
                }
                self.overlay_message("Automatic screen adjustment unavailable".into());
                input
            }
            None => input,
        };
        if self.n64_preserve_aspect && self.n64_crop.is_some() {
            let base_aspect = if self.window_state.preferences.square_pixels {
                f64::from(input.image.width) / f64::from(input.image.height)
            } else {
                input.aspect
            };
            adjusted.aspect = base_aspect * f64::from(adjusted.image.width)
                / f64::from(input.image.width)
                * f64::from(input.image.height)
                / f64::from(adjusted.image.height);
        }
        let (x, y, w, h) = self.content_viewport(dw, dh, adjusted);
        let mut image = adjusted.image;
        let mut top_left = input.top_left;
        let effective_filter = if let Some(pipeline) = self.preset.as_mut() {
            let mut shader_viewport = (w as u32, h as u32);
            if self.shader_resolution > 0 && shader_viewport.1 > self.shader_resolution {
                let scale = self.shader_resolution as f64 / shader_viewport.1 as f64;
                shader_viewport = (
                    (shader_viewport.0 as f64 * scale).round().max(1.) as u32,
                    self.shader_resolution,
                );
            }
            image = pipeline.render(image, top_left, shader_viewport, input.frame)?;
            top_left = true;
            if (image.width, image.height) == (w as u32, h as u32) {
                Filter::Nearest
            } else {
                Filter::Linear
            }
        } else {
            self.filter
        };
        self.cached = Some((
            FrameView {
                image,
                top_left,
                ..adjusted
            },
            effective_filter,
        ));
        self.paint_image(image, top_left, effective_filter, (x, y, w, h))?;
        self.paint_overlay()?;
        if swap {
            self.window.gl_swap_window();
        }
        Ok(before.elapsed())
    }
    /// Repaint UI over the last completed image. Never upload pixels, execute
    /// shader passes, or advance temporal history on a UI-only redraw.
    /// Shared-texture callers must retain the source lease until replaced.
    pub fn redraw(&mut self) -> Result<(), String> {
        self.redraw_inner(true)
    }
    fn redraw_inner(&mut self, swap: bool) -> Result<(), String> {
        self.ensure_current()?;
        if self.cached.is_none() {
            if let Some(source) = self.last_source {
                self.present_texture(source, swap)?;
                return Ok(());
            }
        }
        if let Some((view, filter)) = self.cached {
            let (dw, dh) = self.window.drawable_size();
            self.paint_image(
                view.image,
                view.top_left,
                filter,
                self.content_viewport(dw, dh, view),
            )?;
            self.paint_overlay()?;
            if swap {
                self.window.gl_swap_window();
            }
        }
        Ok(())
    }
    fn paint_image(
        &mut self,
        image: chain::Image,
        top_left: bool,
        effective_filter: Filter,
        (x, y, w, h): (i32, i32, i32, i32),
    ) -> Result<(), String> {
        let height = image.height;
        unsafe {
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(image.texture));
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.disable(glow::BLEND);
            self.gl.clear_color(0., 0., 0., 1.);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
            self.gl.viewport(x, y, w, h);
            self.gl.bind_sampler(
                0,
                Some(if effective_filter == Filter::Nearest {
                    self.nearest_sampler
                } else {
                    self.linear_sampler
                }),
            );
            self.gl.use_program(Some(self.program));
            if self.last_uniforms != Some((effective_filter, height, top_left)) {
                self.gl
                    .uniform_1_i32(self.source_top_left.as_ref(), i32::from(top_left));
                self.gl.uniform_1_i32(
                    self.filter_mode.as_ref(),
                    match effective_filter {
                        Filter::Nearest => 0,
                        Filter::Linear => 1,
                        Filter::Scanlines => 2,
                    },
                );
                self.gl
                    .uniform_1_f32(self.source_height.as_ref(), height as f32);
                self.last_uniforms = Some((effective_filter, height, top_left));
            }
            self.gl.bind_vertex_array(Some(self.vao));
            self.gl.draw_arrays(glow::TRIANGLES, 0, 3);
            let error = self.gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!("GPU presentation error: {error:#x}"));
            }
        }
        Ok(())
    }
}
impl Drop for Presenter {
    fn drop(&mut self) {
        if let Err(error) = self.flush_shader_settings() {
            eprintln!("Could not remember shader settings: {error}");
        }
        if self.window.gl_make_current(&self.context).is_ok() {
            if let Some(mut overlay) = self.overlay.take() {
                overlay.destroy();
            }
            self.preset.take();
            // SAFETY: same owning thread/context; no external leases exist.
            unsafe {
                if let Some(mut crop) = self.n64_crop.take() {
                    crop.destroy();
                }
                self.gl.delete_sampler(self.nearest_sampler);
                self.gl.delete_sampler(self.linear_sampler);
                self.gl.delete_texture(self.texture);
                self.gl.delete_vertex_array(self.vao);
                self.gl.delete_program(self.program);
            }
        }
        // If restoring the context failed, context destruction releases objects.
    }
}
// SAFETY: called only while the presenter's context is current. Shader/program
// handles remain local until successfully linked; errors clean up temporary GL objects.
unsafe fn program(gl: &glow::Context) -> Result<glow::Program, String> {
    let program = gl.create_program()?;
    for (kind, source) in [
        (glow::VERTEX_SHADER, VERTEX),
        (glow::FRAGMENT_SHADER, FRAGMENT),
    ] {
        let shader = match gl.create_shader(kind) {
            Ok(shader) => shader,
            Err(error) => {
                gl.delete_program(program);
                return Err(error);
            }
        };
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let error = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            gl.delete_program(program);
            return Err(format!("Shader compile failed: {error}"));
        }
        gl.attach_shader(program, shader);
        gl.delete_shader(shader);
    }
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        let error = gl.get_program_info_log(program);
        gl.delete_program(program);
        return Err(format!("Shader link failed: {error}"));
    }
    Ok(program)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_uses_physical_pixels_and_centers_borders() {
        assert_eq!(viewport(1920, 1080, 4. / 3.), (240, 0, 1440, 1080));
        assert_eq!(viewport(800, 800, 4. / 3.), (0, 100, 800, 600));
        assert_eq!(viewport(0, 0, 4. / 3.), (0, 0, 0, 0));
    }
    #[test]
    #[ignore = "requires desktop OpenGL; run explicitly on one test thread"]
    fn live_gpu_colors_orientation_filters_and_resize() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        let pixels: Vec<u8> = [0xffff0000u32, 0xff00ff00, 0xff0000ff, 0xffffffff]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        fn read(p: &Presenter, x: i32, y: i32) -> [u8; 4] {
            let mut pixel = [0; 4];
            // Diagnostic-only readback. Ordinary draw/present never reads pixels.
            unsafe {
                p.gl.read_pixels(
                    x,
                    y,
                    1,
                    1,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixel)),
                );
                assert_eq!(p.gl.get_error(), glow::NO_ERROR);
            }
            pixel
        }
        for (width, height) in [(960, 720), (600, 800)] {
            presenter.window.set_size(width, height).unwrap();
            // SDL may report a compositor-adjusted physical size.
            presenter.set_filter(Filter::Nearest);
            presenter.draw_inner(2, 2, 1., &pixels, false).unwrap();
            let (dw, dh) = presenter.window.drawable_size();
            let (x, y, w, h) = viewport(dw, dh, 1.);
            assert_eq!(read(&presenter, x + w / 4, y + 3 * h / 4), [255, 0, 0, 255]);
            assert_eq!(
                read(&presenter, x + 3 * w / 4, y + 3 * h / 4),
                [0, 255, 0, 255]
            );
            assert_eq!(read(&presenter, x + w / 4, y + h / 4), [0, 0, 255, 255]);
            assert_eq!(
                read(&presenter, x + 3 * w / 4, y + h / 4),
                [255, 255, 255, 255]
            );
            if x > 0 || y > 0 {
                assert_eq!(read(&presenter, 0, 0), [0, 0, 0, 255]);
            }
            presenter.set_filter(Filter::Linear);
            presenter.draw_inner(2, 2, 1., &pixels, false).unwrap();
            let center = read(&presenter, x + w / 2, y + h / 2);
            assert!(
                center[..3].iter().all(|c| (120..=136).contains(c)),
                "{center:?}"
            );
            presenter.set_filter(Filter::Scanlines);
            presenter.draw_inner(2, 2, 1., &pixels, false).unwrap();
            let scan = read(&presenter, x + w / 2, y + h / 2);
            assert!(scan[0] < center[0] && scan[1] < center[1] && scan[2] < center[2]);
        }
        presenter.toggle_fullscreen().unwrap();
        assert_eq!(presenter.window.fullscreen_state(), FullscreenType::Desktop);
        presenter.draw_inner(2, 2, 1., &pixels, false).unwrap();
        presenter.toggle_fullscreen().unwrap();
        assert_eq!(presenter.window.fullscreen_state(), FullscreenType::Off);
        // Change texture storage size, including non-square rows.
        presenter.draw_inner(3, 1, 3., &[255; 12], false).unwrap();
        assert!(presenter.draw_inner(3, 1, 3., &[0; 4], false).is_err());
    }
}
