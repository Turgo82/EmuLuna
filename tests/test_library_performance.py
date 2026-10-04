"""Large-library regressions: visible-only image work, cache bounds and lazy lists."""
import os
import hashlib
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
                # A valid preview from the previous release must not mask the
                # higher-quality tier when reopening an existing library.
                old_name = hashlib.sha256(game['id'].encode()).hexdigest()[:32] + '-0.png'
                old_preview = cache.preview_directory / old_name
                old_preview.parent.mkdir(parents=True)
                image.scaled(28, 40).save(str(old_preview))
                cache.warm_previews([game])
                cache.warm_pool.waitForDone()
                self.assertTrue(cache.preview_path(game).is_file())
                saved = QImage(str(cache.preview_path(game)))
                self.assertEqual(saved.height(), 96)
                self.assertLessEqual(abs(saved.width() / saved.height() - .7), .02)
            finally:
                cache.close()
            reopened = ThumbnailCache(root)
            try:
                preview = reopened.icon(game)
                self.assertIsNotNone(preview)
                self.assertFalse(preview.isNull())
                self.assertEqual(preview.pixmap(200, 200).size().height(), 96)
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
                         if 0 <= window.games.visualItemRect(window.games.item(i)).top() - viewport.bottom() < window.games.cover_height + 82]
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

    def test_landscape_cards_use_compact_rows_without_size_slider(self):
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
                self.assertFalse(hasattr(window, 'cover_size'))
                self.assertFalse(hasattr(window, 'heading'))
                first = window.games.item(0)
                rect = window.games.visualItemRect(first)
                art = window.games.itemDelegate().art_rect(rect, first.icon(), '0')
                self.assertLess(rect.height(), 380)
                self.assertGreater(rect.bottom() - art.bottom(), 40)  # Title, empty stars and breathing room.
                self.assertLess(art.top(), 16)
            finally:
                window.close()

    def test_fixed_cover_size_and_balanced_horizontal_gaps_on_resize(self):
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
                left_edges = {}
                for section in ('all', 'snes', 'genesis'):
                    if section != 'all':
                        window.nav.setCurrentRow(next(i for i in range(window.nav.count())
                            if window.nav.item(i).data(Qt.UserRole) == section))
                    first_dimensions = None
                    for size in (850, 1140, 1450):
                        window.resize(size, 750)
                        for _ in range(5):
                            QTest.qWait(20)
                        pairs = 0
                        row_gaps = {}
                        dimensions = dict(window.games.display_dimensions)
                        if first_dimensions is None:
                            first_dimensions = dimensions
                        self.assertEqual(dimensions, first_dimensions, (section, size))
                        for item_id, value in dimensions.items():
                            self.assertLessEqual(max(value.width(), value.height()), 256)
                            self.assertEqual(value.height(), 180 if int(item_id) % 2 == 0 else 256)
                        rects = [window.games.visualItemRect(window.games.item(i))
                                 for i in range(window.games.count())]
                        delegate = window.games.itemDelegate()
                        row_starts = {}
                        for i, rect in enumerate(rects):
                            item = window.games.item(i)
                            art = delegate.art_rect(rect, item.icon(), item.data(Qt.UserRole))
                            if rect.top() not in row_starts:
                                row_starts[rect.top()] = art.left()
                                self.assertEqual(art.left(), window.games.cover_inset, (section, size, i))
                        first_art = delegate.art_rect(rects[0], window.games.item(0).icon(), window.games.item(0).data(Qt.UserRole))
                        from PySide6.QtCore import QPoint
                        edge = window.games.viewport().mapTo(window, QPoint(first_art.left(), 0)).x()
                        self.assertEqual(edge, left_edges.setdefault(size, edge), (section, size))
                        for i in range(window.games.count() - 1):
                            first, second = window.games.item(i), window.games.item(i + 1)
                            left = window.games.visualItemRect(first)
                            right = window.games.visualItemRect(second)
                            if left.top() != right.top():
                                continue
                            delegate = window.games.itemDelegate()
                            a = delegate.art_rect(left, first.icon(), first.data(Qt.UserRole))
                            b = delegate.art_rect(right, second.icon(), second.data(Qt.UserRole))
                            if first.data(Qt.UserRole + 3) == second.data(Qt.UserRole + 3):
                                self.assertEqual(a.height(), b.height(), (section, size))
                            gap = b.left() - a.right() - 1
                            self.assertGreaterEqual(gap, 32, (section, size))
                            self.assertLessEqual(gap, 64, (section, size))
                            previous = row_gaps.setdefault(left.top(), gap)
                            self.assertLessEqual(abs(gap - previous), 1, (section, size))
                            pairs += 1
                        if len(row_starts) < len(rects):
                            self.assertGreater(pairs, 0, (section, size))
            finally:
                window.close()

    def test_saturn_mixed_cover_shapes_have_equal_gaps_and_partial_row_alignment(self):
        from PySide6.QtCore import Qt, QSize
        from PySide6.QtGui import QIcon, QPixmap
        from PySide6.QtWidgets import QListWidgetItem, QListWidget
        from emuluna.library_widgets import GameGrid, CoverDelegate
        grid = GameGrid()
        grid.setViewMode(QListWidget.IconMode)
        grid.setResizeMode(QListWidget.Adjust)
        grid.setMovement(QListWidget.Static)
        grid.setItemDelegate(CoverDelegate(grid))
        original = None
        try:
            for i, width in enumerate((150, 150, 240, 150, 230, 150, 150, 150)):
                item = QListWidgetItem('Example', grid)
                pixmap = QPixmap(16, 16)
                pixmap.fill(QColor('red'))
                item.setIcon(QIcon(pixmap))
                item.setData(Qt.UserRole, str(i))
                item.setData(Qt.UserRole + 1, 'Example')
                item.setData(Qt.UserRole + 2, 0)
                item.setData(Qt.UserRole + 3, 'saturn')
                grid.cover_dimensions[str(i)] = QSize(width, 256)
            grid.show()
            for width in (700, 1200, 1600):
                grid.resize(width, 720)
                QTest.qWait(80)
                dimensions = dict(grid.display_dimensions)
                self.assertEqual(dimensions, original or dimensions)
                original = dimensions
                rows = {}
                for i in range(grid.count()):
                    item = grid.item(i)
                    rect = grid.visualItemRect(item)
                    art = grid.itemDelegate().art_rect(rect, item.icon(), str(i))
                    rows.setdefault(rect.top(), []).append(art)
                for arts in rows.values():
                    self.assertEqual(arts[0].left(), 28)
                    gaps = [b.left() - a.right() - 1 for a, b in zip(arts, arts[1:])]
                    if gaps:
                        self.assertEqual(min(gaps), max(gaps))
                        self.assertGreaterEqual(min(gaps), 32)
                        self.assertLessEqual(max(gaps), 64)
        finally:
            grid.close()
