"""SDL2 controller discovery and mapped input, independent of the window system."""
import ctypes as C
import ctypes.util
import time
from .controls import LAYOUTS
from .controller_profiles import gamepad_state


class Gamepad:
    def __init__(self, layout=None, *, bindings=None, device='auto', player=0):
        self.layout = layout or LAYOUTS["default"]
        self.bindings = bindings
        self.device = device
        self.player = player
        self.axes = (0, 0, 0, 0)
        self.raw_axes = (0, 0, 0, 0, 0, 0)
        self.buttons = set()
        self.sdl = None
        self.pad = None
        self.rumble_fn = None
        self.rumble_error = ""
        self.rumble_state = (0, 0)
        self.rumble_refresh = 0.0
        try:
            self.sdl = C.CDLL(ctypes.util.find_library("SDL2-2.0") or "libSDL2-2.0.so.0")
            set_hint = getattr(self.sdl, "SDL_SetHint", None)
            if set_hint:
                set_hint.restype = C.c_int
                set_hint.argtypes = [C.c_char_p, C.c_char_p]
                # SDL2 otherwise leaves Bluetooth DualSense/DualShock output
                # reports disabled. EmuLuna owns controller I/O in this process,
                # so extended HID reports are safe and are required for rumble.
                for hint in (b"SDL_JOYSTICK_HIDAPI_PS5", b"SDL_JOYSTICK_HIDAPI_PS5_RUMBLE",
                             b"SDL_JOYSTICK_HIDAPI_PS4", b"SDL_JOYSTICK_HIDAPI_PS4_RUMBLE"):
                    set_hint(hint, b"1")
            specs = {
                "SDL_InitSubSystem": (C.c_int, [C.c_uint]),
                "SDL_QuitSubSystem": (None, [C.c_uint]),
                "SDL_NumJoysticks": (C.c_int, []),
                "SDL_IsGameController": (C.c_int, [C.c_int]),
                "SDL_GameControllerOpen": (C.c_void_p, [C.c_int]),
                "SDL_GameControllerClose": (None, [C.c_void_p]),
                "SDL_GameControllerGetAttached": (C.c_int, [C.c_void_p]),
                "SDL_GameControllerUpdate": (None, []),
                "SDL_GameControllerGetButton": (C.c_ubyte, [C.c_void_p, C.c_int]),
                "SDL_GameControllerGetAxis": (C.c_int16, [C.c_void_p, C.c_int]),
                "SDL_GameControllerNameForIndex": (C.c_char_p, [C.c_int]),
            }
            for name, (ret, args) in specs.items():
                f = getattr(self.sdl, name)
                f.restype, f.argtypes = ret, args
            self.rumble_fn = getattr(self.sdl, "SDL_GameControllerRumble", None)
            if self.rumble_fn:
                self.rumble_fn.restype = C.c_int
                self.rumble_fn.argtypes = [C.c_void_p, C.c_uint16, C.c_uint16, C.c_uint32]
            self.get_error_fn = getattr(self.sdl, "SDL_GetError", None)
            if self.get_error_fn:
                self.get_error_fn.restype = C.c_char_p
                self.get_error_fn.argtypes = []
            if self.sdl.SDL_InitSubSystem(0x2000) != 0:
                self.sdl = None
        except (OSError, AttributeError):
            self.sdl = None

    def poll(self):
        self.axes = (0, 0, 0, 0)
        self.raw_axes = (0, 0, 0, 0, 0, 0)
        self.buttons = set()
        if not self.sdl:
            return 0
        s = self.sdl
        s.SDL_GameControllerUpdate()
        if self.pad and not s.SDL_GameControllerGetAttached(self.pad):
            s.SDL_GameControllerClose(self.pad)
            self.pad = None
        if not self.pad:
            available = [i for i in range(s.SDL_NumJoysticks()) if s.SDL_IsGameController(i)]
            if available:
                if self.device.startswith('index:'):
                    requested = int(self.device.split(':', 1)[1])
                    index = requested if requested in available else None
                else:
                    index = available[self.player] if self.player < len(available) else None
                if index is not None:
                    self.pad = s.SDL_GameControllerOpen(index)
        if not self.pad:
            return 0
        self.buttons = {button for button in range(21) if s.SDL_GameControllerGetButton(self.pad, button)}
        self.raw_axes = tuple(s.SDL_GameControllerGetAxis(self.pad, axis) for axis in range(6))
        if self.bindings is not None:
            keys, self.axes = gamepad_state(self.bindings, self.buttons, self.raw_axes)
            return keys
        mapping = {int(button): bit for button, bit in self.layout['gamepad'].items()}
        keys = 0
        for button, bit in mapping.items():
            if button in self.buttons:
                keys |= bit
        self.axes = self.raw_axes[:4]
        for axis, bit in self.layout.get('triggers', {}).items():
            if s.SDL_GameControllerGetAxis(self.pad, int(axis)) > 8000:
                keys |= bit
        x, y = self.axes[:2]
        if self.layout.get('stick_as_dpad'):
            if x > 16000: keys |= 16
            if x < -16000: keys |= 32
            if y < -16000: keys |= 64
            if y > 16000: keys |= 128
        return keys

    def set_rumble(self, strong, weak):
        """Apply Libretro's two rumble motors without restarting every frame."""
        state = (max(0, min(0xffff, int(strong))), max(0, min(0xffff, int(weak))))
        now = time.monotonic()
        if not self.pad or not self.rumble_fn:
            self.rumble_error = "No vibration-capable controller is connected."
            self.rumble_state = (0, 0)
            return False
        if state == self.rumble_state and (state == (0, 0) or now < self.rumble_refresh):
            return True
        # A short renewable duration prevents a stuck motor after a crash or
        # controller disconnect while still allowing continuous vibration.
        ok = self.rumble_fn(self.pad, state[0], state[1], 250) == 0
        self.rumble_error = ""
        if not ok and getattr(self, "get_error_fn", None):
            detail = self.get_error_fn()
            if detail:
                self.rumble_error = detail.decode(errors="replace")
        if not ok and not self.rumble_error:
            self.rumble_error = "The controller did not accept the vibration command."
        self.rumble_state = state if ok else (0, 0)
        self.rumble_refresh = now + 0.15 if ok and state != (0, 0) else 0.0
        return ok

    def stop_rumble(self):
        if self.pad and self.rumble_fn:
            self.rumble_fn(self.pad, 0, 0, 0)
        self.rumble_state = (0, 0)
        self.rumble_refresh = 0.0

    @classmethod
    def devices(cls):
        """Return SDL's current controller order for player assignment UI."""
        probe = cls()
        try:
            if not probe.sdl:
                return []
            return [(f'index:{index}', (probe.sdl.SDL_GameControllerNameForIndex(index) or b'Controller').decode(errors='replace'))
                    for index in range(probe.sdl.SDL_NumJoysticks()) if probe.sdl.SDL_IsGameController(index)]
        finally:
            probe.close()

    def close(self):
        if self.sdl:
            if self.pad:
                self.stop_rumble()
                self.sdl.SDL_GameControllerClose(self.pad)
                self.pad = None
            self.sdl.SDL_QuitSubSystem(0x2000)
            self.sdl = None
