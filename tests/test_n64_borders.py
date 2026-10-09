"""N64 crop settings stay game-specific and use declared core options."""
import json
import os
import tempfile
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtWidgets import QApplication, QMainWindow
from emuluna.library import Library
from emuluna.n64_borders import N64BorderControls, border_settings, core_border_options
from emuluna.player import Player


class N64BorderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_saved_values_are_validated_and_uncropped_by_default(self):
        default = {"enabled": False, "top": 0, "bottom": 0, "left": 0, "right": 0}
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
            self.assertEqual(saved, {"enabled": True, "top": 0, "bottom": 24, "left": 16, "right": 12})
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


if __name__ == "__main__":
    unittest.main()
