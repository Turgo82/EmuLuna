"""Cover launch/resume controls and confirmed, locked automatic-state removal."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from media_stub import isolate_audio
isolate_audio()
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from PySide6.QtCore import Qt
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication, QMessageBox
from emuluna.app import Window
from emuluna.library import Library
from emuluna.player import Player
from emuluna.core import CoreError
from emuluna.core_manager import CoreManager
from core_fixture import install_core
from snes_rom import snes


class HoverActions(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.lib = Library(self.tmp.name)
        rom = Path(self.tmp.name) / 'Hover.sfc'
        rom.write_bytes(snes())
        self.game = self.lib.import_file(rom)[0]
        install_core(self.lib)
        self.window = Window(self.lib, auto_artwork=False)
        self.window.show()
        QTest.qWait(20)
        self.player = None

    def tearDown(self):
        if self.player and not self.player.closed:
            self.player.close()
        self.window.processes.clear()
        self.window.close()
        self.tmp.cleanup()

    def open_player(self, **kwargs):
        with patch.object(Player, 'setup_audio'):
            self.player = Player(self.lib, self.game, muted=True, **kwargs)
        self.player.timer.stop()
        for _ in range(120): self.player.core.frame()
        return self.player

    def test_hover_play_becomes_resume_with_matching_state_and_no_footer_button(self):
        grid = self.window.games
        item = grid.item(0)
        QTest.mouseMove(grid.viewport(), grid.visualItemRect(item).center())
        self.assertTrue(grid.actions_bar.isVisible())
        self.assertIn('Play', grid.cover_play.text())
        self.assertFalse(grid.cover_restart.isVisible())
        self.assertFalse(hasattr(self.window, "media_open_button"))
        self.open_player().close()
        automatic = self.window.automatic_state(self.game)
        self.assertTrue(automatic.is_file())
        grid.hide_actions()
        grid.show_actions(item)
        self.assertIn('Resume', grid.cover_play.text())
        self.assertTrue(grid.cover_restart.isVisible())
        with patch('emuluna.app.QProcess') as process:
            QTest.mouseClick(grid.cover_play, Qt.LeftButton)
            args = process.return_value.setArguments.call_args.args[0]
            self.assertEqual(args[args.index('--state-file') + 1], str(automatic))
        self.window.processes.clear()
        self.window.resize(850, 560)
        QTest.qWait(40)
        grid.show_actions(item)
        self.assertTrue(grid.viewport().rect().contains(grid.actions_bar.geometry()))
        grid.verticalScrollBar().valueChanged.emit(10)
        self.assertFalse(grid.actions_bar.isVisible())
        self.lib.set_setting('core.snes','missing-build')
        self.assertIsNone(self.window.automatic_state(self.game))
        self.assertTrue(automatic.is_file())

    def test_cancel_keeps_save_and_confirm_only_passes_restart_request(self):
        self.open_player().close()
        automatic = self.window.automatic_state(self.game)
        original = automatic.read_bytes()
        with patch('emuluna.app.QMessageBox.question', return_value=QMessageBox.Cancel), patch.object(self.window,'launch_game') as launch:
            self.window.restart_game(self.game)
            launch.assert_not_called()
        self.assertEqual(automatic.read_bytes(), original)
        with patch('emuluna.app.QMessageBox.question', return_value=QMessageBox.Yes), patch('emuluna.app.QProcess') as process:
            self.window.restart_game(self.game)
            args = process.return_value.setArguments.call_args.args[0]
            self.assertIn('--restart-auto', args)
            self.assertNotIn('--state-file', args)
        self.assertEqual(automatic.read_bytes(), original)  # The locked helper clears it.

    def test_restart_preserves_manual_states_and_battery_and_respects_running_lock(self):
        player = self.open_player()
        player.save()
        manual = player.state_path()
        manual_bytes = manual.read_bytes()
        player.close()
        automatic = self.window.automatic_state(self.game)
        player = self.open_player(restart_auto=True)
        self.assertFalse(automatic.exists())
        self.assertEqual(manual.read_bytes(), manual_bytes)
        player.save_auto()
        with self.assertRaisesRegex(CoreError, 'already running'):
            Player(self.lib, self.game, restart_auto=True)
        self.assertTrue(automatic.exists())
        player.close()
        automatic_bytes = automatic.read_bytes()
        with patch('emuluna.player.Core', side_effect=CoreError('Core unavailable')):
            with self.assertRaisesRegex(CoreError, 'Core unavailable'):
                Player(self.lib, self.game, restart_auto=True)
        self.assertEqual(automatic.read_bytes(), automatic_bytes)

    def test_double_click_and_plain_helper_launch_resume_and_failed_resume_keeps_save(self):
        self.open_player().close()
        automatic = self.window.automatic_state(self.game)
        original = automatic.read_bytes()
        self.window.restore_selection([self.game])
        for view in ('grid', 'list'):
            self.window.change_view(view)
            with patch('emuluna.app.QProcess') as process:
                widget = self.window.games if view == 'grid' else self.window.table
                widget.itemDoubleClicked.emit(widget.item(0) if view == 'grid' else widget.item(0, 0))
                args = process.return_value.setArguments.call_args.args[0]
                self.assertEqual(args[args.index('--state-file') + 1], str(automatic))
                self.assertNotIn('--restart-auto', args)
            self.window.processes.clear()
        player = self.open_player()  # No explicit state argument: helper must find it.
        self.assertEqual(player.pending_state, automatic)
        player.focus_paused = False
        with patch.object(player.core, 'load_state', side_effect=CoreError('Damaged state')), patch.object(player, 'report'):
            for _ in range(3):
                player.next_frame = 0
                player.tick()
        self.assertTrue(player.state_restore_failed)
        self.assertTrue(player.paused)
        player.toggle_pause()
        self.assertTrue(player.state_restore_failed)
        player.close()
        self.assertEqual(automatic.read_bytes(), original)

    def test_core_update_resumes_with_retained_original_build(self):
        self.open_player().close()
        manager = CoreManager(self.lib.root)
        original = manager.installed()['snes9x']
        automatic = self.window.automatic_state(self.game)
        with patch('emuluna.core_manager.probe', return_value={
                'name':'Snes9x', 'version':'updated', 'extensions':'smc|sfc'}):
            updated = manager.install_bytes(manager.path(original).read_bytes() + b'\0', 'snes9x')
        self.assertNotEqual(updated['sha256'], original['sha256'])
        self.assertEqual(self.window.automatic_state(self.game), automatic)
        with patch('emuluna.app.QProcess') as process:
            self.window.launch_game(self.game, automatic)
            args = process.return_value.setArguments.call_args.args[0]
            self.assertEqual(args[args.index('--core-id') + 1], 'snes9x')
            self.assertEqual(args[args.index('--core-sha256') + 1], original['sha256'])
        self.window.processes.clear()
        with patch.object(Player, 'setup_audio'):
            player = Player(self.lib, self.game, muted=True, frame_limit=10000,
                            state_file=automatic, core_id='snes9x', core_sha256=original['sha256'])
        player.timer.stop()
        self.player = player
        for _ in range(3):
            player.next_frame = 0
            player.tick()
        self.assertIsNone(player.pending_state)
        self.assertFalse(player.state_restore_failed)
