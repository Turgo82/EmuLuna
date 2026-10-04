"""Restore missing managed disc files without replacing surviving content."""
from pathlib import Path
import tempfile
import unittest
from emuluna.content import copy_content, inventory


class DiscCopyRecovery(unittest.TestCase):
    def test_partial_set_restores_nested_track_and_preserves_existing_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'source'
            source.mkdir()
            (source / 'tracks').mkdir()
            cue = source / 'game.cue'
            cue.write_text('FILE "tracks/one.bin" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\n')
            (source / 'tracks/one.bin').write_bytes(b'Original synthetic track')
            rows = inventory(cue)
            destination = root / 'library/game'
            copy_content(cue, destination, rows)
            surviving = destination / cue.name
            old_inode = surviving.stat().st_ino
            (destination / 'tracks/one.bin').unlink()
            (destination / 'notes.txt').write_text('Keep this')
            copy_content(cue, destination, rows)
            self.assertEqual(surviving.stat().st_ino, old_inode)
            self.assertEqual((destination / 'tracks/one.bin').read_bytes(), b'Original synthetic track')
            self.assertEqual((destination / 'notes.txt').read_text(), 'Keep this')

    def test_mismatching_existing_copy_prevents_any_restoration(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            cue = root / 'game.cue'
            cue.write_text('FILE "one.bin" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\n')
            (root / 'one.bin').write_bytes(b'Synthetic track')
            rows = inventory(cue)
            destination = root / 'library/game'
            copy_content(cue, destination, rows)
            (destination / 'game.cue').unlink()
            (destination / 'one.bin').write_bytes(b'Different data')
            with self.assertRaisesRegex(ValueError, 'existing managed disc copy differs'):
                copy_content(cue, destination, rows)
            self.assertFalse((destination / 'game.cue').exists())
            self.assertEqual((destination / 'one.bin').read_bytes(), b'Different data')
