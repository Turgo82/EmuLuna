"""Console-shaped missing art and stable proportions during background loading."""
import os
from pathlib import Path
import tempfile
import unittest
from media_stub import isolate_audio
isolate_audio()
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from PySide6.QtWidgets import QApplication
from PySide6.QtGui import QImage, QColor
from PySide6.QtCore import QSize, Qt
from emuluna.app import Window, cover_pixmap
from emuluna.library import Library, SYSTEMS
from emuluna.thumbnails import ThumbnailCache


class CoverShapes(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_missing_and_unreadable_art_use_console_proportions(self):
        with tempfile.TemporaryDirectory() as folder:
            library=Library(folder)
            cache=ThumbnailCache(library.root)
            try:
                for key in SYSTEMS:
                    game=dict(id=key, system=key, title='An example game', cover=None, cover_revision=0)
                    expected=QSize(*SYSTEMS[key].cover_size)
                    self.assertEqual(cache.dimensions(game), expected)
                    self.assertEqual(cover_pixmap(game,library).size(), expected)
                    game['cover']='covers/missing.png'
                    self.assertEqual(cache.dimensions(game), expected)
                    self.assertEqual(cover_pixmap(game,library).size(), expected)
                self.assertGreater(SYSTEMS['snes'].cover_size[0], SYSTEMS['snes'].cover_size[1])
                self.assertLess(SYSTEMS['nes'].cover_size[0], SYSTEMS['nes'].cover_size[1])
                self.assertEqual(SYSTEMS['gb'].cover_size[0], SYSTEMS['gb'].cover_size[1])
            finally:
                cache.close();library.close()

    def test_loading_placeholder_matches_actual_art_and_refreshes_after_replacement(self):
        with tempfile.TemporaryDirectory() as folder:
            library=Library(folder)
            image=QImage(100,200,QImage.Format_RGB32);image.fill(QColor('red'))
            image.save(str(library.root/'covers'/'test.png'))
            with library.db:
                library.db.execute('INSERT INTO games(id,title,system,rom_path,source,added,cover,original_filename) VALUES(?,?,?,?,?,?,?,?)',
                                   ('test','Example','snes','roms/Example.sfc','',0,'covers/test.png','Example.sfc'))
            window=Window(library,auto_artwork=False)
            try:
                # Request before processing worker callbacks, while still showing loading art.
                first=window.cover_icon('test')
                self.assertEqual(window.games.cover_dimensions['test'], QSize(100,200))
                self.assertEqual(first.pixmap(256,256).size(), QSize(128,256))
                image=QImage(300,200,QImage.Format_RGB32);image.fill(QColor('blue'))
                image.save(str(library.root/'covers'/'test.png'))
                library.set_manual_cover('test','covers/test.png')
                window.refresh()
                changed=window.cover_icon('test')
                self.assertEqual(changed.pixmap(256,256).size(), QSize(256,170))
                self.assertEqual(window.games.cover_dimensions['test'], QSize(300,200))
                with library.db:
                    library.db.execute("UPDATE games SET cover=NULL,cover_revision=cover_revision+1 WHERE id='test'")
                window.refresh()
                missing=window.cover_icon('test')
                self.assertEqual(missing.pixmap(256,256).size(), QSize(*SYSTEMS['snes'].cover_size))
            finally:
                window.close()
