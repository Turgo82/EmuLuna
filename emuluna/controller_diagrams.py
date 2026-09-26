"""Aspect-preserving hardware artwork with data-driven control hit areas."""
import json
from functools import lru_cache

from PySide6.QtCore import Qt, Signal, QRectF, QSize
from PySide6.QtGui import QImage, QPainter, QPen, QPalette, QTransform
from PySide6.QtSvg import QSvgRenderer
from PySide6.QtWidgets import QWidget, QSizePolicy

from .controller_profiles import SPECS, actions
from .systems import DATA

ART = DATA / 'controllers'


@lru_cache(maxsize=1)
def layouts():
    result = json.loads((ART / 'layouts.json').read_text())
    # Kept separate so regenerating a fallback SVG cannot overwrite imported
    # artwork or its carefully aligned click targets.
    result.update(json.loads((ART / 'imported_layouts.json').read_text()))
    return result


@lru_cache(maxsize=8)
def raster_image(path):
    return QImage(str(path))


class RasterRenderer:
    def __init__(self, path):
        self.image = raster_image(path)

    def isValid(self):
        return not self.image.isNull()

    def render(self, painter, bounds):
        painter.save()
        painter.setRenderHint(QPainter.SmoothPixmapTransform)
        painter.drawImage(bounds, self.image)
        painter.restore()


class ControllerDiagram(QWidget):
    controlPicked = Signal(str)
    controlHovered = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.selected = None
        # The layout may allot less height when notes or device selectors wrap.
        # Scale within that space instead of forcing the artwork over its labels.
        self.setMinimumSize(180, 60)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.setMouseTracking(True)
        self.set_system('snes')

    def sizeHint(self):
        return QSize(420, 290)

    def set_system(self, system):
        self.system = system
        self.selected = None
        key = SPECS[system]['diagram']
        self.layout_spec = layouts()[key]
        self.labels = dict(actions(system))
        self.controls = [part for part in self.layout_spec['controls']
                         if part['action'] in self.labels]
        if image := self.layout_spec.get('image'):
            self.renderer = RasterRenderer(ART / image)
        else:
            self.renderer = QSvgRenderer(str(ART / f'{key}.svg'))
        self.setAccessibleName(self.layout_spec['title'])
        self.setAccessibleDescription('Controller preview. All bindings are also available in the button list.')
        self.update()

    def set_selected(self, action):
        if self.selected != action:
            self.selected = action
            self.update()

    def artwork_transform(self):
        width, height = self.layout_spec['size']
        scale = max(.01, min((self.width()-24)/width, (self.height()-24)/height))
        transform = QTransform()
        transform.translate((self.width()-width*scale)/2, (self.height()-height*scale)/2)
        transform.scale(scale, scale)
        return transform

    def action_at(self, position):
        point = self.artwork_transform().inverted()[0].map(position)
        for part in self.controls:
            rect = QRectF(*part['rect'])
            local_point = self.control_transform(part).inverted()[0].map(point)
            if rect.contains(local_point):
                if part['ellipse']:
                    dx = (local_point.x()-rect.center().x())/(rect.width()/2)
                    dy = (local_point.y()-rect.center().y())/(rect.height()/2)
                    if dx*dx + dy*dy > 1:
                        continue
                return part['action']
        return None

    @staticmethod
    def control_transform(part):
        """Keep tilted controls' hit areas and highlights aligned with the art."""
        transform = QTransform()
        if angle := part.get('rotation', 0):
            center = QRectF(*part['rect']).center()
            transform.translate(center.x(), center.y())
            transform.rotate(angle)
            transform.translate(-center.x(), -center.y())
        return transform

    def mouseMoveEvent(self, event):
        action = self.action_at(event.position())
        self.setCursor(Qt.PointingHandCursor if action else Qt.ArrowCursor)
        self.set_selected(action)
        self.setToolTip(self.labels.get(action, ''))
        self.controlHovered.emit(action or '')
        super().mouseMoveEvent(event)

    def mousePressEvent(self, event):
        action = self.action_at(event.position())
        if event.button() == Qt.LeftButton and action:
            self.controlPicked.emit(action)
            event.accept()
        else:
            super().mousePressEvent(event)

    def leaveEvent(self, event):
        self.set_selected(None)
        self.controlHovered.emit('')
        super().leaveEvent(event)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        transform = self.artwork_transform()
        painter.setTransform(transform)
        width, height = self.layout_spec['size']
        self.renderer.render(painter, QRectF(0, 0, width, height))
        accent = self.palette().color(QPalette.Highlight)
        painter.setPen(QPen(accent, 3/transform.m11()))
        fill = accent.toRgb()
        fill.setAlpha(55)
        painter.setBrush(fill)
        for part in self.controls:
            if part['action'] != self.selected:
                continue
            painter.save()
            painter.setTransform(self.control_transform(part), combine=True)
            rect = QRectF(*part['rect']).adjusted(-3, -3, 3, 3)
            if part['ellipse']:
                painter.drawEllipse(rect)
            else:
                painter.drawRoundedRect(rect, 5, 5)
            painter.restore()
