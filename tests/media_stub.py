"""Keep desktop unit tests independent of host audio services.

Real device/audio-stream validation runs separately in the live smoke check.
"""
from PySide6.QtCore import QObject, Signal


class NoAudioDevice:
    def isNull(self): return True


class TestMediaDevices(QObject):
    audioOutputsChanged = Signal()

    @staticmethod
    def audioOutputs(): return []

    @staticmethod
    def defaultAudioOutput(): return NoAudioDevice()


class TestSoundEffect(QObject):
    """Avoid connecting desktop tests to the host sound server."""
    statusChanged = Signal()
    Null, Loading, Ready, Error = range(4)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._source = None
        self._volume = 1.0
        self.played = False

    def setSource(self, source): self._source = source
    def source(self): return self._source
    def setVolume(self, volume): self._volume = volume
    def status(self): return self.Ready
    def play(self): self.played = True


def isolate_audio():
    from emuluna import app, player, settings
    app.QSoundEffect = TestSoundEffect
    player.QMediaDevices = TestMediaDevices
    settings.QMediaDevices = TestMediaDevices
