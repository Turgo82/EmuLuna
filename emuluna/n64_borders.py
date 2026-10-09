"""Per-game overscan controls using the N64 renderer's own crop options."""
import json

from PySide6.QtCore import Qt
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
    result = {"enabled": saved.get("enabled") is True}
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
            "Changes are remembered for this game and renderer. Close and reopen the game to apply them.")
        description.setWordWrap(True)
        layout.addWidget(description)
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
        for field in self.fields.values():
            field.setEnabled(enabled)
        self.changed({"enabled": enabled, **{edge: field.value() for edge, field in self.fields.items()}})

    def restore(self):
        for widget in (self.enabled, *self.fields.values()):
            widget.blockSignals(True)
        self.enabled.setChecked(False)
        for field in self.fields.values():
            field.setValue(0)
        for widget in (self.enabled, *self.fields.values()):
            widget.blockSignals(False)
        self.apply()
