"""Smooth discrete mouse wheels while preserving direct touchpad scrolling."""
from PySide6.QtCore import QObject, QEvent, QVariantAnimation, QEasingCurve
from PySide6.QtWidgets import QApplication


class SmoothScroll(QObject):
    def __init__(self, view):
        super().__init__(view)
        self.view = view
        self.bar = view.verticalScrollBar()
        self.target = self.bar.value()
        self.writing = False
        self.animation = QVariantAnimation(self)
        self.animation.setDuration(160)
        self.animation.setEasingCurve(QEasingCurve.OutCubic)
        self.animation.valueChanged.connect(self.write_value)
        self.bar.valueChanged.connect(self.external_change)
        self.bar.rangeChanged.connect(self.stop)
        view.viewport().installEventFilter(self)
        view.installEventFilter(self)

    def stop(self, *_):
        self.animation.stop()
        self.target = self.bar.value()

    def external_change(self, *_):
        if not self.writing:
            self.stop()

    def write_value(self, value):
        self.writing = True
        try:
            self.bar.setValue(round(value))
        finally:
            self.writing = False

    def handle_wheel(self, event):
        from PySide6.QtCore import Qt, QAbstractAnimation
        if (event.modifiers() & (Qt.ControlModifier | Qt.ShiftModifier | Qt.AltModifier)
                or self.bar.maximum() <= self.bar.minimum()):
            return False
        pixels = event.pixelDelta()
        if not pixels.isNull():
            if not pixels.y():
                return False
            self.stop()
            self.bar.setValue(self.bar.value() - pixels.y())
            event.accept()
            return True
        angle = event.angleDelta().y()
        if not angle:
            return False
        delta = -angle / 120 * max(1, QApplication.wheelScrollLines()) * 40
        running = self.animation.state() == QAbstractAnimation.Running
        current = self.bar.value()
        # Reversing direction should respond immediately, without fighting
        # momentum left over from the previous wheel direction.
        previous = self.target if running and (self.target-current) * delta > 0 else current
        target = min(self.bar.maximum(), max(self.bar.minimum(), previous + delta))
        self.animation.stop()
        self.target = target
        self.animation.setStartValue(float(current))
        self.animation.setEndValue(float(target))
        if target != current:
            self.animation.start()
        event.accept()
        return True

    def eventFilter(self, watched, event):
        if event.type() == QEvent.Wheel:
            return self.handle_wheel(event)
        if event.type() in (QEvent.MouseButtonPress, QEvent.KeyPress, QEvent.Hide):
            self.stop()
        return super().eventFilter(watched, event)
