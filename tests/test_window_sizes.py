"""Window sizes survive reopen without storing temporary fullscreen/minimization."""
import json
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
import tempfile
from pathlib import Path
import unittest
from PySide6.QtCore import QSize
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication, QMainWindow
from emuluna.desktop import save_window_size, restore_window_size
from emuluna.app import Window
from emuluna.library import Library


class WindowSizes(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_library_remembers_size_and_maximized_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            library = Library(root)
            window = Window(library, auto_artwork=False)
            window.resize(1040, 680)
            window.show(); QTest.qWait(250)
            window.showMaximized(); QTest.qWait(250)
            window.close()
            library = Library(root)
            reopened = Window(library, auto_artwork=False)
            try:
                reopened.show(); QTest.qWait(250)
                self.assertTrue(reopened.isMaximized())
                reopened.showNormal(); QTest.qWait(250)
                self.assertEqual(reopened.size(), QSize(1040, 680))
            finally:
                reopened.close()

    def test_fullscreen_saves_normal_size(self):
        with tempfile.TemporaryDirectory() as tmp:
            library = Library(Path(tmp))
            window = QMainWindow()
            window.resize(640, 460)
            window.show(); QTest.qWait(10)
            window.showFullScreen(); QTest.qWait(10)
            save_window_size(window, library, 'window.game.psx.size')
            saved = json.loads(library.setting('window.game.psx.size'))
            self.assertEqual((saved['width'], saved['height']), (640, 460))
            self.assertFalse(saved['maximized'])
            window.close(); library.close()

    def test_corrupt_size_keeps_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            library = Library(Path(tmp))
            window = QMainWindow()
            window.resize(640, 460)
            for saved in ('invalid', '[]', '{"width": -20, "height": 500}', '{"width": 1000000, "height": 500}'):
                library.set_setting('window.library.size', saved)
                restore_window_size(window, library, 'window.library.size')
                self.assertEqual(window.size(), QSize(640, 460))
            window.close(); library.close()
