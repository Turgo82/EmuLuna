"""Automatic N64 frame alignment and per-game native overscan controls."""
import json
import time

from PySide6.QtCore import Qt, QRect
from PySide6.QtGui import QImage
from PySide6.QtWidgets import (QCheckBox, QDialog, QDialogButtonBox,
                              QFormLayout, QLabel, QSpinBox, QVBoxLayout)


EDGES = ("top", "bottom", "left", "right")


def border_settings(value="{}"):
    try:
        saved = json.loads(value)
    except (TypeError, ValueError):
        saved = {}
    if not isinstance(saved, dict):
        saved = {}
    result = {"enabled": saved.get("enabled") is True, "automatic": saved.get("automatic", True) is not False}
    for edge in EDGES:
        amount = saved.get(edge, 0)
        result[edge] = max(0, min(50, amount)) if type(amount) is int else 0
    return result


def core_border_options(settings, renderer="gliden64"):
    if renderer == "angrylion":
        return {"parallel-n64-angrylion-overscan": "enabled" if settings["enabled"] else "disabled"}
    if renderer != "gliden64":
        raise ValueError("Unsupported N64 border renderer")
    prefix = "parallel-n64-gliden64-"
    options = {prefix + "EnableOverscan": "Enabled" if settings["enabled"] else "Disabled"}
    options.update({prefix + "Overscan" + edge.title(): str(settings[edge]) for edge in EDGES})
    return options


def _frame_margins(image):
    """Find fully black padding; leave fades and cinematic framing alone."""
    empty = (0, 0, 0, 0)
    if image.isNull():
        return empty
    if image.format() != QImage.Format_RGB32:
        image = image.convertToFormat(QImage.Format_RGB32)
    width, height = image.width(), image.height()
    data = bytes(image.constBits())
    stride = image.bytesPerLine()
    black = b'\0\0\0\xff' * width
    top = bottom = 0
    limit_y = height // 2
    while top < limit_y and data[top*stride:top*stride+width*4] == black:
        top += 1
    while bottom < limit_y and data[(height-bottom-1)*stride:(height-bottom-1)*stride+width*4] == black:
        bottom += 1
    if not limit_y or top == limit_y or bottom == limit_y:
        return None
    zero_column = bytes(height - top - bottom)

    def black_column(x):
        start = top * stride + x * 4
        end = (height - bottom) * stride + x * 4
        return all(data[start + channel:end + channel:stride] == zero_column
                   for channel in range(3))

    left = right = 0
    limit_x = width // 2
    while left < limit_x and black_column(left):
        left += 1
    while right < limit_x and black_column(width-right-1):
        right += 1
    if (not limit_x or left == limit_x or right == limit_x
            or top + bottom > height // 4 or left + right > width // 4):
        return empty
    return left, top, right, bottom


def _crop_frame(image, margins):
    left, top, right, bottom = margins
    if not any(margins):
        return image
    return image.copy(QRect(left, top, image.width()-left-right,
                           image.height()-top-bottom))


def trim_n64_frame(image):
    """Single-frame trim for previews; playback uses settled border detection."""
    return _crop_frame(image, _frame_margins(image) or (0, 0, 0, 0))


def _mostly_black(image):
    """Ignore fades and dark scenes when choosing the session's crop."""
    if image.format() != QImage.Format_RGB32:
        image = image.convertToFormat(QImage.Format_RGB32)
    data = bytes(image.constBits())
    stride = image.bytesPerLine()
    dark = samples = 0
    for y in range(0, image.height(), max(1, image.height() // 24)):
        for x in range(0, image.width(), max(1, image.width() // 32)):
            offset = y * stride + x * 4
            dark += max(data[offset:offset + 3]) <= 24
            samples += 1
    return dark >= samples * 0.8


class N64FrameBorders:
    """Choose persistent borders once, then keep them for the play session."""
    SETTLE_SECONDS = 3.0

    def __init__(self):
        self.size = None
        self.margins = (0, 0, 0, 0)
        self.pending = None
        self.pending_since = 0.0
        self.locked = False

    def apply(self, image, now=None):
        now = time.monotonic() if now is None else now
        if image.isNull():
            return image
        size = (image.width(), image.height())
        if self.locked:
            # Some cores change resolution between menus and gameplay. Keep
            # the same proportional crop without restarting border detection.
            width, height = self.size
            margins = tuple(round(value * scale) for value, scale in zip(
                self.margins, (size[0]/width, size[1]/height,
                               size[0]/width, size[1]/height)))
            return _crop_frame(image, margins)
        if size != self.size:
            self.__init__()
            self.size = size
        if _mostly_black(image):
            self.pending = None
            return image
        detected = _frame_margins(image)
        if not detected or not any(detected):
            self.pending = None
        elif self.pending is None:
            self.pending = detected
            self.pending_since = now
        else:
            # Retain only padding that stayed black throughout the settling time.
            self.pending = tuple(min(old, new) for old, new in zip(self.pending, detected))
            if now - self.pending_since >= self.SETTLE_SECONDS:
                self.margins = self.pending
                self.locked = any(self.margins)
                self.pending = None
        return _crop_frame(image, self.margins)


class N64BorderControls(QDialog):
    def __init__(self, settings, changed, parent=None, *, renderer="gliden64"):
        super().__init__(parent)
        self.setWindowTitle("Screen borders")
        self.setAttribute(Qt.WA_DeleteOnClose)
        self.changed = changed
        layout = QVBoxLayout(self)
        description = QLabel(
            ("Let the software core trim the unused edges of its picture automatically. "
             if renderer == "angrylion" else
             "Trim the black margins drawn by this game. Adjust each edge separately "
             "to avoid cutting off the picture. ") +
            "Automatic trimming ignores dark scenes and locks its first settled adjustment for this play session. "
            "Core crop changes require reopening the game. "
            "Choices are remembered for this game and renderer.")
        description.setWordWrap(True)
        layout.addWidget(description)
        self.automatic = QCheckBox("Automatically trim and center unused borders")
        self.automatic.setChecked(settings["automatic"])
        self.automatic.setEnabled(not settings["enabled"])
        self.automatic.toggled.connect(self.apply)
        layout.addWidget(self.automatic)
        self.enabled = QCheckBox("Hide overscan" if renderer == "angrylion" else "Crop screen borders")
        self.enabled.setChecked(settings["enabled"])
        layout.addWidget(self.enabled)
        form = QFormLayout()
        self.fields = {}
        for edge in (() if renderer == "angrylion" else EDGES):
            field = QSpinBox()
            field.setRange(0, 50)
            field.setSuffix(" pixels")
            field.setAccessibleName(edge.title() + " border")
            field.setKeyboardTracking(False)
            field.setValue(settings[edge])
            field.setEnabled(settings["enabled"])
            field.valueChanged.connect(self.apply)
            form.addRow(edge.title(), field)
            self.fields[edge] = field
        layout.addLayout(form)
        buttons = QDialogButtonBox(QDialogButtonBox.Close | QDialogButtonBox.RestoreDefaults)
        buttons.rejected.connect(self.close)
        buttons.button(QDialogButtonBox.RestoreDefaults).clicked.connect(self.restore)
        layout.addWidget(buttons)
        self.enabled.toggled.connect(self.apply)
        self.setMinimumWidth(380)

    def apply(self):
        enabled = self.enabled.isChecked()
        self.automatic.setEnabled(not enabled)
        for field in self.fields.values():
            field.setEnabled(enabled)
        self.changed({"enabled": enabled, "automatic": self.automatic.isChecked(), **{edge: field.value() for edge, field in self.fields.items()}})

    def restore(self):
        for widget in (self.enabled, self.automatic, *self.fields.values()):
            widget.blockSignals(True)
        self.enabled.setChecked(False)
        self.automatic.setChecked(True)
        for field in self.fields.values():
            field.setValue(0)
        for widget in (self.enabled, self.automatic, *self.fields.values()):
            widget.blockSignals(False)
        self.apply()
