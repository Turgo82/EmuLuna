"""NES artwork → saved mappings → live player → SDL → unmodified core."""
import ctypes as C
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import Qt, QPointF
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from emuluna.controller_diagrams import ControllerDiagram
from emuluna.controller_profiles import bind, defaults, load_profile, save_profile
from emuluna.library import Library
from emuluna.player import Player
from emuluna.settings import SettingsDialog
from core_fixture import install_core
from nes_rom import nes


class NesControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_nes_button_centers_open_the_correct_binding(self):
        with tempfile.TemporaryDirectory() as tmp:
            library = Library(tmp)
            dialog = SettingsDialog(library)
            try:
                dialog.show(); dialog.show_page('controls')
                page = dialog.controls_page
                page.system.setCurrentIndex(page.system.findData('nes'))
                QTest.qWait(20)
                # Coordinates taken from the supplied illustration, not the
                # hotspot metadata: a swapped or moved hit area must fail.
                for x, action, key in ((771,'button:2',Qt.Key_V),(915,'button:1',Qt.Key_C)):
                    point = page.diagram.artwork_transform().map(QPointF(x,340)).toPoint()
                    QTest.mouseClick(page.diagram, Qt.LeftButton, pos=point)
                    button = page.mapping_buttons[action]
                    self.assertTrue(button.recording)
                    QTest.keyClick(button, key)
                    self.assertEqual(load_profile(library,'nes',0)['keyboard'][action], [f'key:{int(key)}'])
            finally:
                dialog.close(); library.close()

    def test_live_nes_remap_with_virtual_controller_and_keyboard(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            library = Library(root/'library')
            writer = None
            player = None
            joy = None
            index = -1
            try:
                install_core(library, 'nestopia')
                rom = root/'NES buttons.nes'; rom.write_bytes(nes())
                game = library.import_file(rom)[0]
                with patch.object(Player, 'setup_audio'):
                    player = Player(library, game, muted=True, frame_limit=10000)
                player.timer.stop()
                player.show(); player.activateWindow(); QTest.qWait(20)
                for _ in range(180): player.core.frame()
                original_handle = player.core.handle
                pad = player.pad
                if not pad.sdl:
                    self.skipTest('SDL2 is unavailable')
                s = pad.sdl
                for name, ret, args in (
                    ('SDL_JoystickAttachVirtual', C.c_int, [C.c_int]*4),
                    ('SDL_JoystickOpen', C.c_void_p, [C.c_int]),
                    ('SDL_JoystickSetVirtualButton', C.c_int, [C.c_void_p,C.c_int,C.c_ubyte]),
                    ('SDL_JoystickClose', None, [C.c_void_p]),
                    ('SDL_JoystickDetachVirtual', C.c_int, [C.c_int])):
                    fn = getattr(s,name); fn.restype=ret; fn.argtypes=args
                index = s.SDL_JoystickAttachVirtual(1,6,21,0)
                self.assertGreaterEqual(index,0)
                joy = s.SDL_JoystickOpen(index)
                pad.pad = s.SDL_GameControllerOpen(index)
                self.assertTrue(pad.pad)

                # A separate DB connection models the library/settings process.
                writer = Library(library.root)
                profile = defaults('nes',0)
                bind(profile,'gamepad','button:1','button:2')  # west → NES A
                bind(profile,'gamepad','button:2','button:0')  # south → NES B
                bind(profile,'keyboard','button:1',f'key:{int(Qt.Key_V)}')
                bind(profile,'keyboard','button:2',f'key:{int(Qt.Key_C)}')
                save_profile(writer,'nes',0,profile)
                player.keys = 1  # Remapping must release any old held key.
                QTest.qWait(1150)
                self.assertEqual(player.keys,0)
                self.assertEqual(player.control_profiles[0],profile)
                self.assertEqual(player.core.handle,original_handle)
                self.assertIs(player.pad,pad)  # Mapping edits need no SDL reopen.
                QTest.keyPress(player,Qt.Key_X)
                self.assertEqual(player.keys,0)
                QTest.keyRelease(player,Qt.Key_X)
                for physical, key, expected in ((2,Qt.Key_V,1),(0,Qt.Key_C,2)):
                    QTest.keyPress(player,key)
                    self.assertEqual(player.keys,expected)
                    QTest.keyRelease(player,key)
                    self.assertEqual(player.keys,0)
                    s.SDL_JoystickSetVirtualButton(joy,physical,1)
                    frames=[]
                    for frame in range(40):
                        buttons=pad.poll()
                        self.assertEqual(buttons,expected)
                        pixels,_=player.core.frame(buttons)
                        if frame>10: frames.append(pixels)
                    self.assertTrue(all(image==frames[0] for image in frames))
                    b,g,r,a=pixels[10000:10004]
                    self.assertTrue(r>2*g and r>2*b if expected==1 else g>2*r and g>2*b)
                    s.SDL_JoystickSetVirtualButton(joy,physical,0)
                    self.assertEqual(pad.poll(),0)
                # Unrelated preference edits must not interrupt held input.
                player.keys=2
                writer.set_setting('volume','50')
                player.refresh_controls()
                self.assertEqual(player.keys,2)
            finally:
                if joy: s.SDL_JoystickClose(joy)
                if index>=0: s.SDL_JoystickDetachVirtual(index)
                if player: player.close()
                if writer: writer.close()
                library.close()
