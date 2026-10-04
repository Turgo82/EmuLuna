"""Synthetic disc headers and complete-set import; no commercial game data."""
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from pathlib import Path
import tempfile
import unittest
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtWidgets import QApplication
from PySide6.QtCore import Qt
from emuluna.library import Library, ImportProblem
from emuluna.disc_detection import detect_disc
from emuluna.importing import Importer, ImportIssuesDialog


def image(system, raw=False):
    stride, skip = (2352,24) if raw else (2048,0)
    data = bytearray(stride * 32)
    signatures = {'segacd': b'SEGADISCSYSTEM   ', 'saturn':b'SEGA SEGASATURN  '}
    if system in signatures:
        data[skip:skip+len(signatures[system])] = signatures[system]
    elif system == 'pcecd':
        marker = b'PC Engine CD-ROM SYSTEM'
        data[5000:5000+len(marker)] = marker
    else:
        base = stride * 16 + skip
        data[base:base+7] = b'\x01CD001\x01'
        label = b'PSP GAME' if system == 'psp' else b'PLAYSTATION'
        data[base+8:base+40] = label.ljust(32,b' ')
        if system in ('psx','ps2'):
            start = 0x24e0 if raw else 0x2008
            data[start:start+16] = b'  Licensed  by  '
        if system == 'ps2':
            data[50000:50005] = b'BOOT2'
    return data


class DiscIdentification(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.lib = Library(self.root/'library')

    def tearDown(self):
        self.lib.close(); self.tmp.cleanup()

    def cue(self, system, pregap=False):
        folder=self.root/system; folder.mkdir(exist_ok=True)
        raw=image(system,True)
        offset=2352*150 if pregap else 0
        (folder/'Track 01.BIN').write_bytes(bytes(offset)+raw)
        (folder/'Track 02.bin').write_bytes(b'audio' * 500)
        cue=folder/'Original Name.CUE'
        cue.write_text('FILE "Track 01.BIN" BINARY\nTRACK 01 MODE2/2352\nINDEX 01 '+('00:02:00' if pregap else '00:00:00')+'\nFILE "Track 02.bin" BINARY\nTRACK 02 AUDIO\nINDEX 01 00:00:00\n')
        return cue

    def test_identifies_data_tracks_preserves_audio_and_promotes_bin(self):
        for system in ('psx','saturn','segacd','pcecd'):
            cue=self.cue(system,pregap=True)
            self.assertEqual(detect_disc(cue),system)
            key=self.lib.import_file(cue.parent/'Track 01.BIN')[0]
            game=self.lib.get(key)
            self.assertEqual(game['system'],system)
            self.assertEqual(Path(game['rom_path']).name, cue.name)
            self.assertEqual(len(self.lib.content_rows(key)),3)
            self.assertEqual(self.lib.import_file(cue),[key])
            self.assertEqual((self.lib.root/game['rom_path']).read_bytes(),cue.read_bytes())
            self.assertTrue((cue.parent/'Track 02.bin').is_file())
        worker=Importer(self.lib.root,[self.root/'psx'])
        self.assertEqual(worker.scan(self.lib,[]),[self.root/'psx'/'Original Name.CUE'])

    def test_iso_signatures_not_names_and_conflicting_override(self):
        for system in ('psx','psp','segacd'):
            iso=self.root/(system+'.ISO');iso.write_bytes(image(system))
            key=self.lib.import_file(iso)[0]
            self.assertEqual(self.lib.get(key)['system'],system)
        with self.assertRaisesRegex(ImportProblem,'identifies as'):
            self.lib.import_file(self.root/'psp.ISO',system_override='psx')
        fake=self.root/'PlayStation game.iso';fake.write_bytes(image('ps2'))
        with self.assertRaises(ImportProblem) as error:self.lib.import_file(fake)
        self.assertEqual(error.exception.code,'unknown_disc')
        missing=self.root/'Missing cue.bin';missing.write_bytes(image('psx',True))
        with self.assertRaises(ImportProblem) as error:self.lib.import_file(missing)
        self.assertEqual(error.exception.code,'missing_cue')

    def test_playlist_detects_console_and_rejects_mixed_consoles_or_missing_track(self):
        first=self.cue('psx');second=self.cue('saturn')
        playlist=self.root/'Set.m3u';playlist.write_text('psx/Original Name.CUE\n')
        key=self.lib.import_file(playlist)[0]
        self.assertEqual(self.lib.get(key)['system'],'psx')
        self.assertEqual(len(self.lib.content_rows(key)),4)
        playlist.write_text('psx/Original Name.CUE\nsaturn/Original Name.CUE\n')
        with self.assertRaisesRegex(ImportProblem,'different consoles'):self.lib.import_file(playlist)
        (second.parent/'Track 02.bin').unlink()
        with self.assertRaisesRegex(ImportProblem,'Missing referenced file'):self.lib.import_file(second)

    def test_chd_uses_explicit_console_and_batch_resolver(self):
        # Header-only containers exercise import choice, never emulator loading.
        paths=[]
        for number in range(2):
            path=self.root/f'Disc {number}.CHD'
            data=bytearray(256);data[:8]=b'MComprHD';data[8:12]=(124).to_bytes(4,'big');data[12:16]=(5).to_bytes(4,'big');data[-1]=number
            path.write_bytes(data);paths.append(path)
        worker=Importer(self.lib.root,paths);worker.run()
        self.assertEqual(worker.new_games,0)
        self.assertEqual({r['code'] for r in self.lib.import_issues()},{'unknown_disc'})
        dialog=ImportIssuesDialog(self.lib);dialog.table.selectAll()
        self.assertEqual({dialog.system.itemData(i) for i in range(1,dialog.system.count())},{'psx','segacd','saturn','pcecd','dreamcast'})
        emitted=[];dialog.retry_many.connect(lambda files,system:emitted.append((files,system)))
        dialog.retry_selected();self.assertEqual(emitted,[])
        dialog.system.setCurrentIndex(dialog.system.findData('psx'));dialog.retry_selected()
        self.assertEqual(set(emitted[0][0]),set(map(str,paths)))
        self.assertEqual(emitted[0][1],'psx')
        Importer(self.lib.root,emitted[0][0],system_override=emitted[0][1]).run()
        self.assertEqual(len(self.lib.games()),2)
        self.assertEqual(self.lib.import_issues(),[])
        dialog.close()
        bad=self.root/'bad.chd';bad.write_bytes(b'Not a CHD')
        with self.assertRaisesRegex(ImportProblem,'Invalid CHD'):self.lib.import_file(bad,system_override='psx')
