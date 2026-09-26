"""N64 uses both sticks, an independent D-pad and a Z trigger."""
import ctypes as C
import unittest
from PySide6.QtCore import Qt
from emuluna.controls import layout_for, keyboard_axes, combined_axes, keyboard_buttons
from emuluna.gamepad import Gamepad
from emuluna.controller_profiles import defaults


class N64Controls(unittest.TestCase):
    def test_keyboard_sticks_opposite_keys_and_controller_fallback(self):
        layout=layout_for('n64')
        axes=keyboard_axes(layout)
        self.assertEqual(combined_axes(axes,{Qt.Key_Right,Qt.Key_I},(1234,5678,9012,0)),(32767,5678,9012,-32767))
        self.assertEqual(combined_axes(axes,{Qt.Key_Left,Qt.Key_Right},(12000,0,0,0)),(0,0,0,0))
        self.assertEqual(keyboard_buttons(layout)[Qt.Key_Space],4096)

    def test_virtual_gamepad_analog_c_stick_trigger_and_disconnect(self):
        pad=Gamepad(layout_for('n64'))
        if not pad.sdl:
            self.skipTest('SDL2 is unavailable')
        s=pad.sdl
        s.SDL_JoystickAttachVirtual.argtypes=[C.c_int]*4
        s.SDL_JoystickAttachVirtual.restype=C.c_int
        s.SDL_JoystickOpen.argtypes=[C.c_int]; s.SDL_JoystickOpen.restype=C.c_void_p
        s.SDL_JoystickSetVirtualAxis.argtypes=[C.c_void_p,C.c_int,C.c_int16]
        s.SDL_JoystickSetVirtualButton.argtypes=[C.c_void_p,C.c_int,C.c_ubyte]
        s.SDL_JoystickClose.argtypes=[C.c_void_p]
        s.SDL_JoystickDetachVirtual.argtypes=[C.c_int]
        index=s.SDL_JoystickAttachVirtual(1,6,15,0)
        joy=None
        try:
            self.assertGreaterEqual(index,0)
            joy=s.SDL_JoystickOpen(index)
            pad.pad=s.SDL_GameControllerOpen(index)
            self.assertTrue(pad.pad)
            for axis in (4,5): s.SDL_JoystickSetVirtualAxis(joy,axis,-32768)
            s.SDL_JoystickSetVirtualAxis(joy,0,20000)
            s.SDL_JoystickSetVirtualAxis(joy,2,-23000)
            self.assertEqual(pad.poll(),0)  # Analog motion must not also press D-pad.
            self.assertEqual(pad.axes,(20000,0,-23000,0))
            s.SDL_JoystickSetVirtualButton(joy,0,1)
            self.assertEqual(pad.poll(),2)  # South is N64 A.
            s.SDL_JoystickSetVirtualAxis(joy,4,32767)
            self.assertEqual(pad.poll(),2|4096)
            s.SDL_JoystickClose(joy); joy=None
            s.SDL_JoystickDetachVirtual(index); index=-1
            pad.poll()
            if s.SDL_NumJoysticks()==0:
                self.assertEqual(pad.axes,(0,0,0,0))
        finally:
            if joy: s.SDL_JoystickClose(joy)
            if index>=0: s.SDL_JoystickDetachVirtual(index)
            pad.close()

    def test_saved_profile_drives_runtime_buttons_and_axes(self):
        profile = defaults('n64', 0)
        pad=Gamepad(layout_for('n64'), bindings=profile['gamepad'])
        if not pad.sdl:
            self.skipTest('SDL2 is unavailable')
        s=pad.sdl
        s.SDL_JoystickAttachVirtual.argtypes=[C.c_int]*4
        s.SDL_JoystickAttachVirtual.restype=C.c_int
        s.SDL_JoystickOpen.argtypes=[C.c_int]; s.SDL_JoystickOpen.restype=C.c_void_p
        s.SDL_JoystickSetVirtualAxis.argtypes=[C.c_void_p,C.c_int,C.c_int16]
        s.SDL_JoystickSetVirtualButton.argtypes=[C.c_void_p,C.c_int,C.c_ubyte]
        s.SDL_JoystickClose.argtypes=[C.c_void_p]
        s.SDL_JoystickDetachVirtual.argtypes=[C.c_int]
        index=s.SDL_JoystickAttachVirtual(1,6,21,0)
        joy=None
        try:
            joy=s.SDL_JoystickOpen(index)
            pad.pad=s.SDL_GameControllerOpen(index)
            for axis in (4,5): s.SDL_JoystickSetVirtualAxis(joy,axis,-32768)
            s.SDL_JoystickSetVirtualButton(joy,0,1)
            s.SDL_JoystickSetVirtualAxis(joy,0,-22000)
            s.SDL_JoystickSetVirtualAxis(joy,4,24000)
            self.assertEqual(pad.poll(), 2 | 4096)
            self.assertEqual(pad.axes[0], -22000)
        finally:
            if joy: s.SDL_JoystickClose(joy)
            if index>=0: s.SDL_JoystickDetachVirtual(index)
            pad.close()
