"""Qt keyboard/save integration and SDL controller mapping; no physical device needed."""
import ctypes as C
import os
from pathlib import Path
import sys
import tempfile
import unittest
from media_stub import isolate_audio
isolate_audio()
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import Qt, QByteArray
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication, QMainWindow, QMenu
from emuluna.app import Window
from emuluna.gamepad import Gamepad
from emuluna.library import Library
from emuluna.player import Player
from snes_rom import snes
from core_fixture import install_core


class DesktopIntegration(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_snes_library_keyboard_state_shortcuts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            rom = root / "SNES diagnostic.sfc"
            rom.write_bytes(snes())
            library = Library(root / "library")
            game_id = library.import_file(rom)[0]
            install_core(library)
            window = Window(library, auto_artwork=False)
            snes_row = next(i for i in range(window.nav.count()) if window.nav.item(i).data(Qt.UserRole) == "snes")
            window.nav.setCurrentRow(snes_row)
            self.assertEqual(window.games.count(), 1)
            window.games.setCurrentRow(0)
            self.assertEqual(len(window.selected_ids()), 1)
            # Desktop audio is verified separately against the actual audio service.
            with patch.object(Player, "setup_audio"):
                p = Player(library, game_id, muted=True, frame_limit=10000)
            p.timer.stop()
            p.show()
            p.activateWindow()
            QTest.qWait(30)
            def advance(n):
                for _ in range(n):
                    p.next_frame = 0
                    p.tick()
                return p.screen.frame.copy()
            try:
                baseline = advance(120)
                for key in [Qt.Key_X, Qt.Key_Z, Qt.Key_S, Qt.Key_A, Qt.Key_Q, Qt.Key_W,
                            Qt.Key_Up, Qt.Key_Down, Qt.Key_Left, Qt.Key_Right, Qt.Key_Return, Qt.Key_Backspace]:
                    with self.subTest(key=key):
                        QTest.keyPress(p, key)
                        self.assertNotEqual(advance(5), baseline)
                        QTest.keyRelease(p, key)
                        self.assertEqual(advance(5), baseline)
                QTest.keyClick(p, Qt.Key_F5)
                self.assertTrue(p.state_path().is_file())
                QTest.keyPress(p, Qt.Key_X)
                self.assertNotEqual(advance(5), baseline)
                QTest.keyRelease(p, Qt.Key_X)
                QTest.keyClick(p, Qt.Key_F8)
                self.assertEqual(advance(5), baseline)
                self.assertAlmostEqual(p.screen.display_aspect, 4 / 3, places=6)
            finally:
                p.close()
                window.close()
            self.assertTrue(p.state_path("auto.oesavestate").exists())

    def test_hidden_wayland_player_exposes_window_and_restores_library(self):
        with tempfile.TemporaryDirectory() as tmp:
            library = Library(Path(tmp) / "library")
            window = Window(library, auto_artwork=False)
            window.show()
            process = Mock()
            process.log = bytearray()
            process.ready_notified = False
            process.focus_notified = False
            process.readAllStandardOutput.side_effect = [
                QByteArray(b"EMULUNA_GAME_READY\nEMULUNA_GAME_NEEDS_FOCUS\n"),
                QByteArray(),
            ]
            game_id = "focus-diagnostic"
            window.processes[game_id] = process
            try:
                with patch.object(window, "showMinimized") as minimize:
                    window.process_output(process)
                    minimize.assert_called_once_with()
                self.assertTrue(process.ready_notified)
                self.assertTrue(process.focus_notified)
                self.assertIsNotNone(window.game_restore_state)
                window.game_closed(game_id, 0)
                self.assertIsNone(window.game_restore_state)
                self.assertFalse(window.processes)
            finally:
                window.close()
                library.close()

    def test_frozen_player_starts_as_an_independent_application(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            rom = root / "Player process.sfc"
            rom.write_bytes(snes())
            library = Library(root / "library")
            game_id = library.import_file(rom)[0]
            window = Window(library, auto_artwork=False)
            try:
                with (patch("emuluna.app.QProcess") as process,
                      patch("emuluna.app.CoreManager.selection", return_value={"id": "snes9x"}),
                      patch.object(sys, "frozen", True, create=True)):
                    window.launch_game(game_id)
                    child = process.return_value
                    args = child.setArguments.call_args.args[0]
                    self.assertEqual(args[0], "--player-process")
                    environment = child.setProcessEnvironment.call_args.args[0]
                    self.assertEqual(environment.value("PYINSTALLER_RESET_ENVIRONMENT"), "1")
            finally:
                window.processes.clear()
                window.close()
                library.close()

    def test_virtual_controller_buttons_and_disconnect(self):
        pad = Gamepad()
        if not pad.sdl:
            self.skipTest("SDL2 not installed")
        s = pad.sdl
        specs = {
            "SDL_JoystickAttachVirtual": (C.c_int, [C.c_int, C.c_int, C.c_int, C.c_int]),
            "SDL_JoystickOpen": (C.c_void_p, [C.c_int]),
            "SDL_JoystickSetVirtualButton": (C.c_int, [C.c_void_p, C.c_int, C.c_ubyte]),
            "SDL_JoystickDetachVirtual": (C.c_int, [C.c_int]),
            "SDL_JoystickClose": (None, [C.c_void_p]),
        }
        if not hasattr(s, "SDL_JoystickAttachVirtual"):
            pad.close()
            self.skipTest("SDL2 virtual joysticks require SDL 2.0.14 or newer")
        for name, (result, args) in specs.items():
            f = getattr(s, name)
            f.restype, f.argtypes = result, args
        index = s.SDL_JoystickAttachVirtual(1, 6, 15, 0)
        self.assertGreaterEqual(index, 0)
        pad.pad = s.SDL_GameControllerOpen(index)
        joy = s.SDL_JoystickOpen(index)
        self.assertTrue(pad.pad and joy)
        try:
            for button, expected in {1:1, 0:2, 3:1024, 2:2048, 9:512, 10:256,
                                      4:4, 6:8, 14:16, 13:32, 11:64, 12:128}.items():
                with self.subTest(button=button):
                    self.assertEqual(s.SDL_JoystickSetVirtualButton(joy, button, 1), 0)
                    self.assertEqual(pad.poll(), expected)
                    s.SDL_JoystickSetVirtualButton(joy, button, 0)
                    self.assertEqual(pad.poll(), 0)
            s.SDL_JoystickClose(joy)
            joy = None
            s.SDL_JoystickDetachVirtual(index)
            index = -1
            self.assertFalse(s.SDL_GameControllerGetAttached(pad.pad))
            if s.SDL_NumJoysticks() == 0:
                self.assertEqual(pad.poll(), 0)
                self.assertFalse(pad.pad)
        finally:
            if joy: s.SDL_JoystickClose(joy)
            if index >= 0: s.SDL_JoystickDetachVirtual(index)
            pad.close()

    def test_rumble_is_scaled_refreshed_and_stopped(self):
        calls = []
        pad = Gamepad.__new__(Gamepad)
        pad.pad = object()
        pad.rumble_fn = lambda handle, strong, weak, duration: calls.append(
            (handle, strong, weak, duration)) or 0
        pad.rumble_state = (0, 0)
        pad.rumble_refresh = 0.0
        pad.get_error_fn = lambda: b''
        pad.rumble_error = ''
        self.assertTrue(pad.set_rumble(65535, 12345))
        self.assertEqual(calls[-1][1:], (65535, 12345, 250))
        pad.stop_rumble()
        self.assertEqual(calls[-1][1:], (0, 0, 0))

    def test_playstation_bluetooth_rumble_reports_are_enabled(self):
        pad = Gamepad()
        try:
            if not pad.sdl:
                self.skipTest('SDL2 is unavailable')
            get_hint = pad.sdl.SDL_GetHint
            get_hint.restype = C.c_char_p
            get_hint.argtypes = [C.c_char_p]
            self.assertEqual(get_hint(b'SDL_JOYSTICK_HIDAPI_PS5_RUMBLE'), b'1')
            self.assertEqual(get_hint(b'SDL_JOYSTICK_HIDAPI_PS4_RUMBLE'), b'1')
        finally:
            pad.close()

    def test_accessory_menu_updates_live_core_and_persists_choice(self):
        class Store:
            def __init__(self): self.saved = []
            def set_setting(self, key, value): self.saved.append((key, value))
        class CoreStub:
            def __init__(self): self.options = []
            def set_option(self, key, value): self.options.append((key, value))
        class Notice:
            def __init__(self): self.messages = []
            def show_message(self, message): self.messages.append(message)
        player = QMainWindow()
        player.library, player.core, player.notice = Store(), CoreStub(), Notice()
        record = {"group":"Controller Pak", "count":1, "player":1,
                  "key":"parallel-n64-pak1", "setting":"core_option.game.parallel.pak1",
                  "value":"memory", "choices":(("memory","Memory Pak"),("rumble","Rumble Pak")),
                  "restart":False}
        player.accessory_options = [record]
        player.set_accessory = lambda record, value, label: Player.set_accessory(player, record, value, label)
        menu = QMenu(player)
        Player.add_accessory_menus(player, menu)
        def actions(menu):
            return [child for action in menu.actions()
                    for child in (actions(action.menu()) if action.menu() else [action])]
        rumble = next(action for action in actions(menu) if action.text() == "Rumble Pak")
        rumble.trigger()
        self.assertEqual(player.library.saved[-1], (record["setting"], "rumble"))
        self.assertEqual(player.core.options[-1], (record["key"], "rumble"))
        self.assertIn("Rumble Pak", player.notice.messages[-1])

    def test_runtime_audio_toggle_updates_core_and_persists_choice(self):
        class Store:
            def __init__(self): self.saved = []
            def set_setting(self, key, value): self.saved.append((key, value))
        class CoreStub:
            def __init__(self): self.options = []
            def set_option(self, key, value): self.options.append((key, value))
        class Notice:
            def __init__(self): self.messages = []
            def show_message(self, message): self.messages.append(message)
        player = QMainWindow()
        player.library, player.core, player.notice = Store(), CoreStub(), Notice()
        record = {"group":"Genesis Audio", "label":"Model 1 low-pass filter",
                  "key":"genesis_plus_gx_audio_filter",
                  "setting":"core_option.genesis.genesis_plus_gx.genesis_plus_gx_audio_filter",
                  "default":"low-pass", "enabled":"low-pass", "disabled":"disabled",
                  "value":"low-pass"}
        player.runtime_options = [record]
        player.set_runtime_option = lambda record, checked: Player.set_runtime_option(player, record, checked)
        menu = QMenu(player)
        Player.add_runtime_option_menus(player, menu)
        action = record["action"]
        self.assertTrue(action.isChecked())
        action.trigger()
        self.assertEqual(player.library.saved[-1], (record["setting"], "disabled"))
        self.assertEqual(player.core.options[-1], (record["key"], "disabled"))
        self.assertIn("Off", player.notice.messages[-1])


if __name__ == "__main__":
    unittest.main()
