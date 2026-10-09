"""Client-controlled game resizing that never requests a stretched viewport."""
from PySide6.QtCore import QEvent, QPointF, QSize, Qt
from PySide6.QtGui import QColor, QPainter, QPen
from PySide6.QtWidgets import QWidget


class GameSizeGrip(QWidget):
    """Anchor a resize target at the corner and constrain both dimensions.

    Normal game windows have matching minimum/maximum sizes, preventing a
    compositor border drag from applying arbitrary proportions. This grip
    changes those bounds together instead of starting a native resize operation.
    """
    def __init__(self, player):
        super().__init__(player.screen)
        self.player = player
        self.origin = None
        self.setAccessibleName("Resize game window")
        self.setToolTip("Drag to resize the game while keeping its proportions")
        self.setFixedSize(24, 24)
        self.setCursor(Qt.SizeFDiagCursor)
        player.screen.installEventFilter(self)
        player.installEventFilter(self)
        self.position()

    def position(self):
        screen = self.player.screen
        self.move(max(0, screen.width() - self.width()),
                  max(0, screen.height() - self.height()))
        self.setVisible(not self.player.windowState() & (
            Qt.WindowFullScreen | Qt.WindowMaximized | Qt.WindowMinimized))
        self.raise_()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        painter.setPen(QPen(QColor(255, 255, 255, 170), 1.5,
                            Qt.SolidLine, Qt.RoundCap))
        x, y = self.width() - 2, self.height() - 2
        for length in (5, 10, 15):
            painter.drawLine(QPointF(x - length, y), QPointF(x, y - length))

    def eventFilter(self, watched, event):
        if event.type() in (QEvent.Resize, QEvent.Show, QEvent.WindowStateChange):
            self.position()
        return super().eventFilter(watched, event)

    def mousePressEvent(self, event):
        if event.button() == Qt.LeftButton and not self.player.windowState() & (
                Qt.WindowFullScreen | Qt.WindowMaximized | Qt.WindowMinimized):
            self.origin = event.globalPosition()
            self.content_size = self.player.screen.size()
            self.chrome = self.player.size() - self.content_size
            event.accept()
        else:
            event.ignore()

    def mouseMoveEvent(self, event):
        if self.origin is None or not event.buttons() & Qt.LeftButton:
            event.ignore()
            return
        if self.player.windowState() & (Qt.WindowFullScreen | Qt.WindowMaximized | Qt.WindowMinimized):
            self.origin = None
            event.ignore()
            return
        delta = event.globalPosition() - self.origin
        aspect = self.player.gameplay_aspect()
        # Project pointer movement onto the aspect-ratio line. Either horizontal
        # or vertical dragging changes the scale smoothly; neither can stretch it.
        height = round(self.content_size.height() +
                       (aspect * delta.x() + delta.y()) / (aspect * aspect + 1))
        content = QSize(round(height * aspect), height)
        self.player._aspect_resize_axis = 'height'
        desired = self.player.aspect_window_size(content + self.chrome, snap=True)
        self.player.resize_game_window(desired)
        event.accept()

    def mouseReleaseEvent(self, event):
        if event.button() == Qt.LeftButton and self.origin is not None:
            self.origin = None
            event.accept()
        else:
            event.ignore()
