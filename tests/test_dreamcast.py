"""Dreamcast disc safety, Flycast GPU requirements, and physical input mappings."""
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
import tempfile
from pathlib import Path
import unittest
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import QPointF
from PySide6.QtGui import QImage
from PySide6.QtWidgets import QApplication
from emuluna.content import content_files
from emuluna.controller_diagrams import ControllerDiagram
from emuluna.controller_profiles import actions, defaults, gamepad_state
from emuluna.disc_detection import detect_disc
from emuluna.importing import Importer
from emuluna.library import Library, ImportProblem
from emuluna.systems import SYSTEMS, CATALOG, core_launch_options, core_render_options


class DreamcastTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.library = Library(self.root / 'library')

    def tearDown(self):
        self.library.close()
        self.tmp.cleanup()

    def gdi(self):
        folder = self.root / 'Disc'; folder.mkdir()
        data = bytearray(32768)
        data[:16] = b'SEGA SEGAKATANA '
        (folder / 'track 01.bin').write_bytes(data)
        (folder / 'track02.raw').write_bytes(b'audio data')
        (folder / 'track03.bin').write_bytes(b'game data')
        path = folder / 'Test.GDI'
        path.write_text('3\n1 0 4 2048 "track 01.bin" 0\n'
                        '2 450 0 2352 track02.raw 0\n3 45000 4 2352 track03.bin 0\n')
        return path

    def test_gdi_import_preserves_tracks_and_folder_scan_imports_one_game(self):
        path = self.gdi()
        worker = Importer(self.library.root, [path.parent])
        self.assertEqual(worker.scan(self.library, []), [path])
        self.assertEqual(detect_disc(path), 'dreamcast')
        key = self.library.import_file(path)[0]
        game = self.library.get(key)
        self.assertEqual(game['system'], 'dreamcast')
        self.assertEqual(len(self.library.content_rows(key)), 4)
        target = self.library.root / game['rom_path']
        for name, original in content_files(path).items():
            self.assertEqual((target.parent / name).read_bytes(), original.read_bytes())
        self.assertEqual(self.library.import_file(path), [key])
        self.library.validate_game(key)
        (path.parent / 'track03.bin').write_bytes(b'other data')
        self.assertNotEqual(self.library.import_file(path)[0], key)

    def test_gdi_rejects_missing_tracks_bad_rows_and_external_paths(self):
        path = self.gdi()
        text = path.read_text()
        for malformed in ('2\n1 0 4 2048 track.bin 0\n', '1\n1 0 4 1234 track.bin 0\n',
                          '1\n1 0 4 2048 ../escape.bin 0\n',
                          '1\n1 0 4 2048 track.bin -1\n'):
            path.write_text(malformed)
            with self.assertRaises((ValueError, ImportProblem)):
                self.library.import_file(path)
        path.write_text(text)
        (path.parent / 'track03.bin').unlink()
        with self.assertRaisesRegex(ValueError, 'Missing referenced file'):
            self.library.import_file(path)

    def test_gdi_playlist_preserves_full_sets_and_honors_byte_offsets(self):
        path = self.gdi()
        data = (path.parent / 'track 01.bin').read_bytes()
        (path.parent / 'track 01.bin').write_bytes(bytes(2048) + data)
        path.write_text(path.read_text().replace('"track 01.bin" 0', '"track 01.bin" 2048'))
        playlist = self.root / 'Game.m3u'; playlist.write_text('Disc/Test.GDI\n')
        key = self.library.import_file(playlist)[0]
        self.assertEqual(self.library.get(key)['system'], 'dreamcast')
        self.assertEqual(len(self.library.content_rows(key)), 5)
        with self.assertRaisesRegex(ImportProblem, 'identifies as'):
            self.library.import_file(path, system_override='psx')
        path.write_text(path.read_text().replace('"track 01.bin" 2048', '"track 01.bin" 999999'))
        with self.assertRaisesRegex(ImportProblem, 'beyond'):
            self.library.import_file(path)

    def test_cdi_playlist_is_dreamcast_and_chd_choice_includes_dreamcast(self):
        cdi = self.root / 'Homebrew.cdi'; cdi.write_bytes(b'CDI fixture')
        playlist = self.root / 'Homebrew.m3u'; playlist.write_text(cdi.name+'\n')
        key = self.library.import_file(playlist)[0]
        self.assertEqual(self.library.get(key)['system'], 'dreamcast')
        self.assertIn('chd', SYSTEMS['dreamcast'].extensions)

    def test_flycast_requires_gpu_and_defaults_to_per_game_vmus(self):
        self.assertEqual(SYSTEMS['dreamcast'].default_core, 'flycast')
        self.assertTrue(CATALOG['flycast']['hardware_required'])
        self.assertTrue(CATALOG['flycast']['firmware'][0]['optional'])
        options = core_launch_options('flycast', 'dreamcast')
        self.assertEqual(options['reicast_per_content_vmus'], 'All VMUs')
        self.assertEqual(options['reicast_digital_triggers'], 'enabled')
        self.assertEqual(options['reicast_threaded_rendering'], 'disabled')
        self.assertTrue(core_render_options('flycast', (4,6,4,6))[2])
        self.assertTrue(core_render_options('flycast', (0,0,0,0), vulkan_available=True)[2])
        self.assertFalse(core_render_options('flycast', (0,0,0,0))[2])
        self.assertFalse(core_render_options('flycast', (0,0,0,0), vulkan_available=True,
                                           resuming=True, saved_renderer='opengl')[2])
        self.assertFalse(core_render_options('flycast', (4,6,4,6),
                                           resuming=True, saved_renderer='vulkan')[2])

    def test_controller_maps_dreamcast_face_buttons_stick_and_triggers(self):
        labels = dict(actions('dreamcast'))
        self.assertEqual([labels[f'button:{bit}'] for bit in (2,1,2048,1024)], ['A','B','X','Y'])
        profile = defaults('dreamcast', 0)
        bits, axes = gamepad_state(profile['gamepad'], {0,1,2,3}, (16000,-12000,0,0,30000,22000))
        self.assertEqual(bits, 2|1|2048|1024|4096|8192)
        self.assertEqual(axes[:2], (16000,-12000))
        self.assertFalse(any(action.startswith('axis:2:') for action in labels))

    def test_generated_art_and_click_targets_match_physical_controls(self):
        icon = QImage(str(SYSTEMS['dreamcast'].icon))
        self.assertEqual((icon.width(), icon.height()), (32,32))
        self.assertTrue(icon.hasAlphaChannel())
        diagram = ControllerDiagram(); diagram.set_system('dreamcast')
        try:
            self.assertTrue(diagram.renderer.isValid())
            self.assertEqual(diagram.layout_spec['note'], '')
            samples = [(894,464,'button:2'),(984,379,'button:1'),(798,378,'button:2048'),
                       (894,290,'button:1024'),(550,599,'button:8'),(215,97,'button:4096'),
                       (880,97,'button:8192'),(237,473,'button:64'),(180,530,'button:32'),
                       (189,238,'axis:1:-1'),(132,294,'axis:0:-1')]
            for size in ((250,180),(650,450),(1100,750)):
                diagram.resize(*size)
                for x,y,action in samples:
                    point = diagram.artwork_transform().map(QPointF(x,y))
                    self.assertEqual(diagram.action_at(point), action, (size,x,y))
        finally:
            diagram.close()
