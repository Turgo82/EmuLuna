"""Choose the native desktop before Qt creates its application."""
import os
import sys
import json

from PySide6.QtCore import Qt, QObject, QEvent, QSize
from PySide6.QtWidgets import QApplication, QWidget


def prefer_native_desktop():
    if sys.platform.startswith("linux") and os.environ.get("WAYLAND_DISPLAY"):
        # Preserve explicit choices, including offscreen tests and X11 overrides.
        os.environ.setdefault("QT_QPA_PLATFORM", "wayland;xcb")


def restore_window_size(window, library, key):
    if not hasattr(window, '_emuluna_size_memory'):
        window._emuluna_size_memory = WindowSizeMemory(window)
    try:
        saved = json.loads(library.setting(key))
        width, height = saved['width'], saved['height']
        if not all(type(value) is int and 32 <= value <= 16384 for value in (width, height)):
            return
        screen = QWidget.screen(window)
        if screen and QApplication.platformName() != 'offscreen':
            available = screen.availableGeometry()
            width, height = min(width, available.width()), min(height, available.height())
        window.resize(width, height)
        window._emuluna_windowed_size = QSize(width, height)
        if saved.get('maximized') is True:
            window.setWindowState(Qt.WindowMaximized)
    except (ValueError, TypeError, KeyError):
        pass


def save_window_size(window, library, key, *, state=None):
    # Fullscreen and temporary minimization must not replace the normal size.
    size = getattr(window, '_emuluna_windowed_size', window.normalGeometry().size())
    if not size.isValid():
        size = window.size()
    state = window.windowState() if state is None else state
    library.set_setting(key, json.dumps({
        'width': size.width(), 'height': size.height(),
        'maximized': bool(state & Qt.WindowMaximized),
    }))


class WindowSizeMemory(QObject):
    """Keep the normal size even when Wayland replaces Qt's normalGeometry."""
    def __init__(self, window):
        super().__init__(window)
        window._emuluna_windowed_size = window.size()
        window.installEventFilter(self)

    def eventFilter(self, window, event):
        if (event.type() == QEvent.Resize
                and not window.windowState() & (Qt.WindowMaximized | Qt.WindowFullScreen | Qt.WindowMinimized)):
            window._emuluna_windowed_size = event.size()
        return False
