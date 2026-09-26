"""Removal is recoverable, scoped to the selection, and independent of play."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import Qt, QTimer
from PySide6.QtWidgets import QApplication, QMessageBox, QDialog, QMenu
from emuluna.app import Window
from emuluna.file_management import (game_locks, removal_plan, remove_games,
                                     remove_media, move_to_trash)
from emuluna.library import Library
from emuluna.media_library import media_entries
from emuluna.removal_dialog import GameRemovalDialog
from emuluna.settings import SettingsDialog
from nes_rom import nes


class FileManagement(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.lib = Library(self.root/'library')
        self.source = self.root/'Original filename.nes'; self.source.write_bytes(nes())
        self.game_id = self.lib.import_file(self.source)[0]
        self.rom = self.lib.root/self.lib.get(self.game_id)['rom_path']
        folder = self.lib.root/'states'/self.game_id/'libretro'/'nestopia'/('a'*64)
        folder.mkdir(parents=True)
        self.state = folder/'slot-1.oesavestate'; self.state.write_bytes(b'state')
        self.preview = self.state.with_suffix('.png'); self.preview.write_bytes(b'preview')
        self.screenshot = self.lib.root/'screenshots'/f'{self.game_id[:12]}-20260924-120000.png'
        self.screenshot.write_bytes(b'screenshot')
        self.battery = self.lib.root/'saves'/self.game_id/'battery.srm'
        self.battery.parent.mkdir(); self.battery.write_bytes(b'battery')
        self.moved = []
        (self.root/'trash').mkdir()
        def fake_trash(path):
            path.rename(self.root/'trash'/f'{len(self.moved)}-{path.name}')
            self.moved.append(path)
        self.trash = patch('emuluna.file_management.move_to_trash',side_effect=fake_trash)
        self.trash.start()
        self.window = None

    def tearDown(self):
        self.trash.stop()
        if self.window: self.window.close()
        else: self.lib.close()
        self.tmp.cleanup()

    def test_remove_reference_keeps_every_file_and_cascades_collection_membership(self):
        collection = self.lib.save_collection('Keep collection')
        self.lib.add_to_collection(collection,[self.game_id])
        remove_games(self.lib,[self.game_id])
        self.assertIsNone(self.lib.get(self.game_id))
        self.assertEqual(self.moved,[])
        for path in (self.source,self.rom,self.state,self.preview,self.screenshot,self.battery):
            self.assertTrue(path.exists())
        self.assertIsNotNone(self.lib.collection(collection))
        self.assertEqual(self.lib.db.execute('SELECT COUNT(*) FROM collection_games').fetchone()[0],0)

    def test_managed_rom_and_selected_media_go_to_trash_source_and_battery_survive(self):
        remove_games(self.lib,[self.game_id],trash_roms=True,states=True,screenshots=True)
        self.assertEqual(set(self.moved),{self.rom,self.state,self.preview,self.screenshot})
        self.assertTrue(self.source.exists()); self.assertTrue(self.battery.exists())
        self.assertIsNone(self.lib.get(self.game_id))

    def test_disc_removal_preserves_shared_tracks_and_unrelated_files(self):
        self.lib.set_setting('copy_games','0')
        disc = self.root/'discs'; disc.mkdir()
        track = disc/'shared.bin'; track.write_bytes(bytes(4096))
        unrelated = disc/'notes.txt'; unrelated.write_text('Keep')
        ids = []
        for name in ('first','second'):
            cue = disc/f'{name}.cue'
            cue.write_text(f'REM {name}\nFILE "shared.bin" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\n')
            ids.append(self.lib.import_file(cue,system_override='psx')[0])
        plan = remove_games(self.lib,ids[:1],trash_roms=True)
        self.assertEqual(plan['shared'],[track])
        self.assertTrue(track.exists()); self.assertTrue(unrelated.exists())
        self.assertEqual(self.moved,[disc/'first.cue'])
        remove_games(self.lib,ids[1:],trash_roms=True)
        self.assertIn(track,self.moved)
        self.assertTrue(unrelated.exists())

    def test_individual_state_and_screenshot_removal_keep_game_and_other_media(self):
        game = self.lib.get(self.game_id)
        entry = media_entries(self.lib,'states',[game])[0]
        remove_media(self.lib,entry)
        self.assertEqual(set(self.moved),{self.state,self.preview})
        self.assertTrue(self.screenshot.exists()); self.assertTrue(self.rom.exists())
        remove_media(self.lib,media_entries(self.lib,'screenshots',[game])[0])
        self.assertIn(self.screenshot,self.moved)
        self.assertIsNotNone(self.lib.get(self.game_id)); self.assertTrue(self.battery.exists())

    def test_managed_playlist_trashes_only_its_imported_set(self):
        folder=self.root/'Two discs';folder.mkdir()
        for name in ('Disc 1.iso','Disc 2.iso'):
            (folder/name).write_bytes(name.encode()*100)
        playlist=folder/'Game.m3u';playlist.write_text('Disc 1.iso\nDisc 2.iso\n')
        game=self.lib.import_file(playlist,system_override='psx')[0]
        imported=self.lib.root/self.lib.get(game)['rom_path']
        extra=imported.parent/'readme.txt';extra.write_text('Keep')
        remove_games(self.lib,[game],trash_roms=True)
        self.assertEqual(set(self.moved),{imported,imported.parent/'Disc 1.iso',imported.parent/'Disc 2.iso'})
        self.assertTrue(extra.exists())
        self.assertTrue(all(path.exists() for path in (playlist,folder/'Disc 1.iso',folder/'Disc 2.iso')))

    def test_bad_disc_inventory_still_allows_reference_removal(self):
        with self.lib.db:
            self.lib.db.execute('INSERT INTO game_files VALUES(?,?,?,?)',
                                (self.game_id,'../../unrelated.bin','x',1))
        with self.assertRaises(ValueError): removal_plan(self.lib,[self.game_id])
        plan=removal_plan(self.lib,[self.game_id],include_roms=False)
        plan['rom_error']='Invalid disc inventory'
        dialog=GameRemovalDialog(plan,trash_roms=True)
        try:
            self.assertFalse(dialog.trash.isEnabled())
            self.assertFalse(dialog.trash.isChecked())
            self.assertTrue(dialog.keep.isChecked())
            remove_games(self.lib,[self.game_id])
            self.assertIsNone(self.lib.get(self.game_id))
            self.assertTrue(self.rom.exists())
        finally: dialog.deleteLater()

    def test_running_game_lock_blocks_all_removal(self):
        with game_locks(self.lib,[self.game_id]):
            with self.assertRaisesRegex(ValueError,'Close'):
                remove_games(self.lib,[self.game_id],trash_roms=True)
            with self.assertRaisesRegex(ValueError,'Close'):
                remove_media(self.lib,media_entries(self.lib,'states',[self.lib.get(self.game_id)])[0])
        self.assertEqual(self.moved,[]); self.assertTrue(self.rom.exists())

    def test_trash_failure_preserves_database_and_reports_partial_progress(self):
        real_fake = __import__('emuluna.file_management',fromlist=['move_to_trash']).move_to_trash
        def fail_after_one(path):
            if self.moved: raise OSError('No space')
            return real_fake(path)
        with patch('emuluna.file_management.move_to_trash',side_effect=fail_after_one):
            with self.assertRaisesRegex(OSError,'1 file.*already moved'):
                remove_games(self.lib,[self.game_id],trash_roms=True,states=True)
        self.assertIsNotNone(self.lib.get(self.game_id))
        self.assertTrue(self.source.exists()); self.assertTrue(self.battery.exists())
        self.assertEqual(len(self.moved),1)

    def test_unavailable_os_trash_never_falls_back_to_permanent_deletion(self):
        # Call the unpatched imported function; only stub Qt's platform call.
        with patch('emuluna.file_management.QFile.moveToTrash',return_value=(False,'')):
            with self.assertRaises(OSError): move_to_trash(self.rom)
        self.assertTrue(self.rom.exists())

    def test_external_media_symlink_and_ambiguous_screenshot_prefix_are_preserved(self):
        other = self.root/'external.png'; other.write_bytes(b'Keep')
        (self.state.parent/'linked.png').symlink_to(other)
        collision = self.game_id[:12]+'b'*52
        with self.lib.db:
            self.lib.db.execute('INSERT INTO games(id,title,system,rom_path,source,added) VALUES(?,?,?,?,?,0)',
                (collision,'Other game','nes','roms/other.nes',''))
        plan=remove_games(self.lib,[self.game_id],states=True,screenshots=True)
        self.assertEqual(plan['screenshots'],[])
        self.assertTrue(self.screenshot.exists()); self.assertTrue(other.exists())
        self.assertNotIn(other,self.moved)

    def test_dialog_cancel_and_default_options_keep_files(self):
        dialog=GameRemovalDialog(removal_plan(self.lib,[self.game_id]))
        try:
            self.assertTrue(dialog.keep.isChecked())
            self.assertFalse(dialog.states.isChecked())
            self.assertFalse(dialog.screenshots.isChecked())
            dialog.trash.setChecked(True)
            dialog.states.setChecked(True)
            self.assertIn(str(self.rom),dialog.details.toPlainText())
            self.assertIn(str(self.state),dialog.details.toPlainText())
            self.assertNotIn(str(self.battery),dialog.details.toPlainText())
            dialog.reject()
            self.assertTrue(self.rom.exists()); self.assertEqual(self.moved,[])
        finally: dialog.deleteLater()

    def test_console_shortcut_targets_clicked_console_and_media_cancel_is_safe(self):
        self.window=Window(self.lib,auto_artwork=False)
        window=self.window; window.show()
        item=next(window.nav.item(i) for i in range(window.nav.count())
                  if window.nav.item(i).data(Qt.UserRole)=='nes')
        window.nav.scrollToItem(item)
        with patch.object(window,'open_settings') as settings, patch('emuluna.app.QMenu') as menu:
            menu.return_value.addAction.side_effect=lambda title, callback: callback()
            window.collection_menu(window.nav.visualItemRect(item).center())
            settings.assert_called_once_with(system='nes')
        inspected=[]
        def inspect():
            dialog=next(d for d in window.findChildren(SettingsDialog) if d.isVisible())
            inspected.append((dialog.controls_page.system_key,set(dialog.loaded_pages)))
            dialog.reject()
        QTimer.singleShot(0,inspect)
        window.open_settings(system='nes')
        self.assertEqual(inspected,[('nes',{'general','library','controls'})])
        entry=media_entries(self.lib,'screenshots',[self.lib.get(self.game_id)])[0]
        with patch('emuluna.media_library.QMessageBox.question',return_value=QMessageBox.No):
            window.media_browser.remove_entry(entry)
        self.assertEqual(self.moved,[])
        with patch('emuluna.media_library.QMessageBox.question',return_value=QMessageBox.Yes):
            window.media_browser.remove_entry(entry)
        self.assertEqual(self.moved,[self.screenshot])
        self.assertIsNotNone(self.lib.get(self.game_id))


if __name__=='__main__': unittest.main()
