"""Qt Quick Vulkan presentation for libretro's ordinary software frames."""
from pathlib import Path

from PySide6.QtCore import QObject, Qt, QUrl
from PySide6.QtGui import QColor, QImage
from PySide6.QtQuick import QQuickImageProvider, QQuickWindow, QSGRendererInterface
from PySide6.QtQuickWidgets import QQuickWidget


class FrameProvider(QQuickImageProvider):
    def __init__(self):
        super().__init__(QQuickImageProvider.Image)
        self.frame = QImage()

    def requestImage(self, image_id, size, requested_size):
        if size is not None:
            size.setWidth(self.frame.width())
            size.setHeight(self.frame.height())
        return self.frame


class VulkanViewport(QQuickWidget):
    """Upload frames through Qt's Vulkan scene graph, without a core context."""
    def __init__(self, screen):
        QQuickWindow.setGraphicsApi(QSGRendererInterface.GraphicsApi.Vulkan)
        super().__init__(screen)
        self.screen = screen
        self.provider = FrameProvider()
        self.engine().addImageProvider("emuluna", self.provider)
        self.setResizeMode(QQuickWidget.SizeRootObjectToView)
        self.setClearColor(QColor("#0c0d10"))
        self.setAttribute(Qt.WA_TransparentForMouseEvents)
        self.setFocusPolicy(Qt.NoFocus)
        self.frame_number = 0
        self.quickWindow().sceneGraphInitialized.connect(self.initialized)
        self.sceneGraphError.connect(self.failed)
        self.statusChanged.connect(self.check_status)
        self.setSource(QUrl.fromLocalFile(str(Path(__file__).parent / "data/vulkan_screen.qml")))
        if self.status() == QQuickWidget.Error:
            raise RuntimeError("Qt Quick could not load the Vulkan display scene: " +
                               "; ".join(error.toString() for error in self.errors()))
        self.frame_image = self.rootObject().findChild(QObject, "frameImage")
        if self.frame_image is None:
            raise RuntimeError("The Vulkan display scene has no frame image.")

    def check_status(self, status):
        if status == QQuickWidget.Error:
            self.failed(None, "; ".join(error.toString() for error in self.errors()))

    def initialized(self):
        window = self.quickWindow()
        if window.rendererInterface().graphicsApi() != QSGRendererInterface.GraphicsApi.Vulkan:
            self.failed(None, "Qt Quick initialized a different graphics API.")
            return
        self.screen.backend = "Vulkan · Qt Quick"
        self.screen.backend_changed.emit(self.screen.backend)
        print("Vulkan initialization successful", flush=True)
        print("Video renderer: " + self.screen.backend, flush=True)

    def failed(self, error, message):
        self.screen.vulkan_failed(message or str(error))

    def present(self, frame, target, linear):
        if self.status() != QQuickWidget.Ready or not self.quickWindow().isSceneGraphInitialized():
            return
        if frame.isNull():
            return
        self.provider.frame = frame
        image = self.frame_image
        image.setProperty("x", target.x())
        image.setProperty("y", target.y())
        image.setProperty("width", target.width())
        image.setProperty("height", target.height())
        image.setProperty("smooth", linear)
        self.frame_number += 1
        image.setProperty("source", f"image://emuluna/frame-{self.frame_number}")
