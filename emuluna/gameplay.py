"""Transient gameplay controls; all emulator actions stay in the player API."""
from PySide6.QtCore import Qt, QEvent, QTimer
from PySide6.QtGui import QPainter, QPalette, QIcon
from PySide6.QtWidgets import QFrame, QHBoxLayout, QSlider, QToolButton, QStyle, QApplication, QLabel
from .branding import navigation_icon


def hud_icon(widget, standard):
    if standard in (QStyle.SP_DialogSaveButton, QStyle.SP_DialogOpenButton):
        return widget.style().standardIcon(standard)
    pixmap = widget.style().standardIcon(standard).pixmap(20, 20)
    painter = QPainter(pixmap)
    painter.setCompositionMode(QPainter.CompositionMode_SourceIn)
    painter.fillRect(pixmap.rect(), widget.palette().color(QPalette.WindowText))
    painter.end()
    return QIcon(pixmap)


def set_hud_icon(action, widget, standard):
    # Remember the current symbol, including the dynamic pause/mute states,
    # so recoloring never changes the action's meaning.
    action.setProperty('emuluna_hud_icon', standard if isinstance(standard, str) else int(standard))
    action.setIcon(navigation_icon(standard, '#ffffff' if standard == 'power' else None) if isinstance(standard, str)
                   else hud_icon(widget, standard))


class GameplayHUD(QFrame):
    def __init__(self, screen, *, actions, options, volume, set_volume, paused, hide_cursor=True, order=None):
        super().__init__(screen)
        self.screen, self.paused, self.hide_cursor = screen, paused, hide_cursor
        self.setObjectName("gameplayHUD")
        self.setCursor(Qt.ArrowCursor)
        self.setStyleSheet("""
            QFrame#gameplayHUD {background:palette(window);border:1px solid palette(mid);border-radius:12px;}
            QToolButton {color:palette(window-text);background:transparent;padding:8px;border:0;border-radius:6px;}
            QToolButton:hover {background:palette(midlight);}
            QToolButton#powerButton {background:#b3261e;border:1px solid #7f1d1d;border-radius:6px;padding:8px 12px;}
            QToolButton#powerButton:hover {background:#d13a32;}
            QSlider::groove:horizontal {height:5px;background:palette(mid);}
            QSlider::handle:horizontal {background:palette(highlight);width:13px;margin:-4px 0;border-radius:6px;}
        """)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(10, 6, 10, 6)
        layout.setSpacing(4)
        self.buttons = {}
        for name, action, icon in actions:
            button = QToolButton(self)
            button.setDefaultAction(action)
            set_hud_icon(action, self, icon)
            button.setFocusPolicy(Qt.NoFocus)
            button.setAccessibleName(action.text())
            action.changed.connect(lambda button=button, action=action: button.setAccessibleName(action.text()))
            if name == 'power':
                button.setObjectName('powerButton')
            self.buttons[name] = button
        self.volume = QSlider(Qt.Horizontal, self)
        self.volume.setRange(0, 100)
        self.volume.setValue(volume)
        self.volume.setFixedWidth(85)
        self.volume.setFocusPolicy(Qt.NoFocus)
        self.volume.setAccessibleName("Game volume")
        self.volume.setToolTip("Game volume")
        self.volume.valueChanged.connect(set_volume)
        self.options = QToolButton(self)
        self.options.setText("Options")
        self.options.setIcon(navigation_icon('list'))
        self.options.setMenu(options)
        self.options.setPopupMode(QToolButton.InstantPopup)
        self.options.setFocusPolicy(Qt.NoFocus)
        self.options.setAccessibleName("Gameplay options")
        self.options.setToolTip("Gameplay options")
        widgets = {**self.buttons, 'volume': self.volume, 'options': self.options}
        for name in order or (*self.buttons, 'volume', 'options'):
            if name == 'fullscreen':
                layout.addSpacing(8)
            layout.addWidget(widgets[name])
        self.timer = QTimer(self)
        self.timer.setSingleShot(True)
        self.timer.setInterval(2500)
        self.timer.timeout.connect(self.fade_away)
        screen.setMouseTracking(True)
        screen.installEventFilter(self)
        for widget in (self, *self.buttons.values(), self.volume, self.options):
            widget.setMouseTracking(True)
            widget.installEventFilter(self)
        self.position()
        self.reveal()

    def changeEvent(self, event):
        super().changeEvent(event)
        if event.type() in (QEvent.PaletteChange, QEvent.ApplicationPaletteChange):
            for button in getattr(self, 'buttons', {}).values():
                action = button.defaultAction()
                symbol = action.property('emuluna_hud_icon')
                if symbol is not None:
                    action.setIcon(navigation_icon(symbol, '#ffffff' if symbol == 'power' else None) if isinstance(symbol, str)
                                   else hud_icon(self, QStyle.StandardPixmap(symbol)))
            if hasattr(self, 'options'):
                self.options.setIcon(navigation_icon('list'))

    def position(self):
        for button in (*self.buttons.values(), self.options):
            button.setToolButtonStyle(Qt.ToolButtonIconOnly)
        self.volume.setVisible(self.screen.width() >= 540)
        self.adjustSize()
        self.move(max(8, (self.screen.width() - self.width()) // 2), max(8, self.screen.height() - self.height() - 20))

    def reveal(self):
        self.screen.unsetCursor()
        self.position()
        self.show()
        self.raise_()
        self.timer.start()

    def fade_away(self):
        if QApplication.mouseButtons() != Qt.NoButton or self.paused() or QApplication.activePopupWidget():
            self.timer.start()
            return
        self.hide()
        if self.hide_cursor:
            self.screen.setCursor(Qt.BlankCursor)

    def eventFilter(self, watched, event):
        # Hiding a child under a stationary pointer generates Enter on the
        # screen. That synthetic transition must not immediately reopen us.
        if event.type() == QEvent.MouseMove or (event.type() == QEvent.Enter and self.isVisible()):
            self.reveal()
        elif event.type() == QEvent.Resize and watched is self.screen:
            self.position()
        elif event.type() == QEvent.Leave and watched is self.screen:
            self.screen.unsetCursor()
        return super().eventFilter(watched, event)

    def stop(self):
        self.timer.stop()
        self.screen.unsetCursor()


class GameplayNotice(QLabel):
    """Non-interactive status overlay that remains visible in fullscreen."""
    def __init__(self, screen):
        super().__init__(screen)
        self.screen = screen
        self.setObjectName('gameplayNotice')
        self.setAttribute(Qt.WA_TransparentForMouseEvents)
        self.setFocusPolicy(Qt.NoFocus)
        self.setAlignment(Qt.AlignCenter)
        self.setWordWrap(True)
        self.setStyleSheet('QLabel#gameplayNotice {background:palette(window);color:palette(window-text);border:1px solid palette(mid);border-radius:10px;padding:12px 20px;font-size:16px;font-weight:600;}')
        self.timer = QTimer(self)
        self.timer.setSingleShot(True)
        self.timer.setInterval(2400)
        self.timer.timeout.connect(self.hide)
        screen.installEventFilter(self)
        self.hide()

    def show_message(self, message):
        self.setText(message)
        self.setAccessibleName(message)
        self.position()
        self.show()
        self.raise_()
        self.timer.start()

    def position(self):
        self.setFixedWidth(min(460, max(100, self.screen.width() - 32)))
        self.adjustSize()
        self.move(max(0, (self.screen.width() - self.width()) // 2), 20)

    def eventFilter(self, watched, event):
        if event.type() == QEvent.Resize:
            self.position()
        return super().eventFilter(watched, event)
