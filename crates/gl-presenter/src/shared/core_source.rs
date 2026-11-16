//! A core-owned stable framebuffer plus GPU snapshots into transport slots.
//! The stable target preserves NULL/no-callback duplicate frames across leases.
use super::*;
use emuluna_libretro_host::{Core, Frame};
use std::path::PathBuf;

pub(super) struct CoreSource<'a> {
    // Drop the core (including context_destroy) before deleting its framebuffer.
    core: Core,
    target: Target<'a>,
    frame: Frame,
    bottom_left: bool,
    diagnostics: u64,
}
/// Only the worker's upload/blit bindings are touched. Leave unrelated core
/// texture units alone; every transport operation explicitly uses unit zero.
/// Mirrors the libretro handoff rule: no frontend-owned objects stay bound.
pub(super) unsafe fn finish_transport(gl: &glow::Context) {
    gl.active_texture(glow::TEXTURE0);
    gl.bind_texture(glow::TEXTURE_2D, None);
    gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
    gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
    gl.bind_renderbuffer(glow::RENDERBUFFER, None);
    gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    gl.bind_vertex_array(None);
}
fn core_diagnostics(
    gl: &glow::Context,
    count: &mut u64,
    phase: &str,
    frame: u64,
) -> Result<(), String> {
    loop {
        let error = unsafe { gl.get_error() };
        if error == glow::NO_ERROR {
            return Ok(());
        }
        if error == glow::OUT_OF_MEMORY || error == glow::CONTEXT_LOST {
            return Err(format!(
                "Core graphics resource/context failure during {phase}: {error:#x}"
            ));
        }
        *count += 1;
        if *count <= 8 || count.is_multiple_of(600) {
            eprintln!("Core GL diagnostic during {phase}: {error:#x}; frame={frame} count={count}");
        }
    }
}
fn frontend_errors(gl: &glow::Context, phase: &str) -> Result<(), String> {
    let error = unsafe { gl.get_error() };
    if error == glow::NO_ERROR {
        Ok(())
    } else {
        Err(format!("Frontend GL error during {phase}: {error:#x}"))
    }
}
struct Target<'a> {
    gl: &'a glow::Context,
    texture: Option<glow::Texture>,
    framebuffer: Option<glow::Framebuffer>,
    depth: Option<glow::Renderbuffer>,
    vao: Option<glow::VertexArray>,
    width: u32,
    height: u32,
}
impl Target<'_> {
    fn new(gl: &glow::Context, width: u32, height: u32) -> Result<Target<'_>, String> {
        let mut target = Target {
            gl,
            texture: None,
            framebuffer: None,
            depth: None,
            vao: None,
            width,
            height,
        };
        // SAFETY: the caller owns this current worker context for the whole
        // CoreSource lifetime. Partial allocations are cleaned by Target::drop.
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            target.vao = Some(gl.create_vertex_array()?);
            gl.bind_vertex_array(target.vao);
            target.texture = Some(gl.create_texture()?);
            gl.bind_texture(glow::TEXTURE_2D, target.texture);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            target.depth = Some(gl.create_renderbuffer()?);
            gl.bind_renderbuffer(glow::RENDERBUFFER, target.depth);
            gl.renderbuffer_storage(
                glow::RENDERBUFFER,
                glow::DEPTH24_STENCIL8,
                width as i32,
                height as i32,
            );
            target.framebuffer = Some(gl.create_framebuffer()?);
            gl.bind_framebuffer(glow::FRAMEBUFFER, target.framebuffer);
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                target.texture,
                0,
            );
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_STENCIL_ATTACHMENT,
                glow::RENDERBUFFER,
                target.depth,
            );
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err("Core framebuffer is incomplete".into());
            }
            // A duplicate or partially rendered first frame must never expose
            // uninitialized color/depth/stencil storage around its viewport.
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(true, true, true, true);
            gl.depth_mask(true);
            gl.stencil_mask(0xff);
            gl.clear_color(0., 0., 0., 1.);
            gl.clear_depth_f64(1.);
            gl.clear_stencil(0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
            finish_transport(gl);
        }
        frontend_errors(gl, "core target allocation")?;
        Ok(target)
    }
}
impl Drop for Target<'_> {
    fn drop(&mut self) {
        unsafe {
            if let Some(framebuffer) = self.framebuffer {
                self.gl.delete_framebuffer(framebuffer);
            }
            if let Some(depth) = self.depth {
                self.gl.delete_renderbuffer(depth);
            }
            if let Some(texture) = self.texture {
                self.gl.delete_texture(texture);
            }
            if let Some(vao) = self.vao {
                self.gl.delete_vertex_array(vao);
            }
        }
    }
}
pub(super) struct Metadata {
    pub width: u32,
    pub height: u32,
    pub aspect: f64,
    pub top_left: bool,
    pub audio_frames: u64,
    pub cpu_video_bytes: u64,
    pub core_time: Duration,
    pub snapshot_time: Duration,
}
impl<'a> CoreSource<'a> {
    fn prepare_core_call(&self) -> Result<(), String> {
        unsafe {
            let gl = self.target.gl;
            gl.bind_framebuffer(glow::FRAMEBUFFER, self.target.framebuffer);
            if gl
                .get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING)
                .is_none()
            {
                gl.bind_vertex_array(self.target.vao);
            }
        }
        frontend_errors(self.target.gl, "core call setup")
    }
    fn finish_core_call(&mut self, phase: &str) -> Result<(), String> {
        let diagnostics = core_diagnostics(
            self.target.gl,
            &mut self.diagnostics,
            phase,
            self.frame.sequence,
        );
        unsafe {
            finish_transport(self.target.gl);
        }
        let cleanup = frontend_errors(self.target.gl, "core call cleanup");
        diagnostics?;
        cleanup
    }
    pub fn save_state(
        &mut self,
        path: &std::path::Path,
        identity: &emuluna_state_file::Identity,
    ) -> Result<(), String> {
        self.prepare_core_call()?;
        let result = self.core.save_state(path, identity);
        self.finish_core_call("state save")?;
        result
    }
    fn restore_prepared_raw(&mut self, path: &std::path::Path) -> Result<(), String> {
        for _ in 0..self.core.state_initialization_frames() {
            self.prepare_core_call()?;
            let outcome = self.core.run_into(0, &mut self.frame);
            self.finish_core_call("state initialization")?;
            if outcome? != emuluna_libretro_host::FrameOutcome::Frame {
                return Err("The core stopped while initializing for a saved state".into());
            }
        }
        self.prepare_core_call()?;
        let result = self.core.load_raw_state(path);
        self.finish_core_call("state load")?;
        result
    }
    pub fn load_prepared_state(
        &mut self,
        state: &emuluna_state_file::PreparedState,
    ) -> Result<(), String> {
        self.restore_prepared_raw(state.raw_path())
    }
    pub fn load_state(
        &mut self,
        path: &std::path::Path,
        identity: &emuluna_state_file::Identity,
    ) -> Result<(), String> {
        emuluna_state_file::with_payload(path, identity, |raw| self.restore_prepared_raw(raw))
    }
    pub fn reset(&mut self) -> Result<(), String> {
        self.prepare_core_call()?;
        self.core.reset();
        self.finish_core_call("reset")
    }
    pub fn set_runtime_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.core.set_runtime_option(key, value)
    }

    pub fn new(
        gl: &'a glow::Context,
        paths: &[PathBuf; 5],
        options: &[(String, String)],
    ) -> Result<Self, String> {
        // SAFETY: construction/run/drop all occur in this worker with its
        // context current. CoreSource cannot escape the scoped worker.
        // Advertise only the conservative core-profile version we requested.
        let version = unsafe {
            (
                gl.get_parameter_i32(glow::MAJOR_VERSION),
                gl.get_parameter_i32(glow::MINOR_VERSION),
            )
        };
        if version.0 < 3 || version.1 < 0 {
            return Err("Invalid worker GL capabilities".into());
        }
        let mut core = unsafe {
            Core::open_gl_with_options(paths, (version.0 as u32, version.1 as u32), options)?
        };
        let (width, height, bottom_left) = core.gl_geometry()?;
        eprintln!("core_fps={} core_max={}x{}", core.fps()?, width, height);
        let mut diagnostics = 0;
        core_diagnostics(gl, &mut diagnostics, "load game", 0)?;
        let target = Target::new(gl, width, height)?;
        if let Err(error) = unsafe { core.reset_gl_target(target.framebuffer.unwrap().0.get()) } {
            drop(core);
            drop(target);
            return Err(error);
        }
        let mut source = Self {
            core,
            target,
            frame: Frame::default(),
            bottom_left,
            diagnostics,
        };
        source.finish_core_call("context reset")?;
        Ok(source)
    }
    pub fn diagnostics(&self) -> u64 {
        self.diagnostics
    }
    pub fn set_audio_rate_adjustment(&mut self, ratio: f64) {
        self.core.set_audio_rate_adjustment(ratio);
    }
    pub fn frame_duration(&self) -> Result<Duration, String> {
        Ok(Duration::from_secs_f64(1. / self.core.fps()?))
    }
    pub fn audio(&mut self, muted: bool) -> &[i16] {
        if muted {
            self.frame.audio.fill(0);
        }
        &self.frame.audio
    }
    pub fn render(
        &mut self,
        gl: &glow::Context,
        slot: &mut Slot,
        inputs: &[emuluna_session::input::Snapshot],
    ) -> Result<Option<Metadata>, String> {
        self.prepare_core_call()?;
        let core_started = Instant::now();
        for (port, input) in inputs.iter().enumerate().skip(1) {
            self.core
                .set_port_input(port as u32, input.buttons, input.axes);
        }
        self.core.set_analog(inputs[0].axes);
        match self.core.run_into(inputs[0].buttons, &mut self.frame) {
            Ok(emuluna_libretro_host::FrameOutcome::Frame) => {}
            Ok(emuluna_libretro_host::FrameOutcome::Shutdown) => {
                self.finish_core_call("shutdown")?;
                return Ok(None);
            }
            Err(error) => {
                self.finish_core_call("failed frame")?;
                return Err(error);
            }
        }
        let core_time = core_started.elapsed();
        let snapshot_started = Instant::now();
        // Core-owned flags never poison validation of frontend operations.
        core_diagnostics(gl, &mut self.diagnostics, "retro_run", self.frame.sequence)?;
        let frame = &self.frame;
        if frame.width > self.target.width || frame.height > self.target.height {
            return Err("Core output exceeded its negotiated framebuffer size".into());
        }
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            gl.bind_texture(glow::TEXTURE_2D, Some(slot.texture));
            if slot.dimensions != (frame.width, frame.height) {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    frame.width as i32,
                    frame.height as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                slot.dimensions = (frame.width, frame.height);
            }
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!("Transport texture setup failed: {error:#x}"));
            }
            if frame.hardware {
                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.target.framebuffer);
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(slot.framebuffer));
                gl.disable(glow::SCISSOR_TEST);
                gl.read_buffer(glow::COLOR_ATTACHMENT0);
                gl.draw_buffer(glow::COLOR_ATTACHMENT0);
                if gl.check_framebuffer_status(glow::READ_FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
                    || gl.check_framebuffer_status(glow::DRAW_FRAMEBUFFER)
                        != glow::FRAMEBUFFER_COMPLETE
                {
                    return Err("Transport framebuffer incomplete after core frame".into());
                }
                gl.blit_framebuffer(
                    0,
                    0,
                    frame.width as i32,
                    frame.height as i32,
                    0,
                    0,
                    frame.width as i32,
                    frame.height as i32,
                    glow::COLOR_BUFFER_BIT,
                    glow::NEAREST,
                );
            } else {
                // Cores can switch to a CPU callback without losing the session.
                gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    frame.width as i32,
                    frame.height as i32,
                    glow::BGRA,
                    glow::UNSIGNED_INT_8_8_8_8_REV,
                    glow::PixelUnpackData::Slice(Some(&frame.pixels)),
                );
            }
            finish_transport(gl);
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!("Core GPU snapshot failed: {error:#x}"));
            }
        }
        Ok(Some(Metadata {
            width: frame.width,
            height: frame.height,
            aspect: frame.aspect,
            top_left: !frame.hardware || !self.bottom_left,
            audio_frames: (frame.audio.len() / 2) as u64,
            cpu_video_bytes: frame.pixels.len() as u64,
            core_time,
            snapshot_time: snapshot_started.elapsed(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires desktop GL"]
    fn live_initial_target_is_black_even_with_dirty_clear_state() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let presenter = Presenter::new(&video).unwrap();
        unsafe {
            let gl = &presenter.gl;
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(0, 0, 1, 1);
            gl.color_mask(false, false, false, false);
            gl.depth_mask(false);
            gl.stencil_mask(0);
            let target = Target::new(gl, 4, 4).unwrap();
            assert!(gl.get_parameter_texture(glow::TEXTURE_BINDING_2D).is_none());
            assert!(gl
                .get_parameter_renderbuffer(glow::RENDERBUFFER_BINDING)
                .is_none());
            assert!(gl
                .get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING)
                .is_none());
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, target.framebuffer);
            let mut pixels = [0u8; 64];
            gl.read_pixels(
                0,
                0,
                4,
                4,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            assert!(pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 0, 255]));
            assert_eq!(gl.get_error(), glow::NO_ERROR);
        }
    }
}
