"""Artwork hit testing, resize safety, device persistence and theme regressions."""
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
import tempfile
import unittest
from unittest.mock import patch
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import Qt, QPointF, QRectF, QEvent, QCoreApplication
from PySide6.QtGui import QPalette
from PySide6.QtWidgets import (QApplication, QStyle, QStyleOptionButton,
                              QStyleOptionSlider, QStyleOptionSpinBox)
from PySide6.QtTest import QTest
from emuluna.controller_diagrams import ControllerDiagram
from emuluna.controller_profiles import SPECS, actions, defaults, save_profile, load_profile, gamepad_state
from emuluna.library import Library
from emuluna.settings import SettingsDialog
from emuluna.theme import set_system_theme, theme_palette
from test_theme import palette


class ControllerRedesign(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.library = Library(self.temp.name)
        self.original_palette = self.app.palette()

    def tearDown(self):
        self.app.setPalette(self.original_palette)
        set_system_theme(True)
        self.library.close()
        self.temp.cleanup()

    def close(self, dialog):
        dialog.close()
        dialog.deleteLater()
        QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)

    def test_all_artwork_keeps_its_proportions_and_click_targets_when_resized(self):
        diagram = ControllerDiagram()
        for system in SPECS:
            diagram.set_system(system)
            self.assertTrue(diagram.renderer.isValid(), system)
            self.assertTrue(diagram.controls, system)
            for size in ((180,90),(300,170),(900,600)):
                diagram.resize(*size)
                transform = diagram.artwork_transform()
                self.assertAlmostEqual(transform.m11(), transform.m22())
                w,h = diagram.layout_spec['size']
                self.assertTrue(QRectF(diagram.rect()).contains(transform.mapRect(QRectF(0,0,w,h))), system)
                for part in diagram.controls:
                    center = transform.map(QRectF(*part['rect']).center())
                    self.assertEqual(diagram.action_at(center), part['action'], (system,part,size))
        diagram.close()

    def test_clicking_artwork_captures_correct_action_without_losing_list_position(self):
        dialog = SettingsDialog(self.library)
        try:
            dialog.show(); dialog.show_page('controls')
            page = dialog.controls_page
            page.system.setCurrentIndex(page.system.findData('psx'))
            QTest.qWait(20)
            part = next(p for p in page.diagram.controls if p['action']=='button:32768')
            point = page.diagram.artwork_transform().map(QRectF(*part['rect']).center()).toPoint()
            QTest.mouseClick(page.diagram, Qt.LeftButton, pos=point)
            button = page.mapping_buttons['button:32768']
            self.assertTrue(button.recording)
            scroll = page.mapping_scroll.verticalScrollBar().value()
            self.assertGreater(scroll, 0)
            QTest.keyClick(button, Qt.Key_V)
            self.assertIs(page.mapping_buttons['button:32768'], button)
            self.assertEqual(page.mapping_scroll.verticalScrollBar().value(),scroll)
            self.assertEqual(load_profile(self.library,'psx',0)['keyboard']['button:32768'],['key:86'])
            QTest.mouseClick(button,Qt.LeftButton); QTest.keyClick(button,Qt.Key_Escape)
            self.assertFalse(button.recording)
            self.assertEqual(button.text(),'V')
            QTest.mouseClick(button,Qt.LeftButton); QTest.keyClick(button,Qt.Key_Delete)
            self.assertEqual(load_profile(self.library,'psx',0)['keyboard']['button:32768'],[])
        finally:
            self.close(dialog)

    def test_snes_diagonal_menu_buttons_keep_tilted_hit_areas_when_resized(self):
        diagram = ControllerDiagram()
        diagram.set_system('snes')
        for size in ((180,90),(300,170),(900,600)):
            diagram.resize(*size)
            for action in ('button:4','button:8'):
                part = next(p for p in diagram.controls if p['action']==action)
                rect = QRectF(*part['rect'])
                rotation = diagram.control_transform(part)
                # Both ends of the physical key remain clickable, even outside
                # the old horizontal box. Empty space beside it is not a key.
                for x in (rect.left()+5,rect.right()-5):
                    physical = rotation.map(QPointF(x,rect.center().y()))
                    self.assertEqual(diagram.action_at(diagram.artwork_transform().map(physical)),action)
                empty = rotation.map(QPointF(rect.center().x(),rect.top()-6))
                self.assertIsNone(diagram.action_at(diagram.artwork_transform().map(empty)))
        diagram.close()

    def test_switching_console_and_player_restores_their_own_device_assignment(self):
        for system,player,device in (('snes',0,'index:1'),('snes',1,'index:2'),('psx',0,'index:3')):
            record=defaults(system,player);record['device']=device
            save_profile(self.library,system,player,record)
        dialog=SettingsDialog(self.library)
        try:
            dialog.show_page('controls');page=dialog.controls_page
            for system,player,device in (('snes',0,'index:1'),('snes',1,'index:2'),('psx',0,'index:3'),('snes',0,'index:1')):
                page.system.setCurrentIndex(page.system.findData(system))
                page.player.setCurrentIndex(player)
                self.assertEqual(page.device.currentData(),device)
        finally:
            self.close(dialog)

    def test_small_window_has_no_overlapping_artwork_text_or_clipped_binding_columns(self):
        dialog=SettingsDialog(self.library)
        try:
            dialog.show();dialog.show_page('controls');page=dialog.controls_page
            with patch('emuluna.controller_settings.Gamepad.devices',return_value=[]):
                for size in ((740,550),(1200,800),(740,550)):
                    dialog.resize(*size)
                    for system in SPECS:
                        page.system.setCurrentIndex(page.system.findData(system))
                        for source in ('keyboard','gamepad'):
                            page.source.setCurrentIndex(page.source.findData(source))
                            QTest.qWait(2)
                            self.assertEqual((dialog.width(),dialog.height()),size,system)
                            widgets=[page.diagram_title,page.diagram,page.diagram_hint]
                            if page.diagram_note.isVisible(): widgets.append(page.diagram_note)
                            for first,second in zip(widgets,widgets[1:]):
                                self.assertLess(first.geometry().bottom(),second.geometry().top(),(system,size,source))
                            for button in page.mapping_buttons.values():
                                self.assertGreaterEqual(button.width(),90,(system,size))
                            self.assertEqual(page.mapping_scroll.horizontalScrollBar().maximum(),0)
        finally:
            self.close(dialog)

    def test_hardware_specific_controls_are_named_and_do_not_duplicate_analog_input(self):
        saturn=dict(actions('saturn'))
        self.assertEqual([saturn[f'button:{b}'] for b in (2,1,256,2048,1024,512,4096,8192)],['A','B','C','X','Y','Z','L','R'])
        self.assertEqual(dict(actions('vb'))['button:32768'],'Right pad right')
        self.assertEqual(dict(actions('ws'))['button:8192'],'Y1 (up)')
        self.assertNotIn('button:512',dict(actions('gg')))
        self.assertNotIn('button:1024',dict(actions('atari2600')))
        buttons,axes=gamepad_state(defaults('psx',0)['gamepad'],set(),(24000,0,0,0,0,0))
        self.assertEqual(buttons,0)
        self.assertEqual(axes,(24000,0,0,0))

    def test_rendered_wood_bindings_and_gameplay_surfaces_follow_their_themes(self):
        dialog=SettingsDialog(self.library)
        try:
            dialog.show();dialog.show_page('controls')
            page=dialog.controls_page
            for use_system,dark in ((True,False),(True,True),(False,True),(True,False)):
                self.app.setPalette(palette(dark))
                dialog.use_system_theme.setChecked(use_system)
                QTest.qWait(30)
                # Newly created rows after a palette change must match existing ones.
                page.system.setCurrentIndex(page.system.findData('psx'))
                page.system.setCurrentIndex(page.system.findData('snes'))
                QTest.qWait(10)
                button=page.mapping_buttons['button:64']
                # Bindings keep warm translucent panels over the wood in both
                # themes; gameplay widgets still follow the active palette.
                surface=button.grab().toImage().pixelColor(5,5)
                self.assertGreater(surface.red(),surface.green())
                self.assertGreater(surface.green(),surface.blue())
                self.assertGreater(surface.alpha(),0)
                self.assertLess(surface.alpha(),255)
                self.assertGreater(button.palette().color(QPalette.ButtonText).lightnessF(),.8)
                dialog.show_page('gameplay');QTest.qWait(10)
                box=dialog.focus_pause
                option=QStyleOptionButton();box.initStyleOption(option)
                rect=box.style().subElementRect(QStyle.SE_CheckBoxIndicator,option,box)
                contents=box.style().subElementRect(QStyle.SE_CheckBoxContents,option,box)
                self.assertGreater(contents.left(),rect.right())
                pixels=box.grab().toImage()
                self.assertEqual(pixels.pixelColor(rect.left()+5,rect.top()+4),theme_palette().color(QPalette.Highlight))
                # Repaint immediately after switching without recreating the
                # checkbox; cached widget palettes must not leak old accents.
                opposite = not use_system
                dialog.use_system_theme.setChecked(opposite)
                QTest.qWait(10)
                pixels=box.grab().toImage()
                expected=theme_palette().color(QPalette.Highlight)
                self.assertEqual(pixels.pixelColor(rect.left()+5,rect.top()+4),expected)
                dialog.use_system_theme.setChecked(use_system)
                QTest.qWait(10)
                slider_option=QStyleOptionSlider();dialog.volume.initStyleOption(slider_option)
                groove=dialog.volume.style().subControlRect(QStyle.CC_Slider,slider_option,QStyle.SC_SliderGroove,dialog.volume)
                self.assertEqual(dialog.volume.grab().toImage().pixelColor(groove.center()),theme_palette().color(QPalette.Highlight))
                dialog.show_page('controls')
        finally:
            self.close(dialog)

    def test_gameplay_actions_sit_to_the_right_of_their_controls(self):
        dialog=SettingsDialog(self.library)
        try:
            dialog.show();QTest.qWait(20)
            self.assertLessEqual(dialog.volume.width(), 360)
            dialog.show_page('gameplay');QTest.qWait(20)
            self.assertGreater(dialog.reset_filters.geometry().left(),
                               dialog.video_filter.geometry().right())
            self.assertGreater(dialog.vibration_test.geometry().left(),
                               dialog.rumble_intensity.geometry().right())
            self.assertLessEqual(dialog.video_filter.width(), 300)
            self.assertLessEqual(dialog.rumble_intensity.width(), 360)
            self.assertLess(abs(dialog.reset_filters.geometry().center().y() -
                                dialog.video_filter.geometry().center().y()), 3)
            self.assertLess(abs(dialog.vibration_test.geometry().center().y() -
                                dialog.rumble_intensity.geometry().center().y()), 3)
        finally:
            self.close(dialog)

    def test_gameplay_number_arrows_render_and_still_change_saved_values(self):
        dialog=SettingsDialog(self.library)
        try:
            dialog.show();dialog.show_page('gameplay')
            spin=dialog.fast_speed
            for use_system,dark in ((False,True),(True,False),(True,True)):
                self.app.setPalette(palette(dark))
                dialog.use_system_theme.setChecked(use_system)
                spin.setValue(4)
                QTest.qWait(25)
                option=QStyleOptionSpinBox();spin.initStyleOption(option)
                up=spin.style().subControlRect(QStyle.CC_SpinBox,option,QStyle.SC_SpinBoxUp,spin)
                down=spin.style().subControlRect(QStyle.CC_SpinBox,option,QStyle.SC_SpinBoxDown,spin)
                QTest.mouseClick(spin,Qt.LeftButton,pos=up.center())
                self.assertEqual(spin.value(),5)
                QTest.mouseClick(spin,Qt.LeftButton,pos=down.center())
                self.assertEqual(spin.value(),4)
                QTest.keyClick(spin,Qt.Key_Up)
                self.assertEqual(self.library.setting('fast_forward_speed'),'5')
                image=spin.grab().toImage()
                color=theme_palette().color(QPalette.ButtonText)
                self.assertTrue(any(image.pixelColor(x,y)==color
                    for x in range(up.left(),up.right()+1) for y in range(up.top(),up.bottom()+1)))
        finally:
            self.close(dialog)


if __name__=='__main__':unittest.main()
