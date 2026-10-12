"""Three shipped themes, editable packages, validation and live restyling."""
import json
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from pathlib import Path
import tempfile
import unittest
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtGui import QColor, QPalette
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from emuluna.library import Library
from emuluna.app import Window
from emuluna.settings import SettingsDialog
from emuluna.theme_files import ThemeRegistry
from emuluna.theme import select_theme, reload_themes, theme_color, theme_palette, theme_asset


class ThemePackages(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_three_builtins_and_default_luna(self):
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(folder)
            window = Window(lib, auto_artwork=False)
            settings = SettingsDialog(lib, window)
            try:
                self.assertEqual([settings.theme_choice.itemText(i) for i in range(settings.theme_choice.count())], ['Luna','Daylight','Classic'])
                self.assertEqual(settings.theme_choice.currentData(), 'luna')
                self.assertFalse(settings.use_system_theme.isChecked())
                self.assertEqual(theme_palette().color(QPalette.Highlight), QColor('#b6a0e4'))
                window.show();settings.show()
                for key in ('daylight', 'classic', 'luna'):
                    settings.theme_choice.setCurrentIndex(settings.theme_choice.findData(key))
                    QTest.qWait(30)
                    theme = ThemeRegistry(folder).get(key)
                    self.assertEqual(window.palette().color(QPalette.Window), QColor(theme.palette['window']))
                    self.assertEqual(settings.palette().color(QPalette.Text), QColor(theme.palette['text']))
                    self.assertEqual(lib.setting('appearance.theme', 'luna'), key)
                    self.assertIn(theme.palette['highlight'], window.styleSheet())
                    self.assertNotIn('palette(', window.styleSheet())
                    self.assertNotIn('theme(', settings.styleSheet())
                    self.assertEqual(theme_color('rating-filled', '#000000'), QColor(theme.colors['rating-filled']))
            finally:
                settings.close();window.close();lib.close()

    def test_custom_copy_reload_and_stylesheet_override(self):
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(folder)
            registry = ThemeRegistry(folder)
            key = registry.create('Ocean', 'daylight')
            package = Path(folder) / 'themes' / key
            value = json.loads((package / 'theme.json').read_text())
            value['palette']['highlight'] = '#0088aa'
            value['colors']['rating-filled'] = '#ffd700'
            value['assets'] = {'controls-background': None}
            (package / 'theme.json').write_text(json.dumps(value))
            (package / 'library.qss').write_text('QWidget#sidebar {border:2px solid theme(rating-filled);}')
            window = Window(lib, auto_artwork=False)
            try:
                select_theme(lib, key)
                window.show();QTest.qWait(20)
                self.assertEqual(theme_palette().color(QPalette.Highlight), QColor('#0088aa'))
                self.assertIn('border:2px solid #ffd700', window.styleSheet())
                self.assertIsNone(theme_asset('controls-background'))
                value['palette']['highlight'] = '#007799'
                (package / 'theme.json').write_text(json.dumps(value))
                reload_themes(lib)
                self.assertEqual(window.palette().color(QPalette.Highlight), QColor('#007799'))
                (package / 'theme.json').write_text('{ invalid')
                registry = reload_themes(lib)
                self.assertTrue(registry.errors)
                self.assertEqual(theme_palette().color(QPalette.Highlight), QColor('#b6a0e4'))
                self.assertTrue((package / 'theme.json').is_file())
            finally:
                window.close();lib.close()

    def test_bad_color_and_escaping_asset_are_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            registry = ThemeRegistry(folder)
            key = registry.create('Unsafe')
            path = Path(folder) / 'themes' / key / 'theme.json'
            value = json.loads(path.read_text())
            value['palette']['highlight'] = 'not a color'
            path.write_text(json.dumps(value))
            self.assertTrue(ThemeRegistry(folder).errors)
            value['palette'] = {}
            value['styles'] = {'library':'../outside.qss'}
            path.write_text(json.dumps(value))
            self.assertTrue(ThemeRegistry(folder).errors)
            value['styles'] = {}
            value['assets'] = {'controls-background':'/etc/passwd'}
            path.write_text(json.dumps(value))
            self.assertTrue(ThemeRegistry(folder).errors)
            value['assets'] = {}
            value['styles'] = {'library':'library.qss'}
            path.write_text(json.dumps(value))
            (path.parent/'library.qss').write_text('QWidget {image:url(../../outside.png);}')
            self.assertTrue(ThemeRegistry(folder).errors)

    def test_library_organization_worker_completes_and_refreshes_settings(self):
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(folder)
            lib.set_setting('storage.layout', 'legacy')
            source = Path(folder).parent / (Path(folder).name + '-source.nes')
            source.write_bytes(b'synthetic cartridge')
            settings = None
            try:
                key = lib.import_file(source)[0]
                settings = SettingsDialog(lib)
                settings._begin_storage_migration()
                for _ in range(400):
                    QTest.qWait(10)
                    if settings.storage_worker is None:
                        break
                self.assertIsNone(settings.storage_worker)
                self.assertTrue(lib.console_layout())
                self.assertTrue(settings.organize_storage.isEnabled())
                self.assertTrue(lib.validate_game(key).is_file())
            finally:
                if settings:
                    if settings.storage_worker:
                        settings.storage_worker.wait(10000)
                    settings.close()
                lib.close();source.unlink(missing_ok=True)

    def test_player_cannot_start_while_library_gate_is_held(self):
        from PySide6.QtCore import QLockFile
        from emuluna.player import Player
        from emuluna.core import CoreError
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(folder)
            key = lib._import_bytes('Game.nes', b'synthetic', 'synthetic')
            gate = QLockFile(str(lib.root / 'storage-migration.lock'))
            self.assertTrue(gate.tryLock(0))
            try:
                with self.assertRaisesRegex(CoreError, 'library is being organized'):
                    Player(lib, key)
                self.assertFalse((lib.root / 'saves' / (key + '.lock')).exists())
            finally:
                gate.unlock();lib.close()

    def test_openemu_red_accent_and_gray_navigation_are_theme_overrides(self):
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(folder)
            window = Window(lib, auto_artwork=False)
            settings = SettingsDialog(lib, window)
            try:
                select_theme(lib, 'classic')
                self.assertEqual(theme_palette().color(QPalette.Highlight), QColor('#d12733'))
                self.assertEqual(theme_color('rating-filled','#000000'), QColor('#d12733'))
                from emuluna.theme import theme_font
                self.assertEqual(theme_font('game-title-weight',700),400)
                self.assertEqual(theme_color('sidebar-selected-background','#000000'), QColor('#484848'))
                self.assertIn('#484848',window.styleSheet())
                self.assertIn('border-radius:16px',window.styleSheet())
                select_theme(lib,'luna')
                self.assertEqual(theme_color('rating-filled','#000000'), QColor('#f2c94c'))
                self.assertNotIn('#484848',window.styleSheet())
            finally:
                settings.close();window.close();lib.close()

    def test_wood_text_stays_readable_for_every_console_input_and_theme_switch(self):
        from PySide6.QtWidgets import QLabel, QPushButton, QComboBox
        from emuluna.systems import SYSTEMS
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(folder)
            settings = SettingsDialog(lib)
            settings.show_page('controls');settings.show()
            page = settings.controls_page
            try:
                for key in ('daylight','classic','luna','classic'):
                    select_theme(lib,key);QTest.qWait(25)
                    theme = ThemeRegistry(folder).get(key)
                    foreground = QColor(theme.colors['controls-text'])
                    subtle = QColor(theme.colors['controls-subtle'])
                    for system in SYSTEMS:
                        page.system.setCurrentIndex(page.system.findData(system))
                        for source in ('keyboard','gamepad'):
                            page.source.setCurrentIndex(page.source.findData(source))
                            self.app.processEvents()
                            for widget in page.findChildren(QLabel):
                                if widget.isEnabled():
                                    self.assertIn(widget.palette().color(QPalette.WindowText),
                                                  (foreground,subtle),(key,system,source,widget.text()))
                            for widget in page.findChildren(QPushButton):
                                if widget.isEnabled():
                                    self.assertEqual(widget.palette().color(QPalette.ButtonText),foreground,
                                                     (key,system,source,widget.text()))
                            for widget in page.findChildren(QComboBox):
                                if widget.isEnabled():
                                    self.assertEqual(widget.palette().color(QPalette.Text),foreground,
                                                     (key,system,source))
                    self.assertEqual(settings.done_button.palette().color(QPalette.ButtonText),foreground)
                settings.show_page('general')
                self.assertEqual(settings.done_button.palette().color(QPalette.ButtonText),
                                 theme_palette().color(QPalette.ButtonText))
            finally:
                settings.close();lib.close()

    def test_real_openemu_wood_matches_provenance_in_both_themes(self):
        import hashlib
        from emuluna.theme_files import DATA
        provenance = json.loads((DATA/'openemu-wood.json').read_text())
        for key in ('daylight','classic'):
            theme = ThemeRegistry().get(key)
            wood = Path(theme.assets['controls-background'])
            self.assertEqual(hashlib.sha256(wood.read_bytes()).hexdigest(),provenance['sha256'])

    def test_each_builtin_is_standalone_in_a_separate_folder(self):
        import shutil
        from unittest.mock import patch
        from emuluna.theme_files import DATA
        for key in ('luna', 'daylight', 'classic'):
            with tempfile.TemporaryDirectory() as temp:
                package = Path(temp) / key
                shutil.copytree(DATA / key, package)
                value = json.loads((package / 'theme.json').read_text())
                self.assertNotIn('extends', value)
                loader = ThemeRegistry.__new__(ThemeRegistry)
                loader.themes = {}
                with patch('emuluna.theme_files.DATA', Path(temp) / 'missing'):
                    theme = loader.load(package)
                    self.assertEqual(theme.id, key)
                    for component in ('library', 'settings', 'gameplay', 'onboarding'):
                        self.assertIn('Q', theme.stylesheet(component))
                    self.assertTrue(Path(theme.assets['controls-background']).is_relative_to(package))
                    self.assertTrue(Path(theme.assets['controls-background']).is_file())

    def test_new_custom_copy_is_independent_and_legacy_ids_migrate(self):
        from emuluna.theme import follow_system_theme
        from emuluna.theme_files import LEGACY_IDS
        with tempfile.TemporaryDirectory() as temp:
            registry = ThemeRegistry(temp)
            key = registry.create('Independent', 'daylight')
            package = Path(temp) / 'themes' / key
            value = json.loads((package / 'theme.json').read_text())
            self.assertNotIn('extends', value)
            loader = ThemeRegistry.__new__(ThemeRegistry)
            loader.themes = {}
            theme = loader.load(package, custom=True)
            self.assertEqual(theme.palette, registry.get('daylight').palette)
            self.assertEqual(theme.fonts, registry.get('daylight').fonts)
            self.assertTrue(Path(theme.assets['controls-background']).is_relative_to(package))
            lib = Library(temp)
            try:
                for old, new in LEGACY_IDS.items():
                    lib.set_setting('appearance.theme', old)
                    follow_system_theme(lib)
                    self.assertEqual(lib.setting('appearance.theme'), new)
                    self.assertEqual(registry.get(old).id, new)
            finally:
                lib.close()
