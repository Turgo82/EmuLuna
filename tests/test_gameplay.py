"""Gameplay HUD, hidden-menu shortcuts, saved settings and audio transitions."""
import os
from pathlib import Path
import tempfile
import unittest
from media_stub import isolate_audio
isolate_audio()
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import Qt, QPoint, QByteArray, QSize
from PySide6.QtGui import QImage
from PySide6.QtMultimedia import QAudio
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from emuluna.library import Library
from emuluna.player import Player
from emuluna.core import CoreError
from emuluna.settings import SettingsDialog
from emuluna.video_screen import Screen
from snes_rom import snes
from core_fixture import install_core


class GameplayTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.library = Library(self.root / "library")
        rom = self.root / "HUD diagnostic.sfc"
        rom.write_bytes(snes())
        self.game_id = self.library.import_file(rom)[0]
        self.player = None

    def tearDown(self):
        if self.player:
            self.player.close()
        self.library.close()
        self.tmp.cleanup()

    def make_player(self, frame_limit=10000):
        install_core(self.library)
        with patch.object(Player, "setup_audio"):
            self.player = Player(self.library, self.game_id, muted=True, frame_limit=frame_limit)
        self.player.timer.stop()
        self.player.show()
        self.player.activateWindow()
        QTest.qWait(20)
        self.player.focus_paused = False
        return self.player

    def test_integer_scaling_respects_non_square_n64_pixels(self):
        screen = Screen()
        screen.resize(1064, 760)
        screen.frame = QImage(640, 240, QImage.Format_RGB32)
        screen.display_aspect = 4 / 3
        screen.integer_scale = True
        target = screen.target_rect()
        self.assertEqual(target.size(), QSize(960, 720))
        self.assertEqual(screen.integer_size(), QSize(960, 720))
        self.assertEqual(target.center(), screen.rect().center())
        screen.integer_scale = False
        target = screen.target_rect()
        self.assertEqual(target.height(), 760)
        self.assertAlmostEqual(target.width() / target.height(), 4 / 3, places=2)

    def test_gameplay_scaling_menu_switches_and_saves_mode(self):
        player = self.make_player()
        self.assertTrue(player.fit_action.isChecked())
        player.integer_action.trigger()
        self.assertTrue(player.screen.integer_scale)
        self.assertEqual(self.library.setting('integer_scale'), '1')
        player.fit_action.trigger()
        self.assertFalse(player.screen.integer_scale)
        self.assertEqual(self.library.setting('integer_scale'), '0')
        player.resize(901, 701)
        QTest.qWait(20)
        player.screen.frame = QImage(256, 224, QImage.Format_RGB32)
        player.screen.display_aspect = 8 / 7
        player.resize_integer_action.trigger()
        QTest.qWait(20)
        self.assertTrue(player.screen.integer_scale)
        self.assertEqual(player.screen.target_rect().size(), player.screen.size())

    def test_hud_reveals_hides_keeps_pause_controls_and_fits_small_window(self):
        player = self.make_player()
        items = [player.hud.layout().itemAt(index)
                 for index in range(player.hud.layout().count())]
        widgets = [item.widget() for item in items if item.widget()]
        self.assertEqual(widgets, [player.hud.buttons[name] for name in
            ("power", "pause", "reset", "save", "load")] +
            [player.hud.options, player.hud.buttons["mute"], player.hud.volume,
             player.hud.buttons["fullscreen"]])
        fullscreen_index = next(index for index, item in enumerate(items)
                                if item.widget() is player.hud.buttons["fullscreen"])
        self.assertGreaterEqual(items[fullscreen_index - 1].spacerItem().sizeHint().width(), 8)
        self.assertEqual(player.hud.buttons["power"].objectName(), "powerButton")
        self.assertFalse(player.stop_action.icon().isNull())
        QTest.mouseMove(player.screen, QPoint(20, 20))
        self.assertTrue(player.hud.isVisible())
        QTest.mouseMove(player.hud, player.hud.rect().center())
        player.hud.fade_away()
        QTest.qWait(20)
        self.assertFalse(player.hud.isVisible())
        self.assertEqual(player.screen.cursor().shape(), Qt.BlankCursor)
        QTest.mouseMove(player.screen, QPoint(35, 35))
        self.assertTrue(player.hud.isVisible())
        self.assertNotEqual(player.screen.cursor().shape(), Qt.BlankCursor)
        QTest.mouseClick(player.hud.buttons["pause"], Qt.LeftButton)
        self.assertTrue(player.paused)
        self.assertEqual(player.hud.buttons["pause"].accessibleName(), "Resume")
        player.hud.fade_away()
        self.assertTrue(player.hud.isVisible())
        QTest.mouseClick(player.hud.buttons["pause"], Qt.LeftButton)
        self.assertFalse(player.paused)
        player.resize(420, 400)
        QTest.qWait(20)
        self.assertLessEqual(player.hud.geometry().right(), player.screen.width())
        self.assertGreaterEqual(player.hud.geometry().left(), 0)
        self.assertFalse(player.hud.volume.isVisible())

    def test_fullscreen_shortcuts_and_slots_stay_available_without_toolbar(self):
        player = self.make_player()
        menu_titles = [action.text() for action in player.hud.options.menu().actions()]
        for hidden in ("Restore automatic save", "Full screen", "Mute", "Volume up", "Volume down", "Stop"):
            self.assertNotIn(hidden, menu_titles)
        menu = player.hud.options.menu()
        self.assertTrue(menu.testAttribute(Qt.WA_TranslucentBackground))
        self.assertTrue(all(submenu.testAttribute(Qt.WA_TranslucentBackground)
                            for submenu in menu.findChildren(type(menu))))
        for _ in range(120):
            player.next_frame = 0
            player.tick()
        self.assertFalse(player.game_toolbar.isVisible())
        player.slot_actions[3].trigger()
        self.assertEqual(player.slot.currentIndex(), 3)
        self.assertEqual(player.fullscreen_action.property('emuluna_hud_icon'), 'fullscreen')
        player.fullscreen()
        QTest.qWait(20)
        self.assertTrue(player.isFullScreen())
        self.assertEqual(player.fullscreen_action.property('emuluna_hud_icon'), 'fullscreen-exit')
        self.assertFalse(player.menuBar().isVisible())
        self.assertFalse(player.statusBar().isVisible())
        QTest.keyClick(player, Qt.Key_F5)
        self.assertTrue(player.state_path().is_file())
        QTest.keyClick(player, Qt.Key_F8)
        self.assertIn("Restored slot 4", player.statusBar().currentMessage())
        QTest.keyClick(player, Qt.Key_Escape)
        self.assertFalse(player.isFullScreen())
        self.assertEqual(player.fullscreen_action.property('emuluna_hud_icon'), 'fullscreen')
        self.assertFalse(player.menuBar().isVisible())
        self.assertEqual(player.menuBar().actions(), [])
        self.assertTrue(player.statusBar().isVisible())

    def test_state_notice_visible_in_fullscreen_and_reports_failures(self):
        player = self.make_player()
        for _ in range(120): player.core.frame()
        player.fullscreen()
        QTest.qWait(20)
        player.save()
        self.assertTrue(player.notice.isVisible())
        self.assertEqual(player.notice.text(), 'State saved · Slot 1')
        self.assertFalse(player.menuBar().isVisible())
        self.assertTrue(player.notice.testAttribute(Qt.WA_TransparentForMouseEvents))
        player.load()
        self.assertEqual(player.notice.text(), 'State loaded · Slot 1')
        player.save_auto()
        self.assertEqual(player.notice.text(), 'State loaded · Slot 1')
        player.notice.hide()
        player.save_auto()
        self.assertFalse(player.notice.isVisible())
        player.load_auto()
        self.assertEqual(player.notice.text(), 'Automatic state loaded')
        with patch.object(player.core, 'load_state', side_effect=CoreError('No state')), patch.object(player, 'report'):
            player.load()
        self.assertEqual(player.notice.text(), 'State could not be loaded')
        player.notice.timer.start(30)
        QTest.qWait(60)
        self.assertFalse(player.notice.isVisible())
        player.fullscreen()
        player.resize(380, 350)
        player.notice.show_message('State saved · Slot 1')
        QTest.qWait(20)
        self.assertLessEqual(player.notice.geometry().right(), player.screen.width())
        self.assertEqual(player.menuBar().actions(), [])

    def test_volume_pause_and_fast_forward_reset_audio_and_persist(self):
        self.library.set_setting("fast_forward_speed", "6")
        player = self.make_player()
        audio = Mock()
        player.audio = audio
        player.audio_device = audio.start.return_value
        player.hud.volume.setValue(45)
        self.assertEqual(player.volume, 0.45)
        self.assertEqual(self.library.setting("volume"), "45")
        audio.setVolume.assert_called_with(0)
        player.toggle_mute()
        audio.setVolume.assert_called_with(0.45)
        self.assertEqual(self.library.setting("muted"), "0")
        QTest.keyPress(player, Qt.Key_Tab)
        self.assertTrue(player.fast)
        self.assertEqual(player.fast_speed, 6)
        audio.reset.assert_called()
        audio.suspend.assert_called()
        QTest.keyRelease(player, Qt.Key_Tab)
        self.assertFalse(player.fast)
        audio.resume.assert_called()
        player.toggle_pause()
        audio.suspend.assert_called()
        player.toggle_pause()
        audio.resume.assert_called()
        QTest.keyClick(player, Qt.Key_Up, Qt.ControlModifier)
        self.assertEqual(self.library.setting("volume"), "50")
        self.assertEqual(player.keys, 0)

    def test_gameplay_settings_and_default_fullscreen(self):
        settings = SettingsDialog(self.library)
        settings.show_page('gameplay')
        self.assertTrue(settings.game_mode_keep_awake.isChecked())
        settings.unlock_advanced()
        self.assertFalse(settings.minimize_library.isChecked())
        settings.fullscreen_default.setChecked(True)
        settings.hide_cursor.setChecked(False)
        settings.game_mode_keep_awake.setChecked(False)
        settings.minimize_library.setChecked(True)
        settings.fast_speed.setValue(7)
        settings.latency.setValue(120)
        settings.aspect.setCurrentIndex(settings.aspect.findData("square"))
        settings.close()
        player = self.make_player(frame_limit=0)
        self.assertTrue(player.isFullScreen())
        self.assertFalse(player.hud.hide_cursor)
        self.assertEqual(player.fast_speed, 7)
        self.assertIsNone(player.screen.display_aspect)
        self.assertEqual(self.library.setting("audio_latency"), "120")
        self.assertEqual(self.library.setting("game_mode.keep_awake"), "0")
        self.assertEqual(self.library.setting("experimental.minimize_library_during_game"), "1")

    def test_audio_output_selection_and_disconnect_fallback_keep_pause_state(self):
        class Device:
            def __init__(self, name): self.name = name
            def id(self): return QByteArray(self.name.encode())
            def isNull(self): return False
            def isFormatSupported(self, fmt): return True
        default, selected = Device("default"), Device("selected")
        self.library.set_setting("audio_device", bytes(selected.id()).hex())
        self.library.set_setting("audio_latency", "120")
        player = self.make_player()
        player.paused = True
        with patch("emuluna.player.QMediaDevices.defaultAudioOutput", return_value=default), \
             patch("emuluna.player.QMediaDevices.audioOutputs", return_value=[default, selected]) as devices, \
             patch("emuluna.player.QAudioSink") as sink:
            player.setup_audio()
            self.assertIs(sink.call_args.args[0], selected)
            sink.return_value.setBufferSize.assert_called_with(player.core.sample_rate * 4 * 120 // 1000)
            sink.return_value.suspend.assert_called()
            devices.return_value = [default]
            player.audio_output_changed()
            self.assertIs(sink.call_args.args[0], default)
            sink.return_value.suspend.assert_called()
            self.assertTrue(player.paused)

    def test_audio_cushion_and_underrun_recovery_protect_fullscreen_play(self):
        class Harness:
            pass
        player = Harness()
        player.core = Mock(sample_rate=48000)
        device = Mock()
        device.write.side_effect = lambda data: len(data)
        audio = Mock()
        audio.error.return_value = QAudio.Error.UnderrunError
        player.audio = audio
        player.audio_device = device
        player.audio_latency_ms = 80
        player.audio_underruns = 0
        player.paused = player.focus_paused = player.fast = False
        player.prime_audio = lambda milliseconds=None: Player.prime_audio(player, milliseconds)
        expected = player.core.sample_rate * 4 * 40 // 1000 & ~3
        self.assertEqual(player.prime_audio(), expected)
        self.assertEqual(len(device.write.call_args.args[0]), expected)
        Player.audio_state_changed(player, QAudio.State.IdleState)
        self.assertEqual(player.audio_underruns, 1)
        self.assertEqual(len(device.write.call_args.args[0]), expected)
        player.paused = True
        Player.audio_state_changed(player, QAudio.State.IdleState)
        self.assertEqual(player.audio_underruns, 1)


if __name__ == "__main__":
    unittest.main()
