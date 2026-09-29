"""Settings must open without hardware/file scans or accumulating hidden dialogs."""
import os
import tempfile
import time
import unittest
from unittest.mock import patch

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import QCoreApplication, QEvent, QTimer
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from emuluna.app import Window
from emuluna.library import Library
from emuluna.settings import SettingsDialog


class SettingsPerformance(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_initial_open_does_not_build_hidden_pages_or_scan_devices_and_firmware(self):
        with tempfile.TemporaryDirectory() as folder:
            library = Library(folder)
            with patch('emuluna.settings.QMediaDevices') as audio, \
                 patch('emuluna.settings.bios_status') as firmware, \
                 patch('emuluna.settings.CoreManager.installed') as cores, \
                 patch('emuluna.controller_settings.Gamepad.devices') as controllers:
                dialog = SettingsDialog(library)
                try:
                    dialog.show()
                    QTest.qWait(25)
                    for scan in (audio, firmware, cores, controllers):
                        scan.assert_not_called()
                    dialog.show_page('library')
                    dialog.copy_games.setChecked(False)
                    self.assertEqual(library.setting('copy_games'), '0')
                    self.assertTrue(dialog.startup_art.isChecked())
                    dialog.startup_art.setChecked(False)
                    self.assertEqual(library.setting('artwork_check_at_startup'), '0')
                    self.assertFalse(dialog.closest_art.isChecked())
                    dialog.closest_art.setChecked(True)
                    self.assertEqual(library.setting('artwork_closest_match'), '1')
                    dialog.auto_art.setChecked(False)
                    self.assertFalse(dialog.startup_art.isEnabled())
                    self.assertFalse(dialog.closest_art.isEnabled())
                    dialog.show_page('controls')
                    first = dialog.controls_page
                    dialog.show_page('general')
                    dialog.show_page('controls')
                    self.assertIs(dialog.controls_page, first)
                    for scan in (audio, firmware, cores, controllers):
                        scan.assert_not_called()
                finally:
                    dialog.close()
                    dialog.deleteLater()
                    QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)
                    library.close()

    def test_reopening_with_large_library_preserves_library_style_and_releases_dialogs(self):
        with tempfile.TemporaryDirectory() as folder:
            library = Library(folder)
            library.set_setting('appearance.use_system_theme', '0')
            with library.db:
                library.db.executemany(
                    'INSERT INTO games(id,title,system,rom_path,source,added,original_filename) VALUES(?,?,?,?,?,?,?)',
                    [(str(i), f'Game {i:04}', 'snes', f'roms/{i}.sfc', '', time.time(), f'{i}.sfc')
                     for i in range(680)])
            window = Window(library, auto_artwork=False)
            try:
                window.show()
                QTest.qWait(60)
                opened = []
                def close_settings():
                    for dialog in window.findChildren(SettingsDialog):
                        if dialog.isVisible():
                            opened.append(dialog.windowTitle())
                            dialog.accept()
                with patch.object(window, 'setStyleSheet', wraps=window.setStyleSheet) as restyle:
                    for _ in range(5):
                        QTimer.singleShot(0, close_settings)
                        window.open_settings()
                        QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)
                        QTest.qWait(10)
                        self.assertEqual(window.findChildren(SettingsDialog), [])
                    self.assertEqual(len(opened), 5)
                    restyle.assert_not_called()
            finally:
                window.close()

    def test_settings_remembers_geometry_across_restarts_and_close_methods(self):
        with tempfile.TemporaryDirectory() as folder:
            for close_method in ('accept', 'reject', 'close'):
                library = Library(folder)
                dialog = SettingsDialog(library)
                dialog.show()
                dialog.resize(770, 575)
                dialog.move(10, 25)
                QTest.qWait(10)
                size, position = dialog.size(), dialog.pos()
                getattr(dialog, close_method)()
                self.assertTrue(library.setting('window.settings.geometry'))
                dialog.deleteLater()
                QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)
                library.close()

                reopened_library = Library(folder)
                reopened = SettingsDialog(reopened_library)
                reopened.show()
                QTest.qWait(10)
                self.assertEqual(reopened.size(), size, close_method)
                self.assertEqual(reopened.pos(), position, close_method)
                reopened.close()
                reopened.deleteLater()
                QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)
                reopened_library.close()


if __name__ == '__main__':
    unittest.main()
