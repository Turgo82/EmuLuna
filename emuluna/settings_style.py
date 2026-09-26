"""Settings surfaces use one palette, including native-style indicators."""
from PySide6.QtCore import Qt
from PySide6.QtGui import QPainter, QPen, QPalette, QPainterPath
from PySide6.QtWidgets import (QCheckBox, QComboBox, QSpinBox, QAbstractSpinBox, QStyle, QStyleOptionButton,
                              QStyleOptionComboBox, QStyleOptionSpinBox, QStylePainter)

SETTINGS_STYLE = """
QDialog {background:palette(window);color:palette(window-text);}
QLabel {color:palette(window-text);background:transparent;}
QLabel#subtle {color:palette(placeholder-text);}
QLabel#controlTitle {font-weight:600;}
QLabel#controlGroup {color:palette(placeholder-text);font-weight:600;padding:7px 0 3px;}
QFrame#settingsCard {background:palette(base);border:1px solid palette(mid);border-radius:8px;}
QWidget#controlMappings {background:palette(base);}
QTabWidget::pane {background:palette(window);border:1px solid palette(mid);border-radius:7px;top:-1px;}
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
"""


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
