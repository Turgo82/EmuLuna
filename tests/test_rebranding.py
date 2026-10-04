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
from PySide6.QtCore import QSize, Qt
from PySide6.QtGui import QIcon, QImage
from PySide6.QtTest import QTest
from emuluna.app import AboutDialog, Window
from emuluna.library import Library, SYSTEMS, default_data_dir
from emuluna.settings import SettingsDialog
from emuluna.branding import (
    ICON, LOGO, MASCOT, UI_ICON_DIR, UNLOCK_SOUND, PaletteSvgIconEngine,
    configure_application, navigation_icon,
)
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

    def test_toolbar_and_settings_icons_are_packaged_scalable_svgs(self):
        names = {
            'menu', 'plus', 'search', 'general', 'gameplay', 'controls',
            'cores', 'downloads', 'bios', 'advanced', 'library', 'states',
            'screenshots', 'grid', 'list', 'bell', 'power', 'fullscreen',
            'fullscreen-exit', 'file-verified', 'file-unverified',
        }
        self.assertEqual({path.stem for path in UI_ICON_DIR.glob('*.svg')}, names)
        for name in names:
            source = (UI_ICON_DIR / f'{name}.svg').read_text(encoding='utf-8')
            self.assertIn('<svg', source)
            size = 20 if name in ('file-verified', 'file-unverified') else 24
            self.assertIn(f'viewBox="0 0 {size} {size}"', source)
            icon = navigation_icon(name)
            self.assertFalse(icon.isNull())
            for mode, state in (
                    (QIcon.Normal, QIcon.Off),
                    (QIcon.Selected, QIcon.On),
                    (QIcon.Disabled, QIcon.Off)):
                self.assertFalse(icon.pixmap(QSize(24, 24), mode, state).isNull())

        # Qt asks icon engines for a physical-size image on a high-DPI display.
        # Keep the logical size while retaining all 48 rendered pixels at 2x.
        engine = PaletteSvgIconEngine(UI_ICON_DIR / 'grid.svg')
        high_dpi = engine.scaledPixmap(QSize(24, 24), QIcon.Normal, QIcon.Off, 2.0)
        self.assertEqual(high_dpi.size(), QSize(48, 48))
        self.assertEqual(high_dpi.devicePixelRatio(), 2.0)

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
                self.assertGreater(settings.table.rowCount(), 0)
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

    def test_about_logo_unlocks_advanced_settings_after_four_clicks(self):
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(Path(folder) / 'library')
            window = Window(lib, auto_artwork=False)
            settings = None
            try:
                window.show_about()
                dialog = window.about_dialog
                self.assertTrue(UNLOCK_SOUND.is_file())
                self.assertEqual(Path(dialog.unlock_sound.source().toLocalFile()), UNLOCK_SOUND)
                with patch.object(dialog, 'play_unlock_chime') as chime, \
                     patch.object(dialog.logo_animation, 'start') as spin:
                    for _ in range(3):
                        QTest.mouseClick(dialog.logo_button, Qt.LeftButton)
                    self.assertFalse(window.advanced_settings_unlocked)
                    self.assertFalse(dialog.unlock_notice.isVisible())
                    chime.assert_not_called()
                    spin.assert_not_called()

                    QTest.mouseClick(dialog.logo_button, Qt.LeftButton)
                    self.assertTrue(window.advanced_settings_unlocked)
                    self.assertTrue(dialog.unlock_notice.isVisible())
                    chime.assert_called_once_with()
                    spin.assert_called_once_with()

                    # A single extra click does not toggle or retrigger it.
                    QTest.mouseClick(dialog.logo_button, Qt.LeftButton)
                    chime.assert_called_once_with()
                    spin.assert_called_once_with()

                settings = SettingsDialog(
                    lib, window, advanced_unlocked=window.advanced_settings_unlocked)
                self.assertIsNone(settings.tabs.cornerWidget(Qt.TopLeftCorner))
                self.assertEqual(settings.tabs.tabText(settings.tabs.currentIndex()), 'Advanced')
                self.assertEqual(settings.tabs.count(), 7)
                advanced = settings.tabs.widget(settings.page_keys.index('advanced'))
                self.assertTrue(advanced.isAncestorOf(settings.minimize_library))
                self.assertTrue(advanced.isAncestorOf(settings.experimental_hardware))
                self.assertTrue(advanced.isAncestorOf(settings.show_fps))
                self.assertTrue(advanced.isAncestorOf(settings.show_renderer_debug))
            finally:
                if settings:
                    settings.close()
                window.close()

    def test_advanced_unlock_persists_toggles_off_and_notice_opens_settings(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder) / 'library'
            lib = Library(root)
            window = Window(lib, auto_artwork=False)
            try:
                window.show_about()
                for _ in range(4):
                    QTest.mouseClick(window.about_dialog.logo_button, Qt.LeftButton)
                self.assertEqual(lib.setting('advanced.unlocked'), '1')
                window.about_dialog.close()
            finally:
                window.close()

            lib = Library(root)
            window = Window(lib, auto_artwork=False)
            try:
                self.assertTrue(window.advanced_settings_unlocked)
                settings = SettingsDialog(lib)
                self.assertIn('advanced', settings.page_keys)
                settings.close()
                window.show_about()
                self.assertTrue(window.about_dialog.unlock_notice.isVisible())
                with patch.object(window, 'open_settings') as open_settings:
                    QTest.mouseClick(window.about_dialog.unlock_notice, Qt.LeftButton)
                    QTest.qWait(20)
                    open_settings.assert_called_once_with(page='advanced')
                window.show_about()
                for _ in range(3):
                    QTest.mouseClick(window.about_dialog.logo_button, Qt.LeftButton)
                self.assertTrue(window.advanced_settings_unlocked)
                QTest.mouseClick(window.about_dialog.logo_button, Qt.LeftButton)
                self.assertFalse(window.advanced_settings_unlocked)
                self.assertFalse(window.about_dialog.unlock_notice.isVisible())
                self.assertEqual(lib.setting('advanced.unlocked'), '0')
                settings = SettingsDialog(lib)
                self.assertNotIn('advanced', settings.page_keys)
                settings.close()
                window.about_dialog.close()
            finally:
                window.close()
            lib = Library(root)
            try:
                self.assertEqual(lib.setting('advanced.unlocked'), '0')
            finally:
                lib.close()

    def test_about_unlock_falls_back_when_multimedia_is_unavailable(self):
        with patch('emuluna.app.QSoundEffect', None), \
             patch.object(QApplication, 'beep') as beep:
            dialog = AboutDialog()
            try:
                self.assertIsNone(dialog.unlock_sound)
                dialog.play_unlock_chime()
                beep.assert_called_once_with()
            finally:
                dialog.close()
