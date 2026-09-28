"""Compact, animated library activity shown at the foot of the sidebar."""
from collections import OrderedDict
import re

from PySide6.QtCore import QEasingCurve, QParallelAnimationGroup, QPropertyAnimation, QSize, QTimer, Qt
from PySide6.QtWidgets import (QGraphicsOpacityEffect, QHBoxLayout, QLabel, QProgressBar,
                               QStackedWidget, QToolButton, QVBoxLayout, QWidget)

from .branding import navigation_icon


class ElidedLabel(QLabel):
    """Single-line label that keeps long task details inside the sidebar."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.full_text = ""

    def setText(self, text):
        self.full_text = str(text)
        self.setToolTip(self.full_text)
        self._elide()

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self._elide()

    def _elide(self):
        super().setText(self.fontMetrics().elidedText(self.full_text, Qt.ElideRight, max(1, self.width())))


class SidebarActivity(QWidget):
    """Become a progress panel while library background work is active."""

    COLLAPSED_HEIGHT = 34
    EXPANDED_HEIGHT = 82

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("sidebarActivity")
        self.setMaximumHeight(self.COLLAPSED_HEIGHT)
        self.tasks = OrderedDict()
        self.current_key = None

        outer = QVBoxLayout(self)
        outer.setContentsMargins(20, 0, 20, 0)
        outer.setSpacing(0)
        self.pages = QStackedWidget()
        outer.addWidget(self.pages)

        idle = QWidget()
        idle_layout = QHBoxLayout(idle)
        idle_layout.setContentsMargins(0, 0, 0, 0)
        self.add_button = QToolButton()
        self.add_button.setObjectName("sidebarAddButton")
        self.add_button.setIcon(navigation_icon("plus"))
        self.add_button.setIconSize(QSize(18, 18))
        self.add_button.setFixedSize(32, 32)
        self.add_button.setToolTip("Add games")
        self.add_button.setAccessibleName("Add games")
        self.add_button.setPopupMode(QToolButton.InstantPopup)
        idle_layout.addWidget(self.add_button, 0, Qt.AlignLeft | Qt.AlignBottom)
        idle_layout.addStretch()
        self.pages.addWidget(idle)

        busy = QWidget()
        busy_layout = QVBoxLayout(busy)
        busy_layout.setContentsMargins(0, 0, 0, 0)
        busy_layout.setSpacing(3)
        self.title = QLabel()
        self.title.setObjectName("sidebarActivityTitle")
        busy_layout.addWidget(self.title)
        progress_row = QHBoxLayout()
        progress_row.setContentsMargins(0, 0, 0, 0)
        progress_row.setSpacing(6)
        self.progress = QProgressBar()
        self.progress.setObjectName("sidebarProgress")
        self.progress.setTextVisible(False)
        self.progress.setFixedHeight(9)
        progress_row.addWidget(self.progress, 1)
        self.cancel_button = QToolButton()
        self.cancel_button.setObjectName("sidebarCancelButton")
        self.cancel_button.setText("×")
        self.cancel_button.setFixedSize(20, 20)
        self.cancel_button.setToolTip("Cancel")
        self.cancel_button.setAccessibleName("Cancel current activity")
        self.cancel_button.clicked.connect(self.cancel_current)
        progress_row.addWidget(self.cancel_button)
        busy_layout.addLayout(progress_row)
        self.detail = ElidedLabel()
        self.detail.setObjectName("sidebarActivityDetail")
        self.detail.setTextFormat(Qt.PlainText)
        self.detail.setMinimumWidth(0)
        busy_layout.addWidget(self.detail)
        self.pages.addWidget(busy)

        self._opacity = QGraphicsOpacityEffect(self.pages)
        self.pages.setGraphicsEffect(self._opacity)
        self._opacity.setOpacity(1.0)
        self._return_timer = QTimer(self)
        self._return_timer.setSingleShot(True)
        self._return_timer.setInterval(450)
        self._return_timer.timeout.connect(self._show_idle)
        self._animation = None

    def set_menu(self, menu):
        self.add_button.setMenu(menu)

    def begin(self, key, title, detail="Preparing…", *, current=0, total=0, cancel=None):
        self._return_timer.stop()
        self.tasks[key] = {"title": title, "detail": detail, "current": current,
                           "total": total, "cancel": cancel}
        self.tasks.move_to_end(key)
        self.current_key = key
        self._render()
        if self.pages.currentIndex() != 1:
            self.pages.setCurrentIndex(1)
            self._animate(self.EXPANDED_HEIGHT)

    def update(self, key, *, title=None, detail=None, current=None, total=None, cancel=None):
        if key not in self.tasks:
            self.begin(key, title or "Working", detail or "Preparing…", current=current or 0,
                       total=total or 0, cancel=cancel)
            return
        task = self.tasks[key]
        for name, value in (("title", title), ("detail", detail), ("current", current),
                            ("total", total), ("cancel", cancel)):
            if value is not None:
                task[name] = value
        self.tasks.move_to_end(key)
        self.current_key = key
        self._render()

    def update_message(self, key, title, message, *, cancel=None):
        """Extract determinate progress from worker messages when available."""
        current = total = 0
        match = re.search(r"(?:^|\s)(\d+)(?:\s+of\s+|/)(\d+)(?:\s|\b)", message)
        if match:
            current, total = map(int, match.groups())
        self.update(key, title=title, detail=message, current=current, total=total, cancel=cancel)

    def finish(self, key):
        self.tasks.pop(key, None)
        if self.tasks:
            self.current_key = next(reversed(self.tasks))
            self._render()
        else:
            self.current_key = None
            # A following queued task normally starts in the same event-loop
            # turn. This prevents a distracting +/bar flash between them.
            self._return_timer.start()

    def cancel_current(self):
        task = self.tasks.get(self.current_key)
        callback = task.get("cancel") if task else None
        if callback:
            self.cancel_button.setEnabled(False)
            self.detail.setText("Cancelling…")
            callback()

    def refresh_icon(self):
        self.add_button.setIcon(navigation_icon("plus"))

    def _render(self):
        task = self.tasks.get(self.current_key)
        if not task:
            return
        self.title.setText(task["title"])
        self.detail.setText(task["detail"])
        total = max(0, int(task["total"] or 0))
        current = max(0, int(task["current"] or 0))
        self.progress.setRange(0, total)
        if total:
            self.progress.setValue(min(current, total))
        self.cancel_button.setVisible(task["cancel"] is not None)
        self.cancel_button.setEnabled(task["cancel"] is not None)
        self.setToolTip(task["detail"])

    def _show_idle(self):
        if self.tasks:
            return
        self.pages.setCurrentIndex(0)
        self.setToolTip("")
        self._animate(self.COLLAPSED_HEIGHT)

    def _animate(self, height):
        if self._animation:
            self._animation.stop()
            self._animation.deleteLater()
        height_animation = QPropertyAnimation(self, b"maximumHeight", self)
        height_animation.setDuration(190)
        height_animation.setStartValue(self.maximumHeight())
        height_animation.setEndValue(height)
        height_animation.setEasingCurve(QEasingCurve.OutCubic)
        fade = QPropertyAnimation(self._opacity, b"opacity", self)
        fade.setDuration(170)
        fade.setStartValue(0.25)
        fade.setEndValue(1.0)
        fade.setEasingCurve(QEasingCurve.OutCubic)
        group = QParallelAnimationGroup(self)
        group.addAnimation(height_animation)
        group.addAnimation(fade)
        def complete():
            if self._animation is group:
                self._animation = None
            group.deleteLater()
        group.finished.connect(complete)
        self._animation = group
        group.start()
