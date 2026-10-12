from pathlib import Path
"""Settings surfaces use one palette, including native-style indicators."""
from PySide6.QtCore import Qt, QSize, QRect, QRectF, QEvent
from PySide6.QtGui import QPainter, QPen, QPalette, QPainterPath, QColor, QFontMetrics, QIcon, QRegion
from PySide6.QtWidgets import (QCheckBox, QComboBox, QSpinBox, QAbstractSpinBox, QStyle, QStyleOptionButton,
                              QStyleOptionComboBox, QStyleOptionSpinBox, QStylePainter,
                              QStyledItemDelegate, QStyleOptionViewItem, QTabBar, QStyleOptionTab)

SETTINGS_STYLE = (Path(__file__).with_name("data") / "themes/base/settings.qss").read_text(encoding="utf-8")


class SettingsTabBar(QTabBar):
    """Keep native tab behavior with centered icons above their labels."""
    def tabSizeHint(self, index):
        font = self.font()
        font.setBold(True)
        metrics = QFontMetrics(font)
        return QSize(max(84, metrics.horizontalAdvance(self.tabText(index)) + 24),
                     self.iconSize().height() + metrics.height() + 21)

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
                              rect.top() + 6, size.width(), size.height())
            mode = QIcon.Normal if self.isTabEnabled(index) else QIcon.Disabled
            self.tabIcon(index).paint(painter, icon_rect, Qt.AlignCenter, mode)
            font = self.font()
            font.setBold(index == self.currentIndex())
            painter.setFont(font)
            group = QPalette.Active if self.isTabEnabled(index) else QPalette.Disabled
            text_color = self.window().palette().color(group, QPalette.WindowText)
            if index == self.currentIndex() and self.isTabEnabled(index):
                from .theme import theme_color
                text_color = theme_color('settings-tab-selected-text', text_color.name())
            painter.setPen(text_color)
            label_rect = QRect(rect.left() + 8, icon_rect.bottom() + 5,
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
        from .theme import theme_palette, on_controls_surface
        palette = self.palette() if on_controls_surface(self) else theme_palette()
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
    def showPopup(self):
        popup = self.view().window()
        popup.installEventFilter(self)
        super().showPopup()
        self.round_popup(popup)

    def round_popup(self, popup):
        # The native popup container paints outside the styled list view.
        # Clip the entire surface so its background cannot leave square corners.
        path = QPainterPath()
        path.addRoundedRect(QRectF(popup.rect()), 7, 7)
        popup.setMask(QRegion(path.toFillPolygon().toPolygon()))

    def eventFilter(self, watched, event):
        if watched == self.view().window() and event.type() in (QEvent.Resize, QEvent.Show):
            self.round_popup(watched)
        return super().eventFilter(watched, event)

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
