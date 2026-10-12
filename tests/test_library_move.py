"""Console relocation preserves identities and recovers interrupted pointer switches."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from PySide6.QtCore import QLockFile
from emuluna.library import Library
from emuluna.library_move import move_console_library, recover_console_move, _publish
from emuluna.library_audit import audit_library
from emuluna.media_library import media_entries
from emuluna.file_management import media_files
from emuluna.library_paths import console_root


class LibraryMove(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.base = Path(self.temp.name)
        self.lib = Library(self.base / 'app')
        self.source = self.base / 'Game.nes'
        self.source.write_bytes(b'original game')
        self.key = self.lib.import_file(self.source)[0]
        self.dest = self.base / 'chosen'
        self.dest.mkdir()

    def tearDown(self):
        self.lib.close()
        self.temp.cleanup()

    def test_move_reopen_import_and_move_again_preserve_relative_paths_and_media(self):
        stored = self.lib.get(self.key)['rom_path']
        self.lib.db.execute('UPDATE games SET rating=4,favorite=1 WHERE id=?', (self.key,))
        self.lib.db.commit()
        paths = []
        for kind, name in [('saves','battery.srm'), ('states','libretro/example/'+'a'*64+'/auto.oesavestate'), ('screenshots','shot.png')]:
            path = self.lib.game_folder(kind,self.key) / name
            path.parent.mkdir(parents=True,exist_ok=True)
            path.write_bytes(kind.encode())
            paths.append((path.relative_to(self.lib.root),kind.encode()))
        (self.lib.root / 'consoles/notes.txt').write_text('keep unindexed files too')
        self.assertTrue(move_console_library(self.lib, self.dest))
        self.assertEqual(console_root(self.lib.root).resolve(), self.dest/'consoles')
        self.assertFalse(list(self.lib.root.glob('.consoles-before-*')))
        self.assertEqual(self.lib.get(self.key)['rom_path'],stored)
        self.assertEqual(self.lib.validate_game(self.key).read_bytes(),b'original game')
        self.assertTrue(self.source.exists())
        self.assertTrue((self.lib.root/'library.sqlite3').exists())
        for relative,data in paths:
            self.assertEqual((self.lib.root/relative).read_bytes(),data)
        self.assertEqual(len(media_entries(self.lib,'states',self.lib.games())),1)
        self.assertEqual(len(media_files(self.lib,[self.key],'screenshots')),1)
        self.assertFalse(audit_library(self.lib, verify=True)['issues'])
        from emuluna.core_manager import CoreManager
        manager = CoreManager(self.lib.root)
        with patch.object(manager,'build',return_value={'id':'example'}) as build:
            self.assertEqual(manager.state_build(self.lib.root / paths[1][0],self.key,'nes')['id'],'example')
            build.assert_called_once_with('example','a'*64,'nes')
        self.lib.close(); self.lib=Library(self.base/'app')
        self.assertEqual(self.lib.get(self.key)['rating'],4)
        other=self.base/'Other.nes';other.write_bytes(b'new game')
        new=self.lib.import_file(other)[0]
        self.assertTrue(self.lib.validate_game(new).resolve().is_relative_to(self.dest))
        second=self.base/'second';second.mkdir()
        self.assertTrue(move_console_library(self.lib,second))
        self.assertFalse((self.dest/'consoles').exists())
        self.assertEqual((second/'consoles/notes.txt').read_text(),'keep unindexed files too')
        self.assertEqual(self.lib.validate_game(self.key).read_bytes(),b'original game')

    def test_destination_collision_and_nested_destinations_keep_originals(self):
        (self.dest/'consoles').mkdir();(self.dest/'consoles/precious').write_text('keep')
        with self.assertRaisesRegex(ValueError,'already contains'):
            move_console_library(self.lib,self.dest)
        self.assertEqual((self.dest/'consoles/precious').read_text(),'keep')
        with self.assertRaisesRegex(ValueError,'outside'):
            move_console_library(self.lib,self.lib.root/'consoles/nes')
        self.assertFalse((self.lib.root/'consoles').is_symlink())

    def test_failed_verification_and_pointer_switch_roll_back(self):
        stored=self.lib.get(self.key)['rom_path']
        with patch('emuluna.library_move.file_hexdigest',return_value='bad'):
            with self.assertRaisesRegex(ValueError,'verification'):
                move_console_library(self.lib,self.dest)
        with patch.object(Path,'symlink_to',side_effect=OSError('switch failed')):
            with self.assertRaisesRegex(OSError,'switch failed'):
                move_console_library(self.lib,self.dest)
        self.assertFalse((self.dest/'consoles').exists())
        self.assertFalse(list(self.dest.glob('.emuluna-console-copy-*')))
        self.assertFalse((self.lib.root/'console-move.json').exists())
        self.assertEqual((self.lib.root/stored).read_bytes(),b'original game')

    def test_interruption_before_commit_restores_old_external_pointer(self):
        move_console_library(self.lib,self.dest)
        other=self.base/'other';other.mkdir()
        from emuluna.storage_migration import write_json
        real_write=write_json
        def interrupted(path,value):
            if Path(path).name=='console-location.json':
                raise RuntimeError('interrupted')
            return real_write(path,value)
        # Interrupt rollback too, as if the process exited before recovery.
        with patch('emuluna.library_move.write_json',side_effect=interrupted), patch('emuluna.library_move._recover',side_effect=RuntimeError('interrupted')):
            with self.assertRaises(RuntimeError):
                move_console_library(self.lib,other)
        recover_console_move(self.lib)
        self.assertEqual(console_root(self.lib.root).resolve(),self.dest/'consoles')
        self.assertFalse((other/'consoles').exists())
        self.assertEqual(self.lib.validate_game(self.key).read_bytes(),b'original game')

    def test_interruption_after_commit_finishes_cleanup(self):
        with patch('emuluna.library_move._recover',side_effect=RuntimeError('interrupted')):
            with self.assertRaises(RuntimeError): move_console_library(self.lib,self.dest)
        recover_console_move(self.lib)
        self.assertTrue((self.lib.root/'consoles').is_symlink())
        self.assertFalse(list(self.lib.root.glob('.consoles-before-*')))
        self.assertFalse((self.lib.root/'console-move.json').exists())
        self.assertEqual(self.lib.validate_game(self.key).read_bytes(),b'original game')

    def test_partial_old_cleanup_can_resume(self):
        with patch('emuluna.library_move._recover',side_effect=RuntimeError('interrupted')):
            with self.assertRaises(RuntimeError): move_console_library(self.lib,self.dest)
        journal=self.lib.root/'console-move.json';record=json.loads(journal.read_text())
        record['cleanup_started']=True;journal.write_text(json.dumps(record))
        backup=next(self.lib.root.glob('.consoles-before-*'))
        next(p for p in backup.rglob('*') if p.is_file()).unlink()
        recover_console_move(self.lib)
        self.assertFalse(backup.exists())
        self.assertFalse(journal.exists())
        self.assertTrue(self.lib.validate_game(self.key).is_file())

    def test_changed_old_files_are_retained_without_blocking_new_library(self):
        with patch('emuluna.library_move._recover',side_effect=RuntimeError('interrupted')):
            with self.assertRaises(RuntimeError): move_console_library(self.lib,self.dest)
        backup=next(self.lib.root.glob('.consoles-before-*'))
        (backup/'extra.txt').write_text('external change')
        recover_console_move(self.lib)
        self.assertTrue((backup/'extra.txt').exists())
        self.assertTrue(list(self.lib.root.glob('console-move-retained-*')))
        self.assertTrue(self.lib.validate_game(self.key).is_file())

    def test_unapproved_symlink_and_disconnected_destination_are_rejected(self):
        move_console_library(self.lib,self.dest)
        hidden=self.base/'unmounted';(self.dest/'consoles').rename(hidden)
        with self.assertRaisesRegex(ValueError,'disconnected'):
            move_console_library(self.lib,self.base)
        self.assertFalse((self.dest/'consoles').exists())
        self.assertTrue(audit_library(self.lib)['issues'])
        self.lib.export_inventory()
        self.assertFalse((self.dest/'consoles').exists())
        self.assertEqual(media_entries(self.lib,'states',self.lib.games()),[])
        self.assertEqual(media_entries(self.lib,'screenshots',self.lib.games()),[])
        hidden.rename(self.dest/'consoles')
        pointer=self.lib.root/'consoles';pointer.unlink();pointer.symlink_to(self.base)
        with self.assertRaisesRegex(ValueError,'does not match'): console_root(self.lib.root)

    def test_atomic_publish_does_not_overwrite_existing_empty_folder(self):
        stage=self.base/'stage';stage.mkdir();(stage/'file').write_text('new')
        destination=self.base/'existing';destination.mkdir()
        with self.assertRaisesRegex(ValueError,'changed'): _publish(stage,destination)
        self.assertTrue((stage/'file').exists());self.assertFalse(list(destination.iterdir()))

    def test_running_game_and_internal_symlinks_block_move(self):
        lock=QLockFile(str(self.lib.root/'saves'/(self.key+'.lock')))
        self.assertTrue(lock.tryLock(0))
        try:
            with self.assertRaises(ValueError): move_console_library(self.lib,self.dest)
        finally: lock.unlock()
        (self.lib.root/'consoles/link').symlink_to(self.source)
        with self.assertRaisesRegex(ValueError,'symlink'): move_console_library(self.lib,self.dest)
        self.assertTrue(self.source.exists())

    def test_source_changes_during_copy_roll_back(self):
        def progress(message):
            if message.startswith('Copying'):
                (self.lib.root/'consoles/new-file.txt').write_text('keep this change')
        with self.assertRaisesRegex(ValueError,'changed during'):
            move_console_library(self.lib,self.dest,progress)
        self.assertFalse((self.dest/'consoles').exists())
        self.assertEqual((self.lib.root/'consoles/new-file.txt').read_text(),'keep this change')
        self.assertEqual(self.lib.validate_game(self.key).read_bytes(),b'original game')

    def test_legacy_move_worker_organizes_before_relocation(self):
        from emuluna.library_move import LibraryMoveWorker
        legacy = Library(self.base/'legacy')
        try:
            legacy.set_setting('storage.layout','legacy')
            key = legacy.import_file(self.source)[0]
            worker = LibraryMoveWorker(legacy.root,self.dest)
            worker.run()
            self.assertTrue(worker.succeeded)
            self.assertTrue(legacy.console_layout())
            self.assertEqual(legacy.validate_game(key).read_bytes(),b'original game')
            self.assertTrue(legacy.validate_game(key).resolve().is_relative_to(self.dest/'consoles'))
            self.assertTrue(list((legacy.root/'backups').glob('*')))
        finally:
            legacy.close()
