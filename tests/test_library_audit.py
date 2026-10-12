"""Missing-file recovery and migration prompts keep identities and saved data safe."""
import json
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtWidgets import QApplication, QDialog, QMessageBox
from PySide6.QtTest import QTest
from emuluna.library import Library
from emuluna.library_audit import audit_library, LibraryAuditDialog
from emuluna.settings import SettingsDialog


class LibraryAudit(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.lib = Library(self.root / 'library')
        self.lib.set_setting('storage.layout', 'legacy')
        self.source = self.root / 'Original.nes'
        self.source.write_bytes(b'original cartridge')
        self.key = self.lib.import_file(self.source)[0]
        self.dialogs = []

    def tearDown(self):
        for dialog in self.dialogs:
            dialog.close()
            for _ in range(1000):
                if getattr(dialog, 'worker', None) is None:
                    break
                QTest.qWait(10)
            dialog.deleteLater()
        self.lib.close();self.temp.cleanup()

    def settle(self, dialog):
        for _ in range(1000):
            QTest.qWait(10)
            if dialog.worker is None:
                return
        self.fail('Audit did not finish')

    def missing(self):
        self.lib.validate_game(self.key).unlink()

    def test_quick_scan_lists_missing_rom_and_track_including_external_games(self):
        self.missing()
        cue = self.root / 'Disc.cue'
        cue.write_text('FILE "one.bin" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\n')
        track = self.root / 'one.bin';track.write_bytes(b'track')
        disc = self.lib.import_content(cue, 'psx')
        (self.lib.validate_game(disc).parent/'one.bin').unlink()
        self.lib.set_setting('copy_games', '0')
        external = self.root / 'External.nes';external.write_bytes(b'external cartridge')
        ext = self.lib.import_file(external)[0];external.unlink()
        report = audit_library(self.lib)
        self.assertEqual(report['checked'], 3)
        by_id = {issue['game_id']:issue for issue in report['issues']}
        self.assertEqual(by_id[self.key]['problem'], 'Missing ROM')
        self.assertEqual(by_id[disc]['problem'], 'Missing disc files')
        self.assertEqual(by_id[ext]['problem'], 'Missing ROM')
        self.assertEqual(len(self.lib.games()), 3)

    def test_verify_detects_changed_cartridge_without_modifying_it(self):
        path = self.lib.validate_game(self.key)
        path.write_bytes(b'changed cartridge!')
        self.assertEqual(audit_library(self.lib)['issues'], [])
        report = audit_library(self.lib, verify=True)
        self.assertEqual(report['issues'][0]['problem'], 'Changed or invalid ROM')
        self.assertEqual(path.read_bytes(), b'changed cartridge!')

    def test_missing_rom_repair_matches_identity_updates_inventory_and_allows_migration(self):
        self.missing()
        self.lib.rate([self.key], 4)
        dialog = LibraryAuditDialog(self.lib, migration=True)
        self.dialogs.append(dialog);self.settle(dialog)
        self.assertFalse(dialog.continue_button.isEnabled())
        dialog.accept()
        self.assertNotEqual(dialog.result(), QDialog.Accepted)
        with patch('emuluna.library_audit.QFileDialog.getOpenFileName', return_value=(str(self.source),'')):
            dialog.locate_selected()
        self.settle(dialog)
        # Repair launches a new scan after the repair worker finishes.
        self.settle(dialog)
        self.assertEqual(dialog.report['issues'], [])
        self.assertTrue(dialog.continue_button.isEnabled())
        self.assertEqual(self.lib.get(self.key)['rating'], 4)
        self.assertEqual(self.lib.validate_game(self.key).read_bytes(), self.source.read_bytes())
        inventory = json.loads((self.lib.root/'consoles/nes/games.json').read_text())
        self.assertEqual(inventory['games'][0]['rom'], self.lib.get(self.key)['rom_path'])

    def test_wrong_replacement_stays_unresolved(self):
        self.missing()
        wrong = self.root/'Wrong.nes';wrong.write_bytes(b'wrong cartridge')
        dialog = LibraryAuditDialog(self.lib, migration=True)
        self.dialogs.append(dialog);self.settle(dialog)
        with patch('emuluna.library_audit.QFileDialog.getOpenFileName', return_value=(str(wrong),'')):
            dialog.locate_selected()
        self.settle(dialog)
        self.assertFalse(dialog.continue_button.isEnabled())
        self.assertIn('different ROM', dialog.status.text())
        self.assertEqual(len(self.lib.games()), 1)

    def test_reference_removal_requires_confirmation_and_keeps_saved_files(self):
        self.missing()
        saved = self.lib.root/'saves'/self.key/'battery.srm'
        saved.parent.mkdir();saved.write_bytes(b'saved game')
        dialog = LibraryAuditDialog(self.lib, migration=True)
        self.dialogs.append(dialog);self.settle(dialog)
        with patch('emuluna.library_audit.QMessageBox.question', return_value=QMessageBox.No):
            dialog.remove_selected()
        self.assertIsNotNone(self.lib.get(self.key))
        with patch('emuluna.library_audit.QMessageBox.question', return_value=QMessageBox.Yes):
            dialog.remove_selected()
        self.settle(dialog)
        self.assertIsNone(self.lib.get(self.key))
        self.assertEqual(saved.read_bytes(), b'saved game')
        self.assertTrue(self.source.exists())
        self.assertTrue(dialog.continue_button.isEnabled())

    def test_migration_cancel_and_final_confirmation(self):
        settings = SettingsDialog(self.lib)
        self.dialogs.append(settings)
        class FakeAudit:
            from PySide6.QtCore import Signal
            def __init__(self, *args, **kwargs):
                self.changed = type('SignalStub', (), {'connect':lambda *_:None})()
            def exec(self): return QDialog.Rejected
            def deleteLater(self): pass
        with patch('emuluna.settings.QFileDialog.getExistingDirectory', return_value=str(self.root / 'destination')), patch('emuluna.library_audit.LibraryAuditDialog', FakeAudit), patch.object(settings, '_begin_storage_migration') as begin:
            settings.start_storage_migration();begin.assert_not_called()
        FakeAudit.exec = lambda self: QDialog.Accepted
        with patch('emuluna.settings.QFileDialog.getExistingDirectory', return_value=str(self.root / 'destination')), patch('emuluna.library_audit.LibraryAuditDialog', FakeAudit), patch.object(settings, '_begin_storage_migration') as begin:
            with patch('emuluna.settings.QMessageBox.question', return_value=QMessageBox.No):
                settings.start_storage_migration();begin.assert_not_called()
            with patch('emuluna.settings.QMessageBox.question', return_value=QMessageBox.Yes):
                settings.start_storage_migration();begin.assert_called_once()

    def test_audit_is_in_unlocked_advanced_settings(self):
        settings = SettingsDialog(self.lib, advanced_unlocked=True)
        self.dialogs.append(settings)
        self.assertEqual(settings.audit_library_button.text(), 'Audit library…')
