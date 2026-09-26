"""Shared menu ordering, preference migration and unchanged emulator lifetime."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtWidgets import QApplication
from emuluna.video_filters import FILTERS,ALL_FILTERS,valid_filter
from emuluna.player import Player
from emuluna.settings import SettingsDialog
from emuluna.library import Library,SYSTEMS
from emuluna.core_manager import CoreWorker,CoreManager,CATALOG
from core_fixture import install_core
from snes_rom import snes


class VideoFilters(unittest.TestCase):
    @classmethod
    def setUpClass(cls):cls.app=QApplication.instance() or QApplication([])

    def test_menu_order_and_preference_migration(self):
        self.assertEqual(list(FILTERS.values()),sorted(FILTERS.values(),key=str.lower))
        self.assertEqual(valid_filter('crt'),'slang:crt-geom')
        self.assertEqual(valid_filter('lcd'),'slang:lcd-psp')
        self.assertEqual(valid_filter('scanlines'),'shader:zfast-crt')
        self.assertEqual(valid_filter('unknown'),'nearest')

    def test_game_window_live_changes_and_default_setting(self):
        with tempfile.TemporaryDirectory() as folder:
            library=Library(folder);rom=Path(folder)/'Video.sfc';rom.write_bytes(snes())
            game=library.import_file(rom)[0];install_core(library)
            with patch.object(Player,'setup_audio'):player=Player(library,game,muted=True)
            player.timer.stop()
            try:
                player.focus_paused=False;handle=player.core.handle;before=player.frames
                for mode in ALL_FILTERS:
                    player.filter_actions[mode].trigger()
                    self.assertEqual(player.screen.video_filter,mode)
                    player.next_frame=0;player.tick()
                self.assertEqual(player.frames,before+len(ALL_FILTERS));self.assertEqual(player.core.handle,handle)
                self.assertEqual(player.filters_menu.actions()[0].text(),'Configure Shader…')
                player.set_video_filter('slang:crt-geom')
            finally:player.close()
            self.assertEqual(library.setting('video_filter.snes'),'slang:crt-geom')
            with patch.object(Player,'setup_audio'):reopened=Player(library,game,muted=True)
            self.assertEqual(reopened.screen.video_filter,'slang:crt-geom');reopened.close()
            dialog=SettingsDialog(library);dialog.show_page('gameplay')
            self.assertEqual(dialog.video_filter.count(),len(ALL_FILTERS))
            dialog.video_filter.setCurrentIndex(dialog.video_filter.findData('slang:lcd-psp'))
            dialog.reset_video_filters()
            self.assertEqual(library.setting('video_filter'),'slang:lcd-psp')
            self.assertEqual(library.setting('video_filter.snes'),'')
            dialog.close();library.close()

    def test_install_all_covers_every_console_and_continues_after_failure(self):
        with tempfile.TemporaryDirectory() as folder:
            library=Library(folder);worker=CoreWorker(library.root,install_all=True)
            results=[];worker.result.connect(lambda message,success:results.append((message,success)))
            downloaded=[]
            def download(core_id,downloads,progress,index):
                downloaded.append(core_id);progress('Downloading')
                if core_id=='gambatte':raise RuntimeError('Temporary server error')
                return {},True
            with patch.object(CoreManager,'remote_index',return_value={}),patch.object(CoreManager,'download',side_effect=download):worker.run()
            self.assertEqual(set(downloaded),set(CATALOG))
            self.assertEqual(set(SYSTEMS),{system for core in downloaded for system in CATALOG[core]['systems']})
            self.assertFalse(results[-1][1]);self.assertIn('Temporary server error',results[-1][0])
            dialog=SettingsDialog(library);dialog.show_page('cores')
            with patch.object(dialog,'start_job') as start:
                dialog.install_all_button.click();start.assert_called_once_with(install_all=True)
            dialog.close();library.close()
