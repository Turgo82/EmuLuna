"""A quiet, bounded notification history with background-task controls."""
from datetime import datetime
from PySide6.QtCore import Qt, QPoint, QSize, QRect, QEvent
from PySide6.QtGui import QPainter, QPainterPath, QPen, QColor, QIcon, QPixmap, QPalette
from PySide6.QtWidgets import (QToolButton, QFrame, QVBoxLayout, QHBoxLayout,
    QLabel, QPushButton, QListWidget, QListWidgetItem, QAbstractItemView)


class NotificationBell(QToolButton):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName('notificationBell')
        self.setFixedSize(32, 32)
        self.setIconSize(QSize(24, 24))
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
        pixmap = QPixmap(24, 24)
        pixmap.fill(Qt.transparent)
        p = QPainter(pixmap)
        p.setRenderHint(QPainter.Antialiasing)
        p.setPen(QPen(self.palette().color(QPalette.WindowText), 1.6, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin))
        path = QPainterPath()
        path.moveTo(5, 17)
        path.cubicTo(7, 15, 7, 13, 7, 10)
        path.cubicTo(7, 3, 17, 3, 17, 10)
        path.cubicTo(17, 13, 17, 15, 19, 17)
        path.closeSubpath()
        p.drawPath(path)
        p.drawLine(12, 3, 12, 4)
        p.drawArc(QRect(10, 18, 4, 3), 180 * 16, 180 * 16)
        if self.unread:
            p.setPen(QPen(self.palette().color(QPalette.Window), 1.5))
            p.setBrush(self.palette().color(QPalette.Highlight))
            p.drawEllipse(QPoint(19, 5), 4, 4)
        p.end()
        self.setIcon(QIcon(pixmap))
        label = 'Notifications' + (f' · {self.unread} unread' if self.unread else '')
        self.setToolTip(label)
        self.setAccessibleName(label)
        self.clear_button.setEnabled(self.history.count() > 0)

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
