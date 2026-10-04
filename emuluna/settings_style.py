"""Settings surfaces use one palette, including native-style indicators."""
from PySide6.QtCore import Qt, QSize, QRect
from PySide6.QtGui import QPainter, QPen, QPalette, QPainterPath, QColor, QFontMetrics, QIcon
from PySide6.QtWidgets import (QCheckBox, QComboBox, QSpinBox, QAbstractSpinBox, QStyle, QStyleOptionButton,
                              QStyleOptionComboBox, QStyleOptionSpinBox, QStylePainter,
                              QStyledItemDelegate, QStyleOptionViewItem, QTabBar, QStyleOptionTab)

SETTINGS_STYLE = """
QDialog {background:palette(window);color:palette(window-text);}
QLabel {color:palette(window-text);background:transparent;}
QLabel#subtle {color:palette(placeholder-text);}
QLabel#controlTitle {font-weight:600;}
QLabel#controlGroup {color:palette(placeholder-text);font-weight:600;padding:7px 0 3px;}
QFrame#settingsCard {background:palette(base);border:1px solid palette(mid);border-radius:8px;}
QFrame#controllerPreviewCard {background:transparent;border:0;}
QWidget#controlMappings {background:palette(base);}
QTabWidget::pane {background:transparent;border:0;}
QTabWidget::tab-bar {alignment:center;}
QTabBar {background:palette(alternate-base);outline:0;}
QTabBar::tab {background:transparent;color:palette(window-text);border:0;border-bottom:3px solid transparent;padding:10px 12px;margin:0 1px;}
QTabBar::tab:selected {background:palette(button);border-bottom-color:palette(highlight);font-weight:600;}
QTabBar::tab:!selected:hover {background:palette(midlight);}
QPushButton {background:palette(button);color:palette(button-text);border:1px solid palette(mid);border-radius:5px;padding:7px 12px;min-height:18px;}
QPushButton:hover {background:palette(midlight);border-color:palette(placeholder-text);}
QPushButton:focus {border-color:palette(highlight);}
QPushButton:pressed,QPushButton:checked {background:palette(highlight);color:palette(highlighted-text);border-color:palette(highlight);}
QPushButton#controlBinding {padding:5px 8px;text-align:left;}
QPushButton:disabled {color:palette(disabled-text);background:palette(alternate-base);}
QComboBox,QLineEdit,QSpinBox {background:palette(base);color:palette(text);border:1px solid palette(mid);border-radius:5px;padding:6px 8px;min-height:18px;selection-background-color:palette(highlight);selection-color:palette(highlighted-text);}
QComboBox {padding-right:26px;}
QComboBox:hover,QLineEdit:hover,QSpinBox:hover {border-color:palette(placeholder-text);}
QComboBox:focus,QLineEdit:focus,QSpinBox:focus {border-color:palette(highlight);}
QComboBox:disabled,QLineEdit:disabled,QSpinBox:disabled {color:palette(disabled-text);background:palette(alternate-base);}
QComboBox::drop-down {width:24px;border:0;background:transparent;}
QComboBox::down-arrow {image:none;}
QSpinBox {padding-right:30px;}
QSpinBox::up-button {subcontrol-origin:border;subcontrol-position:top right;width:24px;border:0;border-left:1px solid palette(mid);border-bottom:1px solid palette(mid);border-top-right-radius:5px;background:palette(button);}
QSpinBox::down-button {subcontrol-origin:border;subcontrol-position:bottom right;width:24px;border:0;border-left:1px solid palette(mid);border-bottom-right-radius:5px;background:palette(button);}
QSpinBox::up-button:hover,QSpinBox::down-button:hover {background:palette(midlight);}
QSpinBox::up-arrow,QSpinBox::down-arrow {image:none;}
QComboBox QAbstractItemView {background:palette(base);color:palette(text);border:1px solid palette(mid);outline:0;selection-background-color:palette(highlight);selection-color:palette(highlighted-text);}
QCheckBox {color:palette(window-text);background:transparent;spacing:9px;padding:4px 0;}
QCheckBox:disabled {color:palette(disabled-text);}
QCheckBox::indicator {width:18px;height:18px;}
QSlider::groove:horizontal {height:5px;background:palette(mid);border:0;border-radius:2px;}
QSlider::sub-page:horizontal {background:palette(highlight);border-radius:2px;}
QSlider::handle:horizontal {background:palette(button);border:2px solid palette(highlight);width:14px;margin:-6px 0;border-radius:8px;}
QSlider::handle:horizontal:hover,QSlider::handle:horizontal:focus {background:palette(highlight);}
QSlider::handle:horizontal:disabled {border-color:palette(mid);}
QTableWidget {background:palette(base);color:palette(text);alternate-background-color:palette(alternate-base);gridline-color:palette(mid);border:1px solid palette(mid);selection-background-color:palette(highlight);selection-color:palette(highlighted-text);}
QHeaderView::section {background:palette(button);color:palette(button-text);border:0;border-bottom:1px solid palette(mid);padding:8px;}
QScrollArea {background:transparent;border:0;}
QScrollBar:vertical {background:palette(base);width:12px;margin:2px;}
QScrollBar:horizontal {background:palette(base);height:12px;margin:2px;}
QScrollBar::handle {background:palette(placeholder-text);border:2px solid palette(base);border-radius:5px;}
QScrollBar::handle:vertical {min-height:28px;}
QScrollBar::handle:horizontal {min-width:28px;}
QScrollBar::handle:hover,QScrollBar::handle:pressed {background:palette(highlight);}
QScrollBar::add-line,QScrollBar::sub-line {width:0;height:0;border:0;background:transparent;}
QScrollBar::add-page,QScrollBar::sub-page {background:transparent;}
QMenu {background:palette(window);color:palette(window-text);border:1px solid palette(mid);padding:4px;}
QMenu::item {padding:6px 22px;border-radius:3px;}
QMenu::item:selected {background:palette(highlight);color:palette(highlighted-text);}
QMenu::item:disabled {color:palette(disabled-text);}
QMenu::separator {height:1px;background:palette(mid);margin:4px;}
/* Controls uses a single wood surface from the tab strip to the bottom edge. */
QTabWidget#settingsTabs[woodControls="true"]::pane {background:transparent;border:0;}
QWidget#woodControlsPage {background:transparent;}
QWidget#woodControlsPage QLabel {color:#fff5e9;}
QWidget#woodControlsPage QLabel#subtle,QWidget#woodControlsPage QLabel#controlGroup {color:#ead7bc;}
QWidget#woodControlsPage QFrame#settingsCard {background:rgba(34,21,13,105);border:1px solid rgba(228,184,116,100);}
QWidget#woodControlsPage QWidget#controlMappings {background:transparent;}
QWidget#woodControlsPage QPushButton,QWidget#woodControlsPage QComboBox,
QPushButton#settingsDone[woodControls="true"] {background:rgba(34,21,13,140);color:#fff5e9;border-color:rgba(228,184,116,150);}
QWidget#woodControlsPage QPushButton:hover,QWidget#woodControlsPage QComboBox:hover,
QPushButton#settingsDone[woodControls="true"]:hover {background:rgba(107,69,40,190);border-color:#e4b874;}
QWidget#woodControlsPage QPushButton:focus,QWidget#woodControlsPage QComboBox:focus,
QPushButton#settingsDone[woodControls="true"]:focus {border-color:#e4b874;}
QWidget#woodControlsPage QComboBox QAbstractItemView {background:#302015;color:#fff5e9;border-color:#b48a5b;}
QWidget#woodControlsPage QScrollBar {background:rgba(34,21,13,105);}
QWidget#woodControlsPage QScrollBar::handle {background:#c2a17b;border-color:transparent;}
"""


class SettingsTabBar(QTabBar):
    """Keep native tab behavior with centered icons above their labels."""
    def tabSizeHint(self, index):
        font = self.font()
        font.setBold(True)
        metrics = QFontMetrics(font)
        return QSize(max(84, metrics.horizontalAdvance(self.tabText(index)) + 24),
                     self.iconSize().height() + metrics.height() + 29)

    def minimumTabSizeHint(self, index):
        return self.tabSizeHint(index)

    def paintEvent(self, event):
        painter = QStylePainter(self)
        for index in range(self.count()):
            option = QStyleOptionTab()
            self.initStyleOption(option, index)
            painter.drawControl(QStyle.CE_TabBarTabShape, option)
            rect = self.tabRect(index)
            size = self.iconSize()
            icon_rect = QRect(rect.center().x() - size.width() // 2,
                              rect.top() + 10, size.width(), size.height())
            mode = QIcon.Normal if self.isTabEnabled(index) else QIcon.Disabled
            self.tabIcon(index).paint(painter, icon_rect, Qt.AlignCenter, mode)
            font = self.font()
            font.setBold(index == self.currentIndex())
            painter.setFont(font)
            group = QPalette.Active if self.isTabEnabled(index) else QPalette.Disabled
            painter.setPen(self.window().palette().color(group, QPalette.WindowText))
            label_rect = QRect(rect.left() + 8, icon_rect.bottom() + 7,
                               rect.width() - 16, QFontMetrics(font).height())
            painter.drawText(label_rect, Qt.AlignCenter | Qt.TextSingleLine, self.tabText(index))


def verification_green(background):
    """Choose a green that remains legible against the actual row color."""
    def luminance(color):
        channels = [channel / 12.92 if channel <= .04045 else ((channel + .055) / 1.055) ** 2.4
                    for channel in (color.redF(), color.greenF(), color.blueF())]
        return sum(channel * weight for channel, weight in zip(channels, (.2126, .7152, .0722)))
    base = luminance(background)
    colors = [QColor(value) for value in ('#67dc9b', '#0b5728', '#00240f', '#b6f7d3')]
    def contrast(color):
        value = luminance(color)
        return (max(base, value) + .05) / (min(base, value) + .05)
    return next((color for color in colors if contrast(color) >= 4.5), max(colors, key=contrast))


class SystemFileDelegate(QStyledItemDelegate):
    """Keep verification green on selected rows and every desktop theme."""
    def __init__(self, parent=None):
        super().__init__(parent)
        from .branding import navigation_icon
        self.verified_icon = navigation_icon('file-verified', '#198754')
        self.unverified_icon = navigation_icon('file-unverified')

    def initStyleOption(self, option, index):
        super().initStyleOption(option, index)
        verified = index.siblingAtColumn(3).data(Qt.DisplayRole) == 'Verified'
        if index.column() == 3 and verified:
            background = option.palette.color(QPalette.Highlight if option.state & QStyle.State_Selected
                                               else QPalette.Base)
            green = verification_green(background)
            option.palette.setColor(QPalette.Text, green)
            option.palette.setColor(QPalette.HighlightedText, green)
            option.font.setBold(True)
        elif index.column() == 0:
            # SVG decorations reserve the native indicator's space without
            # allowing the desktop style to replace them with check bitmaps.
            option.features &= ~QStyleOptionViewItem.HasCheckIndicator
            option.features |= QStyleOptionViewItem.HasDecoration
            option.icon = self.verified_icon if verified else self.unverified_icon
            option.decorationSize = QSize(18, 18)
            option.decorationAlignment = Qt.AlignCenter


class SettingsCheckBox(QCheckBox):
    """Keep the native label/focus layout, but avoid OS-colored check bitmaps."""
    def paintEvent(self, event):
        option = QStyleOptionButton()
        self.initStyleOption(option)
        painter = QStylePainter(self)
        label_option = QStyleOptionButton(option)
        label_option.rect = self.style().subElementRect(QStyle.SE_CheckBoxContents, option, self)
        painter.drawControl(QStyle.CE_CheckBoxLabel, label_option)
        rect = self.style().subElementRect(QStyle.SE_CheckBoxIndicator, option, self).adjusted(1, 1, -1, -1)
        # Read the current application choice at paint time. A checkbox may
        # already be open while the user switches between the OS palette and
        # EmuLuna's palette, so its inherited widget palette can be one event
        # behind the selected theme.
        from .theme import theme_palette
        palette = theme_palette()
        checked = self.checkState() != Qt.Unchecked
        # Tabs, sliders, selections and checks all use Highlight. Accent can
        # remain tied to the desktop theme even when EmuLuna's theme is active.
        accent = palette.color(QPalette.Highlight if self.isEnabled() else QPalette.Mid)
        painter.setRenderHint(QPainter.Antialiasing)
        painter.setPen(QPen(accent if checked or self.hasFocus() else palette.color(QPalette.Mid), 1.5))
        painter.setBrush(accent if checked else palette.color(QPalette.Base))
        painter.drawRoundedRect(rect, 4, 4)
        if checked:
            path = QPainterPath()
            if self.checkState() == Qt.PartiallyChecked:
                path.moveTo(rect.left()+3, rect.center().y())
                path.lineTo(rect.right()-3, rect.center().y())
            else:
                path.moveTo(rect.left()+3, rect.center().y())
                path.lineTo(rect.left()+6, rect.bottom()-4)
                path.lineTo(rect.right()-3, rect.top()+4)
            painter.setPen(QPen(palette.color(QPalette.HighlightedText), 2, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin))
            painter.drawPath(path)


class SettingsComboBox(QComboBox):
    """The arrow is palette-drawn too; some desktop styles use fixed bitmaps."""
    def paintEvent(self, event):
        super().paintEvent(event)
        option = QStyleOptionComboBox()
        self.initStyleOption(option)
        rect = self.style().subControlRect(QStyle.CC_ComboBox, option, QStyle.SC_ComboBoxArrow, self)
        center = rect.center()
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        color = self.palette().color(QPalette.Active if self.isEnabled() else QPalette.Disabled, QPalette.Text)
        painter.setPen(QPen(color, 1.5, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin))
        path = QPainterPath()
        path.moveTo(center.x()-4, center.y()-2)
        path.lineTo(center.x(), center.y()+2)
        path.lineTo(center.x()+4, center.y()-2)
        painter.drawPath(path)


class SettingsSpinBox(QSpinBox):
    def paintEvent(self, event):
        super().paintEvent(event)
        option = QStyleOptionSpinBox()
        self.initStyleOption(option)
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        for control, direction, flag in ((QStyle.SC_SpinBoxUp,-1,QAbstractSpinBox.StepEnabledFlag.StepUpEnabled),
                                         (QStyle.SC_SpinBoxDown,1,QAbstractSpinBox.StepEnabledFlag.StepDownEnabled)):
            rect = self.style().subControlRect(QStyle.CC_SpinBox, option, control, self)
            center = rect.center()
            enabled = self.isEnabled() and bool(option.stepEnabled & flag)
            color = self.palette().color(QPalette.Active if enabled else QPalette.Disabled, QPalette.ButtonText)
            painter.setPen(QPen(color,1.5,Qt.SolidLine,Qt.RoundCap,Qt.RoundJoin))
            path = QPainterPath()
            path.moveTo(center.x()-3,center.y()-direction)
            path.lineTo(center.x(),center.y()+2*direction)
            path.lineTo(center.x()+3,center.y()-direction)
            painter.drawPath(path)
