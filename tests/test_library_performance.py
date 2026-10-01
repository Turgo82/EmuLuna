"""Large-library regressions: visible-only image work, cache bounds and lazy lists."""
import os
from pathlib import Path
import tempfile
import time
import unittest
from media_stub import isolate_audio
isolate_audio()
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from PySide6.QtGui import QImage, QColor
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from emuluna.app import Window
from emuluna.library import Library
from emuluna.thumbnails import ThumbnailCache


class LibraryPerformance(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_persistent_cover_preview_and_explicit_rebuild(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'covers').mkdir()
            source = root / 'covers' / 'fixture.png'
            image = QImage(700, 1000, QImage.Format_RGB32)
            image.fill(QColor('red'))
            image.save(str(source))
            game = {'id': 'preview-test', 'system': 'snes', 'cover': 'covers/fixture.png',
                    'cover_revision': 0}
            cache = ThumbnailCache(root)
            try:
                cache.warm_previews([game])
                cache.warm_pool.waitForDone()
                self.assertTrue(cache.preview_path(game).is_file())
            finally:
                cache.close()
            reopened = ThumbnailCache(root)
            try:
                preview = reopened.icon(game)
                self.assertIsNotNone(preview)
                self.assertFalse(preview.isNull())
                self.assertEqual(preview.pixmap(200, 200).size().height(), 200)
                image.fill(QColor('blue'))
                image.save(str(source))
                reopened.rebuild()
                self.assertFalse(reopened.preview_path(game).exists())
                reopened.warm_previews([game])
                reopened.warm_pool.waitForDone()
                self.assertTrue(reopened.preview_path(game).is_file())
                rebuilt = reopened.icon(game)
                self.assertEqual(rebuilt.pixmap(200, 200).toImage().pixelColor(50, 50).name(),
                                 '#0000ff')
            finally:
                reopened.close()

    def test_visible_thumbnails_cache_replacement_and_lazy_table(self):
        with tempfile.TemporaryDirectory() as folder:
            library = Library(folder)
            image = QImage(700, 1000, QImage.Format_RGB32)
            image.fill(QColor('red'))
            cover = library.root / 'covers' / 'fixture.png'
            image.save(str(cover))
            with library.db:
                library.db.executemany('INSERT INTO games(id,title,system,rom_path,source,added,cover,original_filename) VALUES(?,?,?,?,?,?,?,?)',
                    [(f'{i:064x}', f'Game {i:04}', 'snes', f'roms/Game{i}.sfc', '', time.time(), 'covers/fixture.png', f'Game{i}.sfc') for i in range(680)])
            window = Window(library, auto_artwork=False)
            try:
                self.assertEqual(window.thumbnails.decoded_count, 0)
                self.assertEqual(window.table.rowCount(), 0)
                window.show()
                def settle():
                    deadline = time.monotonic() + 5
                    stable = 0
                    while time.monotonic() < deadline:
                        QTest.qWait(10)
                        stable = stable + 1 if not window.thumbnails.pending else 0
                        if stable >= 3: return
                    self.fail('Thumbnail loading did not settle')
                settle()
                first_count = window.thumbnails.decoded_count
                self.assertGreater(first_count, 0)
                self.assertLess(first_count, 70)  # Visible covers plus nearby prefetch rows.
                window.refresh()
                settle()
                self.assertEqual(window.thumbnails.decoded_count, first_count)
                # The immediately following row is already decoded before scrolling.
                viewport = window.games.viewport().rect()
                below = [window.games.item(i) for i in range(window.games.count())
                         if 0 <= window.games.visualItemRect(window.games.item(i)).top() - viewport.bottom() < window.cover_size.value() + 66]
                self.assertTrue(below)
                for item in below:
                    key = item.data(256)
                    self.assertTrue(any(entry[0] == key for entry in window.thumbnails.cache))
                window.games.scrollToBottom()
                settle()
                self.assertGreater(window.thumbnails.decoded_count, first_count)
                self.assertLessEqual(len(window.thumbnails.cache), 256)
                # The cache key includes the artwork revision, including same-path replacements.
                game = library.get(f'{679:064x}')
                first = window.thumbnails.icon(game)
                self.assertEqual(first.pixmap(20, 20).toImage().pixelColor(10, 10).name(), '#ff0000')
                image.fill(QColor('blue'))
                image.save(str(cover))
                library.set_manual_cover(game['id'], 'covers/fixture.png')
                window.refresh()
                settle()
                changed = window.thumbnails.icon(library.get(game['id']))
                self.assertEqual(changed.pixmap(20, 20).toImage().pixelColor(10, 10).name(), '#0000ff')
                window.restore_selection([game['id']])
                window.change_view('list')
                self.assertEqual(window.table.rowCount(), 680)
                self.assertEqual(window.selected_ids(), [game['id']])
                self.assertEqual(window.table.item(0, 0).text(), 'Game 0000')
            finally:
                window.close()

    def test_landscape_cards_use_compact_rows_and_slider_resets(self):
        from PySide6.QtCore import Qt
        with tempfile.TemporaryDirectory() as folder:
            library = Library(folder)
            image = QImage(600, 400, QImage.Format_RGB32); image.fill(QColor('red'))
            image.save(str(library.root / 'covers' / 'wide.png'))
            with library.db:
                library.db.executemany('INSERT INTO games(id,title,system,rom_path,source,added,cover,original_filename) VALUES(?,?,?,?,?,?,?,?)',
                    [(str(i), 'Example', 'snes', f'roms/{i}.sfc', '', time.time(), 'covers/wide.png', f'{i}.sfc') for i in range(30)])
            window = Window(library, auto_artwork=False)
            try:
                window.show(); QTest.qWait(50)
                slider = window.cover_size
                slider.setValue(172)
                QTest.mouseRelease(slider, Qt.LeftButton)
                self.assertEqual(slider.value(), 172)
                slider.setValue(256)
                QTest.mouseDClick(slider, Qt.LeftButton)
                self.assertEqual(slider.value(), 176)
                self.assertFalse(hasattr(window, 'heading'))
                first = window.games.item(0)
                rect = window.games.visualItemRect(first)
                art = window.games.itemDelegate().art_rect(rect, first.icon(), '0')
                self.assertLess(rect.height(), 200)
                self.assertGreater(rect.bottom() - art.bottom(), 40)  # Title, empty stars and breathing room.
                self.assertLess(art.top(), 16)
            finally:
                window.close()

    def test_cover_gaps_match_for_landscape_portrait_and_mixed_libraries(self):
        from PySide6.QtCore import Qt
        with tempfile.TemporaryDirectory() as folder:
            library = Library(folder)
            library.needs_filename_restore = lambda: False
            with library.db:
                for i in range(24):
                    system = 'snes' if i % 2 == 0 else 'genesis'
                    library.db.execute('INSERT INTO games(id,title,system,rom_path,source,added,original_filename) VALUES(?,?,?,?,?,?,?)',
                        (str(i), f'Game {i:02}', system, f'roms/{i}.rom', '', time.time(), f'{i}.rom'))
            window = Window(library, auto_artwork=False)
            try:
                window.show()
                for section in ('all', 'snes', 'genesis'):
                    if section != 'all':
                        window.nav.setCurrentRow(next(i for i in range(window.nav.count())
                            if window.nav.item(i).data(Qt.UserRole) == section))
                    for size in (96, 176, 256):
                        window.cover_size.setValue(size)
                        QTest.qWait(30)
                        pairs = 0
                        for i in range(window.games.count() - 1):
                            first, second = window.games.item(i), window.games.item(i + 1)
                            left = window.games.visualItemRect(first)
                            right = window.games.visualItemRect(second)
                            if left.top() != right.top():
                                continue
                            delegate = window.games.itemDelegate()
                            a = delegate.art_rect(left, first.icon(), first.data(Qt.UserRole))
                            b = delegate.art_rect(right, second.icon(), second.data(Qt.UserRole))
                            self.assertEqual(b.left() - a.right() - 1, 40, (section, size))
                            pairs += 1
                        self.assertGreater(pairs, 0)
            finally:
                window.close()
