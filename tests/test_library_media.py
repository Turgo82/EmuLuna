"""Library sections, console grouping and safe launch of existing states."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from media_stub import isolate_audio
isolate_audio()
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from PySide6.QtCore import Qt, QPoint
from PySide6.QtGui import QImage, QColor
from PySide6.QtWidgets import QApplication, QPushButton, QLabel
from PySide6.QtTest import QTest
from emuluna.app import Window
from emuluna.library import Library, SYSTEMS
from emuluna.core import CoreError
from emuluna.player import Player
from core_fixture import install_core
from snes_rom import snes
from roms import gameboy


class LibraryMedia(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.lib = Library(self.root / 'library')
        self.ids = []
        for name, data in [('Game.sfc', snes()), ('Handheld.gb', gameboy())]:
            path = self.root / name
            path.write_bytes(data)
            self.ids.extend(self.lib.import_file(path))
        self.window = self.player = None

    def tearDown(self):
        if self.player and not self.player.closed: self.player.close()
        if self.window: self.window.close()
        else: self.lib.close()
        self.tmp.cleanup()

    def open_window(self):
        self.window = Window(self.lib, auto_artwork=False)
        self.window.show()
        QTest.qWait(30)
        return self.window

    def player_for(self, state_file=None):
        with patch.object(Player, 'setup_audio'):
            self.player = Player(self.lib, self.ids[0], muted=True, state_file=state_file)
        self.player.timer.stop()
        self.player.focus_paused = False
        return self.player

    def test_centered_navigation_and_protected_collections(self):
        window = self.open_window()
        for size in [(1140, 750), (850, 560)]:
            window.resize(*size)
            QTest.qWait(20)
            self.assertIs(window.library_tabs.parentWidget(), window.library_toolbar)
            center = window.library_tabs.geometry().center().x()
            self.assertLessEqual(abs(center - window.library_toolbar.rect().center().x()), 1)
            self.assertLess(window.view_controls.geometry().right(), window.library_tabs.geometry().left())
            search_x = window.search_control.mapTo(window.library_toolbar, QPoint()).x()
            self.assertGreater(search_x, window.library_tabs.geometry().right())
            self.assertLessEqual(window.notifications.mapTo(window.library_toolbar, QPoint()).x() + window.notifications.width(), window.library_toolbar.width())
            for key in ('states', 'screenshots', 'library'):
                QTest.mouseClick(window.section_buttons[key], Qt.LeftButton)
                self.assertEqual(window.library_tab, key)
                self.assertTrue(window.section_buttons[key].isChecked())
                self.assertFalse(window.section_buttons[key].icon().isNull())
        sidebar = window.findChild(QLabel, 'brand')
        self.assertIsNone(sidebar)
        self.assertNotIn('Never played', [window.nav.item(i).text() for i in range(window.nav.count())])
        for key in ('all', 'recent', 'favorites'):
            item = next(window.nav.item(i) for i in range(window.nav.count()) if window.nav.item(i).data(Qt.UserRole) == key)
            self.assertEqual(item.data(Qt.UserRole + 1), 'builtin')
            with patch('emuluna.app.QMenu') as menu:
                window.collection_menu(window.nav.visualItemRect(item).center())
                menu.assert_not_called()
        self.assertFalse(any('Import games' in button.text() or 'New collection' in button.text() or 'test ROM' in button.text()
                             for button in window.findChildren(QPushButton)))
        self.assertTrue(window.import_action.isEnabled())

    def test_media_grouped_by_console_filtered_and_preserves_game_selection(self):
        for game_id in self.ids:
            folder = self.lib.root / 'states' / game_id / 'libretro' / 'fixture' / ('a' * 64)
            folder.mkdir(parents=True)
            (folder / 'slot-1.oesavestate').write_bytes(b'fixture, never loaded')
        image = QImage(32, 24, QImage.Format_RGB32)
        image.fill(QColor('red'))
        image.save(str(self.lib.root / 'screenshots' / (self.ids[0][:12] + '-20260913-120000.png')))
        window = self.open_window()
        window.restore_selection(self.ids[:1])
        window.change_library_tab('states')
        browser = window.media_browser
        self.assertEqual(browser.entry_count, 2)
        headings = [browser.grid.item(i).text() for i in range(browser.grid.count())
                    if browser.grid.item(i).data(Qt.UserRole + 1) == 'console']
        self.assertEqual(headings, sorted((SYSTEMS['gb'].name, SYSTEMS['snes'].name)))
        browser.select_path(next(iter(browser.entries)))
        chosen = browser.selected_entry()['path']
        window.change_view('list')
        self.assertIs(browser.pages.currentWidget(), browser.state_tree)
        self.assertEqual(browser.state_tree.topLevelItemCount(), 2)
        self.assertEqual(browser.selected_entry()['path'], chosen)
        window.search.setText('Handheld')
        self.assertEqual(browser.entry_count, 1)
        window.search.clear()
        self.assertEqual(browser.entry_count, 2)
        window.change_library_tab('screenshots')
        self.assertEqual(browser.entry_count, 1)
        browser.select_path(next(iter(browser.entries)))
        with patch('emuluna.app.QDesktopServices.openUrl', return_value=True) as opened:
            window.activate_media()
            self.assertTrue(opened.called)
        window.change_library_tab('library')
        self.assertEqual(window.selected_ids(), self.ids[:1])

    def test_battery_timer_does_not_create_state_and_close_saves_silently(self):
        install_core(self.lib)
        player = self.player_for()
        for _ in range(120): player.core.frame()
        automatic = player.state_path('auto.oesavestate')
        with patch.object(player.core, 'save_state', wraps=player.core.save_state) as save:
            player.battery_timer.timeout.emit()
            save.assert_not_called()
            self.assertFalse(automatic.exists())
            self.assertFalse(player.notice.isVisible())
            player.close()
            save.assert_called_once_with(automatic)
        self.assertTrue(automatic.is_file())

    def test_state_screenshots_and_corner_cover(self):
        install_core(self.lib)
        player = self.player_for()
        for _ in range(3):
            player.next_frame = 0
            player.tick()
        self.assertFalse(player.screen.frame.isNull())
        player.save()
        state = player.state_path()
        preview = state.with_suffix('.png')
        self.assertEqual(QImage(str(preview)), player.screen.frame)
        before = preview.read_bytes()
        with patch.object(player.core, 'save_state', side_effect=CoreError('fixture failure')):
            with self.assertRaises(CoreError):
                player.save_state_with_preview(state)
        self.assertEqual(preview.read_bytes(), before)
        player.close()
        self.assertFalse(QImage(str(player.state_path('auto.oesavestate').with_suffix('.png'))).isNull())
        # Contrasting fixtures make the screenshot and cover positions unambiguous.
        screen = QImage(256, 192, QImage.Format_RGB32); screen.fill(QColor('blue'))
        screen.save(str(preview))
        cover = QImage(64, 90, QImage.Format_RGB32); cover.fill(QColor('red'))
        cover.save(str(self.lib.root / 'covers' / 'test.png'))
        with self.lib.db:
            self.lib.db.execute('UPDATE games SET cover=? WHERE id=?', ('covers/test.png', self.ids[0]))
        window = self.open_window()
        window.change_library_tab('states')
        browser = window.media_browser
        key = str(state.resolve())
        browser.preview_icon(key)
        for _ in range(50):
            QTest.qWait(10)
            if not window.thumbnails.pending: break
        image = browser.preview_icon(key).pixmap(256,192).toImage()
        self.assertEqual(image.pixelColor(100, 90), QColor('blue'))
        self.assertEqual(image.pixelColor(230, 160), QColor('red'))
        preview.unlink()
        browser.refresh('states', self.lib.games())
        self.assertEqual(browser.entries[key]['preview'], '')
        self.assertFalse(browser.preview_icon(key).isNull())

    def test_state_library_load_uses_matching_build_and_player_restores(self):
        installed = install_core(self.lib)
        player = self.player_for()
        for _ in range(120): player.core.frame()
        player.save()
        path = player.state_path()
        player.close()
        window = self.open_window()
        window.change_library_tab('states')
        window.media_browser.select_path(str(path))
        with patch('emuluna.app.QProcess') as process:
            window.activate_media()
            args = process.return_value.setArguments.call_args.args[0]
            self.assertEqual(args[args.index('--state-file') + 1], str(path))
        window.processes.clear()
        player = self.player_for(path)
        for _ in range(3):
            player.next_frame = 0
            player.tick()
        self.assertIsNone(player.pending_state)
        self.assertFalse(player.state_restore_failed)
        self.assertEqual(player.notice.text(), 'State loaded')
        player.close()
        with patch.object(window.notifications, 'post') as warning, patch('emuluna.app.QProcess') as process:
            other = path.parent.parent / ('b' * 64) / path.name
            window.launch_game(self.ids[0], other)
            warning.assert_called_once()
            process.assert_not_called()
        with self.assertRaisesRegex(CoreError, 'different core build'):
            self.player_for(other)
