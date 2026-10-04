"""All catalog systems expose persistent, usable player control profiles."""
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from PySide6.QtCore import Qt
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication

from media_stub import isolate_audio
isolate_audio()
from emuluna.controller_profiles import (SPECS, actions, bind, defaults, gamepad_state,
    keyboard_layout, load_profile, save_profile)
from emuluna.controls import keyboard_buttons
from emuluna.library import Library
from emuluna.settings import SettingsDialog
from emuluna.systems import SYSTEMS


class ControllerSettings(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.library = Library(Path(self.tmp.name))

    def tearDown(self):
        self.library.close()
        self.tmp.cleanup()

    def test_every_system_and_player_has_a_complete_data_driven_profile(self):
        self.assertEqual(set(SPECS), set(SYSTEMS))
        self.assertEqual(len(SPECS), 33)
        for system, spec in SPECS.items():
            self.assertGreaterEqual(spec['players'], 1, system)
            self.assertLessEqual(spec['players'], 4, system)
            available = dict(actions(system))
            for player in range(spec['players']):
                profile = defaults(system, player)
                self.assertEqual(set(profile), {'device', 'keyboard', 'gamepad'})
                self.assertTrue(set(profile['gamepad']).intersection(available), (system, player))
                self.assertLessEqual(set(profile['gamepad']), set(available), system)
                self.assertLessEqual(set(profile['keyboard']), set(available), system)
                if player == 0:
                    self.assertTrue(profile['keyboard'], system)

    def test_keyboard_and_controller_remaps_persist_without_duplicates(self):
        profile = defaults('snes', 0)
        bind(profile, 'keyboard', 'button:1', f'key:{int(Qt.Key_V)}')
        bind(profile, 'keyboard', 'button:2', f'key:{int(Qt.Key_V)}')
        self.assertEqual(profile['keyboard']['button:1'], [])
        profile['device'] = 'index:3'
        save_profile(self.library, 'snes', 0, profile)
        loaded = load_profile(self.library, 'snes', 0)
        self.assertEqual(loaded['device'], 'index:3')
        self.assertEqual(loaded['keyboard']['button:2'], [f'key:{int(Qt.Key_V)}'])
        self.assertEqual(keyboard_buttons(keyboard_layout('snes', loaded))[Qt.Key_V], 2)
        # Unknown actions and malformed tokens are ignored at the database edge.
        self.library.set_setting('controls.snes.0', json.dumps({'keyboard': {
            'not-an-action': ['key:88'], 'button:1': ['shell:bad']}}))
        repaired = load_profile(self.library, 'snes', 0)
        self.assertNotIn('not-an-action', repaired['keyboard'])
        self.assertNotEqual(repaired['keyboard']['button:1'], ['shell:bad'])

    def test_controller_buttons_axes_and_directions_map_to_libretro_input(self):
        bindings = {'button:1': ['button:0'], 'button:4096': ['axis:4:1'],
                    'axis:0:-1': ['axis:0:-1'], 'axis:1:1': ['axis:1:1']}
        buttons, axes = gamepad_state(bindings, {0}, (-21000, 19000, 0, 0, 24000, 0))
        self.assertEqual(buttons, 1 | 4096)
        self.assertEqual(axes, (-21000, 19000, 0, 0))

    def test_turbografx_mode_switch_is_safe_and_legacy_trigger_binding_migrates(self):
        for system in ('pce', 'pcecd'):
            self.assertNotIn('button:4096', defaults(system, 0)['gamepad'])
        legacy = defaults('pcecd', 0)
        legacy['gamepad']['button:4096'] = ['axis:4:1']
        self.library.set_setting('controls.pcecd.0', json.dumps(legacy))
        repaired = load_profile(self.library, 'pcecd', 0)
        self.assertNotIn('button:4096', repaired['gamepad'])
        self.assertEqual(self.library.setting('controls_migration.safe_special_buttons.pcecd.0'), '1')
        # After the one-time migration, an intentional user mapping is kept.
        repaired['gamepad']['button:4096'] = ['axis:4:1']
        save_profile(self.library, 'pcecd', 0, repaired)
        self.assertEqual(load_profile(self.library, 'pcecd', 0)['gamepad']['button:4096'], ['axis:4:1'])

    def test_shoulder_trigger_and_stick_click_controls_are_exposed(self):
        snes = dict(actions('snes'))
        self.assertEqual((snes['button:512'], snes['button:256']), ('L', 'R'))

        n64 = dict(actions('n64'))
        self.assertEqual(n64['button:4096'], 'Z trigger')
        self.assertEqual((n64['button:512'], n64['button:256']), ('L', 'R'))

        playstation = dict(actions('psx'))
        self.assertEqual([playstation[f'button:{bit}'] for bit in
                          (512, 256, 4096, 8192, 16384, 32768)],
                         ['L1', 'R1', 'L2', 'R2', 'L3', 'R3'])
        profile = defaults('psx', 0)
        self.assertEqual(profile['gamepad']['button:4096'], ['axis:4:1'])
        self.assertEqual(profile['gamepad']['button:8192'], ['axis:5:1'])
        self.assertEqual(profile['gamepad']['button:16384'], ['button:7'])
        self.assertEqual(profile['gamepad']['button:32768'], ['button:8'])

    def test_settings_navigation_and_live_keyboard_capture(self):
        dialog = SettingsDialog(self.library)
        try:
            dialog.show()
            QTest.qWait(30)
            self.assertEqual([dialog.tabs.tabText(i) for i in range(dialog.tabs.count())],
                ['General', 'Library', 'Gameplay', 'Controls', 'Cores', 'System Files'])
            dialog.show_page('controls')
            page = dialog.controls_page
            QTest.qWait(10)
            self.assertEqual(page.system.count(), len(SYSTEMS))
            page.system.setCurrentIndex(page.system.findData('n64'))
            self.assertEqual(page.player.count(), 4)
            page.player.setCurrentIndex(3)
            self.assertIn('Player 4', page.summary.text())
            page.system.setCurrentIndex(page.system.findData('snes'))
            page.player.setCurrentIndex(0)
            first = page.mapping_buttons['button:64']
            QTest.mouseClick(first, Qt.LeftButton)
            QTest.keyClick(first, Qt.Key_V)
            profile = load_profile(self.library, 'snes', 0)
            self.assertIn(f'key:{int(Qt.Key_V)}', profile['keyboard']['button:64'])
            page.source.setCurrentIndex(page.source.findData('gamepad'))
            self.assertTrue(page.device.isVisible())
            self.assertTrue(page.device_label.isVisible())
            page.source.setCurrentIndex(page.source.findData('keyboard'))
            self.assertFalse(page.device.isVisible())
            self.assertFalse(page.device_label.isVisible())
        finally:
            dialog.controls_page.device_timer.stop()
            dialog.core_watch.stop()
            dialog.close()

    def test_opening_settings_does_not_scan_controllers_until_requested(self):
        with patch('emuluna.controller_settings.Gamepad.devices') as devices:
            dialog = SettingsDialog(self.library)
            try:
                dialog.show()
                QTest.qWait(30)
                devices.assert_not_called()
                dialog.show_page('controls')
                QTest.qWait(20)
                devices.assert_not_called()  # Keyboard mode needs no devices.
                dialog.controls_page.source.setCurrentIndex(
                    dialog.controls_page.source.findData('gamepad'))
                QTest.qWait(30)
                devices.assert_called()
            finally:
                dialog.controls_page.capture_timer.stop()
                dialog.controls_page.device_timer.stop()
                dialog.core_watch.stop()
                dialog.close()

    def test_controller_capture_accepts_shoulders_and_analog_triggers(self):
        class FakeGamepad:
            @classmethod
            def devices(cls):
                return [('index:0', 'Test controller')]

            def __init__(self, *args, **kwargs):
                self.pad = object()
                self.buttons = set()
                self.raw_axes = (0, 0, 0, 0, -32768, -32768)
                self.pressed = None
                self.trigger = False

            def poll(self):
                self.buttons = {self.pressed} if self.pressed is not None else set()
                self.raw_axes = (0, 0, 0, 0, 26000 if self.trigger else -32768, -32768)
                return 0

            def close(self):
                self.pad = None

        with patch('emuluna.controller_settings.Gamepad', FakeGamepad):
            dialog = SettingsDialog(self.library)
            try:
                dialog.show_page('controls')
                page = dialog.controls_page
                page.system.setCurrentIndex(page.system.findData('psx'))
                page.source.setCurrentIndex(page.source.findData('gamepad'))

                def binding_button(action):
                    return page.mapping_buttons[action]

                page.start_controller_capture('button:512', binding_button('button:512'))
                page.capture_pad.pressed = 10
                page.poll_controller_capture()
                self.assertEqual(load_profile(self.library, 'psx', 0)['gamepad']['button:512'],
                                 ['button:10'])

                page.start_controller_capture('button:4096', binding_button('button:4096'))
                # Releasing a trigger that was already held when capture began
                # must not create a backwards-axis binding.
                page.capture_neutral_axes = (0, 0, 0, 0, 26000, -32768)
                page.poll_controller_capture()
                self.assertIsNotNone(page.capture_pad)
                page.capture_pad.trigger = True
                page.poll_controller_capture()
                self.assertEqual(load_profile(self.library, 'psx', 0)['gamepad']['button:4096'],
                                 ['axis:4:1'])
            finally:
                dialog.controls_page.capture_timer.stop()
                dialog.controls_page.device_timer.stop()
                dialog.core_watch.stop()
                dialog.close()


if __name__ == '__main__':
    unittest.main()
