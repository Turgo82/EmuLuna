"""Typed API to the reusable C++ host for standard, unmodified libretro cores."""
import ctypes as C
import hashlib
import json
import os
from pathlib import Path
import tempfile
from .hashing import file_hexdigest
from .systems import core_launch_options

ROOT = Path(__file__).resolve().parents[1]



class CoreError(RuntimeError):
    pass


def saved_state_renderer(path):
    """Return the renderer recorded in a state without reading its large payload."""
    try:
        with Path(path).open('rb') as file:
            magic = file.readline(32).rstrip(b'\n')
            meta = json.loads(file.readline(16384))
        if magic != b"OELINUX1" or meta.get("format") != 1:
            return None
        # States written before renderer tagging used the software plugin.
        return meta.get("renderer", "software")
    except (OSError, ValueError, TypeError):
        return None


class Core:
    def __init__(self, rom, system, save_dir, *, libretro_path=None, core_id=None, system_dir=None,
                 options=None, content_digest=None, allow_hardware=False, allow_vulkan=False):
        self.rom = Path(rom).resolve()
        if libretro_path is None:
            raise CoreError("A standard libretro core is required. Download a core for this system in Settings.")
        self.is_libretro = True
        # The core's rendering path is independent of the frontend display API.
        # Switch this label only after a hardware context is actually attached.
        self.renderer = "Software"
        self.state_renderer = "software"
        self.hardware = None
        self.path = Path(os.environ.get("EMULUNA_CORE_DIR", ROOT / "build/cores")) / "libemuluna_host.so"
        binary_digest = hashlib.sha256(Path(libretro_path).read_bytes()).hexdigest()
        self.name = f"libretro:{core_id}:{binary_digest}"
        self.handle = None
        if not self.path.is_file():
            raise CoreError(f"The {self.name} core has not been built. Run build.sh first.")
        self.lib = C.CDLL(str(self.path))
        signatures = {
            "create": (C.c_void_p, [C.c_char_p, C.c_char_p]),
            "set_analog": (None, [C.c_void_p] + [C.c_int16] * 4),
            "set_port_input": (None, [C.c_void_p, C.c_uint, C.c_uint] + [C.c_int16] * 4),
            "rumble_strength": (C.c_uint16, [C.c_void_p, C.c_uint, C.c_uint]),
            "set_runtime_option": (None, [C.c_void_p, C.c_char_p, C.c_char_p]),
            "destroy": (None, [C.c_void_p]), "reset": (None, [C.c_void_p]),
            "frame": (C.c_int, [C.c_void_p, C.c_uint]),
            "pixels": (C.c_void_p, [C.c_void_p]), "audio": (C.c_void_p, [C.c_void_p]),
            "width": (C.c_uint, [C.c_void_p]), "height": (C.c_uint, [C.c_void_p]),
            "aspect_ratio": (C.c_double, [C.c_void_p]),
            "fps": (C.c_double, [C.c_void_p]), "sample_rate": (C.c_uint, [C.c_void_p]),
            "save_state": (C.c_int, [C.c_void_p, C.c_char_p]),
            "load_state": (C.c_int, [C.c_void_p, C.c_char_p]),
            "flush": (None, [C.c_void_p]),
            "hardware_active": (C.c_int, [C.c_void_p]),
            "hardware_context_type": (C.c_uint, [C.c_void_p]),
            "hardware_version_major": (C.c_uint, [C.c_void_p]),
            "hardware_version_minor": (C.c_uint, [C.c_void_p]),
            "hardware_max_width": (C.c_uint, [C.c_void_p]),
            "hardware_max_height": (C.c_uint, [C.c_void_p]),
            "hardware_bottom_left": (C.c_int, [C.c_void_p]),
            "set_hardware_framebuffer": (None, [C.c_void_p, C.c_size_t]),
            "hardware_context_reset": (C.c_int, [C.c_void_p]),
            "hardware_context_destroy": (None, [C.c_void_p]),
            "last_frame_hardware": (C.c_int, [C.c_void_p]),
        }
        for name, (result, args) in signatures.items():
            fn = getattr(self.lib, "el_" + name)
            fn.restype, fn.argtypes = result, args
        Path(save_dir).mkdir(parents=True, exist_ok=True)
        with self.rom.open("rb") as file:
            self.digest = content_digest or file_hexdigest(file, "sha256")
        self.lib.el_libretro_create.restype = C.c_void_p
        self.lib.el_libretro_create.argtypes = [C.c_char_p] * 4
        self.lib.el_error.restype = C.c_char_p
        self.lib.el_error.argtypes = []
        system_dir = Path(system_dir or Path(save_dir) / "system")
        system_dir.mkdir(parents=True, exist_ok=True)
        self.lib.el_clear_options.argtypes, self.lib.el_clear_options.restype = [], None
        self.lib.el_set_option.argtypes, self.lib.el_set_option.restype = [C.c_char_p, C.c_char_p], None
        self.lib.el_set_hardware_allowed.argtypes, self.lib.el_set_hardware_allowed.restype = [C.c_int], None
        self.lib.el_set_hardware_capabilities.argtypes = [C.c_uint] * 4
        self.lib.el_set_hardware_capabilities.restype = None
        self.lib.el_set_vulkan_allowed.argtypes = [C.c_int]
        self.lib.el_set_vulkan_allowed.restype = None
        self.lib.el_vulkan_initialize.argtypes = [C.c_void_p]
        self.lib.el_vulkan_initialize.restype = C.c_int
        self.lib.el_clear_options()
        capabilities = (0, 0, 0, 0)
        if allow_hardware:
            from .hardware_render import probe_hardware_contexts
            capabilities = probe_hardware_contexts()
        self.lib.el_set_hardware_allowed(int(bool(allow_hardware and
                                                  (any(capabilities) or allow_vulkan))))
        self.lib.el_set_vulkan_allowed(int(bool(allow_hardware and allow_vulkan)))
        self.lib.el_set_hardware_capabilities(*capabilities)
        launch_options = core_launch_options(core_id, system)
        launch_options.update(options or {})
        for key, value in launch_options.items():
            self.lib.el_set_option(str(key).encode(), str(value).encode())
        self.handle = self.lib.el_libretro_create(os.fsencode(libretro_path), os.fsencode(self.rom),
                                                  os.fsencode(save_dir), os.fsencode(system_dir))
        if not self.handle:
            raise CoreError(self.error_message(f"{self.name} could not load {self.rom.name}. Check that the ROM is valid."))
        self.width = self.lib.el_width(self.handle)
        self.height = self.lib.el_height(self.handle)
        self.fps = self.lib.el_fps(self.handle)
        self.aspect = self.lib.el_aspect_ratio(self.handle)
        self.sample_rate = self.lib.el_sample_rate(self.handle)
        self.hardware_requested = bool(self.lib.el_hardware_active(self.handle))
        self.hardware_context_type = self.lib.el_hardware_context_type(self.handle)
        self.hardware_version = (self.lib.el_hardware_version_major(self.handle),
                                 self.lib.el_hardware_version_minor(self.handle))
        self.hardware_max_width = self.lib.el_hardware_max_width(self.handle)
        self.hardware_max_height = self.lib.el_hardware_max_height(self.handle)
        self.hardware_bottom_left = bool(self.lib.el_hardware_bottom_left(self.handle))

    def attach_hardware(self, hardware):
        """Finish the context handshake after the core has declared its needs."""
        if not self.hardware_requested:
            raise CoreError("This core did not request a supported hardware renderer.")
        self.hardware = hardware
        framebuffer = hardware.fbo.handle() if self.hardware_context_type != 6 else 0
        self.lib.el_set_hardware_framebuffer(self.handle, framebuffer)
        if not self.lib.el_hardware_context_reset(self.handle):
            self.hardware = None
            raise CoreError("The core could not initialize its hardware renderer.")
        self.renderer = hardware.description
        self.state_renderer = "vulkan" if self.hardware_context_type == 6 else "opengl"

    def frame(self, keys=0, axes=(0, 0, 0, 0), players=()):
        if not self.handle:
            raise CoreError("The game has already stopped.")
        if len(axes) != 4:
            raise CoreError("Expected two pairs of analog-stick axes.")
        values = [max(-32768, min(32767, int(value))) for value in axes]
        if len(players) > 3:
            raise CoreError("A maximum of four players is supported.")
        for port, (buttons, stick_axes) in enumerate(players, 1):
            if len(stick_axes) != 4:
                raise CoreError("Expected two pairs of analog-stick axes per player.")
            self.lib.el_set_port_input(self.handle, port, buttons,
                *[max(-32768, min(32767, int(value))) for value in stick_axes])
        self.lib.el_set_analog(self.handle, *values)
        if self.hardware:
            self.hardware.begin()
        try:
            n = self.lib.el_frame(self.handle, keys)
            if n < 0 or n > 8192:
                raise CoreError(self.error_message("The emulator did not complete a frame."))
            self.width = self.lib.el_width(self.handle)
            self.height = self.lib.el_height(self.handle)
            self.fps = self.lib.el_fps(self.handle)
            self.aspect = self.lib.el_aspect_ratio(self.handle)
            self.sample_rate = self.lib.el_sample_rate(self.handle)
            limit = 4096 if self.hardware else 1024
            if not (0 < self.width <= limit and 0 < self.height <= limit):
                raise CoreError("The emulator returned invalid frame dimensions.")
            if self.hardware and self.lib.el_last_frame_hardware(self.handle):
                pixels = self.hardware.capture(self.width, self.height, self.hardware_bottom_left)
            else:
                pixels = C.string_at(self.lib.el_pixels(self.handle), self.width * self.height * 4)
            audio = C.string_at(self.lib.el_audio(self.handle), n * 4)
            return pixels, audio
        finally:
            if self.hardware:
                self.hardware.end()

    def error_message(self, default):
        if self.is_libretro:
            return (self.lib.el_error() or default.encode()).decode(errors="replace")
        return default

    def flush(self):
        self._with_context(self.lib.el_flush, self.handle)

    def rumble(self, port=0):
        """Return the core's requested strong and weak motor intensity."""
        if not self.handle or not 0 <= port < 4:
            return (0, 0)
        return tuple(self.lib.el_rumble_strength(self.handle, port, effect) for effect in range(2))

    def set_option(self, key, value):
        """Notify a running core that one of its declared options changed."""
        if self.handle:
            self.lib.el_set_runtime_option(self.handle, str(key).encode(), str(value).encode())

    def import_battery(self, path):
        self.lib.el_import_battery.restype = C.c_int
        self.lib.el_import_battery.argtypes = [C.c_void_p, C.c_char_p]
        return bool(self.lib.el_import_battery(self.handle, os.fsencode(path)))

    def save_state(self, path):
        """Atomically publish an integrity-checked state, keyed to ROM and core."""
        path = Path(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=path.parent) as tmp:
            raw = Path(tmp) / "raw.state"
            if not self._with_context(self.lib.el_save_state, self.handle, os.fsencode(raw)):
                raise CoreError("Could not save the game state.")
            data = raw.read_bytes()
            header = json.dumps({"format": 1, "core": self.name, "rom": self.digest,
                                 "renderer": self.state_renderer,
                                 "sha256": hashlib.sha256(data).hexdigest()}).encode()
            output = Path(tmp) / "state"
            with output.open("wb") as f:
                f.write(b"OELINUX1\n" + header + b"\n" + data)
                f.flush()
                os.fsync(f.fileno())
            os.replace(output, path)

    def load_state(self, path):
        path = Path(path)
        if not path.is_file():
            raise CoreError("There is no saved state in this slot yet.")
        if path.stat().st_size > 32 * 1024 * 1024:
            raise CoreError("The saved state is too large.")
        try:
            magic, header, raw = path.read_bytes().split(b"\n", 2)
            meta = json.loads(header)
            renderer = meta.get("renderer", "software")
            valid = (magic == b"OELINUX1" and meta["format"] == 1 and meta["core"] == self.name
                     and meta["rom"] == self.digest and meta["sha256"] == hashlib.sha256(raw).hexdigest())
        except (ValueError, KeyError, TypeError):
            valid = False
        if not valid:
            raise CoreError("This state is damaged or belongs to a different game or core.")
        if renderer != self.state_renderer:
            raise CoreError(
                "This state uses a different video renderer. Resume it with the renderer that created it."
            )
        with tempfile.TemporaryDirectory() as tmp:
            state = Path(tmp) / "state"
            state.write_bytes(raw)
            if not self._with_context(self.lib.el_load_state, self.handle, os.fsencode(state)):
                raise CoreError("The emulator could not restore this state.")
    def reset(self):
        self._with_context(self.lib.el_reset, self.handle)

    def _with_context(self, function, *args):
        if self.hardware:
            self.hardware.begin()
        try:
            return function(*args)
        finally:
            if self.hardware:
                self.hardware.end()

    def close(self):
        if self.handle:
            hardware = self.hardware
            hardware_current = False
            if hardware:
                try:
                    hardware.begin()
                    hardware_current = True
                except RuntimeError as error:
                    print(f"Core hardware context unavailable during unload: {error}", flush=True)
            try:
                if hardware_current:
                    try:
                        self.lib.el_hardware_context_destroy(self.handle)
                    finally:
                        self.lib.el_destroy(self.handle)
                else:
                    self.lib.el_destroy(self.handle)
            finally:
                self.handle = None
                self.hardware = None
                if hardware:
                    try:
                        if hardware_current:
                            hardware.end()
                    finally:
                        hardware.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
