import os
import unittest
from unittest.mock import patch

from emuluna.desktop import prefer_native_desktop


class DesktopPlatform(unittest.TestCase):
    def test_wayland_session_prefers_wayland_with_x11_fallback(self):
        with patch.dict(os.environ, {'WAYLAND_DISPLAY': 'wayland-0'}, clear=True), patch('sys.platform', 'linux'):
            prefer_native_desktop()
            self.assertEqual(os.environ['QT_QPA_PLATFORM'], 'wayland;xcb')

    def test_explicit_platform_override_is_preserved(self):
        with patch.dict(os.environ, {'WAYLAND_DISPLAY': 'wayland-0', 'QT_QPA_PLATFORM': 'xcb'}, clear=True):
            prefer_native_desktop()
            self.assertEqual(os.environ['QT_QPA_PLATFORM'], 'xcb')

    def test_x11_session_keeps_qt_default(self):
        with patch.dict(os.environ, {'DISPLAY': ':0'}, clear=True):
            prefer_native_desktop()
            self.assertNotIn('QT_QPA_PLATFORM', os.environ)
