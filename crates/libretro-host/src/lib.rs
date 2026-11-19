//! Linux-only adapter to the existing native host. Core and library stay on one thread.
use std::os::unix::ffi::OsStrExt;
use std::{
    ffi::{c_char, c_int, c_uint, c_void, CStr, CString},
    marker::PhantomData,
    path::Path,
    ptr::NonNull,
    rc::Rc,
    sync::{Mutex, MutexGuard},
};
static SESSION: Mutex<()> = Mutex::new(());
#[link(name = "dl")]
extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}
struct Library(NonNull<c_void>);
impl Library {
    fn open(path: &Path) -> Result<Self, String> {
        let path = path_string(path)?;
        // SAFETY: NUL-terminated path; RTLD_NOW resolves symbols before use.
        NonNull::new(unsafe { dlopen(path.as_ptr(), 2) })
            .map(Self)
            .ok_or("Could not load native host".into())
    }
    fn symbol(&self, name: &CStr) -> Result<NonNull<c_void>, String> {
        // SAFETY: library remains loaded for every returned function pointer.
        NonNull::new(unsafe { dlsym(self.0.as_ptr(), name.as_ptr()) })
            .ok_or_else(|| format!("Missing host function {}", name.to_string_lossy()))
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            dlclose(self.0.as_ptr());
        }
    }
}
fn path_string(path: &Path) -> Result<CString, String> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| "Path contains NUL".into())
}
fn native_error(error: unsafe extern "C" fn() -> *const c_char, fallback: &str) -> String {
    // SAFETY: host-owned NUL-terminated message is read on the same owner thread
    // before any other host call or library unload can invalidate its storage.
    let pointer = unsafe { error() };
    if pointer.is_null() {
        return fallback.into();
    }
    let message = unsafe { CStr::from_ptr(pointer) }.to_string_lossy();
    if message.is_empty() {
        fallback.into()
    } else {
        message.into_owned()
    }
}
macro_rules! load {
    ($lib:expr,$name:literal,$kind:ty) => {{
        let name = CStr::from_bytes_with_nul(concat!($name, "\0").as_bytes()).unwrap();
        // SAFETY: signatures below match the bundled host's C ABI.
        unsafe { std::mem::transmute::<*mut c_void, $kind>($lib.symbol(name)?.as_ptr()) }
    }};
}
/// Owned snapshot: no borrowed native pixels survive the next core frame.
#[derive(Default)]
pub struct Frame {
    pub sequence: u64,
    pub generation: u64,
    pub aspect: f64,
    pub width: u32,
    pub height: u32,
    /// Native-endian ARGB32 bytes, owned until this frame is dropped.
    pub pixels: Vec<u8>,
    pub audio: Vec<i16>,
    /// True when pixels remain in the caller-owned GL framebuffer.
    pub hardware: bool,
}
struct HardwareApi {
    active: unsafe extern "C" fn(*mut c_void) -> c_int,
    context_type: unsafe extern "C" fn(*mut c_void) -> c_uint,
    max_width: unsafe extern "C" fn(*mut c_void) -> c_uint,
    max_height: unsafe extern "C" fn(*mut c_void) -> c_uint,
    bottom_left: unsafe extern "C" fn(*mut c_void) -> c_int,
    set_framebuffer: unsafe extern "C" fn(*mut c_void, usize),
    reset: unsafe extern "C" fn(*mut c_void) -> c_int,
    destroy: unsafe extern "C" fn(*mut c_void),
    last_frame: unsafe extern "C" fn(*mut c_void) -> c_int,
}
/// A core can end a session normally through RETRO_ENVIRONMENT_SHUTDOWN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum FrameOutcome {
    Frame,
    Shutdown,
}
pub const MOUSE_PORT_OPTION: &str = "emuluna_mouse_port";

pub fn mouse_enabled(options: &[(String, String)]) -> bool {
    options
        .iter()
        .any(|(key, value)| key == MOUSE_PORT_OPTION && matches!(value.as_str(), "1" | "2"))
}

type SetMouseInput = unsafe extern "C" fn(*mut c_void, i16, i16, c_uint);

pub struct Core {
    mouse_input: Option<SetMouseInput>,
    handle: NonNull<c_void>,
    frame: unsafe extern "C" fn(*mut c_void, c_uint) -> c_int,
    pixels: unsafe extern "C" fn(*mut c_void) -> *const u32,
    audio: unsafe extern "C" fn(*mut c_void) -> *const i16,
    width: unsafe extern "C" fn(*mut c_void) -> c_uint,
    height: unsafe extern "C" fn(*mut c_void) -> c_uint,
    aspect: unsafe extern "C" fn(*mut c_void) -> f64,
    fps: unsafe extern "C" fn(*mut c_void) -> f64,
    destroy: unsafe extern "C" fn(*mut c_void),
    hardware: HardwareApi,
    error: unsafe extern "C" fn() -> *const c_char,
    sequence: u64,
    audio_rate: unsafe extern "C" fn(*mut c_void, f64),
    analog: unsafe extern "C" fn(*mut c_void, i16, i16, i16, i16),
    port_input: unsafe extern "C" fn(*mut c_void, c_uint, c_uint, i16, i16, i16, i16),
    reset: unsafe extern "C" fn(*mut c_void),
    runtime_option: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char),
    save_state: unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int,
    load_state: unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int,

    _library: Library,
    _session: MutexGuard<'static, ()>,
    _owner: PhantomData<Rc<()>>,
}
impl Core {
    pub fn open(
        host: &Path,
        binary: &Path,
        rom: &Path,
        saves: &Path,
        bios: &Path,
    ) -> Result<Self, String> {
        Self::open_config(host, binary, rom, saves, bios, None, &[])
    }
    pub fn open_with_options(
        paths: &[std::path::PathBuf; 5],
        options: &[(String, String)],
    ) -> Result<Self, String> {
        Self::open_config(
            &paths[0], &paths[1], &paths[2], &paths[3], &paths[4], None, options,
        )
    }
    /// Load an OpenGL-core libretro renderer on the current owning thread.
    ///
    /// # Safety
    /// A compatible desktop core-profile GL context must remain current for
    /// initialization, every core operation and Drop. It must outlive this Core.
    /// The supplied version must not exceed that context's actual capabilities.
    pub unsafe fn open_gl(
        host: &Path,
        binary: &Path,
        rom: &Path,
        saves: &Path,
        bios: &Path,
        version: (u32, u32),
    ) -> Result<Self, String> {
        Self::open_config(host, binary, rom, saves, bios, Some(version), &[])
    }
    /// Load with explicit core options. The same context lifetime requirements
    /// as `open_gl` apply; strings are copied by the native host before loading.
    ///
    /// # Safety
    /// The specified current GL context must outlive every core operation/Drop.
    pub unsafe fn open_gl_with_options(
        paths: &[std::path::PathBuf; 5],
        version: (u32, u32),
        options: &[(String, String)],
    ) -> Result<Self, String> {
        Self::open_config(
            &paths[0],
            &paths[1],
            &paths[2],
            &paths[3],
            &paths[4],
            Some(version),
            options,
        )
    }
    fn open_config(
        host: &Path,
        binary: &Path,
        rom: &Path,
        saves: &Path,
        bios: &Path,
        gl: Option<(u32, u32)>,
        options: &[(String, String)],
    ) -> Result<Self, String> {
        let mouse_enabled = mouse_enabled(options);
        let options: Vec<_> = options
            .iter()
            .map(|(key, value)| {
                if key.is_empty() {
                    return Err("Core option key is empty".to_string());
                }
                Ok((
                    CString::new(key.as_bytes()).map_err(|_| "Core option key contains NUL")?,
                    CString::new(value.as_bytes()).map_err(|_| "Core option value contains NUL")?,
                ))
            })
            .collect::<Result<_, String>>()?;
        let guard = SESSION
            .try_lock()
            .map_err(|_| "Only one core may own the native host in a process")?;
        let lib = Library::open(host)?;
        let mouse_input = if mouse_enabled {
            Some(load!(lib, "el_set_mouse_input", SetMouseInput))
        } else {
            None
        };
        let create = load!(
            lib,
            "el_libretro_create",
            unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
            ) -> *mut c_void
        );
        let set_option = load!(
            lib,
            "el_set_option",
            unsafe extern "C" fn(*const c_char, *const c_char)
        );
        let audio_rate = load!(
            lib,
            "el_set_audio_rate_adjustment",
            unsafe extern "C" fn(*mut c_void, f64)
        );
        let save_state = load!(
            lib,
            "el_save_state",
            unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int
        );
        let load_state = load!(
            lib,
            "el_load_state",
            unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int
        );
        let reset = load!(lib, "el_reset", unsafe extern "C" fn(*mut c_void));
        let analog = load!(
            lib,
            "el_set_analog",
            unsafe extern "C" fn(*mut c_void, i16, i16, i16, i16)
        );
        let port_input = load!(
            lib,
            "el_set_port_input",
            unsafe extern "C" fn(*mut c_void, c_uint, c_uint, i16, i16, i16, i16)
        );
        let clear = load!(lib, "el_clear_options", unsafe extern "C" fn());
        let hw = load!(lib, "el_set_hardware_allowed", unsafe extern "C" fn(c_int));
        let vk = load!(lib, "el_set_vulkan_allowed", unsafe extern "C" fn(c_int));
        let error = load!(lib, "el_error", unsafe extern "C" fn() -> *const c_char);
        let destroy = load!(lib, "el_destroy", unsafe extern "C" fn(*mut c_void));
        let frame = load!(
            lib,
            "el_frame",
            unsafe extern "C" fn(*mut c_void, c_uint) -> c_int
        );
        let pixels = load!(
            lib,
            "el_pixels",
            unsafe extern "C" fn(*mut c_void) -> *const u32
        );
        let audio = load!(
            lib,
            "el_audio",
            unsafe extern "C" fn(*mut c_void) -> *const i16
        );
        let width = load!(lib, "el_width", unsafe extern "C" fn(*mut c_void) -> c_uint);
        let height = load!(
            lib,
            "el_height",
            unsafe extern "C" fn(*mut c_void) -> c_uint
        );
        let fps = load!(lib, "el_fps", unsafe extern "C" fn(*mut c_void) -> f64);
        let aspect = load!(
            lib,
            "el_aspect_ratio",
            unsafe extern "C" fn(*mut c_void) -> f64
        );
        let caps = load!(
            lib,
            "el_set_hardware_capabilities",
            unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint)
        );
        let hardware = HardwareApi {
            active: load!(
                lib,
                "el_hardware_active",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            context_type: load!(
                lib,
                "el_hardware_context_type",
                unsafe extern "C" fn(*mut c_void) -> c_uint
            ),
            max_width: load!(
                lib,
                "el_hardware_max_width",
                unsafe extern "C" fn(*mut c_void) -> c_uint
            ),
            max_height: load!(
                lib,
                "el_hardware_max_height",
                unsafe extern "C" fn(*mut c_void) -> c_uint
            ),
            bottom_left: load!(
                lib,
                "el_hardware_bottom_left",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            set_framebuffer: load!(
                lib,
                "el_set_hardware_framebuffer",
                unsafe extern "C" fn(*mut c_void, usize)
            ),
            reset: load!(
                lib,
                "el_hardware_context_reset",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            destroy: load!(
                lib,
                "el_hardware_context_destroy",
                unsafe extern "C" fn(*mut c_void)
            ),
            last_frame: load!(
                lib,
                "el_last_frame_hardware",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
        };
        let (binary, rom, saves, bios) = (
            path_string(binary)?,
            path_string(rom)?,
            path_string(saves)?,
            path_string(bios)?,
        );
        // SAFETY: valid strings and serialized ownership. The unsafe GL entry
        // point additionally requires a live/current owning context.
        unsafe {
            clear();
            for (key, value) in &options {
                set_option(key.as_ptr(), value.as_ptr());
            }
            hw(i32::from(gl.is_some()));
            vk(0);
            let (major, minor) = gl.unwrap_or((0, 0));
            caps(0, 0, major, minor);
        }
        let handle = NonNull::new(unsafe {
            create(binary.as_ptr(), rom.as_ptr(), saves.as_ptr(), bios.as_ptr())
        })
        .ok_or_else(|| {
            native_error(
                error,
                "Core failed to load; check core, ROM and BIOS compatibility",
            )
        })?;
        Ok(Self {
            mouse_input,
            handle,
            frame,
            pixels,
            audio,
            width,
            height,
            fps,
            aspect,
            destroy,
            hardware,
            error,
            sequence: 0,
            audio_rate,
            analog,
            port_input,
            reset,
            runtime_option: load!(
                lib,
                "el_set_runtime_option",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)
            ),
            save_state,
            load_state,
            _library: lib,
            _session: guard,
            _owner: PhantomData,
        })
    }
    pub fn gl_geometry(&self) -> Result<(u32, u32, bool), String> {
        let h = self.handle.as_ptr();
        unsafe {
            if (self.hardware.active)(h) == 0 || (self.hardware.context_type)(h) != 3 {
                return Err("Core did not negotiate an OpenGL core-profile renderer".into());
            }
            let (w, hgt) = ((self.hardware.max_width)(h), (self.hardware.max_height)(h));
            if !(1..=8192).contains(&w) || !(1..=8192).contains(&hgt) {
                return Err("Unsupported core GL framebuffer dimensions".into());
            }
            Ok((w, hgt, (self.hardware.bottom_left)(h) != 0))
        }
    }
    /// # Safety
    /// The complete framebuffer must belong to the current context and remain
    /// alive until core teardown. It must meet the negotiated maximum geometry.
    pub unsafe fn reset_gl_target(&mut self, framebuffer: u32) -> Result<(), String> {
        if framebuffer == 0 {
            return Err("Core GL target must be an owned framebuffer".into());
        }
        (self.hardware.set_framebuffer)(self.handle.as_ptr(), framebuffer as usize);
        if (self.hardware.reset)(self.handle.as_ptr()) == 0 {
            return Err("Core GL reset failed".into());
        }
        Ok(())
    }
    pub fn set_audio_rate_adjustment(&mut self, ratio: f64) {
        let ratio = if ratio.is_finite() {
            ratio.clamp(0.995, 1.005)
        } else {
            1.
        };
        unsafe {
            (self.audio_rate)(self.handle.as_ptr(), ratio);
        }
    }
    pub fn save_state(
        &mut self,
        path: &Path,
        identity: &emuluna_state_file::Identity,
    ) -> Result<(), String> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let raw = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        self.save_raw_state(raw.path())?;
        emuluna_state_file::write_from_raw(path, raw.path(), identity)
    }
    pub fn load_state(
        &mut self,
        path: &Path,
        identity: &emuluna_state_file::Identity,
    ) -> Result<(), String> {
        emuluna_state_file::with_payload(path, identity, |raw| {
            self.initialize_for_state()?;
            self.load_raw_state(raw)
        })
    }
    /// Match the established player's initial two frames before restoration.
    /// Some cores create their device/plugin state only during retro_run.
    pub fn state_initialization_frames(&self) -> u64 {
        2u64.saturating_sub(self.sequence)
    }
    pub fn initialize_for_state(&mut self) -> Result<(), String> {
        let mut frame = Frame::default();
        for _ in 0..self.state_initialization_frames() {
            if self.run_into(0, &mut frame)? != FrameOutcome::Frame {
                return Err("The core stopped while initializing for a saved state".into());
            }
        }
        Ok(())
    }
    /// Low-level raw payload; applications must wrap this with a validated
    /// state-file envelope. A GL core requires its owning context current.
    pub fn save_raw_state(&mut self, path: &Path) -> Result<(), String> {
        let path = path_string(path)?;
        if unsafe { (self.save_state)(self.handle.as_ptr(), path.as_ptr()) } == 0 {
            return Err("The core could not save the game state.".into());
        }
        Ok(())
    }
    /// Only call after envelope identity/checksum/renderer validation. This
    /// remains on the owning core thread and compatible current GL context.
    pub fn load_raw_state(&mut self, path: &Path) -> Result<(), String> {
        let path = path_string(path)?;
        if unsafe { (self.load_state)(self.handle.as_ptr(), path.as_ptr()) } == 0 {
            return Err("The core could not restore the game state.".into());
        }
        Ok(())
    }
    pub fn set_runtime_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        let key = CString::new(key).map_err(|_| "Invalid core option key")?;
        let value = CString::new(value).map_err(|_| "Invalid core option value")?;
        unsafe {
            (self.runtime_option)(self.handle.as_ptr(), key.as_ptr(), value.as_ptr());
        }
        Ok(())
    }
    pub fn reset(&mut self) {
        self.set_analog([0; 4]);
        unsafe {
            (self.reset)(self.handle.as_ptr());
        }
    }
    pub fn set_mouse_input(&mut self, dx: i16, dy: i16, buttons: u32) {
        if let Some(set_input) = self.mouse_input {
            unsafe { set_input(self.handle.as_ptr(), dx, dy, buttons) };
        }
    }
    pub fn set_port_input(&mut self, port: u32, buttons: u32, [lx, ly, rx, ry]: [i16; 4]) {
        unsafe {
            (self.port_input)(self.handle.as_ptr(), port, buttons, lx, ly, rx, ry);
        }
    }
    pub fn set_analog(&mut self, axes: [i16; 4]) {
        unsafe {
            (self.analog)(self.handle.as_ptr(), axes[0], axes[1], axes[2], axes[3]);
        }
    }
    pub fn fps(&self) -> Result<f64, String> {
        let value = unsafe { (self.fps)(self.handle.as_ptr()) };
        if value.is_finite() && (10.0..=240.0).contains(&value) {
            Ok(value)
        } else {
            Err("Invalid core timing".into())
        }
    }
    pub fn run(&mut self, buttons: u32) -> Result<Option<Frame>, String> {
        let mut frame = Frame::default();
        match self.run_into(buttons, &mut frame)? {
            FrameOutcome::Frame => Ok(Some(frame)),
            FrameOutcome::Shutdown => Ok(None),
        }
    }
    /// Reuse caller-owned buffers; native pointers never escape this call.
    pub fn run_into(&mut self, buttons: u32, output: &mut Frame) -> Result<FrameOutcome, String> {
        let h = self.handle.as_ptr();
        // SAFETY: live core used exclusively by its owning thread.
        let n = unsafe { (self.frame)(h, buttons) };
        if n == -2 {
            return Ok(FrameOutcome::Shutdown);
        }
        if n < 0 {
            return Err(native_error(
                self.error,
                "Core stopped or reported a frame error",
            ));
        }
        let (w, t) = unsafe { ((self.width)(h), (self.height)(h)) };
        let hardware = unsafe { (self.hardware.last_frame)(h) != 0 };
        let limit = if hardware { 8192 } else { 4096 };
        if !(0..=96000).contains(&n) || !(1..=limit).contains(&w) || !(1..=limit).contains(&t) {
            return Err(format!(
                "Invalid core frame: video={w}x{t}, stereo_audio_frames={n}, hardware={hardware}"
            ));
        }
        let (p, a) = unsafe { ((self.pixels)(h), (self.audio)(h)) };
        if (!hardware && p.is_null()) || (n > 0 && a.is_null()) {
            return Err("Missing frame storage".into());
        }
        // SAFETY: host guarantees these validated buffer lengths until the next run.
        output.pixels.clear();
        if !hardware {
            output.pixels.extend_from_slice(unsafe {
                std::slice::from_raw_parts(p.cast::<u8>(), (w * t * 4) as usize)
            });
        }
        output.hardware = hardware;
        output.audio.clear();
        if n > 0 {
            output
                .audio
                .extend_from_slice(unsafe { std::slice::from_raw_parts(a, n as usize * 2) });
        }
        let aspect = unsafe { (self.aspect)(h) };
        let aspect = if aspect.is_finite() && aspect > 0. && aspect < 10. {
            aspect
        } else {
            w as f64 / t as f64
        };
        output.sequence = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        output.aspect = aspect;
        output.width = w;
        output.height = t;
        Ok(FrameOutcome::Frame)
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            (self.hardware.destroy)(self.handle.as_ptr());
            (self.destroy)(self.handle.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires native host and compiled original rust_input_core.cpp fixture"]
    fn native_input_receives_full_signed_axes_and_releases_them() {
        let host = std::env::var_os("EMULUNA_RUST_HOST").expect("EMULUNA_RUST_HOST");
        let fixture =
            std::env::var_os("EMULUNA_RUST_INPUT_FIXTURE").expect("EMULUNA_RUST_INPUT_FIXTURE");
        let dir = std::env::temp_dir().join(format!(
            "emuluna-input-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let rom = dir.join("diagnostic.bin");
        std::fs::write(&rom, [0]).unwrap();
        let saves = dir.join("saves");
        std::fs::create_dir(&saves).unwrap();
        let mut core =
            Core::open(Path::new(&host), Path::new(&fixture), &rom, &saves, &dir).unwrap();
        let axes = [-32768, 32767, -12345, 12345];
        core.set_analog(axes);
        core.set_port_input(1, 1, [11111, 0, 0, 0]);
        core.set_port_input(2, 4, [-22222, 0, 0, 0]);
        core.set_port_input(3, 8, [32767, 0, 0, 0]);
        let frame = core.run(2 | 8).unwrap().unwrap();
        let pixels: Vec<_> = frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| u32::from_ne_bytes(*v) as u16)
            .collect();
        assert_eq!(&pixels[..4], &axes.map(|a| a as u16));
        assert_eq!(pixels[4], 1 | 8); // RetroPad B and Start, translated by native host.
        assert_eq!(
            &pixels[7..13],
            &[256, 11111, 4, (-22222i16) as u16, 8, 32767]
        );
        for port in 1..4 {
            core.set_port_input(port, 0, [0; 4]);
        }
        core.set_analog([0; 4]);
        let frame = core.run(0).unwrap().unwrap();
        assert!(frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .take(13)
            .enumerate()
            .filter(|(index, _)| !matches!(index, 5 | 6))
            .map(|(_, value)| value)
            .all(|v| u32::from_ne_bytes(*v) as u16 == 0));
        drop(core);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[ignore = "requires native host and compiled original input/state fixture"]
    fn native_state_round_trip_preserves_newer_battery_saves() {
        use emuluna_state_file::{self as states, Identity, Renderer};
        let host = std::env::var_os("EMULUNA_RUST_HOST").expect("host");
        let fixture = std::env::var_os("EMULUNA_RUST_INPUT_FIXTURE").expect("fixture");
        let root = tempfile::tempdir().unwrap();
        let rom = root.path().join("diagnostic.bin");
        std::fs::write(&rom, [0]).unwrap();
        let saves = root.path().join("saves");
        std::fs::create_dir(&saves).unwrap();
        let mut core = Core::open(
            Path::new(&host),
            Path::new(&fixture),
            &rom,
            &saves,
            root.path(),
        )
        .unwrap();
        let id = Identity::new(
            "fixture",
            &states::digest_file(Path::new(&fixture)).unwrap(),
            &states::digest_file(&rom).unwrap(),
            Renderer::Software,
        )
        .unwrap();
        let pixel = |frame: &Frame, index: usize| {
            u32::from_ne_bytes(frame.pixels[index * 4..index * 4 + 4].try_into().unwrap())
                & 0x00ffffff
        };
        let first = core.run(0).unwrap().unwrap();
        assert_eq!((pixel(&first, 5), pixel(&first, 6)), (0, 8));
        let slot = root.path().join("slot");
        core.save_state(&slot, &id).unwrap();
        core.run(0).unwrap().unwrap();
        let third = core.run(0).unwrap().unwrap();
        assert_eq!((pixel(&third, 5), pixel(&third, 6)), (2, 10));
        core.load_state(&slot, &id).unwrap();
        let restored = core.run(0).unwrap().unwrap();
        assert_eq!((pixel(&restored, 5), pixel(&restored, 6)), (1, 11));
        core.reset();
        let reset = core.run(0).unwrap().unwrap();
        assert_eq!((pixel(&reset, 5), pixel(&reset, 6)), (0, 12));
        drop(core);
        assert_eq!(
            std::fs::read(saves.join("battery.srm")).unwrap(),
            [12, 8, 9, 10]
        );
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    #[test]
    #[ignore = "requires native host and original EMULUNA_RUST_VIDEO_FIXTURE"]
    fn audio_rate_changes_apply_at_callback_boundary() {
        let work = tempfile::tempdir().unwrap();
        let rom = work.path().join("original.bin");
        std::fs::write(&rom, [0x5a, 0xa5]).unwrap();
        let mut core = Core::open_with_options(
            &[
                std::env::var_os("EMULUNA_RUST_HOST").unwrap().into(),
                std::env::var_os("EMULUNA_RUST_VIDEO_FIXTURE")
                    .unwrap()
                    .into(),
                rom,
                work.path().to_path_buf(),
                work.path().to_path_buf(),
            ],
            &[("fixture_split_rate".into(), "enabled".into())],
        )
        .unwrap();
        let mut frame = Frame::default();
        for sequence in 0..6 {
            assert_eq!(core.run_into(0, &mut frame).unwrap(), FrameOutcome::Frame);
            let count = if sequence < 4 { 800 } else { 960 };
            assert_eq!(frame.audio.len() / 2, count, "frame {sequence}");
            let stereo = frame.audio.as_chunks::<2>().0;
            assert!(stereo[..count / 2].iter().all(|p| *p == [1000, -1000]));
            assert!(stereo[count / 2..].iter().all(|p| *p == [2000, -2000]));
            assert_eq!(core.fps().unwrap(), if sequence < 4 { 60. } else { 50. });
        }
    }
    #[test]
    #[ignore = "requires native host and original EMULUNA_RUST_VIDEO_FIXTURE"]
    fn software_formats_padded_rows_duplicates_geometry_timing_and_shutdown() {
        let host = std::path::PathBuf::from(std::env::var_os("EMULUNA_RUST_HOST").expect("host"));
        let fixture = std::path::PathBuf::from(
            std::env::var_os("EMULUNA_RUST_VIDEO_FIXTURE").expect("video fixture"),
        );
        let work = tempfile::tempdir().unwrap();
        let rom = work.path().join("original.bin");
        std::fs::write(&rom, [0x5a, 0xa5]).unwrap();
        for (index, option) in [None, Some("565"), Some("8888")].into_iter().enumerate() {
            let saves = work.path().join(format!("save-{index}"));
            std::fs::create_dir(&saves).unwrap();
            let options: Vec<_> = option
                .into_iter()
                .map(|value| ("fixture_format".into(), value.into()))
                .collect();
            let mut core = Core::open_with_options(
                &[
                    host.clone(),
                    fixture.clone(),
                    rom.clone(),
                    saves.clone(),
                    work.path().to_path_buf(),
                ],
                &options,
            )
            .unwrap();
            assert_eq!(core.fps().unwrap(), 60.);
            let mut frame = Frame::default();
            for sequence in 0..6 {
                assert_eq!(core.run_into(0, &mut frame).unwrap(), FrameOutcome::Frame);
                assert_eq!(frame.sequence, sequence);
                let pixels: Vec<_> = frame
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| u32::from_ne_bytes(*p))
                    .collect();
                if sequence < 3 {
                    assert_eq!((frame.width, frame.height), (2, 2));
                    assert_eq!(pixels, [0xffff0000, 0xff00ff00, 0xff0000ff, 0xffffffff]);
                    assert_eq!(frame.aspect, 1.);
                } else {
                    assert_eq!((frame.width, frame.height), (3, 1));
                    if sequence < 5 {
                        assert_eq!(pixels, [0xffff0000, 0xff00ff00, 0xff0000ff]);
                    } else {
                        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                        let features = [
                            std::is_x86_feature_detected!("sse2"),
                            std::is_x86_feature_detected!("avx2"),
                            std::is_x86_feature_detected!("aes"),
                        ];
                        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
                        let features = [false; 3];
                        let colors = [0xff0000, 0x00ff00, 0x0000ff];
                        assert_eq!(
                            pixels,
                            std::array::from_fn::<_, 3, _>(
                                |i| 0xff000000 | if features[i] { colors[i] } else { 0 }
                            )
                        );
                    }
                    assert!(
                        (frame.aspect - if sequence == 3 { 16. / 9. } else { 3. }).abs() < 1e-6
                    );
                }
                assert!(!frame.audio.is_empty());
                assert!(frame
                    .audio
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|p| *p == [1000, -1000]));
                assert_eq!(core.fps().unwrap(), if sequence >= 4 { 50. } else { 60. });
            }
            assert_eq!(
                core.run_into(0, &mut frame).unwrap(),
                FrameOutcome::Shutdown
            );
            assert_eq!(
                core.run_into(0, &mut frame).unwrap(),
                FrameOutcome::Shutdown
            );
            core.reset();
            assert_eq!(core.run_into(0, &mut frame).unwrap(), FrameOutcome::Frame);
            drop(core);
            assert_eq!(std::fs::read_dir(saves).unwrap().count(), 0);
        }
    }
}
