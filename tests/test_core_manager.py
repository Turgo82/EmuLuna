"""Core installation, updates, persistence and Settings without live networking."""
import io
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from media_stub import isolate_audio
isolate_audio()
from unittest.mock import patch
import zipfile
import zlib

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication, QMenu
from emuluna.app import Window
from emuluna.artwork import Downloads
from emuluna.core import CoreError
from emuluna.core_manager import CoreManager, BUILDBOT
from emuluna.library import Library
from emuluna.settings import SettingsDialog


def binary(version=1):
    data = bytearray(128)
    data[:6] = b"\x7fELF\x02\x01"
    data[18:20] = (62).to_bytes(2, "little")
    data[-1] = version
    return bytes(data)


class ManagerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.lib = Library(self.tmp.name)
        self.manager = CoreManager(self.lib.root)
        self.probe = patch("emuluna.core_manager.probe", return_value={
            "name": "Snes9x", "version": "test", "extensions": "smc|sfc"})
        self.probe.start()

    def tearDown(self):
        self.probe.stop()
        self.lib.close()
        self.tmp.cleanup()

    def archive(self, data):
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as zip:
            zip.writestr("snes9x_libretro.so", data)
            zip.writestr("../../unexpected.so", data)
        return buffer.getvalue()

    def test_download_update_integrity_and_unchanged_build(self):
        downloads = Downloads()
        first = binary()
        index = {"snes9x_libretro.so.zip": {"build": "2026-09-10", "crc": f"{zlib.crc32(first):08x}"}}
        with patch.object(downloads, "get", return_value=self.archive(first)) as get:
            old, updated = self.manager.download("snes9x", downloads, index=index)
            self.assertTrue(updated)
            self.assertFalse((self.lib.root.parent / "unexpected.so").exists())
            _, updated = self.manager.download("snes9x", downloads, index=index)
            self.assertFalse(updated)
            self.assertEqual(get.call_count, 1)
        old_path = self.manager.path(old)
        second = binary(2)
        second_index = {"snes9x_libretro.so.zip": {"build": "2026-09-11", "crc": f"{zlib.crc32(second):08x}"}}
        with patch.object(downloads, "get", return_value=self.archive(first)):
            with self.assertRaises(CoreError):
                self.manager.download("snes9x", downloads, index=second_index)
        self.assertEqual(self.manager.installed()["snes9x"], old)
        with patch.object(downloads, "get", return_value=self.archive(second)):
            new, changed = self.manager.download("snes9x", downloads, index=second_index)
        self.assertTrue(changed)
        self.assertNotEqual(old_path, self.manager.path(new))
        self.assertEqual(old_path.read_bytes(), first)  # Running builds are retained.
        self.assertEqual(self.manager.path(new).read_bytes(), second)
        previous = self.manager.history("snes9x")
        self.assertEqual([item["sha256"] for item in previous], [old["sha256"]])
        self.assertEqual(self.manager.build("snes9x", old["sha256"], "snes")["sha256"], old["sha256"])
        with self.assertRaisesRegex(CoreError, "no longer installed"):
            self.manager.build("snes9x", "0" * 64, "snes")
        restored = self.manager.restore("snes9x", old["sha256"])
        self.assertEqual(self.manager.path(restored).read_bytes(), first)
        self.assertEqual(self.manager.history("snes9x")[0]["sha256"], new["sha256"])
        self.assertTrue(self.manager.remove("snes9x", self.lib))
        self.assertEqual(self.manager.history("snes9x"), [])
        self.assertFalse(old_path.exists())
        self.assertFalse((self.lib.root / new["path"]).exists())

    def test_redownload_repairs_corrupt_file(self):
        first = binary()
        record = self.manager.install_bytes(first, "snes9x", source="buildbot", crc=f"{zlib.crc32(first):08x}")
        path = self.manager.path(record)
        path.write_bytes(b"corrupt")
        with self.assertRaises(CoreError):
            self.manager.path(record)
        index = {"snes9x_libretro.so.zip": {"build": "2026-09-10", "crc": record["crc"]}}
        with patch.object(Downloads, "get", return_value=self.archive(first)):
            repaired, changed = self.manager.download("snes9x", Downloads(), index=index)
        self.assertTrue(changed)
        self.assertEqual(self.manager.path(repaired).read_bytes(), first)

    def test_import_bad_binary_and_failed_probe_preserve_install(self):
        record = self.manager.install_bytes(binary(), "snes9x")
        with self.assertRaises(CoreError):
            self.manager.install_bytes(b"not an ELF file", "snes9x")
        with patch("emuluna.core_manager.probe", side_effect=CoreError("Missing dependency")):
            with self.assertRaises(CoreError):
                self.manager.install_bytes(binary(2), "snes9x")
        self.assertEqual(self.manager.installed()["snes9x"], record)
        local = self.lib.root / "my_libretro.so"
        local.write_bytes(binary(3))
        imported = self.manager.import_file(local)
        self.assertEqual(imported["source"], "local")
        self.assertEqual(imported["systems"], ["snes"])
        self.assertEqual(local.read_bytes(), binary(3))

    def test_settings_selection_and_general_options_persist(self):
        self.manager.install_bytes(binary(), "snes9x")
        dialog = SettingsDialog(self.lib)
        dialog.show_page('gameplay')
        dialog.focus_pause.setChecked(False)
        dialog.integer.setChecked(True)
        dialog.volume.setValue(37)
        dialog.accept()
        window = Window(self.lib, auto_artwork=False)
        try:
            menu = QMenu(window)
            core_menu = window.add_console_core_menu(menu, 'snes')
            selected = next(action for action in core_menu.actions()
                            if action.data() == 'snes9x')
            selected.trigger()
            self.assertTrue(selected.isCheckable())
        finally:
            window.close()
        reopened = Library(self.lib.root)
        try:
            self.assertEqual(reopened.setting("volume"), "37")
            self.assertEqual(reopened.setting("pause_unfocused"), "0")
            self.assertEqual(reopened.setting("integer_scale"), "1")
            self.assertEqual(self.manager.selection(reopened, "snes")["id"], "snes9x")
            with self.assertRaises(CoreError):
                self.manager.selection(reopened, "gb")
            reopened.set_setting("core.snes", "missing")
            with self.assertRaises(CoreError):
                self.manager.selection(reopened, "snes")
        finally:
            reopened.close()

    def test_settings_controls_are_grouped_with_their_features(self):
        dialog = SettingsDialog(self.lib)
        try:
            general = dialog.tabs.widget(dialog.page_keys.index('general'))
            library = dialog.tabs.widget(dialog.page_keys.index('library'))
            self.assertTrue(general.isAncestorOf(dialog.volume))
            self.assertTrue(library.isAncestorOf(dialog.auto_art))
            self.assertTrue(library.isAncestorOf(dialog.backup_art))

            dialog.show_page('gameplay')
            gameplay = dialog.tabs.widget(dialog.page_keys.index('gameplay'))
            self.assertTrue(gameplay.isAncestorOf(dialog.focus_pause))
            self.assertTrue(gameplay.isAncestorOf(dialog.integer))

            dialog.unlock_advanced()
            advanced = dialog.tabs.widget(dialog.page_keys.index('advanced'))
            self.assertTrue(advanced.isAncestorOf(dialog.minimize_library))
            self.assertTrue(advanced.isAncestorOf(dialog.experimental_hardware))
            self.assertTrue(advanced.isAncestorOf(dialog.show_fps))
            self.assertTrue(advanced.isAncestorOf(dialog.show_renderer_debug))
            self.assertFalse(gameplay.isAncestorOf(dialog.minimize_library))

            dialog.show_page('bios')
            system_files = dialog.tabs.widget(dialog.page_keys.index('bios'))
            self.assertTrue(system_files.isAncestorOf(dialog.bios_path))
            self.assertTrue(system_files.isAncestorOf(dialog.open_bios_button))
            self.assertFalse(general.isAncestorOf(dialog.bios_path))

            target = Path(self.tmp.name) / 'custom-system-folder'
            dialog.bios_path.setText(str(target))
            with patch('emuluna.settings.QDesktopServices.openUrl', return_value=True) as opened:
                dialog.open_bios_button.click()
            self.assertTrue(target.is_dir())
            self.assertEqual(Path(opened.call_args.args[0].toLocalFile()), target)
        finally:
            dialog.close()

    def test_settings_download_button_installs_and_enables_selection(self):
        data = binary()
        def get(_downloads, url, *args):
            if url.endswith(".index-extended"):
                return f"2026-09-10 {zlib.crc32(data):08x} snes9x_libretro.so.zip\n".encode()
            self.assertEqual(url, BUILDBOT + "snes9x_libretro.so.zip")
            return self.archive(data)
        dialog = SettingsDialog(self.lib)
        dialog.show_page('cores')
        with patch.object(Downloads, "get", get):
            dialog.start_job(core_id="snes9x")
            deadline = time.monotonic() + 3
            while dialog.worker and time.monotonic() < deadline:
                QTest.qWait(10)
            self.assertIsNone(dialog.worker)
        installed_rows = [dialog.table.item(row, 1).text()
                          for row in range(dialog.table.rowCount())]
        self.assertIn('Snes9x', installed_rows)
        self.assertTrue(dialog.update_button.isEnabled())
        self.assertIn("installed or updated", dialog.status.text())
        dialog.accept()


if __name__ == "__main__":
    unittest.main()
