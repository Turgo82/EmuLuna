"""Only relevant firmware is shown; shared files stay importable once merged."""
import os
import hashlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication
from emuluna.bios import checklist
from emuluna.library import Library
from emuluna.settings import SettingsDialog
from emuluna.systems import CATALOG


class FirmwareChecklist(unittest.TestCase):
    def test_shared_core_firmware_is_scoped_and_deduplicated(self):
        rows = checklist([('segacd', 'genesis_plus_gx'), ('segacd', 'picodrive')])
        self.assertEqual(len(rows), 3)
        self.assertTrue(all(row['essential'] and len(row['uses']) == 2 for row in rows))
        cartridge = checklist([('genesis', 'genesis_plus_gx')])
        self.assertNotIn('bios_CD_U.bin', [row['entry']['path'] for row in cartridge])
        self.assertTrue(all(not row['essential'] for row in cartridge))
        self.assertEqual(checklist([('nes', 'nestopia')]), [])
        self.assertEqual(len(checklist([('fds', 'nestopia'), ('fds', 'fceumm')])[0]['uses']), 2)
        self.assertEqual(checklist([('pce', 'mednafen_pce_fast')]), [])
        for core in CATALOG.values():
            for entry in core.get('firmware', []):
                self.assertLessEqual(set(entry.get('systems', core['systems'])), set(core['systems']))
                self.assertLessEqual(set(entry.get('required_systems', [])),
                                     set(entry.get('systems', core['systems'])))

    def test_different_checksums_are_not_merged(self):
        records = {name: {'systems': ['psx'], 'firmware': [
            {'path': 'bios.bin', 'md5': name, 'optional': optional}]}
            for name, optional in [('a', True), ('b', False)]}
        with patch('emuluna.bios.CATALOG', records):
            rows = checklist([('psx', 'a'), ('psx', 'b')])
        self.assertEqual(len(rows), 2)
        self.assertEqual([row['essential'] for row in rows], [True, False])


class FirmwareSettings(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.library = Library(Path(self.tmp.name))
        self.dialog = None

    def tearDown(self):
        if self.dialog:
            self.dialog.close()
        self.library.close()
        self.tmp.cleanup()

    def files(self):
        return [self.dialog.bios_table.item(row, 0).text()
                for row in range(self.dialog.bios_table.rowCount())]

    def add_console(self, system):
        self.library.db.execute(
            'INSERT INTO games(id,title,system,rom_path,source,added,original_filename) VALUES(?,?,?,?,?,?,?)',
            (system, 'Game', system, system + '.rom', '', 0, 'Game.rom'))
        self.library.db.commit()

    def open(self):
        self.dialog = SettingsDialog(self.library)
        self.dialog.show_page('bios')

    def test_library_defaults_optional_toggle_and_shared_files_keep_selection(self):
        self.add_console('segacd')
        self.add_console('snes')
        self.library.set_setting('core.snes', 'bsnes')
        self.open()
        self.assertEqual(self.dialog.bios_system.currentData(), 'library')
        self.assertEqual(set(self.files()), {'bios_CD_E.bin', 'bios_CD_U.bin', 'bios_CD_J.bin'})
        self.assertIn('optional files hidden', self.dialog.bios_summary.text())
        row = self.files().index('bios_CD_U.bin')
        self.dialog.bios_table.selectRow(row)
        self.dialog.bios_optional.setChecked(True)
        self.assertIn('dsp1.data.rom', self.files())
        self.assertEqual(self.dialog.bios_table.item(self.dialog.bios_table.currentRow(), 0).text(), 'bios_CD_U.bin')
        self.dialog.bios_core_scope.setCurrentIndex(1)
        self.assertEqual(self.files().count('bios_CD_U.bin'), 1)
        item = self.dialog.bios_table.item(self.dialog.bios_table.currentRow(), 0)
        self.assertEqual(item.data(Qt.UserRole)['path'], 'bios_CD_U.bin')
        self.assertIn('2efd74e3232ff260e371b99f84024f7f', self.dialog.bios_details.text())
        self.assertTrue(self.dialog.bios_import.isEnabled())
        self.assertNotIn('dc/dc_boot.bin', self.files())

    def test_selected_imported_beetle_core_and_other_cores_merge_regional_bios(self):
        self.add_console('psx')
        self.library.set_setting('core.psx', 'my_beetle')
        installed = {'my_beetle': {'id': 'my_beetle', 'catalog_id': 'mednafen_psx_hw', 'systems': ['psx']}}
        with patch('emuluna.settings.CoreManager.installed', return_value=installed):
            self.open()
            self.assertEqual(len(self.files()), 3)
            self.assertIn('Required for', self.dialog.bios_table.item(0, 2).text())
            self.dialog.bios_core_scope.setCurrentIndex(1)
            self.assertEqual(len(self.files()), 3)
            self.dialog.bios_optional.setChecked(True)
            self.assertEqual(len(self.files()), 5)
            self.assertEqual(self.files().count('scph5501.bin'), 1)

    def test_empty_optional_console_and_unknown_imported_core(self):
        self.add_console('dreamcast')
        self.open()
        self.assertEqual(self.files(), [])
        self.assertFalse(self.dialog.bios_import.isEnabled())
        self.assertEqual(self.dialog.bios_details.text(), '')
        self.dialog.bios_optional.setChecked(True)
        self.assertEqual(self.files(), ['dc/dc_boot.bin'])
        self.library.set_setting('core.dreamcast', 'custom')
        with patch('emuluna.settings.CoreManager.installed', return_value={
            'custom': {'id': 'custom', 'systems': ['dreamcast']}}):
            self.dialog.refresh_bios()
        self.assertIn('No checklist is available', self.dialog.bios_summary.text())
        self.assertFalse(self.dialog.bios_import.isEnabled())

    def test_importing_a_shared_file_updates_its_single_row(self):
        self.add_console('segacd')
        payload = b'user-supplied firmware fixture'
        source = Path(self.tmp.name) / 'original.bin'
        source.write_bytes(payload)
        entry = {'path': 'shared.bin', 'description': 'Shared test BIOS',
                 'optional': False, 'md5': hashlib.md5(payload).hexdigest()}
        records = {key: {**CATALOG[key], 'firmware': [entry]}
                   for key in ('genesis_plus_gx', 'picodrive')}
        with patch.dict(CATALOG, records):
            self.open()
            self.dialog.bios_core_scope.setCurrentIndex(1)
            self.assertEqual(self.files(), ['shared.bin'])
            with patch('emuluna.settings.QFileDialog.getOpenFileName', return_value=(str(source), '')):
                self.dialog.import_selected_bios()
            self.assertEqual(self.files(), ['shared.bin'])
            self.assertEqual(self.dialog.bios_table.item(0, 3).text(), 'Verified')
            self.assertEqual(source.read_bytes(), payload)
            self.assertEqual((self.library.root / 'system/shared.bin').read_bytes(), payload)
