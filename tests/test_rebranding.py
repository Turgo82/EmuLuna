"""Standalone identity, legacy library discovery and optional empty consoles."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from media_stub import isolate_audio
isolate_audio()
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from PySide6.QtWidgets import QApplication
from PySide6.QtCore import Qt
from PySide6.QtGui import QImage
from emuluna.app import Window
from emuluna.library import Library, SYSTEMS, default_data_dir
from emuluna.settings import SettingsDialog
from emuluna.branding import ICON, LOGO, MASCOT, configure_application
from snes_rom import snes


class Rebranding(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_identity_icon_and_legacy_library_discovery(self):
        configure_application(self.app)
        self.assertEqual(self.app.applicationName(), 'EmuLuna')
        self.assertFalse(self.app.windowIcon().isNull())
        icon = QImage(str(ICON))
        self.assertTrue(icon.hasAlphaChannel())
        self.assertEqual(icon.pixelColor(0,0).alpha(), 0)
        for artwork in (LOGO, MASCOT):
            image = QImage(str(artwork))
            self.assertFalse(image.isNull())
            self.assertTrue(image.hasAlphaChannel())
            self.assertEqual(image.pixelColor(0, 0).alpha(), 0)
        with tempfile.TemporaryDirectory() as folder, patch.dict(os.environ, {'XDG_DATA_HOME': folder}):
            base = Path(folder)
            self.assertEqual(default_data_dir(), base/'emuluna')
            legacy = Library(base/'openemu-linux')
            legacy.set_setting('volume', 37)
            legacy.close()
            self.assertEqual(default_data_dir(), base/'openemu-linux')
            current = Library(base/'emuluna'); current.close()
            self.assertEqual(default_data_dir(), base/'emuluna')
            with patch.dict(os.environ, {'EMULUNA_DATA_DIR':str(base/'explicit')}):
                explicit=Library(); self.assertEqual(explicit.root, base/'explicit');explicit.close()

    def test_empty_console_toggle_import_search_and_removal(self):
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(Path(folder)/'library')
            window = Window(lib, auto_artwork=False)
            settings = SettingsDialog(lib, window)
            settings.changed.connect(window.refresh)
            def consoles():
                return [window.nav.item(i).data(Qt.UserRole) for i in range(window.nav.count())
                        if window.nav.item(i).data(Qt.UserRole) in SYSTEMS]
            try:
                self.assertEqual(set(consoles()), set(SYSTEMS))
                settings.hide_empty_consoles.setChecked(True)
                self.assertEqual(consoles(), [])
                settings.show_page('cores')
                self.assertEqual(len(settings.choices), len(SYSTEMS))
                rom=Path(folder)/'My game.sfc';rom.write_bytes(snes())
                game=lib.import_file(rom)[0];window.refresh()
                self.assertEqual(consoles(), ['snes'])
                window.search.setText('No matching title')
                self.assertEqual(window.games.count(), 0)
                self.assertEqual(consoles(), ['snes'])
                window.search.clear()
                index=next(i for i in range(window.nav.count()) if window.nav.item(i).data(Qt.UserRole)=='snes')
                window.nav.setCurrentRow(index)
                with lib.db: lib.db.execute('DELETE FROM games WHERE id=?',(game,))
                window.refresh()
                self.assertEqual(consoles(), [])
                self.assertEqual(window.nav.currentItem().data(Qt.UserRole),'all')
                self.assertEqual(lib.setting('library.hide_empty_consoles'),'1')
                settings.hide_empty_consoles.setChecked(False)
                self.assertEqual(set(consoles()),set(SYSTEMS))
            finally:
                settings.close();window.close()
