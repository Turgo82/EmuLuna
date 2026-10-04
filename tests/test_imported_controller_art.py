"""Imported physical controls select the expected bindings, including hidden ones."""
import hashlib
import json
import os
import tempfile
import unittest

os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import QPointF, QRectF
from PySide6.QtGui import QImage
from PySide6.QtWidgets import QApplication, QLabel
from emuluna.controller_diagrams import ART, ControllerDiagram, layouts, raster_image
from emuluna.controller_profiles import SPECS, actions
from emuluna.library import Library
from emuluna.settings import SettingsDialog


class ImportedControllerArt(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([])

    def test_all_matching_art_is_shipped_with_credit_and_bounded_dimensions(self):
        imported=json.loads((ART/'imported_layouts.json').read_text())
        manifest=json.loads((ART/'artwork_manifest.json').read_text())
        fallback={'atari5200','odyssey2','sg1000','ngp','ws'}
        diagrams={spec['diagram'] for spec in SPECS.values()}
        self.assertEqual(set(imported),diagrams-fallback-{'dreamcast'})
        self.assertEqual(set(imported),set(manifest))
        self.assertEqual(sum(spec['diagram'] in imported for spec in SPECS.values()),27)
        for key,record in imported.items():
            path=ART/record['image']
            self.assertTrue(path.is_file(),key)
            image=QImage(str(path))
            self.assertFalse(image.isNull(),key)
            self.assertTrue(image.hasAlphaChannel(),key)
            self.assertLessEqual(max(image.width(),image.height()),1100,key)
            self.assertEqual(record['size'],manifest[key]['source_size'])
            self.assertEqual(manifest[key]['artist'],'Pinapple_Graphics')
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),manifest[key]['image_sha256'])
        for key in fallback:
            self.assertNotIn('image',layouts()[key])

    def test_physical_button_locations_and_stick_clicks_across_sizes(self):
        # Independently recorded positions from the source illustrations, rather
        # than deriving expected actions from the hit regions under test.
        samples={
            'psx':[(1117,313,'button:1'),(1021,409,'button:2'),(454,505,'button:16384'),
                   (831,505,'button:32768'),(454,456,'axis:1:-1'),(880,505,'axis:2:1')],
            'n64':[(877,462,'button:2'),(797,382,'button:2048'),(966,246,'axis:3:-1'),
                   (899,312,'axis:2:-1'),(578,590,'axis:1:-1')],
            'pce':[(884,439,'button:2'),(1019,439,'button:1'),(666,437,'button:8')],
            'vb':[(719,296,'button:2'),(812,251,'button:1'),(962,124,'button:4096'),(905,181,'button:8192')],
            'gb':[(416,685,'button:2'),(514,641,'button:1'),(216,813,'button:4')],
            'gbc':[(403,657,'button:2'),(509,622,'button:1')],
            'gba':[(963,295,'button:2'),(1069,257,'button:1'),(231,439,'button:8'),(137,47,'button:512')],
            'nds':[(897,672,'button:1024'),(956,730,'button:1'),(823,977,'button:4')],
            'psp':[(1199,229,'button:1'),(1125,303,'button:2'),(141,374,'axis:1:-1')],
            'gg':[(1023,305,'button:2'),(1115,235,'button:1'),(1037,143,'button:8')],
            'lynx':[(1026,86,'button:2'),(1026,394,'button:2'),(1108,73,'button:1'),(1108,407,'button:1')],
            'ngpc':[(896,283,'button:1'),(1000,211,'button:2')],
            'wsc':[(145,113,'button:8192'),(145,233,'button:4096'),(89,473,'button:32'),(1024,458,'button:1')],
            'sms':[(775,285,'button:2'),(961,285,'button:1')],
            'atari2600':[(144,144,'button:2'),(417,343,'button:64')],
            'atari7800':[(547,370,'button:2'),(792,370,'button:1')],
            'colecovision':[(101,524,'button:2048'),(326,752,'axis:1:1'),(214,865,'axis:0:1'),(326,865,'button:4')],
            'intellivision':[(244,341,'button:32768'),(140,545,'button:4096'),(348,545,'button:8192'),(244,759,'axis:1:-1')],
            'vectrex':[(510,196,'button:1'),(649,196,'button:2'),(788,196,'button:1024'),(928,196,'button:2048')],
        }
        diagram=ControllerDiagram()
        try:
            for system,points in samples.items():
                diagram.set_system(system)
                for size in ((250,180),(650,450),(1100,750)):
                    diagram.resize(*size)
                    for x,y,action in points:
                        self.assertEqual(diagram.action_at(diagram.artwork_transform().map(QPointF(x,y))),
                                         action,(system,size,x,y))
        finally: diagram.close()

    def test_hidden_triggers_and_advanced_controls_remain_configurable(self):
        with tempfile.TemporaryDirectory() as tmp:
            library=Library(tmp)
            dialog=SettingsDialog(library)
            try:
                dialog.show_page('controls');page=dialog.controls_page
                for system,bits,group in (
                    ('psx',(4096,8192),'Shoulders & triggers'),('n64',(4096,),'Shoulders & triggers'),
                    ('nds',(512,256),'Shoulders & triggers'),('vb',(512,256),'Shoulders & triggers'),
                    ('pce',(2048,1024,512,256,4096),'Six-button pad'),
                    ('pcecd',(2048,1024,512,256,4096),'Six-button pad')):
                    page.system.setCurrentIndex(page.system.findData(system))
                    self.assertEqual(set(page.mapping_buttons),set(dict(actions(system))))
                    self.assertTrue(page.diagram.layout_spec['note'])
                    self.assertIn(group,[label.text() for label in page.findChildren(QLabel)])
                    for bit in bits:
                        self.assertTrue(page.mapping_buttons[f'button:{bit}'].isEnabled())
                # Cycling through the full library cannot decode/retain every
                # controller permanently; the established cache remains bounded.
                for system in SPECS: page.system.setCurrentIndex(page.system.findData(system))
                self.assertLessEqual(raster_image.cache_info().currsize,8)
            finally:
                dialog.close();dialog.deleteLater();library.close()


if __name__=='__main__': unittest.main()
