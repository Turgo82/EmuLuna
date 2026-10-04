"""Persistent small cover previews followed by bounded full thumbnails."""
from collections import OrderedDict
import hashlib
import os
from pathlib import Path
import shutil
import threading
from .systems import SYSTEMS

from PySide6.QtCore import QObject, QRunnable, QThreadPool, Signal, Slot, Qt, QSize
from PySide6.QtGui import QImage, QImageReader, QPixmap, QIcon

PREVIEW_SIZE = 96


class ThumbnailSignals(QObject):
    preview = Signal(object, QImage)
    finished = Signal(object, QImage)


def _read_scaled(path, limit):
    reader = QImageReader(str(path))
    reader.setAutoTransform(True)
    size = reader.size()
    if not size.isValid() or size.width() * size.height() > 40_000_000:
        return QImage()
    reader.setScaledSize(size.scaled(QSize(limit, limit), Qt.KeepAspectRatio))
    return reader.read()


def _preview_valid(source, preview):
    try:
        return preview.stat().st_size > 0 and preview.stat().st_mtime_ns >= source.stat().st_mtime_ns
    except OSError:
        return False


def _preview_image(source, preview):
    if _preview_valid(source, preview):
        image = QImage(str(preview))
        if not image.isNull():
            return image
    image = _read_scaled(source, PREVIEW_SIZE)
    if image.isNull():
        return image
    try:
        preview.parent.mkdir(parents=True, exist_ok=True)
        temporary = preview.with_name(preview.name + f'.{threading.get_ident()}.tmp')
        if image.save(str(temporary), 'PNG'):
            os.replace(temporary, preview)
        else:
            temporary.unlink(missing_ok=True)
    except OSError:
        pass  # A read-only cache still allows an in-memory preview.
    return image


def _preview_icon(image):
    # Keep the fallback tier small in memory; the delegate scales it to fill
    # the exact cover rectangle while the sharper image loads.
    return QIcon(QPixmap.fromImage(image))


class ThumbnailJob(QRunnable):
    def __init__(self, key, path, preview_path):
        super().__init__()
        self.key, self.path, self.preview_path = key, Path(path), Path(preview_path)
        self.signals = ThumbnailSignals()

    def run(self):
        image = QImage()
        try:
            preview = _preview_image(self.path, self.preview_path)
            if not preview.isNull():
                self.signals.preview.emit(self.key, preview)
            image = _read_scaled(self.path, 512)
        finally:
            self.signals.finished.emit(self.key, image)


class PreviewWarmJob(QRunnable):
    def __init__(self, entries):
        super().__init__()
        self.entries = entries
        self.cancelled = threading.Event()

    def run(self):
        for source, preview in self.entries:
            if self.cancelled.is_set():
                break
            if not _preview_valid(source, preview):
                _preview_image(source, preview)


class ThumbnailCache(QObject):
    ready = Signal(str)

    def __init__(self, root, parent=None, capacity=256):
        super().__init__(parent)
        self.root, self.capacity = root, capacity
        self.cache = OrderedDict()
        self.previews = OrderedDict()
        self.preview_directory = self.root / 'cache' / 'cover-previews'
        self.sizes = OrderedDict()
        self.pending = {}
        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(2)
        self.warm_pool = QThreadPool(self)
        self.warm_pool.setMaxThreadCount(1)
        self.warm_job = None
        self.warm_signature = None
        self.generation = 0
        self.closed = False
        self.decoded_count = 0

    def preview_path(self, game):
        identity = hashlib.sha256(str(game['id']).encode()).hexdigest()[:32]
        # Include quality in the cache identity so older, blurrier previews
        # are regenerated automatically without touching original artwork.
        return self.preview_directory / f"{identity}-{game['cover_revision']}-{PREVIEW_SIZE}px.png"

    def warm_previews(self, games):
        """Fill the small on-disk tier off-thread, so later fast scrolls never flash blanks."""
        if self.closed:
            return
        entries = [(game['id'], game['cover'], game['cover_revision'])
                   for game in games if game['cover']]
        signature = tuple(entries)
        if signature == self.warm_signature:
            return
        self.warm_signature = signature
        if self.warm_job:
            self.warm_job.cancelled.set()
        self.warm_pool.clear()
        paths = [(self.root / cover, self.preview_path({
            'id': game_id, 'cover_revision': revision}))
                 for game_id, cover, revision in entries]
        self.warm_job = PreviewWarmJob(paths)
        self.warm_pool.start(self.warm_job)

    def rebuild(self):
        """Drop the generated previews and regenerate them for the current view."""
        if self.warm_job:
            self.warm_job.cancelled.set()
        self.warm_pool.clear()
        self.warm_pool.waitForDone()
        self.pool.clear()
        self.pool.waitForDone()
        self.pending.clear()
        self.warm_job = None
        self.warm_signature = None
        self.generation += 1
        self.cache.clear()
        self.previews.clear()
        self.sizes.clear()
        shutil.rmtree(self.preview_directory, ignore_errors=True)

    def dimensions(self, game):
        fallback = QSize(*SYSTEMS[game['system']].cover_size)
        if not game['cover']:
            return fallback
        key = (game['id'], game['cover'], game['cover_revision'], self.generation)
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
        key = (game['id'], game['cover'], game['cover_revision'], self.generation)
        if key in self.cache:
            self.cache.move_to_end(key)
            return self.cache[key]
        preview = self.previews.get(key)
        if preview is not None:
            self.previews.move_to_end(key)
        else:
            source = self.root / game['cover']
            path = self.preview_path(game)
            if _preview_valid(source, path):
                image = QImage(str(path))
                if not image.isNull():
                    preview = _preview_icon(image)
                    self._remember_preview(key, preview)
        if key not in self.pending and len(self.pending) < 48:
            job = ThumbnailJob(key, self.root / game['cover'], self.preview_path(game))
            job.signals.preview.connect(self.preview_ready)
            job.signals.finished.connect(self.completed)
            self.pending[key] = job
            self.pool.start(job)
        return preview

    def _remember_preview(self, key, icon):
        self.previews[key] = icon
        self.previews.move_to_end(key)
        while len(self.previews) > 1024:
            self.previews.popitem(last=False)

    @Slot(object, QImage)
    def preview_ready(self, key, image):
        if self.closed or key[-1] != self.generation or key in self.cache:
            return
        self._remember_preview(key, _preview_icon(image))
        self.ready.emit(key[0])

    @Slot(object, QImage)
    def completed(self, key, image):
        self.pending.pop(key, None)
        if self.closed or key[-1] != self.generation:
            return
        self.decoded_count += 1
        self.cache[key] = QIcon(QPixmap.fromImage(image)) if not image.isNull() else QIcon()
        self.previews.pop(key, None)
        self.cache.move_to_end(key)
        while len(self.cache) > self.capacity:
            self.cache.popitem(last=False)
        self.ready.emit(key[0])

    def discard(self, game_ids):
        """Drop cached artwork for games that no longer belong to the library."""
        removed = set(game_ids)
        for store in (self.cache, self.previews, self.sizes):
            for key in list(store):
                if key[0] in removed:
                    store.pop(key, None)
        for game_id in removed:
            identity = hashlib.sha256(str(game_id).encode()).hexdigest()[:32]
            for preview in self.preview_directory.glob(identity + '-*.png'):
                preview.unlink(missing_ok=True)

    def close(self):
        self.closed = True
        if self.warm_job:
            self.warm_job.cancelled.set()
        self.warm_pool.clear()
        self.warm_pool.waitForDone()
        self.pool.clear()
        self.pool.waitForDone()
        self.pending.clear()
        self.cache.clear()
        self.previews.clear()
