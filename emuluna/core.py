"""Typed API to the reusable C++ host for standard, unmodified libretro cores."""
import ctypes as C
import hashlib
import json
import os
from pathlib import Path
import tempfile
from .systems import core_launch_options

ROOT = Path(__file__).resolve().parents[1]



class CoreError(RuntimeError):
    pass


class Core:
    def __init__(self, rom, system, save_dir, *, libretro_path=None, core_id=None, system_dir=None, options=None, content_digest=None):
        self.rom = Path(rom).resolve()
        if libretro_path is None:
            raise CoreError("A standard libretro core is required. Download a core for this system in Settings.")
        self.is_libretro = True
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
        }
        for name, (result, args) in signatures.items():
            fn = getattr(self.lib, "el_" + name)
            fn.restype, fn.argtypes = result, args
        Path(save_dir).mkdir(parents=True, exist_ok=True)
        with self.rom.open("rb") as file:
            self.digest = content_digest or hashlib.file_digest(file, "sha256").hexdigest()
        self.lib.el_libretro_create.restype = C.c_void_p
        self.lib.el_libretro_create.argtypes = [C.c_char_p] * 4
        self.lib.el_error.restype = C.c_char_p
        self.lib.el_error.argtypes = []
        system_dir = Path(system_dir or Path(save_dir) / "system")
        system_dir.mkdir(parents=True, exist_ok=True)
        self.lib.el_clear_options.argtypes, self.lib.el_clear_options.restype = [], None
        self.lib.el_set_option.argtypes, self.lib.el_set_option.restype = [C.c_char_p, C.c_char_p], None
        self.lib.el_clear_options()
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
        n = self.lib.el_frame(self.handle, keys)
        if n < 0 or n > 8192:
            raise CoreError(self.error_message("The emulator did not complete a frame."))
        self.width = self.lib.el_width(self.handle)
        self.height = self.lib.el_height(self.handle)
        self.fps = self.lib.el_fps(self.handle)
        self.aspect = self.lib.el_aspect_ratio(self.handle)
        self.sample_rate = self.lib.el_sample_rate(self.handle)
        if not (0 < self.width <= 1024 and 0 < self.height <= 1024):
            raise CoreError("The emulator returned invalid frame dimensions.")
        return (C.string_at(self.lib.el_pixels(self.handle), self.width * self.height * 4),
                C.string_at(self.lib.el_audio(self.handle), n * 4))

    def error_message(self, default):
        if self.is_libretro:
            return (self.lib.el_error() or default.encode()).decode(errors="replace")
        return default

    def flush(self):
        self.lib.el_flush(self.handle)

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
            if not self.lib.el_save_state(self.handle, os.fsencode(raw)):
                raise CoreError("Could not save the game state.")
            data = raw.read_bytes()
            header = json.dumps({"format": 1, "core": self.name, "rom": self.digest,
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
            valid = (magic == b"OELINUX1" and meta["format"] == 1 and meta["core"] == self.name
                     and meta["rom"] == self.digest and meta["sha256"] == hashlib.sha256(raw).hexdigest())
        except (ValueError, KeyError, TypeError):
            valid = False
        if not valid:
            raise CoreError("This state is damaged or belongs to a different game or core.")
        with tempfile.TemporaryDirectory() as tmp:
            state = Path(tmp) / "state"
            state.write_bytes(raw)
            if not self.lib.el_load_state(self.handle, os.fsencode(state)):
                raise CoreError("The emulator could not restore this state.")

    def reset(self):
        self.lib.el_reset(self.handle)

    def close(self):
        if self.handle:
            self.lib.el_destroy(self.handle)
            self.handle = None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
