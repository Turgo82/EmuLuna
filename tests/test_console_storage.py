"""Migration safety, shared ROM folders, disc references and media continuity."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from PySide6.QtCore import QLockFile
from emuluna.library import Library
from emuluna.storage_migration import migrate_console_storage, write_json
from emuluna.file_management import removal_plan, remove_games
from emuluna.media_library import media_entries


class ConsoleStorage(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.lib = Library(self.root / 'library')

    def tearDown(self):
        self.lib.close()
        self.temp.cleanup()

    def cartridge(self, name='Example.nes', data=b'example cartridge'):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return self.lib.import_file(path)[0]

    def disc(self, name='Disc'):
        folder = self.root / name
        folder.mkdir()
        (folder / 'tracks').mkdir()
        (folder / 'tracks/one.bin').write_bytes(b'example track ' + name.encode())
        path = folder / 'game.cue'
        path.write_text('FILE "tracks/one.bin" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\n')
        return self.lib.import_content(path, 'psx')

    def manifest(self, system='nes'):
        return json.loads((self.lib.root / 'consoles' / system / 'games.json').read_text())

    def test_shared_rom_folder_preserves_bytes_and_disambiguates_names(self):
        first = self.cartridge('Example.nes', b'first cartridge')
        second = self.cartridge('other/Example.nes', b'second cartridge')
        paths = [self.lib.validate_game(key) for key in (first, second)]
        self.assertEqual(paths[0].parent, paths[1].parent)
        self.assertEqual(paths[0].parent, self.lib.root / 'consoles/nes/roms')
        self.assertNotEqual(paths[0].name, paths[1].name)
        self.assertEqual(paths[0].read_bytes(), b'first cartridge')
        self.assertEqual(paths[1].read_bytes(), b'second cartridge')
        self.assertEqual(len(self.manifest()['games']), 2)
        self.lib.rename(first, 'Renamed game')
        self.assertIn('Renamed game', [game['title'] for game in self.manifest()['games']])
        self.lib.remove(first)
        self.assertEqual([game['id'] for game in self.manifest()['games']], [second])
        self.assertTrue(paths[0].exists())

    def test_multitrack_discs_have_isolated_folders(self):
        first, second = self.disc('First'), self.disc('Second')
        a, b = self.lib.validate_game(first), self.lib.validate_game(second)
        self.assertNotEqual(a.parent, b.parent)
        self.assertEqual(a.parent.parent, self.lib.root / 'consoles/psx/roms')
        self.assertTrue((a.parent / 'tracks/one.bin').is_file())
        self.assertEqual(len(removal_plan(self.lib, [first])['roms']), 2)
        self.assertEqual(len(self.manifest('psx')['games']), 2)

    def legacy_media(self, key):
        files = {'saves': 'battery.srm', 'states': 'libretro/example/' + 'a'*64 + '/auto.oesavestate'}
        for kind, name in files.items():
            path = self.lib.root / kind / key / name
            path.parent.mkdir(parents=True)
            path.write_bytes(kind.encode())
        shot = self.lib.root / 'screenshots' / (key[:12] + '-20261010-120000.png')
        shot.write_bytes(b'screenshot')

    def test_migration_retains_ids_metadata_external_files_and_media(self):
        self.lib.set_setting('storage.layout', 'legacy')
        key = self.cartridge()
        disc = self.disc()
        self.lib.rate([key], 4)
        self.lib.favorite(key)
        self.legacy_media(key)
        self.lib.set_setting('copy_games', '0')
        external = self.cartridge('External.nes', b'external cartridge')
        source = self.lib.validate_game(external)
        self.assertEqual(migrate_console_storage(self.lib), 3)
        self.assertEqual(self.lib.validate_game(external), source)
        self.assertTrue(source.exists())
        self.assertEqual(self.lib.get(key)['rating'], 4)
        self.assertEqual(self.lib.get(key)['favorite'], 1)
        self.assertEqual(self.lib.validate_game(key).read_bytes(), b'example cartridge')
        self.assertTrue(self.lib.validate_game(disc).is_file())
        for kind in ('saves', 'states', 'screenshots'):
            self.assertTrue(any(self.lib.game_folder(kind, key).rglob('*')))
        self.assertEqual(len(media_entries(self.lib, 'states', self.lib.games())), 1)
        self.assertEqual(len(media_entries(self.lib, 'screenshots', self.lib.games())), 1)
        self.assertEqual(len(removal_plan(self.lib, [key])['states']), 1)
        self.assertTrue(list((self.lib.root / 'backups').glob('*.sqlite3')))
        self.assertEqual(migrate_console_storage(self.lib), 0)
        self.assertFalse((self.lib.root / 'storage-migration.json').exists())
        self.assertTrue(next(game for game in self.manifest()['games'] if game['id']==external)['external'])

    def test_failed_file_move_rolls_back_database_and_every_file(self):
        self.lib.set_setting('storage.layout', 'legacy')
        key = self.cartridge()
        self.legacy_media(key)
        old = self.lib.get(key)['rom_path']
        original_link = __import__('os').link
        calls = []
        def link(source, target):
            calls.append(target)
            if len(calls) == 2:
                raise OSError('Simulated disk failure')
            original_link(source, target)
        with patch('emuluna.storage_migration.os.link', side_effect=link):
            with self.assertRaisesRegex(OSError, 'Simulated'):
                migrate_console_storage(self.lib)
        self.assertFalse(self.lib.console_layout())
        self.assertEqual(self.lib.get(key)['rom_path'], old)
        self.assertTrue(self.lib.validate_game(key).is_file())
        self.assertTrue((self.lib.root / 'saves' / key / 'battery.srm').is_file())
        self.assertFalse((self.lib.root / 'storage-migration.json').exists())

    def test_startup_recovers_link_created_before_unlink_and_moved_file(self):
        self.lib.set_setting('storage.layout', 'legacy')
        key = self.cartridge()
        source = self.lib.validate_game(key)
        target = self.lib.root / 'consoles/nes/roms/Example.nes'
        target.parent.mkdir(parents=True)
        __import__('os').link(source, target)
        journal = self.lib.root / 'storage-migration.json'
        moves = [{'source': str(source.relative_to(self.lib.root)), 'target': str(target.relative_to(self.lib.root))}]
        write_json(journal, {'format':1, 'id':'interrupted', 'moves':moves})
        self.lib.close(); self.lib = Library(self.root / 'library')
        self.assertTrue(source.is_file()); self.assertFalse(target.exists())
        target.parent.mkdir(parents=True, exist_ok=True)
        source.rename(target)
        write_json(journal, {'format':1, 'id':'interrupted', 'moves':moves})
        self.lib.close(); self.lib = Library(self.root / 'library')
        self.assertTrue(source.is_file()); self.assertFalse(target.exists())
        self.assertTrue(self.lib.validate_game(key).is_file())

    def test_running_game_blocks_migration_without_changing_files(self):
        self.lib.set_setting('storage.layout', 'legacy')
        key = self.cartridge()
        path = self.lib.validate_game(key)
        lock = QLockFile(str(self.lib.root / 'saves' / (key + '.lock')))
        self.assertTrue(lock.tryLock(0))
        try:
            with self.assertRaises(ValueError):
                migrate_console_storage(self.lib)
            self.assertEqual(self.lib.validate_game(key), path)
            self.assertFalse(self.lib.console_layout())
        finally:
            lock.unlock()

    def test_new_media_removal_does_not_touch_other_games(self):
        key = self.cartridge()
        other = self.cartridge('Other.nes', b'other cartridge')
        for current in (key, other):
            folder = self.lib.game_folder('screenshots', current)
            folder.mkdir(parents=True)
            (folder / 'screenshot.png').write_bytes(b'picture')
        plan = removal_plan(self.lib, [key])
        self.assertEqual(len(plan['screenshots']), 1)
        moved = []
        def fake_trash(path):
            moved.append(path); path.unlink()
        with patch('emuluna.file_management.move_to_trash', side_effect=fake_trash):
            remove_games(self.lib, [key], trash_roms=True, screenshots=True)
        self.assertTrue(self.lib.validate_game(other).exists())
        self.assertTrue((self.lib.game_folder('screenshots', other) / 'screenshot.png').exists())

    def test_duplicate_names_migrate_with_battery_name_continuity(self):
        self.lib.set_setting('storage.layout', 'legacy')
        first = self.cartridge('Same.nes', b'first')
        second = self.cartridge('another/Same.nes', b'second')
        for key in (first, second):
            folder = self.lib.root / 'saves' / key / 'libretro/example'
            folder.mkdir(parents=True)
            (folder / 'Same.srm').write_bytes(key.encode())
        migrate_console_storage(self.lib)
        for key in (first, second):
            game = self.lib.get(key)
            folder = self.lib.game_folder('saves', key) / 'libretro/example'
            self.lib.prepare_save_filenames(game, folder)
            self.assertEqual((folder / (Path(game['rom_path']).stem + '.srm')).read_bytes(), key.encode())
        self.assertNotEqual(self.lib.validate_game(first).name, self.lib.validate_game(second).name)

    def test_same_named_multitrack_sets_migrate_to_separate_folders(self):
        self.lib.set_setting('storage.layout', 'legacy')
        first, second = self.disc('First'), self.disc('Second')
        migrate_console_storage(self.lib)
        a, b = self.lib.validate_game(first), self.lib.validate_game(second)
        self.assertNotEqual(a.parent, b.parent)
        self.assertNotEqual((a.parent/'tracks/one.bin').read_bytes(), (b.parent/'tracks/one.bin').read_bytes())

    def test_large_library_migration_under_low_open_file_limit(self):
        # Run in a child so the Qt test runner keeps its normal resource limits.
        import os
        import subprocess
        import sys
        import textwrap
        result = subprocess.run([sys.executable, '-c', textwrap.dedent('''
            import resource
            import tempfile
            from pathlib import Path
            from emuluna.library import Library
            from emuluna.storage_migration import migrate_console_storage
            with tempfile.TemporaryDirectory() as root:
                library = Library(root)
                library.set_setting('storage.layout', 'legacy')
                ids = [library._import_bytes(f'Game {i}.nes', bytes([i]), 'synthetic') for i in range(100)]
                soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
                resource.setrlimit(resource.RLIMIT_NOFILE, (64, hard))
                try:
                    assert migrate_console_storage(library) == 100
                    assert library.console_layout()
                    assert all(library.validate_game(key).is_file() for key in ids)
                    assert not list((library.root / 'saves').glob('*.lock'))
                finally:
                    resource.setrlimit(resource.RLIMIT_NOFILE, (soft, hard))
                    library.close()
        ''')], capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_running_game_message_names_the_game(self):
        self.lib.set_setting('storage.layout', 'legacy')
        key = self.cartridge('My Game.nes')
        lock = QLockFile(str(self.lib.root / 'saves' / (key + '.lock')))
        self.assertTrue(lock.tryLock(0))
        try:
            with self.assertRaisesRegex(ValueError, 'My Game.*organizing'):
                migrate_console_storage(self.lib)
        finally:
            lock.unlock()
