"""Artwork workflow tests with a real SQLite catalog and bounded fake HTTP."""
import hashlib
from contextlib import closing
import io
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
import zipfile

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QBuffer, QIODevice, QSize, Qt
from PySide6.QtGui import QColor, QImage
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from emuluna import artwork as art
from emuluna.app import Window
from emuluna.cover_picker import (CoverPickerDialog, CoverResultDelegate,
                                  CoverSearchWorker, cover_label)
from emuluna.library import Library, SYSTEMS
from snes_rom import snes


def catalog_file(path, data):
    db = sqlite3.connect(path)
    db.executescript("""
        CREATE TABLE SYSTEMS(systemID INTEGER, systemOEID TEXT);
        CREATE TABLE ROMs(romID INTEGER, systemID INTEGER, romHashMD5 TEXT, romFileName TEXT);
        CREATE TABLE RELEASES(romID INTEGER, releaseTitleName TEXT, releaseCoverFront TEXT, regionLocalizedID INTEGER);
        CREATE TABLE REGIONS(regionID INTEGER, regionName TEXT);
        INSERT INTO SYSTEMS VALUES(26,'openemu.system.snes'),(19,'openemu.system.gb');
        INSERT INTO REGIONS VALUES(1,'USA'),(2,'Japan');
        INSERT INTO RELEASES VALUES(1,'Test Game','https://covers.example/usa.png',1),
                                   (1,'Test Game','https://covers.example/japan.png',2),
                                   (2,'GoldenEye 007','https://covers.example/goldeneye.png',1);
        INSERT INTO ROMs VALUES(2,26,'00000000000000000000000000000000','GoldenEye 007 (USA).sfc');
    """)
    db.execute("INSERT INTO ROMs VALUES(1,26,?,?)",
               (hashlib.md5(data).hexdigest().upper(), "Test Game (USA).sfc"))
    db.commit()
    db.close()


class ArtworkTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.rom = self.root / "Unrelated filename.sfc"
        self.data = snes()
        self.rom.write_bytes(self.data)
        self.catalog = self.root / "catalog.sqlite"
        catalog_file(self.catalog, self.data)
        self.lib = Library(self.root / "library")
        self.game_id = self.lib.import_file(self.rom)[0]
        image = QImage(80, 120, QImage.Format_RGB32)
        image.fill(QColor("#a56cc1"))
        buffer = QBuffer()
        buffer.open(QIODevice.WriteOnly)
        image.save(buffer, "PNG")
        self.png = bytes(buffer.data())

    def tearDown(self):
        self.lib.close()
        self.tmp.cleanup()

    def worker(self, **kwargs):
        worker = art.ArtworkWorker(self.lib.root, **kwargs)
        results = []
        worker.result.connect(results.append)
        worker.run()
        return results[-1]

    def test_hash_match_region_system_and_headered_snes(self):
        with closing(sqlite3.connect(self.catalog)) as db:
            db.row_factory = sqlite3.Row
            for data in (self.data, bytes(512) + self.data):
                self.assertEqual(art.lookup(db, data, "snes", "Japan")[0]["region"], "Japan")
                self.assertEqual(art.lookup(db, data, "snes", "USA")[0]["title"], "Test Game")
            self.assertEqual(art.lookup(db, self.data, "gb"), [])
            self.assertEqual(art.lookup(db, self.data + b"modified", "snes"), [])

    def test_primary_cover_persistence_and_no_repeat_download(self):
        with patch.object(art.Catalog, "ensure", return_value=self.catalog), \
             patch.object(art.Downloads, "get", return_value=self.png) as get:
            summary = self.worker()
            self.assertEqual(summary["downloaded"], 1)
            self.assertEqual(get.call_count, 1)
            row = self.lib.get(self.game_id)
            self.assertEqual(Path(row['cover']).suffix, '.webp')
            self.assertFalse(QImage(str(self.lib.root / row["cover"])).isNull())
            self.assertEqual(self.lib.artwork_info(self.game_id)["status"], "downloaded")
            self.assertEqual(self.worker(force=True)["downloaded"], 0)
            self.assertEqual(get.call_count, 1)
        reopened = Library(self.lib.root)
        self.assertTrue((reopened.root / reopened.get(self.game_id)["cover"]).exists())
        reopened.close()

    def test_blocked_primary_uses_backup_and_records_source(self):
        calls = []
        def get(_downloads, url, limit, progress=lambda *_: None):
            calls.append(url)
            if "covers.example" in url:
                raise HTTPError(url, 403, "Forbidden", {}, None)
            if "api.github.com/repos/libretro-thumbnails" in url:
                return json.dumps({"tree": [{"type": "blob", "path": "Named_Boxarts/Test Game (USA).png"}],
                                   "truncated": False}).encode()
            self.assertIn("/Named_Boxarts/Test%20Game%20%28USA%29.png", url)
            return self.png
        with patch.object(art.Catalog, "ensure", return_value=self.catalog), patch.object(art.Downloads, "get", get):
            self.assertEqual(self.worker()["downloaded"], 1)
        self.assertEqual(sum("covers.example" in url for url in calls), 1)
        self.assertIn("libretro-thumbnails", self.lib.artwork_info(self.game_id)["source_url"])

    def test_manual_cover_wins_race_and_removal_does_not_restore_game(self):
        manual = "covers/custom.png"
        # An earlier custom cover was deleted from disk, then chosen again
        # during the download. Its path alone cannot detect this race.
        self.lib.set_manual_cover(self.game_id, manual)
        def get(*args, **kwargs):
            (self.lib.root / manual).write_bytes(self.png)
            self.lib.set_manual_cover(self.game_id, manual)
            return self.png
        with patch.object(art.Catalog, "ensure", return_value=self.catalog), patch.object(art.Downloads, "get", get):
            self.assertEqual(self.worker()["downloaded"], 0)
        self.assertEqual(self.lib.get(self.game_id)["cover"], manual)
        self.assertEqual((self.lib.root / manual).read_bytes(), self.png)
        self.lib.remove(self.game_id)
        self.assertFalse(self.lib.set_downloaded_cover(self.game_id, "covers/late.png", None))
        self.assertIsNone(self.lib.get(self.game_id))

    def test_failed_and_missing_art_are_retryable_without_hammering(self):
        self.lib.set_setting("artwork_backup", "0")
        with patch.object(art.Catalog, "ensure", return_value=self.catalog), \
             patch.object(art.Downloads, "get", side_effect=OSError("Offline")) as get:
            self.assertEqual(self.worker()["failed"], 1)
            requests = get.call_count
            self.assertEqual(self.lib.artwork_candidates(), [])
            self.assertEqual(len(self.lib.artwork_candidates(force=True)), 1)
            self.worker()
            self.assertEqual(get.call_count, requests)
            self.worker(force=True)
            self.assertGreater(get.call_count, requests)
        self.rom.write_bytes(self.data + b"unknown")
        unknown = self.lib.import_file(self.rom)[0]
        with patch.object(art.Catalog, "ensure", return_value=self.catalog), patch.object(art.Downloads, "get") as get:
            self.assertEqual(self.worker(game_ids={unknown})["not_found"], 1)
            get.assert_not_called()
        self.assertGreater(self.lib.artwork_info(unknown)["retry_after"], time.time() + 86400)

    def test_catalog_download_atomic_install_offline_and_bad_update(self):
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, "w") as z:
            z.write(self.catalog, "openvgdb.sqlite")
            z.writestr("../../unexpected", b"never extract me")
        metadata = [{"tag_name": "v29.0", "assets": [{"name": "openvgdb.zip", "browser_download_url": "https://example.org/db.zip"}]}]
        downloads = art.Downloads()
        catalog = art.Catalog(self.lib.root, downloads)
        with patch.object(downloads, "get", side_effect=[json.dumps(metadata).encode(), archive.getvalue()]):
            self.assertEqual(catalog.ensure(), catalog.path)
        self.assertFalse((self.root / "unexpected").exists())
        with patch.object(downloads, "get", side_effect=OSError("Offline")) as get:
            self.assertEqual(catalog.ensure(), catalog.path)
            get.assert_not_called()
        original = catalog.path.read_bytes()
        state = catalog.root / "openvgdb.json"
        state.write_text(json.dumps({"version": "old", "checked": 0}))
        with patch.object(downloads, "get", side_effect=[json.dumps(metadata).encode(), b"broken zip"]):
            self.assertEqual(catalog.ensure(), catalog.path)
        self.assertEqual(catalog.path.read_bytes(), original)

    def test_image_validation_and_download_limits(self):
        encoded = art.image_webp(self.png, quality=100)
        self.assertEqual(encoded[:4], b'RIFF')
        self.assertEqual(encoded[8:12], b'WEBP')
        self.assertEqual(QImage.fromData(encoded).pixelColor(20, 20), QColor('#a56cc1'))
        with self.assertRaises(ValueError):
            art.image_webp(b"<html>Access denied</html>")
        class Response(io.BytesIO):
            headers = {}
            url = "https://example.org/image"
        downloads = art.Downloads()
        self.assertGreater(len(downloads.ssl_context.get_ca_certs()), 100)
        with patch.object(art, "urlopen", return_value=Response(b"123456")) as open_url:
            with self.assertRaises(ValueError):
                downloads.get(Response.url, 5)
            self.assertIs(open_url.call_args.kwargs["context"], downloads.ssl_context)
        with self.assertRaises(ValueError):
            art.Downloads().get("file:///etc/passwd", 5)
        with self.assertRaises(art.Cancelled):
            art.Downloads(lambda: True).get(Response.url, 5)

    def test_backup_matching_is_exact_not_fuzzy(self):
        backup = art.BackupArt(self.lib.root, art.Downloads())
        backup.indexes["snes"] = ["Test Game (USA).png", "Test Game (Japan).png", "Test Game 2 (USA).png",
                                   "GoldenEye 007 (USA).png", "GoldenEye - Rogue Agent (USA).png"]
        urls = backup.urls({"system": "snes", "title": "Test Game"}, [], "USA")
        self.assertIn("USA", urls[0])
        self.assertFalse(any("Game%202" in url for url in urls))
        self.assertEqual(backup.urls({"system": "snes", "title": "Test Gamb"}, [], "USA"), [])
        self.assertEqual(backup.urls({"system": "snes", "title": "007 - GoldenEye (USA)"}, [], "USA"), [])
        closest = backup.urls(
            {"system": "snes", "title": "007 - GoldenEye (USA)"}, [], "USA", closest=True)
        self.assertIn("GoldenEye%20007%20%28USA%29.png", closest[0])
        self.assertEqual(art.title_similarity("007 - GoldenEye (USA)", "GoldenEye 007 (USA)"), 1.0)
        self.assertEqual(art.normalized_title("Legend of Zelda, The (USA)"), art.normalized_title("The Legend of Zelda"))

    def test_visual_cover_search_ranks_and_downloads_previews(self):
        names = ["GoldenEye - Rogue Agent (USA).png", "GoldenEye 007 (USA).png"]
        worker = CoverSearchWorker(self.lib.root, "snes", "007 - GoldenEye (USA)")
        candidates, summaries = [], []
        worker.candidate.connect(candidates.append)
        worker.result.connect(summaries.append)
        with patch.object(art.BackupArt, "index", return_value=names), \
             patch.object(art.Catalog, "ensure", return_value=self.catalog), \
             patch.object(art.Downloads, "get", return_value=self.png):
            worker.run()
        self.assertEqual(candidates[0]["title"], "GoldenEye 007 (USA)")
        self.assertEqual(candidates[0]["score"], 1.0)
        self.assertEqual(candidates[0]['image'][8:12], b'WEBP')
        self.assertEqual(candidates[0]["metadata"]["title"], "GoldenEye 007")
        self.assertEqual(candidates[0]["metadata"]["region"], "USA")
        # Identical downloaded images from dump-name variants appear once.
        self.assertEqual(summaries[-1]["found"], 1)
        self.assertEqual(summaries[-1]["metadata"], 1)

    def test_cover_cards_are_compact_and_dump_tags_stay_out_of_labels(self):
        candidate = {
            "title": "Galaxian (1984-09-01)(Namco)(JP)[b2]",
            "metadata": {},
        }
        self.assertEqual(cover_label(candidate), ("Galaxian", "Japan"))
        self.assertEqual(CoverResultDelegate.card_size, QSize(178, 226))

    def test_visual_picker_selects_an_explicit_preview(self):
        with patch("emuluna.cover_picker.QTimer.singleShot"):
            dialog = CoverPickerDialog(
                self.lib.root, "snes", "007 - GoldenEye (USA)")
        try:
            dialog.add_candidate({
                "title": "GoldenEye 007 (USA)",
                "url": "https://covers.example/goldeneye.png",
                "image": self.png, "score": 1.0,
                "metadata": {"title": "GoldenEye 007", "region": "USA"},
                "metadata_source": "https://example.org/goldeneye",
            })
            dialog.search_finished({"found": 1, "failed": 0, "metadata": 1,
                                    "metadata_error": "", "error": "", "cancelled": False})
            self.assertEqual(dialog.results.count(), 1)
            self.assertTrue(dialog.use_button.isEnabled())
            dialog.use_selected()
            self.assertEqual(dialog.selection["title"], "GoldenEye 007 (USA)")
            self.assertEqual(dialog.selection["image"], self.png)
        finally:
            dialog.close()

    def test_selected_cover_applies_regional_metadata_and_preserves_edits(self):
        selection = {
            "title": "GoldenEye 007 (USA)",
            "url": "https://covers.example/goldeneye.png",
            "image": self.png,
            "score": 1.0,
            "metadata": {"title": "GoldenEye 007", "region": "USA",
                         "publisher": "Nintendo"},
            "metadata_source": "https://example.org/goldeneye",
        }
        self.lib.update_metadata(self.game_id, {"publisher": "My custom publisher"})

        class Picker:
            def __init__(self, *args, **kwargs): self.selection = selection
            def exec(self): return True
            def deleteLater(self): pass

        window = Window(self.lib, auto_artwork=False)
        try:
            with patch("emuluna.cover_picker.CoverPickerDialog", Picker):
                window.find_cover(self.game_id)
            game = self.lib.get(self.game_id)
            self.assertEqual(game["title"], "GoldenEye 007")
            self.assertTrue((self.lib.root / game["cover"]).is_file())
            metadata = self.lib.metadata(self.game_id)
            self.assertEqual(metadata["region"], "USA")
            self.assertEqual(metadata["publisher"], "My custom publisher")
            self.assertEqual(self.lib.metadata_lookup(self.game_id)["provider"], "OpenVGDB")
        finally:
            window.close()
        self.lib = Library(self.root / "library")

    def test_game_menu_uses_visual_picker_instead_of_missing_cover_download(self):
        window = Window(self.lib, auto_artwork=False)
        try:
            menu = window.game_menu([self.game_id])
            labels = [action.text() for action in menu.actions()]
            self.assertIn("Find cover art…", labels)
            self.assertNotIn("Download missing cover art", labels)
            self.assertNotIn("Download replacement cover art…", labels)
            menu.deleteLater()
        finally:
            window.close()
        self.lib = Library(self.root / "library")

    def test_qt_startup_download_and_system_icons(self):
        window = Window(self.lib)
        window.show()
        for i in range(window.nav.count()):
            item = window.nav.item(i)
            if item.data(Qt.UserRole) in SYSTEMS:
                self.assertFalse(item.icon().isNull())
                self.assertFalse(item.icon().pixmap(24, 24).isNull())
        self.lib.set_setting("artwork_auto", "0")
        window.start_artwork()
        self.assertIsNone(window.art_worker)
        self.lib.set_setting("artwork_auto", "1")
        with patch.object(art.Catalog, "ensure", return_value=self.catalog), \
             patch.object(art.Downloads, "get", return_value=self.png):
            deadline = time.monotonic() + 3
            while (not self.lib.get(self.game_id)["cover"] or window.art_worker) and time.monotonic() < deadline:
                QTest.qWait(10)
            self.assertIsNone(window.art_worker)
            self.assertTrue(self.lib.get(self.game_id)["cover"])
            self.assertTrue(window.download_art_action.isEnabled())
            self.assertIn("1 cover downloaded", window.notifications.last_message)
        window.close()
        # Window.close() owns the connection. tearDown must not close it twice.
        self.lib = Library(self.root / "library")

    def test_startup_cover_check_is_optional_and_never_repeats_on_a_timer(self):
        self.lib.set_setting("core_auto_install", "0")
        self.lib.set_setting("artwork_check_at_startup", "0")
        with patch.object(Window, "start_metadata") as metadata, \
             patch.object(Window, "start_artwork") as artwork:
            window = Window(self.lib)
            try:
                QTest.qWait(300)
                metadata.assert_called_once_with()
                artwork.assert_not_called()
                self.assertFalse(hasattr(window, "art_timer"))
            finally:
                window.close()

        self.lib = Library(self.root / "library")
        self.lib.set_setting("artwork_check_at_startup", "1")
        with patch.object(Window, "start_metadata") as metadata, \
             patch.object(Window, "start_artwork") as artwork:
            window = Window(self.lib)
            try:
                QTest.qWait(300)
                metadata.assert_called_once_with()
                artwork.assert_called_once_with()
                self.assertFalse(hasattr(window, "art_timer"))
            finally:
                window.close()
        self.lib = Library(self.root / "library")

    def test_import_starts_artwork_without_waiting_for_restart(self):
        self.lib.remove(self.game_id)
        window = Window(self.lib, auto_artwork=False)
        with patch.object(art.Catalog, "ensure", return_value=self.catalog), \
             patch.object(art.Downloads, "get", return_value=self.png):
            window.import_paths([self.rom])
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                QTest.qWait(10)
                row = self.lib.get(self.game_id)
                if row and row["cover"] and not window.art_worker and not window.worker.isRunning():
                    break
            self.assertTrue(self.lib.get(self.game_id)["cover"])
            self.assertIsNone(window.art_worker)
            self.assertTrue(window.import_action.isEnabled())
        window.close()
        self.lib = Library(self.root / "library")

    def test_window_close_cancels_download_before_library_closes(self):
        started = threading.Event()
        def waiting(downloads, *args, **kwargs):
            started.set()
            while not downloads.cancelled():
                time.sleep(0.005)
            downloads.check()
        window = Window(self.lib, auto_artwork=False)
        window.show()
        with patch.object(art.Catalog, "ensure", return_value=self.catalog), \
             patch.object(art.Downloads, "get", waiting):
            window.start_artwork()
            deadline = time.monotonic() + 3
            while not started.is_set() and time.monotonic() < deadline:
                QTest.qWait(10)
            self.assertTrue(started.is_set())
            window.close()
            while window.art_worker and time.monotonic() < deadline:
                QTest.qWait(10)
            self.assertIsNone(window.art_worker)
            self.assertFalse(window.isVisible())
        self.lib = Library(self.root / "library")
        self.assertIsNone(self.lib.get(self.game_id)["cover"])
        self.assertEqual(len(self.lib.artwork_candidates()), 1)


if __name__ == "__main__":
    unittest.main()
