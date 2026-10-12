from .theme import apply_theme_style
"""Transient gameplay controls; all emulator actions stay in the player API."""
from collections import deque
import math
from statistics import median

from PySide6.QtCore import Qt, QEvent, QTimer, QRectF, QPointF
from PySide6.QtGui import QPainter, QPalette, QIcon, QPainterPath, QColor, QPen
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
        apply_theme_style(self, "gameplay")
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
        apply_theme_style(self, "gameplay")
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


class GameplayDiagnostics(QLabel):
    """Optional, non-interactive performance details for the hidden settings page."""
    def __init__(self, screen, *, show_fps=False, show_renderer=False):
        super().__init__(screen)
        self.screen = screen
        self.show_fps = show_fps
        self.show_renderer = show_renderer
        self.fps = 0.0
        self.frame_times = deque()
        self.last_frame_time = None
        self.last_graph_refresh = 0.0
        self.core_renderer = "Software"
        self.display_renderer = screen.backend
        self.setObjectName('gameplayDiagnostics')
        self.setAttribute(Qt.WA_TransparentForMouseEvents)
        self.setFocusPolicy(Qt.NoFocus)
        self.setAlignment(Qt.AlignLeft | Qt.AlignTop)
        apply_theme_style(self, "gameplay")
        screen.installEventFilter(self)
        screen.backend_changed.connect(self.set_display_renderer)
        self.refresh()

    def set_display_renderer(self, renderer):
        self.display_renderer = renderer
        self.refresh()

    def set_core_renderer(self, renderer):
        self.core_renderer = renderer
        self.refresh()

    def reset_frame_clock(self):
        # Pauses and fast-forward are not normal-speed frame pacing samples.
        self.last_frame_time = None

    def record_frame(self, now, *, normal_speed=True):
        if not self.show_fps or not normal_speed:
            self.reset_frame_clock()
            return
        previous = self.last_frame_time
        self.last_frame_time = now
        if previous is not None and now > previous:
            self.frame_times.append((now, now - previous))
        while self.frame_times and self.frame_times[0][0] < now - 10:
            self.frame_times.popleft()
        if now - self.last_graph_refresh >= 0.1:
            self.last_graph_refresh = now
            self.refresh()

    def frame_statistics(self):
        intervals = [interval for _, interval in self.frame_times]
        if not intervals:
            return None
        # 1% low = FPS corresponding to the mean of the slowest 1% of
        # frame intervals, rather than the lowest individual FPS sample.
        low = None
        if len(intervals) >= 100:
            slowest = sorted(intervals, reverse=True)[:math.ceil(len(intervals) * .01)]
            low = len(slowest) / sum(slowest)
        return low, intervals[-1] * 1000, max(intervals) * 1000

    def set_fps(self, fps):
        self.fps = max(0.0, float(fps))
        self.refresh()

    def refresh(self):
        details = []
        if self.show_fps:
            details.append(f'{self.fps:0.1f} FPS')
            stats = self.frame_statistics()
            low = f'{stats[0]:.1f} FPS' if stats and stats[0] is not None else '—'
            details.append(f'1% low: {low}')
            if stats:
                details.append(f'Frame: {stats[1]:.1f} ms · peak: {stats[2]:.1f} ms')
            else:
                details.append('Frame: — ms · peak: — ms')
        if self.show_renderer:
            display = self.display_renderer.replace('Hardware acceleration · ', '')
            display = display.split(' (', 1)[0]
            details.extend((f'Core: {self.core_renderer}', f'Display: {display}'))
        self.setContentsMargins(0, 0, 0, 82 if self.show_fps else 0)
        self.setMinimumWidth(min(320, max(120, self.screen.width() - 24)) if self.show_fps else 0)
        self.setText('\n'.join(details))
        self.setVisible(bool(details))
        self.position()

    def paintEvent(self, event):
        super().paintEvent(event)
        if not self.show_fps:
            return
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        graph = QRectF(12, self.height() - 76, max(1, self.width() - 24), 48)
        painter.setPen(QColor(255, 255, 255, 45))
        painter.drawRect(graph)
        ceiling = max(60.0, self.fps * 1.15)
        painter.setPen(QPen(QColor(255, 255, 255, 35), 1, Qt.DashLine))
        painter.drawLine(QPointF(graph.left(), graph.center().y()),
                         QPointF(graph.right(), graph.center().y()))
        if self.frame_times:
            newest = self.frame_times[-1][0]
            path = QPainterPath()
            # Average short buckets to suppress catch-up spikes while preserving
            # brief drops; the 1% low and peak retain every individual interval.
            buckets = {}
            for timestamp, interval in self.frame_times:
                index = int((timestamp - newest + 10) * 10)
                bucket = buckets.setdefault(index, [0.0, 0])
                bucket[0] += interval
                bucket[1] += 1
            baseline = median(count / total for total, count in buckets.values())
            for index, (total, count) in sorted(buckets.items()):
                x = graph.left() + min(100, max(0, index)) / 100 * graph.width()
                fps = count / total
                # A small visual deadband steadies timer noise without smoothing
                # real drops; raw intervals still drive 1% low and peak metrics.
                if abs(fps - baseline) <= baseline * .02:
                    fps = baseline
                y = graph.bottom() - min(1, fps / ceiling) * graph.height()
                if path.elementCount() == 0:
                    path.moveTo(x, y)
                else:
                    path.lineTo(x, y)
            painter.setPen(QPen(QColor('#b6e878'), 1.5))
            painter.drawPath(path)
        painter.setPen(QColor('#bfc3cc'))
        painter.drawText(QRectF(12, self.height() - 24, self.width() - 24, 18),
                         Qt.AlignLeft, f'FPS trend · last 10 s · 0–{ceiling:.0f}')
        painter.end()

    def position(self):
        self.setMaximumWidth(max(120, self.screen.width() - 24))
        self.adjustSize()
        self.move(max(8, self.screen.width() - self.width() - 12), 12)
        self.raise_()

    def eventFilter(self, watched, event):
        if event.type() == QEvent.Resize:
            self.position()
        return super().eventFilter(watched, event)
