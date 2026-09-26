"""Bounded thumbnail decoding off the UI thread, requested by visible cards."""
from collections import OrderedDict
from .systems import SYSTEMS

from PySide6.QtCore import QObject, QRunnable, QThreadPool, Signal, Slot, Qt, QSize
from PySide6.QtGui import QImage, QImageReader, QPixmap, QIcon


class ThumbnailSignals(QObject):
    finished = Signal(object, QImage)


class ThumbnailJob(QRunnable):
    def __init__(self, key, path):
        super().__init__()
        self.key, self.path = key, str(path)
        self.signals = ThumbnailSignals()

    def run(self):
        image = QImage()
        try:
            reader = QImageReader(self.path)
            reader.setAutoTransform(True)
            size = reader.size()
            if size.isValid() and size.width() * size.height() <= 40_000_000:
                reader.setScaledSize(size.scaled(QSize(256, 256), Qt.KeepAspectRatio))
                image = reader.read()
        finally:
            self.signals.finished.emit(self.key, image)


class ThumbnailCache(QObject):
    ready = Signal(str)

    def __init__(self, root, parent=None, capacity=256):
        super().__init__(parent)
        self.root, self.capacity = root, capacity
        self.cache = OrderedDict()
        self.sizes = OrderedDict()
        self.pending = {}
        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(2)
        self.closed = False
        self.decoded_count = 0

    def dimensions(self, game):
        fallback = QSize(*SYSTEMS[game['system']].cover_size)
        if not game['cover']:
            return fallback
        key = (game['id'], game['cover'], game['cover_revision'])
        if key not in self.sizes:
            reader = QImageReader(str(self.root / game['cover']))
            size = reader.size()
            self.sizes[key] = size if size.isValid() and size.width() * size.height() <= 40_000_000 else fallback
            while len(self.sizes) > 2048:
                self.sizes.popitem(last=False)
        self.sizes.move_to_end(key)
        return self.sizes[key]

    def icon(self, game):
        if not game['cover'] or self.closed:
            return None
        key = (game['id'], game['cover'], game['cover_revision'])
        if key in self.cache:
            self.cache.move_to_end(key)
            return self.cache[key]
        if key not in self.pending and len(self.pending) < 48:
            job = ThumbnailJob(key, self.root / game['cover'])
            job.signals.finished.connect(self.completed)
            self.pending[key] = job
            self.pool.start(job)
        return None

    @Slot(object, QImage)
    def completed(self, key, image):
        self.pending.pop(key, None)
        if self.closed:
            return
        self.decoded_count += 1
        self.cache[key] = QIcon(QPixmap.fromImage(image)) if not image.isNull() else QIcon()
        self.cache.move_to_end(key)
        while len(self.cache) > self.capacity:
            self.cache.popitem(last=False)
        self.ready.emit(key[0])

    def close(self):
        self.closed = True
        self.pool.clear()
        self.pool.waitForDone()
        self.pending.clear()
        self.cache.clear()
