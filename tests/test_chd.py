"""Decoded synthetic discs exercise the shipped reader without game data."""
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zlib

from media_stub import isolate_audio
isolate_audio()
from PySide6.QtWidgets import QApplication
from emuluna.chd import track_samples
from emuluna.disc_detection import detect_disc
from emuluna.library import Library, ImportProblem
from emuluna.file_management import remove_games
from emuluna.importing import Importer
from test_disc_detection import image

READER = Path(__file__).resolve().parents[1] / 'build/cores/emuluna-chd-probe'


def synthetic_chd(path, tracks, compressed=False, version=5, gd=False):
    """Create real v5 raw or v3/v4 DEFLATE containers with synthetic sectors.

    tracks = (sector bytes, mode, stored pregap frames); metadata and map follow
    libchdr's documented CHD layout. Each track is padded to four stored frames.
    """
    hunk_bytes = 4 * 2448
    payload, metadata = bytearray(), []
    strides = {'AUDIO': 2352, 'MODE1_RAW': 2352, 'MODE2_RAW': 2352, 'MODE1': 2048}
    for number, (sectors, mode, pregap) in enumerate(tracks, 1):
        stride = strides[mode]
        assert len(sectors) % stride == 0
        frames = len(sectors) // stride
        for start in range(0, len(sectors), stride):
            payload.extend(sectors[start:start + stride])
            payload.extend(bytes(2448 - stride))
        payload.extend(bytes((-frames % 4) * 2448))
        meta = (f'TRACK:{number} TYPE:{mode} SUBTYPE:NONE FRAMES:{frames} '
                + ('PAD:0 ' if gd else '')
                + f'PREGAP:{pregap} PGTYPE:{"V" + mode if pregap else mode} PGSUB:NONE POSTGAP:0\0')
        metadata.append(meta.encode())
    hunks = len(payload) // hunk_bytes
    header_size = {3: 120, 4: 108, 5: 124}[version]
    header = bytearray(header_size)
    header[:8] = b'MComprHD'
    struct.pack_into('>II', header, 8, header_size, version)
    if compressed:
        assert version in (3, 4)
        encoded = []
        for offset in range(0, len(payload), hunk_bytes):
            encoder = zlib.compressobj(wbits=-15)
            encoded.append(encoder.compress(payload[offset:offset + hunk_bytes]) + encoder.flush())
        position = header_size + (hunks + 1) * 16
        mapping = bytearray()
        for chunk in encoded:
            # Legacy map length is stored as big-endian low 16 bits + high byte.
            mapping.extend(struct.pack('>QIHB', position, 0, len(chunk) & 65535, len(chunk) >> 16) + b'\x11')
            position += len(chunk)
        mapping.extend(b'EndOfListCookie\0')
        struct.pack_into('>IIQQ', header, 20, 1, hunks, len(payload), position)
        struct.pack_into('>I', header, 76 if version == 3 else 44, hunk_bytes)
        container = header + mapping + b''.join(encoded)
        meta_start = position
    else:
        assert version == 5
        meta_start = header_size + hunks * 4
        struct.pack_into('>QQQII', header, 32, len(payload), header_size, meta_start, hunk_bytes, 2448)
        mapping = b''.join(struct.pack('>I', n + 1) for n in range(hunks))
        container = header + mapping
    for index, meta in enumerate(metadata):
        next_offset = len(container) + 16 + len(meta) if index + 1 < len(metadata) else 0
        container.extend((b'CHGD' if gd else b'CHT2') + struct.pack('>IQ', len(meta), next_offset) + meta)
    if not compressed:
        assert len(container) < hunk_bytes
        container.extend(bytes(hunk_bytes - len(container)))
        container.extend(payload)
    path.write_bytes(container)
    return path


@unittest.skipUnless(READER.is_file(), 'Build the bundled CHD reader first')
class CHDIdentification(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_decodes_all_supported_console_signatures(self):
        for system in ('psx', 'segacd', 'saturn', 'pcecd', 'psp', 'dreamcast'):
            with self.subTest(system=system):
                data = image(system, True) if system != 'dreamcast' else bytearray(32 * 2352)
                if system == 'dreamcast':
                    data[16:32] = b'SEGA SEGAKATANA '
                path = synthetic_chd(self.root / (system + '.chd'), [(data, 'MODE2_RAW', 0)])
                self.assertEqual(detect_disc(path), system)
                self.assertEqual(track_samples(path), [data])

    def test_compressed_legacy_versions_and_cooked_sectors(self):
        for version in (3, 4):
            with self.subTest(version=version):
                data = image('psx')
                path = synthetic_chd(self.root / f'v{version}.chd', [(data, 'MODE1', 0)],
                                     compressed=True, version=version)
                self.assertEqual(track_samples(path), [data])
                self.assertEqual(detect_disc(path), 'psx')

    def test_skips_audio_padding_and_stored_pregap_in_gdrom(self):
        boot = bytearray(32 * 2352)
        boot[16:32] = b'SEGA SEGAKATANA '
        misleading_audio = image('psx', True) + bytes(2352)
        path = synthetic_chd(self.root / 'gdrom.chd', [
            (misleading_audio, 'AUDIO', 0),
            (bytes(2352 * 3), 'MODE1_RAW', 0),
            (bytes(2352 * 2) + boot, 'MODE1_RAW', 2),
        ], gd=True)
        self.assertEqual(track_samples(path), [bytes(2352 * 3), boot])
        self.assertEqual(detect_disc(path), 'dreamcast')

    def test_import_assigns_console_and_rejects_conflicting_choice(self):
        # The name deliberately suggests a different console.
        path = synthetic_chd(self.root / 'Saturn game.chd', [(image('psx', True), 'MODE2_RAW', 0)])
        lib = Library(self.root / 'library')
        try:
            key = lib.import_file(path)[0]
            self.assertEqual(lib.get(key)['system'], 'psx')
            with self.assertRaisesRegex(ImportProblem, 'identifies as'):
                lib.import_file(path, system_override='saturn')
        finally:
            lib.close()

    def test_reimport_after_trashing_managed_chd_restores_copy_and_clears_issue(self):
        path = synthetic_chd(self.root / "Tony Hawk's Pro Skater 2 (USA).chd",
                             [(image('psx', True), 'MODE2_RAW', 0)])
        lib = Library(self.root / 'library')
        try:
            key = lib.import_file(path)[0]
            managed = lib.root / lib.get(key)['rom_path']
            with patch('emuluna.file_management.move_to_trash',
                       side_effect=lambda file: file.rename(self.root / 'trashed.chd')):
                remove_games(lib, [key], trash_roms=True)
            self.assertFalse(managed.parent.exists())
            self.assertFalse(managed.exists())
            self.assertTrue(path.is_file())
            # Also recover folders left behind by previous versions.
            managed.parent.mkdir()
            lib.record_import_issue(path, 'missing_file', 'Previous failed re-import')
            worker = Importer(lib.root, [path])
            worker.run()
            self.assertIsNone(worker.error)
            self.assertEqual(worker.new_games, 1)
            self.assertEqual(lib.import_issues(), [])
            self.assertEqual(lib.get(key)['system'], 'psx')
            self.assertEqual(managed.read_bytes(), path.read_bytes())
            self.assertEqual((self.root / 'trashed.chd').read_bytes(), path.read_bytes())
        finally:
            lib.close()

    def test_playlist_detects_and_rejects_mixed_consoles(self):
        synthetic_chd(self.root / 'one.chd', [(image('psx', True), 'MODE2_RAW', 0)])
        synthetic_chd(self.root / 'two.chd', [(image('saturn', True), 'MODE1_RAW', 0)])
        playlist = self.root / 'set.m3u'
        playlist.write_text('one.chd\n')
        self.assertEqual(detect_disc(playlist), 'psx')
        playlist.write_text('one.chd\ntwo.chd\n')
        with self.assertRaisesRegex(ValueError, 'different consoles'):
            detect_disc(playlist)

    def test_unknown_damaged_and_unsupported_containers_keep_manual_choice(self):
        path = synthetic_chd(self.root / 'unknown.chd', [(bytes(32 * 2352), 'MODE1_RAW', 0)])
        self.assertIsNone(detect_disc(path))
        lib = Library(self.root / 'library')
        try:
            with self.assertRaises(ImportProblem) as error:
                lib.import_file(path)
            self.assertEqual(error.exception.code, 'unknown_disc')
            key = lib.import_file(path, system_override='saturn')[0]
            self.assertEqual(lib.get(key)['system'], 'saturn')
        finally:
            lib.close()
        path.write_bytes(path.read_bytes()[:124])
        self.assertEqual(track_samples(path), [])
        oversized = bytearray(path.read_bytes())
        struct.pack_into('>I', oversized, 56, 2 * 1024 * 1024)
        path.write_bytes(oversized)
        self.assertEqual(track_samples(path), [])


class ReaderFallback(unittest.TestCase):
    def test_unavailable_timeout_decoder_failure_and_invalid_protocol(self):
        with patch.dict(os.environ, {'EMULUNA_CORE_DIR': '/nonexistent/emuluna-test-reader'}):
            self.assertEqual(track_samples('anything.chd'), [])
        with patch('emuluna.chd.Path.is_file', return_value=True):
            with patch('emuluna.chd.subprocess.run', side_effect=subprocess.TimeoutExpired('probe', 8)):
                self.assertEqual(track_samples('anything.chd'), [])
            for code, output in ((2, b''), (0, b'incomplete'), (0, struct.pack('>II', 2048, 2048)),
                                 (0, struct.pack('>II', 1, 1) + b'x')):
                with patch('emuluna.chd.subprocess.run', return_value=subprocess.CompletedProcess([], code, output)):
                    self.assertEqual(track_samples('anything.chd'), [])
