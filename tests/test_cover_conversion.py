"""Lossless artwork migration, partial failures and concurrent cover changes."""
import os
from pathlib import Path
from tempfile import TemporaryDirectory
import time
import unittest
from unittest.mock import patch
from media_stub import isolate_audio
isolate_audio()
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from PySide6.QtGui import QImage, QColor
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from emuluna.app import Window
from emuluna.artwork import CoverConversionWorker
from emuluna.library import Library
from emuluna.settings import SettingsDialog


class CoverConversionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = TemporaryDirectory()
        self.lib = Library(Path(self.tmp.name) / 'library')
        self.lib.needs_filename_restore = lambda: False

    def tearDown(self):
        self.lib.close()
        self.tmp.cleanup()

    def image(self, path, size=(240, 320)):
        image = QImage(*size, QImage.Format_RGBA8888)
        image.fill(QColor(27, 128, 209, 210))
        image.setPixelColor(5, 5, QColor(176, 3, 245, 112))
        image.save(str(path))
        return image

    def game(self, game_id, cover):
        with self.lib.db:
            self.lib.db.execute('INSERT INTO games(id,title,system,rom_path,source,added,cover,original_filename) VALUES(?,?,?,?,?,?,?,?)',
                (game_id, game_id, 'genesis', f'roms/{game_id}.md', '', 0, str(cover), f'{game_id}.md'))

    def run_worker(self):
        worker = CoverConversionWorker(self.lib.root)
        results = []
        worker.result.connect(results.append)
        worker.run()
        return results[-1]

    def test_shared_cover_preserves_resolution_alpha_and_metadata(self):
        self.lib.set_setting('artwork.webp_quality', '100')
        source = self.lib.root / 'covers/shared.png'
        image = self.image(source, (1200, 1600))
        self.game('first', 'covers/shared.png')
        self.game('second', 'covers/shared.png')
        self.lib.artwork_result('first', 'downloaded', url='https://example.org/cover')
        with self.lib.db:
            self.lib.db.execute("INSERT INTO metadata(game_id,region,developer) VALUES('first','USA','Example')")
        result = self.run_worker()
        self.assertEqual(result['converted'], 2)
        self.assertEqual(result['failed'], 0)
        self.assertFalse(source.exists())
        for game_id in ('first', 'second'):
            row = self.lib.get(game_id)
            path = self.lib.root / row['cover']
            self.assertEqual(path.suffix, '.webp')
            converted = QImage(str(path))
            self.assertEqual(converted.size(), image.size())
            self.assertEqual(converted.pixelColor(5, 5), image.pixelColor(5, 5))
            self.assertEqual(converted.pixelColor(50, 50), image.pixelColor(50, 50))
            self.assertEqual(row['cover_revision'], 1)
        self.assertEqual(self.lib.artwork_info('first')['source_url'], 'https://example.org/cover')
        self.assertEqual(self.lib.db.execute("SELECT developer FROM metadata WHERE game_id='first'").fetchone()[0], 'Example')
        self.assertEqual(self.run_worker()['skipped'], 2)

    def test_external_original_and_broken_covers_are_kept(self):
        source = Path(self.tmp.name) / 'external.png'
        self.image(source)
        original = source.read_bytes()
        self.game('external', source)
        broken = self.lib.root / 'covers/broken.png'
        broken.write_bytes(b'not an image')
        self.game('broken', 'covers/broken.png')
        self.game('missing', 'covers/missing.png')
        result = self.run_worker()
        self.assertEqual(result['converted'], 1)
        self.assertEqual(result['failed'], 2)
        self.assertEqual(source.read_bytes(), original)
        self.assertEqual(broken.read_bytes(), b'not an image')
        self.assertEqual(self.lib.get('broken')['cover'], 'covers/broken.png')

    def test_existing_webp_recompressed_once_at_selected_quality(self):
        source = self.lib.root / 'covers/old.webp'
        self.image(source)
        self.game('webp', 'covers/old.webp')
        self.assertEqual(self.run_worker()['converted'], 1)
        row = self.lib.get('webp')
        self.assertTrue(row['cover'].endswith('.q85.webp'))
        self.assertEqual(QImage(str(self.lib.root / row['cover'])).width(), 240)
        self.assertFalse(source.exists())
        self.assertEqual(self.run_worker()['skipped'], 1)
        self.lib.set_setting('artwork.webp_quality', '75')
        self.assertEqual(self.run_worker()['converted'], 1)
        self.assertTrue(self.lib.get('webp')['cover'].endswith('.q75.webp'))
        self.assertEqual(self.run_worker()['skipped'], 1)

    def test_concurrent_manual_selection_wins(self):
        source = self.lib.root / 'covers/original.png'
        replacement = self.lib.root / 'covers/replacement.png'
        self.image(source)
        self.image(replacement)
        self.game('race', 'covers/original.png')
        setter = Library.set_downloaded_cover
        def choose_new(library, game_id, target, previous, revision):
            library.set_manual_cover(game_id, 'covers/replacement.png')
            return setter(library, game_id, target, previous, revision)
        with patch.object(Library, 'set_downloaded_cover', choose_new):
            result = self.run_worker()
        self.assertEqual(result['converted'], 0)
        self.assertEqual(self.lib.get('race')['cover'], 'covers/replacement.png')
        self.assertTrue(source.exists())
        self.assertTrue(replacement.exists())
        self.assertEqual(list((self.lib.root / 'covers').glob('*.webp')), [])

    def test_cancel_before_commit_preserves_original(self):
        source = self.lib.root / 'covers/original.png'
        self.image(source)
        self.game('cancel', 'covers/original.png')
        worker = CoverConversionWorker(self.lib.root)
        result = []
        worker.result.connect(result.append)
        with patch.object(worker, 'isInterruptionRequested', side_effect=[False, True]):
            worker.run()
        self.assertTrue(result[-1]['cancelled'])
        self.assertEqual(self.lib.get('cancel')['cover'], 'covers/original.png')
        self.assertTrue(source.exists())
        self.assertEqual(list((self.lib.root / 'covers').glob('*.webp')), [])

    def test_advanced_button_background_conversion_and_cache_refresh(self):
        self.image(self.lib.root / 'covers/test.png')
        self.game('test', 'covers/test.png')
        window = Window(self.lib, auto_artwork=False)
        dialog = SettingsDialog(self.lib, window, advanced_unlocked=True)
        try:
            dialog.show_page('advanced')
            self.assertEqual(dialog.webp_quality.currentData(), 85)
            dialog.convert_covers_requested.connect(window.convert_covers)
            dialog.cancel_cover_conversion_requested.connect(window.cancel_cover_conversion)
            window.cover_conversion_status.connect(dialog.update_cover_conversion)
            dialog.convert_covers_button.click()
            self.assertIsNotNone(window.cover_conversion_worker)
            self.assertFalse(dialog.convert_covers_button.isEnabled())
            deadline = time.monotonic() + 5
            while window.cover_conversion_worker and time.monotonic() < deadline:
                QTest.qWait(10)
            self.assertIsNone(window.cover_conversion_worker)
            self.assertTrue(dialog.convert_covers_button.isEnabled())
            self.assertIn('1 covers converted', dialog.cover_conversion_status.text())
            self.assertEqual(Path(window.rows['test']['cover']).suffix, '.webp')
            self.assertTrue(window.rows['test']['cover'].endswith('.q85.webp'))
            self.assertFalse(window.cover_icon('test').isNull())
            self.assertEqual(window.thumbnails.generation, 1)
        finally:
            dialog.close()
            # Window owns/ closes the SQLite connection; reopen for tearDown.
            window.close()
            self.lib = Library(self.lib.root)
