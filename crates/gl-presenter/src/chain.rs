//! Native implementation of the shipped preset vocabulary. No runtime Python.
use super::*;
use emuluna_shader_pack::{Pack, Pass, Preset};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    rc::Rc,
};
#[derive(Clone)]
struct Owner {
    gl: Arc<glow::Context>,
    context: Rc<GLContext>,
    window: Window,
}
impl Owner {
    fn current(&self) -> bool {
        self.context.is_current() || self.window.gl_make_current(&self.context).is_ok()
    }
}
#[derive(Clone, Copy)]
pub(super) struct Image {
    pub texture: glow::Texture,
    pub width: u32,
    pub height: u32,
}
struct Texture {
    owner: Owner,
    image: Image,
    framebuffer: Option<glow::Framebuffer>,
}
impl Texture {
    fn new(
        owner: &Owner,
        width: u32,
        height: u32,
        format: u32,
        target: bool,
    ) -> Result<Self, String> {
        let gl = &owner.gl;
        let texture = unsafe { gl.create_texture()? };
        let mut result = Self {
            owner: owner.clone(),
            image: Image {
                texture,
                width,
                height,
            },
            framebuffer: None,
        };
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                format as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            if target {
                result.framebuffer = Some(gl.create_framebuffer()?);
                gl.bind_framebuffer(glow::FRAMEBUFFER, result.framebuffer);
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                    return Err("Preset framebuffer is incomplete".into());
                }
                gl.clear_color(0., 0., 0., 1.);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }
        }
        Ok(result)
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        if self.owner.current() {
            unsafe {
                if let Some(framebuffer) = self.framebuffer {
                    self.owner.gl.delete_framebuffer(framebuffer);
                }
                self.owner.gl.delete_texture(self.image.texture);
            }
        }
    }
}
struct Program {
    owner: Owner,
    id: glow::Program,
    uniforms: Vec<Uniform>,
}
struct Uniform {
    location: glow::UniformLocation,
    kind: u32,
    source: Semantic,
}
#[derive(Clone, Copy)]
enum TextureRef {
    Source,
    Original,
    History(usize),
    Pass(usize),
    Feedback(usize),
    Lookup(usize),
}
enum Semantic {
    Texture(TextureRef),
    Size(TextureRef),
    OutputSize,
    ViewportSize,
    Matrix,
    Parameter(usize),
    FrameCount,
    Direction,
    Delta,
    Aspect,
    Rotation,
    One,
    Ignored,
}
fn texture_ref(name: &str, spec: &Preset) -> Option<TextureRef> {
    match name {
        "Source" | "Texture" => return Some(TextureRef::Source),
        "Original" => return Some(TextureRef::Original),
        _ => {}
    }
    for (prefix, kind) in [
        ("OriginalHistory", 0),
        ("PassOutput", 1),
        ("PassFeedback", 2),
    ] {
        if let Some(index) = name.strip_prefix(prefix).and_then(|n| n.parse().ok()) {
            return Some(match kind {
                0 => TextureRef::History(index),
                1 => TextureRef::Pass(index),
                _ => TextureRef::Feedback(index),
            });
        }
    }
    for (index, pass) in spec.passes.iter().enumerate() {
        if !pass.alias.is_empty() {
            if name == pass.alias {
                return Some(TextureRef::Pass(index));
            }
            if name.strip_suffix("Feedback") == Some(pass.alias.as_str()) {
                return Some(TextureRef::Feedback(index));
            }
        }
    }
    spec.textures
        .keys()
        .position(|key| key == name)
        .map(TextureRef::Lookup)
}
fn semantic(name: &str, kind: u32, spec: &Preset) -> Result<Semantic, String> {
    let name = name.rsplit('.').next().unwrap_or(name);
    if kind == glow::SAMPLER_2D {
        return texture_ref(name, spec)
            .map(Semantic::Texture)
            .ok_or_else(|| format!("Unsupported preset texture: {name}"));
    }
    if kind == glow::FLOAT_MAT4 {
        return Ok(Semantic::Matrix);
    }
    if let Some(index) = spec.values.keys().position(|key| key == name) {
        return Ok(Semantic::Parameter(index));
    }
    Ok(match name {
        "OutputSize" => Semantic::OutputSize,
        "FinalViewportSize" => Semantic::ViewportSize,
        "InputSize" | "TextureSize" => Semantic::Size(TextureRef::Source),
        "FrameCount" => Semantic::FrameCount,
        "FrameDirection" => Semantic::Direction,
        "FrameTimeDelta" => Semantic::Delta,
        "OriginalAspect" | "OriginalAspectRotated" => Semantic::Aspect,
        "Rotation" => Semantic::Rotation,
        "TotalSubFrames" | "CurrentSubFrame" => Semantic::One,
        _ => name
            .strip_suffix("Size")
            .and_then(|name| texture_ref(name, spec))
            .map(Semantic::Size)
            .unwrap_or(Semantic::Ignored),
    })
}
impl Program {
    fn new(owner: &Owner, pass: &Pass, spec: &Preset) -> Result<Self, String> {
        let gl = &owner.gl;
        let mut result = Self {
            owner: owner.clone(),
            id: unsafe { gl.create_program()? },
            uniforms: Vec::new(),
        };
        unsafe {
            for (kind, source) in [
                (glow::VERTEX_SHADER, &pass.vertex),
                (glow::FRAGMENT_SHADER, &pass.fragment),
            ] {
                let shader = gl.create_shader(kind)?;
                gl.shader_source(shader, source);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let error = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    return Err(format!("Preset stage compilation failed: {error}"));
                }
                gl.attach_shader(result.id, shader);
                gl.delete_shader(shader);
            }
            for (name, index) in [
                ("Position", 0),
                ("VertexCoord", 0),
                ("TexCoord", 1),
                ("COLOR", 2),
            ] {
                gl.bind_attrib_location(result.id, index, name);
            }
            gl.link_program(result.id);
            if !gl.get_program_link_status(result.id) {
                return Err(format!(
                    "Preset link failed: {}",
                    gl.get_program_info_log(result.id)
                ));
            }
            for index in 0..gl.get_active_uniforms(result.id) {
                if let Some(uniform) = gl.get_active_uniform(result.id, index) {
                    if let Some(location) = gl.get_uniform_location(result.id, &uniform.name) {
                        result.uniforms.push(Uniform {
                            location,
                            kind: uniform.utype,
                            source: semantic(&uniform.name, uniform.utype, spec)?,
                        });
                    }
                }
            }
        }
        Ok(result)
    }
}
impl Drop for Program {
    fn drop(&mut self) {
        if self.owner.current() {
            unsafe {
                self.owner.gl.delete_program(self.id);
            }
        }
    }
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Sampling {
    linear: bool,
    wrap: String,
    mipmap: bool,
}
pub(super) struct Pipeline {
    owner: Owner,
    spec: Preset,
    parameter_values: Vec<f32>,
    programs: Vec<Program>,
    samplers: BTreeMap<Sampling, glow::Sampler>,
    pass_samplers: Vec<glow::Sampler>,
    lookup_samplers: Vec<glow::Sampler>,
    vao: Option<glow::VertexArray>,
    vbo: Option<glow::Buffer>,
    read_fbo: Option<glow::Framebuffer>,
    history: Vec<Texture>,
    history_count: usize,
    outputs: Vec<Vec<Texture>>,
    lookups: Vec<Texture>,
    feedback: BTreeSet<usize>,
    phase: usize,
    dimensions: Vec<(u32, u32, u32, usize)>,
    max_size: u32,
    max_units: u32,
    generated: Vec<glow::Texture>,
    layout_key: Option<(u32, u32, u32, u32)>,
    frame_origin: Option<u64>,
    reset_history: bool,
}
impl Pipeline {
    pub fn editable_parameters(&self) -> Vec<(emuluna_shader_pack::Parameter, f32)> {
        self.spec
            .parameters
            .iter()
            .filter_map(|parameter| {
                let index = self
                    .spec
                    .values
                    .keys()
                    .position(|key| key == &parameter.key)?;
                Some((parameter.clone(), self.parameter_values[index]))
            })
            .collect()
    }

    pub fn apply_parameters(&mut self, values: &BTreeMap<String, f32>) {
        for parameter in &self.spec.parameters {
            let Some(value) = values
                .get(&parameter.key)
                .copied()
                .filter(|value| value.is_finite())
            else {
                continue;
            };
            if let Some(index) = self
                .spec
                .values
                .keys()
                .position(|key| key == &parameter.key)
            {
                self.parameter_values[index] = value.clamp(parameter.minimum, parameter.maximum);
            }
        }
    }

    pub fn new(
        gl: Arc<glow::Context>,
        context: Rc<GLContext>,
        window: Window,
        root: &Path,
        key: &str,
    ) -> Result<Self, String> {
        let owner = Owner {
            gl,
            context,
            window,
        };
        if !owner.current() {
            return Err("Preset context is unavailable".into());
        }
        let pack = Pack::open(root)?;
        let spec = pack.preset(key)?;
        let max_size =
            unsafe { owner.gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE) }.min(8192) as u32;
        let max_units = unsafe { owner.gl.get_parameter_i32(glow::MAX_TEXTURE_IMAGE_UNITS) } as u32;
        let mut result = Self {
            owner,
            parameter_values: spec.values.values().copied().collect(),
            spec,
            programs: Vec::new(),
            samplers: BTreeMap::new(),
            pass_samplers: Vec::new(),
            lookup_samplers: Vec::new(),
            vao: None,
            vbo: None,
            read_fbo: None,
            history: Vec::new(),
            history_count: 0,
            outputs: Vec::new(),
            lookups: Vec::new(),
            feedback: BTreeSet::new(),
            phase: 0,
            dimensions: Vec::new(),
            max_size,
            max_units,
            generated: Vec::with_capacity(200),
            layout_key: None,
            frame_origin: None,
            reset_history: false,
        };
        unsafe {
            result.vao = Some(result.owner.gl.create_vertex_array()?);
            result.vbo = Some(result.owner.gl.create_buffer()?);
            result.read_fbo = Some(result.owner.gl.create_framebuffer()?);
            let gl = &result.owner.gl;
            gl.bind_vertex_array(result.vao);
            gl.bind_buffer(glow::ARRAY_BUFFER, result.vbo);
            let vertices: [f32; 16] = [
                0., 0., 0., 0., 1., 0., 1., 0., 0., 1., 0., 1., 1., 1., 1., 1.,
            ];
            let data: Vec<_> = vertices.into_iter().flat_map(f32::to_ne_bytes).collect();
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &data, glow::STATIC_DRAW);
            for (index, offset) in [(0, 0), (1, 8)] {
                gl.enable_vertex_attrib_array(index);
                gl.vertex_attrib_pointer_f32(index, 2, glow::FLOAT, false, 16, offset);
            }
            gl.disable_vertex_attrib_array(2);
            gl.vertex_attrib_4_f32(2, 1., 1., 1., 1.);
        }
        for pass in &result.spec.passes {
            result
                .programs
                .push(Program::new(&result.owner, pass, &result.spec)?);
        }
        for program in &result.programs {
            for uniform in &program.uniforms {
                match uniform.source {
                    Semantic::Texture(TextureRef::History(index)) => {
                        result.history_count = result.history_count.max(index)
                    }
                    Semantic::Texture(TextureRef::Feedback(index)) => {
                        result.feedback.insert(index);
                    }
                    _ => {}
                }
            }
        }
        if result.history_count > 32 || result.feedback.iter().any(|i| *i >= result.programs.len())
        {
            return Err("Preset references unsupported frame history".into());
        }
        let pass_sampling: Vec<_> = result
            .spec
            .passes
            .iter()
            .map(|p| Sampling {
                linear: p.linear,
                wrap: p.wrap.clone(),
                mipmap: p.mipmap,
            })
            .collect();
        for sampling in pass_sampling {
            let sampler = result.sampler(sampling)?;
            result.pass_samplers.push(sampler);
        }
        let lookup_sampling: Vec<_> = result
            .spec
            .textures
            .values()
            .map(|p| Sampling {
                linear: p.linear,
                wrap: p.wrap.clone(),
                mipmap: p.mipmap,
            })
            .collect();
        for sampling in lookup_sampling {
            let sampler = result.sampler(sampling)?;
            result.lookup_samplers.push(sampler);
        }
        for spec in result.spec.textures.values() {
            let image = image::open(pack.asset(&spec.path)?)
                .map_err(|e| format!("Preset texture could not load: {e}"))?
                .to_rgba8();
            let (w, h) = image.dimensions();
            if w == 0 || h == 0 || w > result.max_size || h > result.max_size {
                return Err("Preset texture exceeds driver limits".into());
            }
            let texture = Texture::new(&result.owner, w, h, glow::RGBA8, false)?;
            unsafe {
                let gl = &result.owner.gl;
                gl.bind_texture(glow::TEXTURE_2D, Some(texture.image.texture));
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    w as i32,
                    h as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(image.as_raw())),
                );
                if spec.mipmap {
                    gl.generate_mipmap(glow::TEXTURE_2D);
                }
            }
            result.lookups.push(texture);
        }
        Ok(result)
    }
    fn sampler(&mut self, sampling: Sampling) -> Result<glow::Sampler, String> {
        if let Some(sampler) = self.samplers.get(&sampling) {
            return Ok(*sampler);
        }
        let gl = &self.owner.gl;
        let sampler = unsafe { gl.create_sampler()? };
        unsafe {
            let min = match (sampling.linear, sampling.mipmap) {
                (false, false) => glow::NEAREST,
                (true, false) => glow::LINEAR,
                (false, true) => glow::NEAREST_MIPMAP_NEAREST,
                (true, true) => glow::LINEAR_MIPMAP_LINEAR,
            };
            gl.sampler_parameter_i32(sampler, glow::TEXTURE_MIN_FILTER, min as i32);
            gl.sampler_parameter_i32(
                sampler,
                glow::TEXTURE_MAG_FILTER,
                if sampling.linear {
                    glow::LINEAR
                } else {
                    glow::NEAREST
                } as i32,
            );
            let wrap = match sampling.wrap.as_str() {
                "repeat" => glow::REPEAT,
                "mirrored_repeat" => glow::MIRRORED_REPEAT,
                "clamp_to_border" => glow::CLAMP_TO_BORDER,
                _ => glow::CLAMP_TO_EDGE,
            };
            gl.sampler_parameter_i32(sampler, glow::TEXTURE_WRAP_S, wrap as i32);
            gl.sampler_parameter_i32(sampler, glow::TEXTURE_WRAP_T, wrap as i32);
        }
        self.samplers.insert(sampling, sampler);
        Ok(sampler)
    }
}
impl Drop for Pipeline {
    fn drop(&mut self) {
        if self.owner.current() {
            unsafe {
                let gl = &self.owner.gl;
                for sampler in self.samplers.values() {
                    gl.delete_sampler(*sampler);
                }
                if let Some(vao) = self.vao {
                    gl.delete_vertex_array(vao);
                }
                if let Some(vbo) = self.vbo {
                    gl.delete_buffer(vbo);
                }
                if let Some(fbo) = self.read_fbo {
                    gl.delete_framebuffer(fbo);
                }
            }
        }
    }
}
const MVP: [f32; 16] = [
    2., 0., 0., 0., 0., 2., 0., 0., 0., 0., 1., 0., -1., -1., 0., 1.,
];
fn size_values(width: u32, height: u32) -> [f32; 4] {
    [
        width as f32,
        height as f32,
        1. / width as f32,
        1. / height as f32,
    ]
}
impl Pipeline {
    fn prepare(&mut self, source: Image, viewport: (u32, u32)) -> Result<bool, String> {
        let history_changed = self
            .history
            .first()
            .is_none_or(|t| (t.image.width, t.image.height) != (source.width, source.height));
        let key = (source.width, source.height, viewport.0, viewport.1);
        if self.layout_key == Some(key) {
            return Ok(false);
        }
        let mut sizes = Vec::with_capacity(self.spec.passes.len());
        let mut previous = (source.width, source.height);
        let mut total =
            (source.width as u64 * source.height as u64 * 4) * (self.history_count as u64 + 1);
        for (index, pass) in self.spec.passes.iter().enumerate() {
            let scale = |kind: &str, factor: f64, source: u32, view: u32| -> Result<u32, String> {
                let base = match kind {
                    "source" => source as f64,
                    "viewport" => view as f64,
                    "absolute" => 1.,
                    _ => return Err(format!("Unsupported preset scaling: {kind}")),
                };
                let value = (base * factor).round().max(1.);
                if !value.is_finite() || value > self.max_size as f64 {
                    return Err("Preset exceeds the GPU texture limit".into());
                }
                Ok(value as u32)
            };
            previous = (
                scale(&pass.scale_type_x, pass.scale_x, previous.0, viewport.0)?,
                scale(&pass.scale_type_y, pass.scale_y, previous.1, viewport.1)?,
            );
            let format = if pass.float_buffer {
                glow::RGBA16F
            } else if pass.srgb {
                glow::SRGB8_ALPHA8
            } else {
                glow::RGBA8
            };
            let count = if self.feedback.contains(&index) { 2 } else { 1 };
            total += previous.0 as u64
                * previous.1 as u64
                * if pass.float_buffer { 8 } else { 4 }
                * count as u64;
            sizes.push((previous.0, previous.1, format, count));
        }
        total += self
            .lookups
            .iter()
            .map(|t| t.image.width as u64 * t.image.height as u64 * 4)
            .sum::<u64>();
        if total > 512 * 1024 * 1024 {
            return Err("Preset exceeds its video-memory budget at this size".into());
        }
        if history_changed {
            self.history.clear();
            for _ in 0..=self.history_count {
                self.history.push(Texture::new(
                    &self.owner,
                    source.width,
                    source.height,
                    glow::RGBA8,
                    true,
                )?);
            }
        }
        if self.dimensions != sizes {
            self.outputs.clear();
            for (width, height, format, count) in &sizes {
                let mut group = Vec::with_capacity(*count);
                for _ in 0..*count {
                    group.push(Texture::new(&self.owner, *width, *height, *format, true)?);
                }
                self.outputs.push(group);
            }
            self.dimensions = sizes;
        }
        self.layout_key = Some(key);
        Ok(history_changed)
    }
    fn image_for(&self, reference: TextureRef, source: Image) -> Result<Image, String> {
        Ok(match reference {
            TextureRef::Source => source,
            TextureRef::Original => self.history[0].image,
            TextureRef::History(index) => {
                self.history
                    .get(index)
                    .ok_or("Missing shader history")?
                    .image
            }
            TextureRef::Lookup(index) => {
                self.lookups
                    .get(index)
                    .ok_or("Missing shader lookup")?
                    .image
            }
            TextureRef::Pass(index) | TextureRef::Feedback(index) => {
                let group = self
                    .outputs
                    .get(index)
                    .ok_or("Invalid shader pass reference")?;
                let phase = if group.len() == 2 {
                    if matches!(reference, TextureRef::Feedback(_)) {
                        1 - self.phase
                    } else {
                        self.phase
                    }
                } else {
                    0
                };
                group[phase].image
            }
        })
    }
    pub fn invalidate(&mut self) {
        self.frame_origin = None;
        self.phase = 0;
        self.reset_history = true;
    }
    pub fn render(
        &mut self,
        input: Image,
        top_left: bool,
        viewport: (u32, u32),
        frame: u64,
    ) -> Result<Image, String> {
        if !self.owner.current() {
            return Err("Preset context is unavailable".into());
        }
        let frame = frame.wrapping_sub(*self.frame_origin.get_or_insert(frame));
        let minimum = self.spec.minimum_viewport;
        let scale = 1f64
            .max(minimum[0] as f64 / viewport.0.max(1) as f64)
            .max(minimum[1] as f64 / viewport.1.max(1) as f64);
        let viewport = (
            (viewport.0 as f64 * scale).round().max(1.) as u32,
            (viewport.1 as f64 * scale).round().max(1.) as u32,
        );
        let initialize = self.prepare(input, viewport)?;
        let reset = std::mem::take(&mut self.reset_history);
        let initialize = initialize || reset;
        let gl = &self.owner.gl;
        unsafe {
            for state in [
                glow::BLEND,
                glow::DEPTH_TEST,
                glow::CULL_FACE,
                glow::SCISSOR_TEST,
                glow::STENCIL_TEST,
                glow::FRAMEBUFFER_SRGB,
            ] {
                gl.disable(state);
            }
            gl.color_mask(true, true, true, true);
            if reset {
                gl.clear_color(0., 0., 0., 1.);
                for &index in &self.feedback {
                    for target in &self.outputs[index] {
                        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, target.framebuffer);
                        gl.clear(glow::COLOR_BUFFER_BIT);
                    }
                }
            }
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read_fbo);
            gl.framebuffer_texture_2d(
                glow::READ_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(input.texture),
                0,
            );
            gl.read_buffer(glow::COLOR_ATTACHMENT0);
            self.history.rotate_right(1);
            let history = if initialize {
                &self.history[..]
            } else {
                &self.history[..1]
            };
            for texture in history {
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, texture.framebuffer);
                gl.draw_buffer(glow::COLOR_ATTACHMENT0);
                let (y0, y1) = if top_left {
                    (0, input.height as i32)
                } else {
                    (input.height as i32, 0)
                };
                gl.blit_framebuffer(
                    0,
                    y0,
                    input.width as i32,
                    y1,
                    0,
                    0,
                    input.width as i32,
                    input.height as i32,
                    glow::COLOR_BUFFER_BIT,
                    glow::NEAREST,
                );
            }
        }
        self.phase = 1 - self.phase;
        let mut previous = self.history[0].image;
        self.generated.clear();
        for (index, (program, spec)) in self.programs.iter().zip(&self.spec.passes).enumerate() {
            let group = &self.outputs[index];
            let output = &group[if group.len() == 2 { self.phase } else { 0 }];
            let mut unit = 0;
            unsafe {
                gl.bind_framebuffer(glow::FRAMEBUFFER, output.framebuffer);
                gl.viewport(0, 0, output.image.width as i32, output.image.height as i32);
                if spec.srgb {
                    gl.enable(glow::FRAMEBUFFER_SRGB);
                } else {
                    gl.disable(glow::FRAMEBUFFER_SRGB);
                }
                gl.bind_vertex_array(self.vao);
                gl.use_program(Some(program.id));
                for uniform in &program.uniforms {
                    let location = Some(&uniform.location);
                    if let Semantic::Texture(reference) = uniform.source {
                        if unit >= self.max_units {
                            return Err("Preset exceeds available texture inputs".into());
                        }
                        let image = self.image_for(reference, previous)?;
                        let sampler = if let TextureRef::Lookup(lut) = reference {
                            self.lookup_samplers[lut]
                        } else {
                            self.pass_samplers[index]
                        };
                        let mipmap = if let TextureRef::Lookup(_) = reference {
                            false
                        } else {
                            spec.mipmap
                        };
                        gl.active_texture(glow::TEXTURE0 + unit);
                        gl.bind_texture(glow::TEXTURE_2D, Some(image.texture));
                        if mipmap && !self.generated.contains(&image.texture) {
                            gl.generate_mipmap(glow::TEXTURE_2D);
                            self.generated.push(image.texture);
                        }
                        gl.bind_sampler(unit, Some(sampler));
                        gl.uniform_1_i32(location, unit as i32);
                        unit += 1;
                        continue;
                    }
                    if matches!(uniform.source, Semantic::Matrix) {
                        gl.uniform_matrix_4_f32_slice(location, false, &MVP);
                        continue;
                    }
                    let value = match uniform.source {
                        Semantic::Size(reference) => {
                            let image = self.image_for(reference, previous)?;
                            size_values(image.width, image.height)
                        }
                        Semantic::OutputSize => {
                            size_values(output.image.width, output.image.height)
                        }
                        Semantic::ViewportSize => size_values(viewport.0, viewport.1),
                        Semantic::Parameter(index) => [self.parameter_values[index]; 4],
                        Semantic::FrameCount => {
                            [if spec.frame_count_mod > 0 {
                                (frame % spec.frame_count_mod as u64) as f32
                            } else {
                                frame as f32
                            }; 4]
                        }
                        Semantic::Direction | Semantic::One => [1.; 4],
                        Semantic::Delta => [16667.; 4],
                        Semantic::Aspect => [input.width as f32 / input.height as f32; 4],
                        Semantic::Rotation => [0.; 4],
                        _ => continue,
                    };
                    match uniform.kind {
                        glow::FLOAT => gl.uniform_1_f32(location, value[0]),
                        glow::UNSIGNED_INT => gl.uniform_1_u32(location, value[0] as u32),
                        glow::INT | glow::BOOL => gl.uniform_1_i32(location, value[0] as i32),
                        glow::FLOAT_VEC2 => gl.uniform_2_f32(location, value[0], value[1]),
                        glow::FLOAT_VEC4 => {
                            gl.uniform_4_f32(location, value[0], value[1], value[2], value[3])
                        }
                        _ => {}
                    }
                }
                gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
                for unit in 0..unit {
                    gl.bind_sampler(unit, None);
                }
            }
            previous = output.image;
        }
        unsafe {
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.active_texture(glow::TEXTURE0);
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!("Preset rendering GL error: {error:#x}"));
            }
        }
        Ok(previous)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires desktop GL and EMULUNA_TEST_SHADER_PACK"]
    fn live_all_shipped_presets_compile_and_render() {
        fn capture(presenter: &Presenter) -> Vec<u8> {
            let (w, h) = presenter.output_size();
            let mut result = vec![0; w as usize * h as usize * 4];
            unsafe {
                presenter.gl.read_pixels(
                    0,
                    0,
                    w as i32,
                    h as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut result)),
                );
            }
            result
        }
        let root =
            std::env::var_os("EMULUNA_TEST_SHADER_PACK").expect("Export the shader pack first");
        let root = Path::new(&root);
        let pack = Pack::open(root).unwrap();
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let mut presenter = Presenter::new(&video).unwrap();
        let mut pixels = Vec::with_capacity(64 * 64 * 4);
        for y in 0..64 {
            for x in 0..64 {
                let pixel = 0xff000000u32 | ((x * 4) << 16) | ((y * 4) << 8) | 128;
                pixels.extend(pixel.to_ne_bytes());
            }
        }
        for (key, name) in pack.keys() {
            eprintln!("Native preset: {key} ({name})");
            presenter
                .select_preset(root, key)
                .unwrap_or_else(|e| panic!("{key}: {e}"));
            let mut first_input = Vec::new();
            let mut first_output = Vec::new();
            for frame in 0..3 {
                if std::env::var_os("EMULUNA_TEST_DYNAMIC_PATTERN").is_some() {
                    pixels.clear();
                    for y in 0..64 {
                        for x in 0..64 {
                            let red = if frame == 1 { 255 - x * 4 } else { x * 4 };
                            let green = if frame == 2 { 255 - y * 4 } else { y * 4 };
                            let blue = match frame {
                                1 => 64,
                                2 => 224,
                                _ => 128,
                            };
                            pixels.extend(
                                (0xff000000u32 | red << 16 | green << 8 | blue).to_ne_bytes(),
                            );
                        }
                    }
                }
                presenter
                    .draw_inner(64, 64, 4. / 3., &pixels, false)
                    .unwrap_or_else(|e| panic!("{key}: {e}"));
                let before_redraw = capture(&presenter);
                for _ in 0..3 {
                    presenter.redraw_inner(false).unwrap();
                }
                assert_eq!(
                    capture(&presenter),
                    before_redraw,
                    "Preset {key} advanced during UI-only redraw"
                );
                if frame == 0 {
                    first_input = pixels.clone();
                    first_output = capture(&presenter);
                }
            }
            if let Some(directory) = std::env::var_os("EMULUNA_TEST_CAPTURE_DIR") {
                let filename = format!("{}.ppm", key.replace(':', "_"));
                presenter
                    .capture_diagnostic(&Path::new(&directory).join(filename))
                    .unwrap();
            }
            presenter.accept_generation(presenter.generation.unwrap_or(0) + 1);
            presenter
                .draw_at(64, 64, 4. / 3., &first_input, 712, false)
                .unwrap();
            let after = capture(&presenter);
            let differences: Vec<_> = after
                .iter()
                .zip(&first_output)
                .filter(|(a, b)| a != b)
                .map(|(a, b)| a.abs_diff(*b))
                .collect();
            // Float/sRGB multipass output can round one 8-bit level differently
            // after extra display draws. Temporal redraw itself remains exact.
            let peak = *differences.iter().max().unwrap_or(&0);
            assert!(peak<=1,"Preset {key} retained pre-reset temporal state: {} differing channels, peak {peak}",differences.len());
            if peak > 0 {
                eprintln!(
                    "reset rounding {key}: channels={} peak={peak}",
                    differences.len()
                );
            }
        }
        presenter.set_filter(Filter::Nearest);
        presenter
            .draw_inner(64, 64, 4. / 3., &pixels, false)
            .unwrap();
    }
}
