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
from PySide6.QtTest import QTest
from emuluna.app import Window, cover_pixmap
from emuluna.library import Library, SYSTEMS
from emuluna.thumbnails import ThumbnailCache


class CoverShapes(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_every_system_grid_keeps_art_size_left_edge_and_even_gaps(self):
        """Exercise real Window layouts with regional shapes and unavailable art."""
        from PySide6.QtCore import QPoint
        with tempfile.TemporaryDirectory() as folder:
            library = Library(folder)
            library.needs_filename_restore = lambda: False
            # Most boxes have the console's usual shape, alongside regional
            # variations, square/portrait cases, and missing or unreadable art.
            for key, system in SYSTEMS.items():
                width, height = system.cover_size
                shapes = [(width, height), (int(width * .96), height),
                          (int(width * 1.04), height), (256, 256),
                          (150, 256), (512, 180)]
                paths = []
                for index, shape in enumerate(shapes):
                    path = f'covers/{key}-{index}.png'
                    image = QImage(*shape, QImage.Format_RGB32)
                    image.fill(QColor.fromHsv(index * 50, 160, 200))
                    self.assertTrue(image.save(str(library.root / path)))
                    paths.append(path)
                broken = f'covers/{key}-broken.png'
                (library.root / broken).write_bytes(b'not an image')
                covers = [paths[0]] * 8 + paths[1:] + [None, f'covers/{key}-missing.png', broken]
                with library.db:
                    library.db.executemany(
                        'INSERT INTO games(id,title,system,rom_path,source,added,cover,original_filename) VALUES(?,?,?,?,?,?,?,?)',
                        [(f'{key}-{i}', f'Game {i:02}: A longer example title', key,
                          f'roms/{key}-{i}.rom', '', 0, cover, f'{key}-{i}.rom')
                         for i, cover in enumerate(covers)])
            window = Window(library, auto_artwork=False)
            try:
                window.show()
                shared_left = None
                for key in (*SYSTEMS, 'all'):
                    window.nav.setCurrentRow(next(i for i in range(window.nav.count())
                        if window.nav.item(i).data(Qt.UserRole) == key))
                    original = None
                    for width in (850, 1040, 1440, 2048, 850):
                        with self.subTest(system=key, window_width=width):
                            window.resize(width, 750)
                            QTest.qWait(60)
                            grid = window.games
                            sizes = dict(grid.display_dimensions)
                            self.assertEqual(len(sizes), grid.count())
                            self.assertEqual(sizes, original or sizes)
                            original = sizes
                            rows = {}
                            for i in range(grid.count()):
                                item = grid.item(i)
                                card = grid.visualItemRect(item)
                                art = grid.itemDelegate().art_rect(card, item.icon(), item.data(Qt.UserRole))
                                rows.setdefault(card.top(), []).append(art)
                                self.assertGreater(art.width(), 0)
                                self.assertGreater(art.height(), 0)
                                self.assertLessEqual(max(art.width(), art.height()), 256)
                                source = grid.cover_dimensions[item.data(Qt.UserRole)]
                                self.assertLessEqual(
                                    abs(art.width() * source.height() - art.height() * source.width()),
                                    max(source.width(), source.height()))
                                self.assertGreaterEqual(art.left(), 0)
                                self.assertLess(art.right(), grid.viewport().width())
                                self.assertTrue(card.contains(art), (card, art))
                                if grid.viewport().rect().contains(art.center()):
                                    self.assertIs(grid.itemAt(art.center()), item)
                            for arts in rows.values():
                                self.assertEqual(arts[0].left(), grid.cover_inset)
                                edge = grid.viewport().mapTo(window, QPoint(arts[0].left(), 0)).x()
                                shared_left = edge if shared_left is None else shared_left
                                self.assertEqual(edge, shared_left)
                                gaps = [b.left() - a.right() - 1 for a, b in zip(arts, arts[1:])]
                                if gaps:
                                    self.assertEqual(min(gaps), max(gaps))
                                    self.assertGreaterEqual(min(gaps), grid.minimum_gap)
                                    self.assertLessEqual(max(gaps), grid.maximum_gap)
                            self.assertEqual(grid.horizontalScrollBar().maximum(), 0)
                    if key != 'all':
                        window.search.setText('Game 00:')
                        for width in (850, 2048):
                            with self.subTest(system=key, single_cover_width=width):
                                window.resize(width, 750)
                                QTest.qWait(60)
                                grid = window.games
                                self.assertEqual(grid.count(), 1)
                                item = grid.item(0)
                                art = grid.itemDelegate().art_rect(grid.visualItemRect(item), item.icon(), item.data(Qt.UserRole))
                                self.assertEqual(art.left(), grid.cover_inset)
                                self.assertEqual(grid.viewport().mapTo(window, art.topLeft()).x(), shared_left)
                        window.search.setText('No matching game')
                        QTest.qWait(20)
                        self.assertEqual(grid.count(), 0)
                        self.assertFalse(grid.actions_bar.isVisible())
                        window.search.clear()
            finally:
                window.close()

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
