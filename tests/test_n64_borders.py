"""N64 crop settings stay game-specific and use declared core options."""
import json
import os
import tempfile
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtWidgets import QApplication, QMainWindow
from PySide6.QtCore import QCoreApplication, QEvent
from emuluna.library import Library
from emuluna.n64_borders import N64BorderControls, border_settings, core_border_options
from emuluna.player import Player


class N64BorderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_saved_values_are_validated_and_uncropped_by_default(self):
        default = {"enabled": False, "automatic": True, "top": 0, "bottom": 0, "left": 0, "right": 0}
        for value in ("invalid", "null", "[]", "{}"):
            self.assertEqual(border_settings(value), default)
        settings = border_settings('{"enabled":"yes","top":true,"bottom":-1,"left":70,"right":"12"}')
        self.assertEqual(settings, {**default, "left": 50})
        self.assertEqual(core_border_options(default)["parallel-n64-gliden64-EnableOverscan"], "Disabled")

    def test_dialog_remembers_each_edge_only_for_its_game_and_restores_defaults(self):
        with tempfile.TemporaryDirectory() as temp:
            library = Library(temp)
            player = QMainWindow()
            player.library = library
            player.n64_border_setting = "screen_borders.banjo.parallel_n64.gliden64"
            other = "screen_borders.other.parallel_n64.gliden64"
            library.set_setting(other, '{"enabled":true,"left":3}')
            dialog = N64BorderControls(border_settings(),
                lambda values: Player.set_n64_borders(player, values), player)
            self.assertTrue(all(not field.isEnabled() for field in dialog.fields.values()))
            dialog.enabled.setChecked(True)
            for edge, amount in (("bottom", 24), ("left", 16), ("right", 12)):
                dialog.fields[edge].setValue(amount)
            saved = border_settings(library.setting(player.n64_border_setting))
            self.assertEqual(saved, {"enabled": True, "automatic": True, "top": 0, "bottom": 24, "left": 16, "right": 12})
            options = core_border_options(saved)
            self.assertEqual(options["parallel-n64-gliden64-EnableOverscan"], "Enabled")
            self.assertEqual(options["parallel-n64-gliden64-OverscanBottom"], "24")
            self.assertEqual(options["parallel-n64-gliden64-OverscanTop"], "0")
            self.assertEqual(json.loads(library.setting(other)), {"enabled": True, "left": 3})
            dialog.restore()
            restored = border_settings(library.setting(player.n64_border_setting))
            self.assertEqual(restored, border_settings())
            self.assertTrue(all(not field.isEnabled() for field in dialog.fields.values()))
            dialog.close()
            QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)
            player.close()
            library.close()

    def test_software_uses_the_native_automatic_crop_without_manual_offsets(self):
        changes = []
        dialog = N64BorderControls(border_settings(), changes.append, renderer="angrylion")
        self.assertEqual(dialog.fields, {})
        self.assertEqual(dialog.enabled.text(), "Hide overscan")
        dialog.enabled.setChecked(True)
        options = core_border_options(border_settings(json.dumps(changes[-1])), "angrylion")
        self.assertEqual(options, {"parallel-n64-angrylion-overscan": "enabled"})
        dialog.restore()
        options = core_border_options(border_settings(json.dumps(changes[-1])), "angrylion")
        self.assertEqual(options, {"parallel-n64-angrylion-overscan": "disabled"})
        dialog.close()
        QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)

    def test_automatic_trim_removes_asymmetric_padding_without_losing_edge_detail(self):
        from PySide6.QtGui import QImage,QPainter,QColor
        from PySide6.QtCore import Qt,QRect
        from emuluna.n64_borders import trim_n64_frame
        image=QImage(320,240,QImage.Format_RGB32);image.fill(Qt.black)
        painter=QPainter(image);painter.fillRect(QRect(12,0,300,216),QColor('blue'));painter.end()
        result=trim_n64_frame(image)
        self.assertEqual(result.size(),QRect(0,0,300,216).size())
        self.assertEqual(result.pixelColor(0,0),QColor('blue'))
        self.assertEqual(result.pixelColor(299,215),QColor('blue'))
        # A visible pixel at the edge must prevent trimming that column/row.
        image.setPixelColor(0,0,QColor('red'))
        result=trim_n64_frame(image)
        self.assertEqual(result.width(),312)
        self.assertEqual(result.pixelColor(0,0),QColor('red'))

    def test_letterboxed_cutscenes_and_black_fades_are_untouched(self):
        from PySide6.QtGui import QImage,QPainter,QColor
        from PySide6.QtCore import Qt,QRect
        from emuluna.n64_borders import trim_n64_frame
        image=QImage(320,240,QImage.Format_RGB32);image.fill(Qt.black)
        self.assertEqual(trim_n64_frame(image),image)
        painter=QPainter(image);painter.fillRect(QRect(0,40,320,120),QColor('blue'));painter.end()
        result=trim_n64_frame(image)
        self.assertEqual(result.size(),image.size())
        self.assertEqual(result, image)

    def test_playback_settles_once_and_keeps_crop_for_session(self):
        from PySide6.QtGui import QImage, QColor
        from PySide6.QtCore import Qt
        from emuluna.n64_borders import N64FrameBorders
        image = QImage(100, 100, QImage.Format_RGB32)
        image.fill(QColor('blue'))
        for y in range(90, 100):
            for x in range(100):
                image.setPixelColor(x, y, QColor('black'))
        borders = N64FrameBorders()
        self.assertEqual(borders.apply(image, 0).height(), 100)
        self.assertEqual(borders.apply(image, 2.9).height(), 100)
        self.assertEqual(borders.apply(image, 3).height(), 90)
        # Once chosen, new scene borders, edge detail and fades cannot change it.
        full = QImage(100, 100, QImage.Format_RGB32)
        full.fill(QColor('blue'))
        self.assertTrue(borders.locked)
        self.assertEqual(borders.apply(full, 4).height(), 90)
        self.assertEqual(borders.apply(image, 12).height(), 90)
        fade = full.copy()
        fade.fill(Qt.black)
        self.assertEqual(borders.apply(fade, 13).height(), 90)
        # Resolution changes preserve the proportional session crop.
        self.assertEqual(borders.apply(full.scaled(200, 200), 14).height(), 180)

    def test_mostly_black_scenes_never_choose_a_session_crop(self):
        from PySide6.QtGui import QImage, QPainter, QColor
        from PySide6.QtCore import Qt, QRect
        from emuluna.n64_borders import N64FrameBorders
        dark = QImage(100, 100, QImage.Format_RGB32)
        dark.fill(Qt.black)
        painter = QPainter(dark)
        painter.fillRect(QRect(0, 0, 100, 90), QColor(8, 8, 8))
        painter.fillRect(QRect(45, 40, 10, 10), QColor('white'))
        painter.end()
        borders = N64FrameBorders()
        for now in (0, 3, 10, 30):
            self.assertEqual(borders.apply(dark, now), dark)
            self.assertFalse(borders.locked)
        bright = dark.copy()
        painter = QPainter(bright)
        painter.fillRect(QRect(0, 0, 100, 90), QColor('blue'))
        painter.end()
        self.assertEqual(borders.apply(bright, 31).height(), 100)
        self.assertEqual(borders.apply(dark, 32).height(), 100)
        self.assertEqual(borders.apply(bright, 34).height(), 100)
        self.assertEqual(borders.apply(bright, 37).height(), 90)

    def test_brief_scene_borders_do_not_change_playback_crop(self):
        from PySide6.QtGui import QImage, QColor
        from emuluna.n64_borders import N64FrameBorders
        full = QImage(100, 100, QImage.Format_RGB32)
        full.fill(QColor('blue'))
        bordered = full.copy()
        for y in range(90, 100):
            for x in range(100):
                bordered.setPixelColor(x, y, QColor('black'))
        borders = N64FrameBorders()
        for now in range(20):
            frame = full if now % 3 == 0 else bordered
            self.assertEqual(borders.apply(frame, now).size(), full.size())

    def test_global_toggle_refreshes_running_session_and_resets_detection(self):
        from emuluna.n64_borders import N64FrameBorders
        with tempfile.TemporaryDirectory() as temp:
            library = Library(temp)
            player = QMainWindow()
            player.library = library
            player.automatic_n64_borders = True
            player.n64_frame_borders = N64FrameBorders()
            player.n64_frame_borders.margins = (1, 2, 3, 4)
            library.set_setting("n64.automatic_borders", "0")
            Player.refresh_n64_borders(player)
            self.assertFalse(player.automatic_n64_borders)
            self.assertEqual(player.n64_frame_borders.margins, (0, 0, 0, 0))
            library.set_setting("n64.automatic_borders", "1")
            Player.refresh_n64_borders(player)
            self.assertTrue(player.automatic_n64_borders)
            player.close()
            library.close()

    def test_automatic_option_defaults_on_and_can_be_disabled_per_game(self):
        self.assertTrue(border_settings()['automatic'])
        self.assertFalse(border_settings('{"automatic":false}')['automatic'])


if __name__ == "__main__":
    unittest.main()
