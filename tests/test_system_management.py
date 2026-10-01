"""Core lifecycle, BIOS validation and complete disc-import regression checks."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication
from PySide6.QtWidgets import QMenu
from emuluna.bios import bios_status, import_bios, validate_bios
from emuluna.content import inventory
from emuluna.core import Core, CoreError
from emuluna.core_manager import CoreManager, DefaultCoreWorker
from emuluna.importing import Importer
from emuluna.library import Library, ImportProblem
from emuluna.settings import SettingsDialog
from emuluna.app import Window
from emuluna.systems import SYSTEMS, CATALOG, EXTENSIONS, core_launch_options, core_render_options
from test_core_manager import binary


class SystemManagementTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.library = Library(self.root / 'library')
        self.manager = CoreManager(self.library.root)

    def tearDown(self):
        self.library.close()
        self.tmp.cleanup()

    def test_all_systems_sorted_icons_defaults_and_flat_catalog(self):
        names = [system.name for system in SYSTEMS.values()]
        self.assertEqual(names, sorted(names, key=str.casefold))
        self.assertEqual(len(SYSTEMS), 32)
        for key, system in SYSTEMS.items():
            self.assertTrue(system.icon.is_file(), key)
            self.assertIn(key, CATALOG[system.default_core]['systems'])
        self.assertIsNone(EXTENSIONS['.bin'])
        self.assertIsNone(EXTENSIONS['.cue'])
        dialog = SettingsDialog(self.library)
        dialog.show_page('cores')
        self.assertEqual(dialog.table.rowCount(), len(CATALOG))
        core_rows = [(dialog.table.item(row, 0).text().split(' / ')[0].casefold(),
                      dialog.table.item(row, 1).text().casefold())
                     for row in range(dialog.table.rowCount())]
        self.assertEqual(core_rows, sorted(core_rows))
        self.assertFalse(hasattr(dialog, 'choices'))
        self.assertFalse(hasattr(dialog, 'core_pages'))
        dialog.show_page('bios')
        dialog.bios_system.setCurrentIndex(dialog.bios_system.findData('colecovision'))
        self.assertEqual(dialog.bios_table.rowCount(), 1)
        self.assertEqual(dialog.bios_table.item(0, 3).text(), 'Missing')
        dialog.close()

    def test_vectrex_uses_software_rendering_and_is_launchable(self):
        self.assertEqual(CATALOG['vecx']['options']['vecx_use_hw'], 'Software')
        self.assertNotIn('launch_block', CATALOG['vecx'])
        self.manager.validate_launch({'id':'vecx', 'catalog_id':'vecx'}, 'vectrex',
                                     self.root / 'game.vec', self.root / 'system')

    def test_genesis_uses_model_one_low_pass_without_affecting_shared_systems(self):
        genesis = core_launch_options('genesis_plus_gx', 'genesis')
        self.assertEqual(genesis['genesis_plus_gx_audio_filter'], 'low-pass')
        self.assertEqual(genesis['genesis_plus_gx_lowpass_range'], '60')
        self.assertNotIn('genesis_plus_gx_audio_filter',
                         core_launch_options('genesis_plus_gx', 'sms'))

    def test_turbografx_starts_with_standard_two_button_pads(self):
        for system in ('pce', 'pcecd'):
            options = core_launch_options('mednafen_pce_fast', system)
            self.assertEqual(options['pce_fast_default_joypad_type_p1'], '2 Buttons')
            self.assertEqual(options['pce_fast_default_joypad_type_p2'], '2 Buttons')
            self.assertEqual(options['pce_fast_last_scanline'], '227')

    def test_catalog_has_no_artificial_launch_blocks(self):
        blocked = {key: info['launch_block'] for key, info in CATALOG.items()
                   if info.get('launch_block')}
        self.assertEqual(blocked, {})
        self.assertEqual(CATALOG['ppsspp']['options'], {'ppsspp_internal_resolution': '480x272'})
        with patch('emuluna.bios.validate_bios'):
            self.manager.validate_launch({'id':'ppsspp', 'catalog_id':'ppsspp'}, 'psp',
                                         self.root / 'game.iso', self.root / 'system')

    def test_playstation_beetle_variants_are_selectable_without_changing_default(self):
        self.assertEqual(SYSTEMS['psx'].default_core, 'pcsx_rearmed')
        for core_id in ('mednafen_psx', 'mednafen_psx_hw'):
            self.assertIn('psx', CATALOG[core_id]['systems'])
            self.assertIn('cue', CATALOG[core_id]['extensions'])
        with patch('emuluna.core_manager.probe', return_value={
            'name': 'Beetle PSX HW', 'version': 'test', 'extensions': 'cue|chd|pbp'}):
            installed = self.manager.install_bytes(binary(), 'mednafen_psx_hw')
        self.library.set_setting('core.psx', 'mednafen_psx_hw')
        self.assertEqual(self.manager.choice(self.library, 'psx')['sha256'], installed['sha256'])
        window = Window(self.library, auto_artwork=False)
        try:
            choices = window.add_console_core_menu(QMenu(window), 'psx')
            self.assertTrue(next(action for action in choices.actions()
                                 if action.data() == 'mednafen_psx_hw').isChecked())
            downloads = next(action.menu() for action in choices.actions()
                             if action.text() == 'Available to download')
            self.assertEqual({action.text() for action in downloads.actions()},
                             {'Beetle PSX…', 'PCSX ReARMed…'})
        finally:
            window.close()

    def test_3d_core_rendering_uses_available_gl_and_preserves_software_states(self):
        full_gl = (4, 6, 4, 6)
        no_gl = (0, 0, 0, 0)
        for core_id, key, hardware_value, software_value in (
                ('mednafen_psx_hw', 'beetle_psx_hw_renderer', 'hardware_gl', 'software'),
                ('ppsspp', 'ppsspp_backend', 'opengl', 'none'),
                ('desmume', 'desmume_opengl_mode', 'enabled', 'disabled')):
            options, available, using = core_render_options(core_id, full_gl)
            self.assertTrue(available and using)
            self.assertEqual(options[key], hardware_value)
            options, available, using = core_render_options(
                core_id, full_gl, resuming=True, saved_renderer='software')
            self.assertTrue(available and not using)
            self.assertEqual(options[key], software_value)
            options, available, using = core_render_options(core_id, no_gl)
            self.assertFalse(available or using)
            self.assertEqual(options[key], software_value)
        self.assertFalse(core_render_options('mednafen_psx_hw', (4, 6, 0, 0))[1])

    def test_default_download_only_missing_cores_and_removal_keeps_games_saves(self):
        self.assertEqual(self.manager.missing_defaults(self.library, ['gb','gbc','nes']), ['gambatte','nestopia'])
        with patch('emuluna.core_manager.probe', return_value={'name':'Gambatte','version':'test','extensions':'gb|gbc'}):
            first = self.manager.install_bytes(binary(), 'gambatte')
        self.assertEqual(self.manager.missing_defaults(self.library, ['gb','gbc']), [])
        self.library.set_setting('core.gb', 'gambatte')
        marker = self.library.root / 'saves/progress'; marker.write_bytes(b'keep')
        path = self.manager.path(first)
        self.assertTrue(self.manager.remove('gambatte', self.library))
        self.assertFalse(path.exists())
        self.assertEqual(marker.read_bytes(), b'keep')
        self.assertEqual(self.library.setting('core.gb'), 'auto')
        self.assertEqual(self.manager.missing_defaults(self.library, ['gb','gbc']), [])
        self.library.set_setting('core.removed.gambatte', 0)
        self.assertEqual(self.manager.missing_defaults(self.library, ['gb','gbc']), ['gambatte'])
        self.library.set_setting('core_auto_install', 0)
        self.assertEqual(self.manager.missing_defaults(self.library, ['nes']), [])

    def test_auto_download_worker_installs_once_for_shared_systems(self):
        def download(manager, core_id, *args):
            record = manager.install_bytes(binary(), core_id, systems=['gb','gbc'])
            return record, True
        worker = DefaultCoreWorker(self.library.root, ['gb','gbc'])
        results=[]; worker.result.connect(lambda *args: results.append(args))
        with patch.object(CoreManager,'remote_index',return_value={}), patch.object(CoreManager,'download',autospec=True,side_effect=download) as fetch, patch('emuluna.core_manager.probe',return_value={'name':'Gambatte','version':'test','extensions':'gb|gbc'}):
            worker.run()
        self.assertEqual(fetch.call_count, 1)
        self.assertTrue(results[-1][1])
        self.assertEqual(self.manager.selection(self.library, 'gb')['id'], 'gambatte')

    def test_core_download_serializes_metadata_and_artwork_without_gui_loop(self):
        window = Window(self.library, auto_artwork=False)
        try:
            # This is the state reached when an import starts a missing-core
            # download just before the current metadata worker finishes.
            sentinel = object()
            window.core_worker = sentinel
            metadata = {'force': False, 'game_ids': None}
            artwork = {'force': False, 'game_ids': None, 'replace': False}
            window.metadata_pending.append(metadata)
            window.art_pending.append(artwork)
            window.drain_lookups()
            self.assertEqual(window.metadata_pending, [metadata])
            self.assertEqual(window.art_pending, [artwork])
        finally:
            window.core_worker = None
            window.close()

    def test_bios_check_import_integrity_originals_and_no_overwrite(self):
        payload=b'firmware fixture'
        source=self.root/'user.bin'; source.write_bytes(payload)
        entry={'path':'nested/required.bin','md5':hashlib.md5(payload).hexdigest(),'optional':False}
        folder=self.root/'bios'
        self.assertEqual(bios_status(folder,entry), ('Missing',False))
        target=import_bios(source,folder,entry)
        self.assertEqual(source.read_bytes(),payload)
        self.assertEqual(target.name,'required.bin')
        self.assertEqual(bios_status(folder,entry),('Verified',True))
        source.write_bytes(b'wrong')
        with self.assertRaises(CoreError): import_bios(source,folder,entry)
        self.assertEqual(target.read_bytes(),payload)
        target.write_bytes(b'corrupt')
        self.assertEqual(bios_status(folder,entry),('Checksum mismatch',False))
        with self.assertRaises(CoreError): validate_bios('gearcoleco','colecovision',folder)
        validate_bios('nestopia','nes',folder)
        with self.assertRaises(CoreError): validate_bios('nestopia','fds',folder)
        with self.assertRaises(CoreError): import_bios(source,folder,{'path':'../escape','optional':True})

    def disc(self):
        folder=self.root/'My Disc'; folder.mkdir()
        (folder/'Track 01.bin').write_bytes(b'disc data'*300)
        cue=folder/'Original Name.CUE'
        cue.write_text('FILE "Track 01.bin" BINARY\n TRACK 01 MODE2/2352\n INDEX 01 00:00:00\n')
        return cue

    def test_disc_copies_tracks_preserves_names_detects_changes_and_duplicates(self):
        cue=self.disc()
        with self.assertRaises(ImportProblem): self.library.import_file(cue)
        game_id=self.library.import_file(cue,system_override='psx')[0]
        self.assertEqual(self.library.import_file(cue,system_override='psx'),[game_id])
        game=self.library.get(game_id)
        dest=self.library.root/game['rom_path']
        self.assertEqual(dest.name,cue.name)
        self.assertEqual(dest.read_bytes(),cue.read_bytes())
        self.assertEqual((dest.parent/'Track 01.bin').read_bytes(),(cue.parent/'Track 01.bin').read_bytes())
        self.assertEqual(self.library.validate_game(game_id),dest)
        (dest.parent/'Track 01.bin').write_bytes(b'changed')
        with self.assertRaises(ValueError): self.library.validate_game(game_id)
        self.assertTrue(cue.exists())
        # Identical cue text with different track data must not deduplicate.
        (cue.parent/'Track 01.bin').write_bytes(b'another disc')
        other=self.library.import_file(cue,system_override='psx')[0]
        self.assertNotEqual(other,game_id)

    def test_playlist_folder_scan_consolidation_and_invalid_references(self):
        cue=self.disc()
        playlist=cue.parent/'Two discs.m3u'; playlist.write_text(cue.name+'\n')
        worker=Importer(self.library.root,[cue.parent])
        self.assertEqual(worker.scan(self.library,[]),[playlist])
        self.library.set_setting('copy_games',0)
        game_id=self.library.import_file(playlist,system_override='psx')[0]
        self.assertTrue(Path(self.library.get(game_id)['rom_path']).is_absolute())
        self.assertTrue(self.library.consolidate(game_id))
        self.assertFalse(Path(self.library.get(game_id)['rom_path']).is_absolute())
        self.library.validate_game(game_id)
        playlist.write_text('Two discs.m3u\n')
        with self.assertRaisesRegex(ValueError,'cycle'): inventory(playlist)
        cue.write_text('FILE "../outside.bin" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\n')
        with self.assertRaises(ValueError): inventory(cue)

    def test_removed_openemu_adapter_cannot_be_launched(self):
        with self.assertRaisesRegex(CoreError,'standard libretro'):
            Core(self.root/'game.nes','nes',self.root/'save')
