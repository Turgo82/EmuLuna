"""System palette changes and identical image-only highlighting across sections."""
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
import tempfile
import unittest
from pathlib import Path
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import Qt, QRect, QSize
from PySide6.QtGui import QPalette, QColor, QImage, QPixmap, QIcon, QPainter, QAction
from PySide6.QtWidgets import (QApplication, QListWidgetItem, QStyle, QStyleOptionViewItem,
    QWidget, QMenu, QDialog, QVBoxLayout, QLabel, QComboBox, QMessageBox)
from PySide6.QtTest import QTest
from emuluna.app import Window
from emuluna.library import Library
from emuluna.settings import SettingsDialog
from emuluna.library_widgets import GameGrid, CoverDelegate
from emuluna.media_library import ConsoleGrid, ConsoleGroupDelegate
from emuluna.gameplay import GameplayHUD, GameplayNotice, set_hud_icon
from emuluna.theme import EMULUNA_COLOR_SCHEME, set_system_theme, theme_palette
from emuluna.importing import ImportIssuesDialog


def palette(dark):
    result = QPalette()
    for role, color in ((QPalette.Window, '#252629' if dark else '#f3f4f5'),
                        (QPalette.Base, '#1e1f21' if dark else '#ffffff'),
                        (QPalette.AlternateBase, '#303135' if dark else '#e9eaec'),
                        (QPalette.Text, '#eeeeee' if dark else '#202124'),
                        (QPalette.WindowText, '#eeeeee' if dark else '#202124'),
                        (QPalette.Button, '#303135' if dark else '#e9eaec'),
                        (QPalette.ButtonText, '#eeeeee' if dark else '#202124'),
                        (QPalette.Highlight, '#e2a345' if dark else '#2264c0'),
                        (QPalette.Accent, '#a09020' if dark else '#bf6326'),
                        (QPalette.HighlightedText, '#111111' if dark else '#ffffff'),
                        (QPalette.PlaceholderText, '#aaaaaa' if dark else '#62666a')):
        result.setColor(role, QColor(color))
    return result


class SystemTheme(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_open_windows_follow_palette_changes(self):
        original = self.app.palette()
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(Path(folder))
            window = Window(lib, auto_artwork=False)
            settings = SettingsDialog(lib, window)
            try:
                window.show(); settings.show()
                window.search.setText('Theme check')
                settings_icon_key = settings.tabs.tabIcon(0).cacheKey()
                settings_icon_color = settings.palette().color(QPalette.WindowText)
                for dark in (False, True, False):
                    chosen = palette(dark)
                    self.app.setPalette(chosen)
                    QTest.qWait(30)
                    if chosen.color(QPalette.WindowText) != settings_icon_color:
                        refreshed_key = settings.tabs.tabIcon(0).cacheKey()
                        self.assertNotEqual(settings_icon_key, refreshed_key)
                        settings_icon_key = refreshed_key
                        settings_icon_color = chosen.color(QPalette.WindowText)
                    for widget in (window, settings, window.notifications.panel):
                        self.assertEqual(widget.palette().color(QPalette.Window), chosen.color(QPalette.Window), type(widget).__name__)
                        self.assertEqual(widget.palette().color(QPalette.Text), chosen.color(QPalette.Text), type(widget).__name__)
                    self.assertEqual(window.section_buttons['library'].palette().color(QPalette.Highlight), chosen.color(QPalette.Highlight))
                    self.assertFalse(window.section_buttons['library'].icon().isNull())
                    self.assertEqual(window.media_browser.grid.palette().color(QPalette.Text), chosen.color(QPalette.Text))
                    self.assertEqual(settings.volume.palette().color(QPalette.Accent), chosen.color(QPalette.Accent))
                    self.assertEqual(window.search.height(), window.library_tabs.height())
                    self.assertEqual(window.search.palette().color(QPalette.Text), chosen.color(QPalette.Text))
                    self.assertEqual(window.search.palette().color(QPalette.Highlight), chosen.color(QPalette.Highlight))
                    self.assertEqual(window.search.grab().toImage().pixelColor(3, 17), chosen.color(QPalette.Window))
                settings.use_system_theme.setChecked(False)
                QTest.qWait(30)
                self.assertEqual(lib.setting('appearance.use_system_theme'), '0')
                self.assertEqual(self.app.property('KDE_COLOR_SCHEME_PATH'), str(EMULUNA_COLOR_SCHEME))
                self.assertTrue(EMULUNA_COLOR_SCHEME.is_file())
                self.assertEqual(window.palette().color(QPalette.Window), QColor('#24252b'))
                self.assertEqual(window.games.palette().color(QPalette.Text), QColor('#ebeaf0'))
                self.assertEqual(window.search.palette().color(QPalette.Base), QColor('#24252b'))
                for widget in (settings.volume, settings.focus_pause, window.cover_size):
                    for group in (QPalette.Active, QPalette.Inactive, QPalette.Disabled):
                        self.assertEqual(widget.palette().color(group, QPalette.Accent), QColor('#b6a0e4'))
                self.assertEqual(settings.focus_pause.palette().color(QPalette.Accent),
                                 settings.tabs.palette().color(QPalette.Accent))
                self.app.setPalette(palette(True))
                QTest.qWait(30)
                self.assertEqual(window.palette().color(QPalette.Window), QColor('#24252b'))
                settings.use_system_theme.setChecked(True)
                QTest.qWait(30)
                self.assertEqual(lib.setting('appearance.use_system_theme'), '1')
                self.assertIsNone(self.app.property('KDE_COLOR_SCHEME_PATH'))
                self.assertEqual(window.palette().color(QPalette.Window), palette(True).color(QPalette.Window))
                self.assertEqual(settings.volume.palette().color(QPalette.Accent), palette(True).color(QPalette.Accent))
            finally:
                settings.close(); window.close()
                self.app.setPalette(original)

    def test_library_menu_selection_uses_active_theme_highlight(self):
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(Path(folder))
            lib.set_setting('appearance.use_system_theme', '0')
            window = Window(lib, auto_artwork=False)
            try:
                window.show();set_system_theme(False);QTest.qWait(20)
                menu = window.application_menu
                action = menu.actions()[0]
                menu.popup(window.mapToGlobal(window.rect().center()))
                menu.setActiveAction(action);QTest.qWait(20)
                rect = menu.actionGeometry(action)
                selected = menu.grab().toImage().pixelColor(rect.left()+8, rect.center().y())
                self.assertEqual(selected, theme_palette().color(QPalette.Highlight))
            finally:
                window.application_menu.hide();window.close();lib.close()
                set_system_theme(True)

    def test_selection_and_hover_only_surround_images(self):
        pixmap = QPixmap(176, 132); pixmap.fill(QColor('#684a72'))
        icon = QIcon(pixmap)
        for dark in (False, True):
            for media in (False, True):
                view = ConsoleGrid() if media else GameGrid()
                view.setIconSize(QSize(176, 132 if media else 176))
                item = QListWidgetItem('Game title\nAutomatic save')
                item.setData(Qt.UserRole, 'game')
                if not media:
                    item.setData(Qt.UserRole + 1, 'Game title')
                    item.setData(Qt.UserRole + 2, 0)
                    view.cover_dimensions['game'] = QSize(176, 132)
                view.addItem(item)
                delegate = (ConsoleGroupDelegate if media else CoverDelegate)(view, lambda key: icon)
                option = QStyleOptionViewItem()
                option.rect = QRect(0, 0, 210, 230)
                option.palette = palette(dark)
                option.font = view.font()
                def render(state):
                    result = QImage(210, 230, QImage.Format_ARGB32)
                    result.fill(option.palette.color(QPalette.Window))
                    option.state = QStyle.State_Enabled | state
                    painter = QPainter(result)
                    delegate.paint(painter, option, view.model().index(0,0))
                    painter.end()
                    return result
                normal = render(QStyle.State_None)
                for state in (QStyle.State_Selected, QStyle.State_MouseOver):
                    highlighted = render(state)
                    self.assertNotEqual(normal.copy(0,0,210,150), highlighted.copy(0,0,210,150))
                    self.assertEqual(normal.copy(0,150,210,80), highlighted.copy(0,150,210,80))
                view.close()

    def test_late_dialogs_popups_and_gameplay_follow_theme(self):
        original = self.app.palette()
        widgets = []
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(Path(folder))
            window = Window(lib, auto_artwork=False)
            widgets.append(window)
            try:
                self.app.setPalette(palette(False))
                set_system_theme(False)
                # Create these after the switch: no SettingsDialog call to
                # incidentally refresh their palettes for them.
                dialog = QDialog()
                widgets.append(dialog)
                layout = QVBoxLayout(dialog)
                label = QLabel('New dialog')
                label.setStyleSheet('color:palette(window-text);background:palette(window)')
                layout.addWidget(label)
                combo = QComboBox()
                combo.addItems(['First', 'Second'])
                layout.addWidget(combo)
                dialog.show()
                issues = ImportIssuesDialog(lib, window)
                widgets.append(issues)
                issues.show()
                message = QMessageBox(QMessageBox.Information, 'Information', 'Theme check', parent=window)
                widgets.append(message)
                message.show()
                screen = QWidget()
                widgets.append(screen)
                screen.resize(850, 400)
                menu = QMenu(screen)
                menu.addAction('Gameplay options')
                pause = QAction('Resume', screen)
                hud = GameplayHUD(screen, actions=[('pause', pause, QStyle.SP_MediaPlay)],
                    options=menu, volume=80, set_volume=lambda value: None, paused=lambda: True)
                notice = GameplayNotice(screen)
                notice.show_message('State saved')
                screen.show()
                QTest.qWait(40)
                self.assertEqual(dialog.palette().color(QPalette.Window), QColor('#24252b'))
                self.assertIn('#ebeaf0', label.styleSheet())
                for system, dark in ((False, False), (True, False), (True, True), (False, True)):
                    self.app.setPalette(palette(dark))
                    set_system_theme(system)
                    QTest.qWait(40)
                    expected = theme_palette()
                    combo.showPopup()
                    QTest.qWait(20)
                    targets = (dialog, label, combo.view(), combo.view().window(), issues.table,
                               message, hud, hud.volume, notice, window.notifications.panel)
                    for target in targets:
                        for role in (QPalette.Window, QPalette.Text, QPalette.Accent):
                            # The table stylesheet deliberately uses the Base
                            # color for its background, including Window.
                            expected_role = QPalette.Base if target is issues.table and role == QPalette.Window else role
                            self.assertEqual(target.palette().color(role), expected.color(expected_role), (type(target).__name__, role))
                    combo.hidePopup()
                    menu.popup(screen.mapToGlobal(screen.rect().center()))
                    QTest.qWait(20)
                    self.assertEqual(menu.palette().color(QPalette.Window), expected.color(QPalette.Window))
                    menu.hide()
                    self.assertEqual(hud.grab().toImage().pixelColor(20, 3), expected.color(QPalette.Window))
                    self.assertEqual(notice.grab().toImage().pixelColor(20, 3), expected.color(QPalette.Window))
                    self.assertEqual(pause.property('emuluna_hud_icon'), int(QStyle.SP_MediaPlay))
                    image = pause.icon().pixmap(20, 20).toImage()
                    self.assertTrue(any(image.pixelColor(x, y) == expected.color(QPalette.WindowText)
                        for x in range(image.width()) for y in range(image.height())))
                # Recoloring must preserve a runtime action change as well.
                set_hud_icon(pause, hud, QStyle.SP_MediaPause)
                set_system_theme(True)
                QTest.qWait(30)
                self.assertEqual(pause.property('emuluna_hud_icon'), int(QStyle.SP_MediaPause))
            finally:
                for widget in reversed(widgets):
                    widget.close()
                self.app.setPalette(original)
                set_system_theme(True)
