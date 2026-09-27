"""Desktop idle inhibition used while games are running."""
import unittest
from unittest.mock import Mock, patch

from PySide6.QtDBus import QDBusMessage

from emuluna.game_mode import GameMode


class Reply:
    def __init__(self, cookie):
        self.cookie = cookie

    def type(self):
        return QDBusMessage.ReplyMessage

    def arguments(self):
        return [self.cookie]


class GameModeTests(unittest.TestCase):
    def test_linux_inhibitors_are_acquired_and_released(self):
        bus = Mock()
        bus.isConnected.return_value = True
        interfaces = []

        def interface(*_args):
            item = Mock()
            item.isValid.return_value = True
            item.call.side_effect = lambda method, *_: Reply(len(interfaces) + 1) if method == "Inhibit" else Reply(0)
            interfaces.append(item)
            return item

        with patch("PySide6.QtDBus.QDBusConnection.sessionBus", return_value=bus), \
             patch("PySide6.QtDBus.QDBusInterface", side_effect=interface):
            mode = GameMode()
            self.assertTrue(mode.start())
            self.assertTrue(mode.active)
            mode.stop()

        self.assertFalse(mode.active)
        self.assertEqual(len(interfaces), 2)
        for item in interfaces:
            self.assertEqual(item.call.call_args_list[0].args[0], "Inhibit")
            self.assertEqual(item.call.call_args_list[-1].args[0], "UnInhibit")


if __name__ == "__main__":
    unittest.main()
