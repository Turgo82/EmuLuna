"""Friendly per-console controller setup, backed by persistent input profiles."""
import time
from functools import lru_cache
from PySide6.QtCore import Qt, Signal, QTimer, QRectF
from PySide6.QtGui import QIcon, QPixmap, QPainter, QColor
from PySide6.QtWidgets import (QWidget, QHBoxLayout, QVBoxLayout, QFormLayout,
    QPushButton, QScrollArea, QLabel, QMenu, QSizePolicy, QGridLayout, QFrame,
    QStylePainter, QStyleOptionButton, QStyle)

from .controller_profiles import (SPECS, PAD_BUTTONS, actions, axis_label, bind,
                                  binding_label, defaults, load_profile, save_profile)
from .controller_diagrams import ControllerDiagram
from .settings_style import SettingsComboBox as QComboBox
from .gamepad import Gamepad
from .systems import SYSTEMS, DATA


@lru_cache(maxsize=1)
def controller_wood_texture():
    return QPixmap(str(DATA / 'textures' / 'controller-walnut.png'))


def paint_controller_wood(painter, bounds):
    """One continuous surface beneath the Controls navigation and widgets."""
    texture = controller_wood_texture()
    painter.setRenderHint(QPainter.SmoothPixmapTransform)
    painter.fillRect(bounds, QColor('#52331f'))
    if not texture.isNull() and not bounds.isEmpty():
        scale = max(bounds.width() / texture.width(), bounds.height() / texture.height())
        width, height = bounds.width() / scale, bounds.height() / scale
        source = QRectF((texture.width() - width) / 2,
                        (texture.height() - height) / 2, width, height)
        painter.drawPixmap(bounds, texture, source)
    painter.fillRect(bounds, QColor(0, 0, 0, 18))


class BindingButton(QPushButton):
    highlighted = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName('controlBinding')
        self.setMinimumWidth(90)
        self.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Fixed)
        self.setAutoDefault(False)

    def paintEvent(self, event):
        option = QStyleOptionButton()
        self.initStyleOption(option)
        option.text = self.fontMetrics().elidedText(self.text(), Qt.ElideRight, max(0, self.width()-24))
        painter = QStylePainter(self)
        painter.drawControl(QStyle.CE_PushButton, option)

    def enterEvent(self, event):
        self.highlighted.emit()
        super().enterEvent(event)

    def focusInEvent(self, event):
        self.highlighted.emit()
        super().focusInEvent(event)


class KeyBindingButton(BindingButton):
    captured = Signal(object)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.recording = False
        self.clicked.connect(self.begin)

    def begin(self):
        self.recording = True
        self.setText('Press a key…')
        self.setFocus(Qt.MouseFocusReason)
        self.grabKeyboard()

    def finish(self):
        if self.recording:
            self.releaseKeyboard()
            self.recording = False

    def keyPressEvent(self, event):
        if not self.recording:
            return super().keyPressEvent(event)
        if event.key() == Qt.Key_Escape:
            self.finish()
            self.captured.emit(False)
        elif event.key() == Qt.Key_Delete:
            self.finish()
            self.captured.emit(None)
        elif event.key() not in (Qt.Key_Shift, Qt.Key_Control, Qt.Key_Alt, Qt.Key_Meta):
            self.finish()
            self.captured.emit(f'key:{int(event.key())}')
        event.accept()

    def focusOutEvent(self, event):
        if self.recording:
            self.finish()
            self.captured.emit(False)
        super().focusOutEvent(event)


class ControllerBindingButton(BindingButton):
    key_command = Signal(object)

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape:
            self.key_command.emit(False)
            event.accept()
        elif event.key() == Qt.Key_Delete:
            self.key_command.emit(None)
            event.accept()
        else:
            super().keyPressEvent(event)


class ControlsPage(QWidget):
    changed = Signal()

    def __init__(self, library, parent=None):
        super().__init__(parent)
        self.setObjectName('woodControlsPage')
        self.library = library
        self.loading = False
        self.capture_pad = None
        self.capture_action = None
        self.capture_button = None
        self.capture_previous_buttons = set()
        self.capture_neutral_axes = (0, 0, 0, 0, 0, 0)
        self.capture_deadline = 0
        self.available_devices = []
        self.devices_loaded = False
        self.capture_timer = QTimer(self)
        self.capture_timer.setInterval(16)
        self.capture_timer.timeout.connect(self.poll_controller_capture)
        self.device_scan_timer = QTimer(self)
        self.device_scan_timer.setSingleShot(True)
        self.device_scan_timer.timeout.connect(self.refresh_devices)
        outer = QVBoxLayout(self)
        outer.setContentsMargins(16, 16, 16, 14)
        outer.setSpacing(14)
        selectors = QGridLayout()
        selectors.setHorizontalSpacing(16)
        selectors.setVerticalSpacing(10)
        selectors.setColumnStretch(0, 2)
        selectors.setColumnStretch(1, 1)
        selectors.setColumnStretch(2, 1)
        self.system = QComboBox()
        for key, system in SYSTEMS.items():
            self.system.addItem(QIcon(str(system.icon)), system.name, key)
        self.player = QComboBox()
        self.source = QComboBox()
        self.source.addItem('Keyboard', 'keyboard')
        self.source.addItem('Controller', 'gamepad')
        self.device = QComboBox()
        self.device_label = None
        self.device_field = None
        for index, (label, widget) in enumerate((('Console', self.system), ('Player', self.player), ('Input', self.source), ('Device', self.device))):
            field = QWidget()
            box = QVBoxLayout(field)
            box.setContentsMargins(0, 0, 0, 0)
            caption = QLabel(label)
            caption.setBuddy(widget)
            widget.setAccessibleName(label)
            widget.setMinimumContentsLength(8)
            widget.setSizeAdjustPolicy(QComboBox.AdjustToMinimumContentsLengthWithIcon)
            widget.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
            if label == 'Device':
                self.device_label = caption
                self.device_field = field
            caption.setObjectName('subtle')
            box.addWidget(caption)
            box.addWidget(widget)
            if index == 3:
                selectors.addWidget(field, 1, 0, 1, 3)
            else:
                selectors.addWidget(field, 0, index)
        outer.addLayout(selectors)

        body = QHBoxLayout()
        body.setSpacing(14)
        self.preview_card = QFrame()
        self.preview_card.setObjectName('controllerPreviewCard')
        preview = QVBoxLayout(self.preview_card)
        preview.setContentsMargins(12, 12, 12, 12)
        self.diagram_title = QLabel()
        self.diagram_title.setObjectName('controlTitle')
        self.diagram_title.setWordWrap(True)
        self.diagram_title.setAlignment(Qt.AlignCenter)
        preview.addWidget(self.diagram_title)
        self.diagram = ControllerDiagram()
        self.diagram.controlPicked.connect(self.pick_control)
        self.diagram.controlHovered.connect(self.describe_control)
        preview.addWidget(self.diagram, 1)
        self.diagram_hint = QLabel('Click a control to change its binding')
        self.diagram_hint.setObjectName('subtle')
        self.diagram_hint.setAlignment(Qt.AlignCenter)
        self.diagram_hint.setWordWrap(True)
        preview.addWidget(self.diagram_hint)
        self.diagram_note = QLabel()
        self.diagram_note.setObjectName('subtle')
        self.diagram_note.setWordWrap(True)
        preview.addWidget(self.diagram_note)
        body.addWidget(self.preview_card, 1)
        mapping_card = QFrame()
        mapping_card.setObjectName('settingsCard')
        mapping_card.setMinimumWidth(300)
        # Binding fields need a compact column, not half of a wide window.
        # Leave all additional width to the aspect-preserving artwork preview.
        mapping_card.setMaximumWidth(360)
        mapping_box = QVBoxLayout(mapping_card)
        mapping_box.setContentsMargins(4, 4, 4, 4)
        right = QWidget()
        right.setObjectName('controlMappings')
        self.mapping_layout = QFormLayout(right)
        self.mapping_layout.setContentsMargins(12, 12, 12, 12)
        self.mapping_layout.setSpacing(8)
        self.mapping_layout.setFieldGrowthPolicy(QFormLayout.AllNonFixedFieldsGrow)
        self.mapping_layout.setLabelAlignment(Qt.AlignLeft | Qt.AlignVCenter)
        self.mapping_scroll = QScrollArea()
        self.mapping_scroll.setFrameShape(QFrame.NoFrame)
        self.mapping_scroll.setWidgetResizable(True)
        self.mapping_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.mapping_scroll.setWidget(right)
        self.mapping_scroll.viewport().setAutoFillBackground(False)
        mapping_box.addWidget(self.mapping_scroll)
        body.addWidget(mapping_card)
        outer.addLayout(body, 1)

        footer = QHBoxLayout()
        self.summary = QLabel()
        self.summary.setObjectName('subtle')
        self.summary.setWordWrap(True)
        footer.addWidget(self.summary, 1)
        reset = QPushButton('Restore defaults')
        reset.clicked.connect(self.restore_defaults)
        footer.addWidget(reset)
        outer.addLayout(footer)

        self.system.currentIndexChanged.connect(self.system_changed)
        self.player.currentIndexChanged.connect(self.player_changed)
        self.source.currentIndexChanged.connect(self.source_changed)
        self.device.currentIndexChanged.connect(self.save_device)
        self.device_timer = QTimer(self)
        self.device_timer.setInterval(1500)
        self.device_timer.timeout.connect(self.refresh_devices_if_visible)
        self.device_timer.start()
        self.system_changed()

    def refresh_devices_if_visible(self):
        if (self.isVisible() and self.source.currentData() == 'gamepad'
                and not self.capture_pad):
            self.refresh_devices()

    @property
    def system_key(self):
        return self.system.currentData()

    @property
    def player_index(self):
        return max(0, self.player.currentIndex())

    def system_changed(self):
        previous = self.player_index
        self.loading = True
        self.player.clear()
        for index in range(SPECS[self.system_key]['players']):
            self.player.addItem(f'Player {index + 1}', index)
        self.player.setCurrentIndex(min(previous, self.player.count() - 1))
        self.loading = False
        self.diagram.set_system(self.system_key)
        self.diagram_title.setText(self.diagram.layout_spec['title'])
        self.diagram_note.setText(self.diagram.layout_spec.get('note', ''))
        self.diagram_note.setVisible(bool(self.diagram_note.text()))
        self.populate_devices()
        self.reload()

    def refresh_devices(self):
        self.available_devices = Gamepad.devices()
        self.devices_loaded = True
        self.populate_devices()

    def populate_devices(self):
        self.device.blockSignals(True)
        self.device.clear()
        self.device.addItem(f'Automatic (controller {self.player_index + 1})', 'auto')
        for value, name in self.available_devices:
            self.device.addItem(name, value)
        profile = load_profile(self.library, self.system_key, self.player_index)
        wanted = profile['device']
        index = self.device.findData(wanted)
        if index < 0 and wanted != 'auto':
            self.device.addItem('Disconnected controller', wanted)
            index = self.device.count() - 1
        self.device.setCurrentIndex(max(0, index))
        self.device.blockSignals(False)

    def player_changed(self):
        if self.loading:
            return
        self.populate_devices()
        self.reload()

    def source_changed(self):
        self.reload()
        if (self.source.currentData() == 'gamepad' and self.isVisible()
                and not self.devices_loaded):
            self.device_scan_timer.start(0)

    def clear_form(self):
        for button in getattr(self, 'mapping_buttons', {}).values():
            if isinstance(button, KeyBindingButton):
                button.finish()
        while self.mapping_layout.rowCount():
            self.mapping_layout.removeRow(0)
        self.mapping_buttons = {}

    def reload(self):
        if self.loading or self.player.currentIndex() < 0:
            return
        self.cancel_controller_capture()
        self.clear_form()
        source = self.source.currentData()
        self.device_field.setVisible(source == 'gamepad')
        groups = {}
        part_groups = {part['action']:part['group'] for part in self.diagram.controls}
        part_groups.update(self.diagram.layout_spec.get('extra_groups', {}))
        for action, label in actions(self.system_key):
            group = part_groups.get(action, 'Analog controls' if action.startswith('axis:') else 'Additional controls')
            groups.setdefault(group, []).append((action,label))
        for group, entries in groups.items():
            heading = QLabel(group)
            heading.setObjectName('controlGroup')
            self.mapping_layout.addRow(heading)
            for action, label in entries:
                self.add_binding_row(action, label, source)
        self.refresh_bindings()
        self.mapping_scroll.verticalScrollBar().setValue(0)
        self.summary.setText(f'Player {self.player_index + 1} · Changes also apply to running games')

    def add_binding_row(self, action, label, source):
        button = KeyBindingButton() if source == 'keyboard' else ControllerBindingButton()
        button.setPalette(self.palette())
        button.setAccessibleName(f'{label} binding')
        button.highlighted.connect(lambda: self.highlight_control(action))
        if source == 'keyboard':
            button.captured.connect(lambda token, action=action: self.capture(action, token))
        else:
            button.clicked.connect(lambda checked=False, action=action, button=button:
                                   self.start_controller_capture(action, button))
            button.key_command.connect(lambda token, action=action:
                                       self.controller_key_command(action, token))
            button.setContextMenuPolicy(Qt.CustomContextMenu)
            button.customContextMenuRequested.connect(
                lambda point, action=action, button=button: self.controller_menu(action, button))
        caption = QLabel(label)
        caption.setWordWrap(True)
        caption.setMaximumWidth(118)
        caption.setMinimumWidth(92)
        caption.setBuddy(button)
        self.mapping_layout.addRow(caption, button)
        self.mapping_buttons[action] = button


    def describe_control(self, action):
        self.diagram_hint.setText(dict(actions(self.system_key)).get(action, 'Click a control to change its binding'))

    def highlight_control(self, action):
        self.diagram.set_selected(action)
        self.describe_control(action)

    def pick_control(self, action):
        button = self.mapping_buttons.get(action)
        if button is not None:
            self.mapping_scroll.ensureWidgetVisible(button, 0, 24)
            button.setFocus(Qt.MouseFocusReason)
            button.click()

    def refresh_bindings(self):
        """Update in place: remapping must preserve focus and scroll position."""
        profile = load_profile(self.library, self.system_key, self.player_index)
        source = self.source.currentData()
        instruction = ('Click and press a key; Escape cancels; Delete clears' if source == 'keyboard'
                       else 'Click and press a control; right-click for a list; Escape cancels; Delete clears')
        for action, button in self.mapping_buttons.items():
            binding = binding_label(profile[source].get(action, []))
            button.setText(binding)
            button.setToolTip(f'{binding}\n{instruction}')

    def capture(self, action, token):
        if token is False:
            self.refresh_bindings()
            return
        profile = load_profile(self.library, self.system_key, self.player_index)
        bind(profile, 'keyboard', action, token)
        save_profile(self.library, self.system_key, self.player_index, profile)
        self.changed.emit()
        self.refresh_bindings()

    def controller_menu(self, action, button):
        self.cancel_controller_capture()
        menu = QMenu(button)
        clear = menu.addAction('Not assigned')
        clear.triggered.connect(lambda: self.capture_controller(action, None))
        button_menu = menu.addMenu('Controller button')
        for index, label in enumerate(PAD_BUTTONS):
            button_menu.addAction(label, lambda checked=False, token=f'button:{index}': self.capture_controller(action, token))
        axis_menu = menu.addMenu('Controller axis')
        for index in range(6):
            for sign in (-1, 1):
                axis_menu.addAction(axis_label(index, sign),
                    lambda checked=False, token=f'axis:{index}:{sign}': self.capture_controller(action, token))
        menu.exec(button.mapToGlobal(button.rect().bottomLeft()))

    def start_controller_capture(self, action, button):
        self.cancel_controller_capture()
        pad = Gamepad(device=self.device.currentData() or 'auto', player=self.player_index)
        pad.poll()
        if not pad.pad:
            pad.close()
            self.controller_menu(action, button)
            return
        self.capture_pad = pad
        self.capture_action = action
        self.capture_button = button
        self.capture_previous_buttons = set(pad.buttons)
        self.capture_neutral_axes = tuple(pad.raw_axes)
        self.capture_deadline = time.monotonic() + 8
        button.setText('Press a control…')
        button.setFocus(Qt.MouseFocusReason)
        self.capture_timer.start()

    def poll_controller_capture(self):
        if not self.capture_pad:
            return
        if time.monotonic() >= self.capture_deadline:
            self.cancel_controller_capture(restore=True)
            return
        self.capture_pad.poll()
        if not self.capture_pad.pad:
            self.cancel_controller_capture(restore=True)
            return
        current_buttons = set(self.capture_pad.buttons)
        pressed = current_buttons - self.capture_previous_buttons
        self.capture_previous_buttons = current_buttons
        token = f'button:{min(pressed)}' if pressed else None
        if token is None:
            neutral_axes = list(self.capture_neutral_axes)
            for index, (value, neutral) in enumerate(zip(self.capture_pad.raw_axes,
                                                         neutral_axes)):
                if index < 4:
                    # A stick already held when capture starts must return to
                    # center before its next deliberate direction is accepted.
                    if abs(value) < 8000:
                        neutral_axes[index] = value
                    elif abs(value) >= 16000 and abs(value - neutral) >= 12000:
                        token = f'axis:{index}:{1 if value > 0 else -1}'
                        break
                else:
                    # SDL normalizes triggers as low at rest and high when
                    # pressed. Never mistake releasing a held trigger for a
                    # negative-axis binding.
                    if value < neutral:
                        neutral_axes[index] = value
                    elif value > 8000 and value - neutral >= 16000:
                        token = f'axis:{index}:1'
                        break
            self.capture_neutral_axes = tuple(neutral_axes)
        if token:
            action = self.capture_action
            self.cancel_controller_capture()
            self.capture_controller(action, token)

    def cancel_controller_capture(self, restore=True):
        was_active = self.capture_pad is not None
        action = self.capture_action
        button = self.capture_button
        self.capture_timer.stop()
        if self.capture_pad:
            self.capture_pad.close()
        self.capture_pad = None
        self.capture_action = None
        self.capture_button = None
        if restore and was_active and button is not None and action is not None:
            source = self.source.currentData()
            profile = load_profile(self.library, self.system_key, self.player_index)
            button.setText(binding_label(profile[source].get(action, [])))

    def controller_key_command(self, action, token):
        self.cancel_controller_capture()
        if token is False:
            self.refresh_bindings()
        else:
            self.capture_controller(action, token)

    def capture_controller(self, action, token):
        profile = load_profile(self.library, self.system_key, self.player_index)
        bind(profile, 'gamepad', action, token)
        save_profile(self.library, self.system_key, self.player_index, profile)
        self.changed.emit()
        self.refresh_bindings()

    def save_device(self):
        if self.loading or self.device.currentIndex() < 0:
            return
        self.cancel_controller_capture()
        profile = load_profile(self.library, self.system_key, self.player_index)
        profile['device'] = self.device.currentData()
        save_profile(self.library, self.system_key, self.player_index, profile)
        self.changed.emit()

    def restore_defaults(self):
        self.cancel_controller_capture()
        profile = defaults(self.system_key, self.player_index)
        save_profile(self.library, self.system_key, self.player_index, profile)
        self.changed.emit()
        self.populate_devices()
        self.reload()

    def hideEvent(self, event):
        self.cancel_controller_capture()
        self.device_scan_timer.stop()
        super().hideEvent(event)

    def showEvent(self, event):
        super().showEvent(event)
        if self.source.currentData() == 'gamepad' and not self.devices_loaded:
            self.device_scan_timer.start(0)
