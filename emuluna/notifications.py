"""A quiet, bounded notification history with background-task controls."""
from datetime import datetime
from PySide6.QtCore import Qt, QPoint, QSize, QRect, QEvent
from PySide6.QtGui import QPainter, QPen, QPalette
from PySide6.QtWidgets import (QToolButton, QFrame, QVBoxLayout, QHBoxLayout,
    QLabel, QPushButton, QListWidget, QListWidgetItem, QAbstractItemView)

from .branding import navigation_icon


class NotificationBell(QToolButton):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName('notificationBell')
        self.setFixedSize(32, 32)
        self.setIconSize(QSize(24, 24))
        self.setIcon(navigation_icon('bell'))
        self.setStyleSheet('QToolButton {border:0;border-radius:6px;background:transparent;} QToolButton:hover {background:palette(alternate-base);}')
        self.unread = 0
        self.active = {}
        self.last_message = ''
        self.panel = QFrame(self, Qt.Popup)
        self.panel.setObjectName('notificationPanel')
        self.panel.setFixedSize(380, 440)
        self.panel.setStyleSheet('QFrame#notificationPanel {background:palette(window);border:1px solid palette(mid);border-radius:8px;} QListWidget::item {border-bottom:1px solid palette(mid);padding:7px;color:palette(text);} QPushButton {padding:5px 9px;}')
        layout = QVBoxLayout(self.panel)
        layout.setContentsMargins(14, 12, 14, 12)
        header = QHBoxLayout()
        title = QLabel('Notifications')
        title.setStyleSheet('font-weight:600;font-size:15px;')
        header.addWidget(title)
        header.addStretch()
        self.clear_button = QPushButton('Clear')
        self.clear_button.clicked.connect(self.clear_history)
        header.addWidget(self.clear_button)
        layout.addLayout(header)
        self.activity = QVBoxLayout()
        self.activity.setSpacing(6)
        layout.addLayout(self.activity)
        self.history = QListWidget()
        self.history.setWordWrap(True)
        self.history.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.history.setSelectionMode(QAbstractItemView.NoSelection)
        self.history.setAccessibleName('Recent notifications')
        layout.addWidget(self.history, 1)
        self.history.hide()
        self.empty = QLabel('You’re all caught up.')
        self.empty.setAlignment(Qt.AlignCenter)
        layout.addWidget(self.empty, 1)
        self.clicked.connect(self.toggle_panel)
        self.update_badge()

    def changeEvent(self, event):
        super().changeEvent(event)
        if event.type() in (QEvent.PaletteChange, QEvent.ApplicationPaletteChange) and hasattr(self, 'clear_button'):
            self.update_badge()

    def post(self, message, timeout=0, *, key=None):
        # timeout is accepted when replacing former status-bar calls; history
        # stays available until cleared. Progress replaces its own row.
        message = str(message).strip()
        if not message:
            return
        item = self.active.get(key) if key else None
        if item and item.data(Qt.UserRole) == message:
            return
        if not key and self.history.count() and self.history.item(0).data(Qt.UserRole) == message:
            return
        self.last_message = message
        if item is None:
            item = QListWidgetItem()
            self.history.insertItem(0, item)
            if key:
                self.active[key] = item
            if not self.panel.isVisible():
                self.unread = min(99, self.unread + 1)
        text = datetime.now().strftime('%H:%M') + '  ' + message[:2000]
        item.setText(text)
        item.setData(Qt.UserRole, message)
        item.setToolTip(message)
        height = self.history.fontMetrics().boundingRect(QRect(0, 0, 306, 2000), Qt.TextWordWrap, text).height()
        item.setSizeHint(QSize(320, max(42, height + 20)))
        while self.history.count() > 100:
            # Preserve running operations when trimming older history.
            removable = next((i for i in range(self.history.count()-1, -1, -1)
                              if self.history.item(i) not in self.active.values()), None)
            if removable is None:
                break
            self.history.takeItem(removable)
        self.empty.hide()
        self.history.show()
        self.update_badge()

    def finish(self, key, message):
        existed = key in self.active
        self.post(message, key=key)
        self.active.pop(key, None)
        if existed and not self.panel.isVisible():
            self.unread = min(99, self.unread + 1)
        self.update_badge()

    def clear_history(self):
        for row in range(self.history.count()-1, -1, -1):
            if self.history.item(row) not in self.active.values():
                self.history.takeItem(row)
        self.unread = 0
        self.empty.setVisible(self.history.count() == 0)
        self.history.setVisible(self.history.count() > 0)
        self.update_badge()

    def update_badge(self):
        label = 'Notifications' + (f' · {self.unread} unread' if self.unread else '')
        self.setToolTip(label)
        self.setAccessibleName(label)
        self.clear_button.setEnabled(self.history.count() > 0)
        self.update()

    def paintEvent(self, event):
        super().paintEvent(event)
        if not self.unread:
            return
        # Painting in widget coordinates lets Qt scale the badge cleanly with
        # the vector bell on fractional and high-DPI displays.
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        painter.setPen(QPen(self.palette().color(QPalette.Window), 1.5))
        painter.setBrush(self.palette().color(QPalette.Highlight))
        painter.drawEllipse(QPoint(23, 9), 4, 4)

    def toggle_panel(self):
        if self.panel.isVisible():
            self.panel.hide()
            return
        anchor = self.mapToGlobal(QPoint(self.width(), self.height() + 6))
        available = self.screen().availableGeometry()
        x = max(available.left(), min(anchor.x() - self.panel.width(), available.right() - self.panel.width() + 1))
        y = max(available.top(), min(anchor.y(), available.bottom() - self.panel.height() + 1))
        self.panel.move(x, y)
        self.unread = 0
        self.update_badge()
        self.panel.show()
        self.panel.setFocus()
