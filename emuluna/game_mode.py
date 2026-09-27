"""Keep the desktop awake while an EmuLuna gameplay window is open."""
import ctypes
import sys


class GameMode:
    """Own and release the platform's display-idle inhibition request."""

    _linux_services = (
        ("org.freedesktop.ScreenSaver", "/ScreenSaver", "org.freedesktop.ScreenSaver"),
        ("org.freedesktop.PowerManagement", "/org/freedesktop/PowerManagement/Inhibit",
         "org.freedesktop.PowerManagement.Inhibit"),
    )

    def __init__(self):
        self.active = False
        self._linux_cookies = []

    def start(self):
        if self.active:
            return True
        if sys.platform == "win32":
            # ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
            try:
                result = ctypes.windll.kernel32.SetThreadExecutionState(0x80000003)
            except (AttributeError, OSError):
                result = 0
            self.active = bool(result)
            return self.active
        if not sys.platform.startswith("linux"):
            return False
        try:
            from PySide6.QtDBus import QDBusConnection, QDBusInterface, QDBusMessage
            bus = QDBusConnection.sessionBus()
            if not bus.isConnected():
                return False
            for service, path, interface_name in self._linux_services:
                interface = QDBusInterface(service, path, interface_name, bus)
                if not interface.isValid():
                    continue
                reply = interface.call("Inhibit", "EmuLuna", "Playing a game")
                arguments = reply.arguments()
                if reply.type() == QDBusMessage.ReplyMessage and arguments:
                    self._linux_cookies.append((interface, int(arguments[0])))
        except (ImportError, RuntimeError, TypeError, ValueError):
            self._linux_cookies.clear()
        self.active = bool(self._linux_cookies)
        return self.active

    def stop(self):
        for interface, cookie in self._linux_cookies:
            try:
                interface.call("UnInhibit", cookie)
            except RuntimeError:
                pass
        self._linux_cookies.clear()
        if sys.platform == "win32" and self.active:
            try:
                # ES_CONTINUOUS restores the normal system idle policy.
                ctypes.windll.kernel32.SetThreadExecutionState(0x80000000)
            except (AttributeError, OSError):
                pass
        self.active = False

