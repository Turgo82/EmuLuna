"""Exercise the native callback contract without requiring a GPU or real ROM."""
from pathlib import Path
import os
import shlex
import shutil
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from unittest.mock import Mock
from emuluna.core import Core, CoreError
from emuluna.hardware_render import HardwareCoreDisplay


class HardwareDuplicates(unittest.TestCase):
    def test_entering_context_does_not_clear_the_retained_frame(self):
        display = HardwareCoreDisplay.__new__(HardwareCoreDisplay)
        display.context, display.fbo, display.surface = Mock(), Mock(), Mock()
        display.begin()
        display.context.makeCurrent.assert_called_once_with(display.surface)
        display.fbo.bind.assert_called_once_with()
        display.context.functions.assert_not_called()

    def test_hardware_and_software_duplicates_keep_the_previous_frame(self):
        compiler = shlex.split(os.environ.get('CXX', '')) or [shutil.which('g++') or shutil.which('clang++')]
        if not compiler[0]:
            self.skipTest('A C++ compiler is required for the libretro fixture')
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            binary = directory / 'fixture.so'
            subprocess.run([*compiler, '-shared', '-fPIC', '-std=c++14',
                '-I', str(root / 'native/vendor'), str(root / 'tests/hardware_duplicate_core.cpp'),
                '-o', str(binary)], check=True, capture_output=True)
            rom = directory / 'fixture.bin'
            rom.write_bytes(b'Original test content')
            hardware_pixels = struct.pack('<I', 0xff00ff00) * 16
            software_pixels = struct.pack('<I', 0xff112233) * 16

            class Hardware:
                description = 'OpenGL test fixture'
                fbo = type('Framebuffer', (), {'handle': lambda self: 1})()
                def begin(self): pass
                def end(self): pass
                def close(self): pass
                def capture(self, width, height, bottom_left):
                    return hardware_pixels

            with patch('emuluna.hardware_render.probe_hardware_contexts',
                       return_value=(3, 3, 3, 3)):
                core = Core(rom, 'snes', directory / 'save',
                    libretro_path=binary, core_id='hardware_fixture', allow_hardware=True)
            try:
                self.assertTrue(core.hardware_requested)
                core.attach_hardware(Hardware())
                for _ in range(3):
                    self.assertEqual(core.frame()[0], hardware_pixels)
                self.assertEqual(core.frame()[0], software_pixels)
                self.assertEqual(core.frame()[0], software_pixels)
                # Flycast's commercial-game states exceed the previous 31 MiB cap.
                core.set_option('fixture_state_size', str(36 * 1024 * 1024))
                for name in ('slot-0.oesavestate', 'auto.oesavestate'):
                    state = directory / name
                    core.save_state(state)
                    self.assertGreater(state.stat().st_size, 36 * 1024 * 1024)
                    core.load_state(state)
                automatic = directory / 'auto.oesavestate'
                original = automatic.read_bytes()
                core.set_option('fixture_state_size', str(core.lib.el_state_size_limit() + 1))
                with self.assertRaises(CoreError):
                    core.save_state(automatic)
                self.assertEqual(automatic.read_bytes(), original)
                excessive = directory / 'excessive.state'
                with excessive.open('wb') as file:
                    file.truncate(core.lib.el_state_size_limit() + 16385)
                with self.assertRaisesRegex(CoreError, 'too large'):
                    core.load_state(excessive)
                self.assertFalse(core.lib.el_load_state(core.handle, os.fsencode(excessive)))
            finally:
                core.close()
