"""Native palette styling and the common cover/preview selection treatment."""
import re
from pathlib import Path
from weakref import WeakSet
from shiboken6 import isValid
from PySide6.QtCore import Qt, QObject, QTimer, QEvent
from PySide6.QtGui import QPalette, QPen, QColor
from PySide6.QtWidgets import QStyle, QApplication, QWidget

EMULUNA_COLOR_SCHEME = Path(__file__).with_name('data') / 'EmuLuna.colors'
EMULUNA_WINDOW_COLOR_SCHEME = Path(__file__).with_name('data') / 'EmuLuna-Window.colors'

LIBRARY_STYLE = """
QMainWindow {background:palette(window); color:palette(window-text);}
QWidget#sidebar {background:palette(alternate-base); border:0;}
QWidget#libraryNavigation {background:palette(mid);border:0;border-radius:8px;}
QToolButton#applicationMenuButton {background:transparent; border:0; border-radius:5px; padding:5px;}
QToolButton#applicationMenuButton:hover, QToolButton#applicationMenuButton:pressed {background:palette(midlight);}
QToolButton#applicationMenuButton::menu-indicator {image:none;}
QToolButton#sidebarAddButton {background:transparent;border:0;border-radius:5px;padding:5px;}
QToolButton#sidebarAddButton:hover,QToolButton#sidebarAddButton:pressed {background:palette(midlight);}
QToolButton#sidebarAddButton::menu-indicator {image:none;}
QLabel#sidebarActivityTitle {font-weight:600;color:palette(window-text);background:transparent;}
QLabel#sidebarActivityDetail {font-size:11px;color:palette(placeholder-text);background:transparent;}
QProgressBar#sidebarProgress {background:palette(window);border:1px solid palette(mid);border-radius:4px;}
QProgressBar#sidebarProgress::chunk {background:palette(highlight);border-radius:3px;}
QToolButton#sidebarCancelButton {background:transparent;color:palette(window-text);border:0;border-radius:4px;font-size:17px;padding:0;}
QToolButton#sidebarCancelButton:hover {background:palette(midlight);}
QToolButton#searchButton {background:transparent;border:0;border-radius:5px;padding:5px;}
QToolButton#searchButton:hover,QToolButton#searchButton:focus {background:palette(midlight);}
QToolButton#aboutLogoButton {background:transparent;border:0;padding:0;}
QToolButton#aboutLogoButton:hover,QToolButton#aboutLogoButton:pressed {background:transparent;}
QLineEdit#librarySearch {background:palette(window);color:palette(text);placeholder-text-color:palette(placeholder-text);border:1px solid palette(mid);border-radius:7px;padding:3px 8px;selection-background-color:palette(highlight);selection-color:palette(highlighted-text);}
QLineEdit#librarySearch:focus {border-color:palette(highlight);}
QPushButton#sectionNavigation {background:palette(window);color:palette(window-text);border:1px solid transparent;border-radius:0;padding:4px 18px;}
QPushButton#sectionNavigation[segment="first"] {border-top-left-radius:7px;border-bottom-left-radius:7px;}
QPushButton#sectionNavigation[segment="last"] {border-top-right-radius:7px;border-bottom-right-radius:7px;}
QPushButton#sectionNavigation:hover {background:palette(midlight);}
QPushButton#sectionNavigation:focus {border-color:palette(highlight);}
QPushButton#sectionNavigation:checked {background:palette(highlight); color:palette(highlighted-text); border-color:palette(highlight);}
QPushButton#viewControl {background:transparent; border:0; padding:5px; border-radius:4px;}
QPushButton#viewControl:hover {background:palette(alternate-base);}
QPushButton#viewControl:checked {background:palette(highlight);}
QLabel#subtle {color:palette(placeholder-text); background:transparent;}
QListWidget {border:0; outline:0; background:transparent;}
QListWidget#nav::item {height:28px; border-radius:6px; padding-left:18px; margin:1px 8px;}
QListWidget#nav::item:hover {background:palette(midlight);}
QListWidget#nav::item:selected {background:palette(highlight); color:palette(highlighted-text);}
QTableWidget, QTreeWidget {background:palette(base); alternate-background-color:palette(alternate-base); border:0; gridline-color:palette(mid); selection-background-color:palette(highlight); selection-color:palette(highlighted-text);}
QHeaderView::section {padding:8px;}
QPushButton#primary {font-weight:700; padding:9px 17px;}
QLineEdit {padding:6px 10px;}
QScrollBar:vertical {background:palette(alternate-base); width:14px; margin:0;}
QScrollBar:horizontal {background:palette(alternate-base); height:14px; margin:0;}
QScrollBar::handle {background:palette(placeholder-text); border:2px solid palette(alternate-base); border-radius:6px;}
QScrollBar::handle:vertical {min-height:36px;}
QScrollBar::handle:horizontal {min-width:36px;}
QScrollBar::handle:hover, QScrollBar::handle:pressed {background:palette(highlight);}
QScrollBar::add-line, QScrollBar::sub-line {width:0; height:0; border:0; background:transparent;}
QScrollBar::add-page, QScrollBar::sub-page {background:transparent;}
QMenu {background:palette(window);color:palette(window-text);border:1px solid palette(mid);border-radius:7px;padding:4px;}
QMenu::item {padding:6px 28px 6px 14px;border-radius:4px;}
QMenu::item:selected {background:palette(highlight);color:palette(highlighted-text);}
QMenu::item:disabled {color:palette(disabled-text);}
QMenu::separator {height:1px;background:palette(mid);margin:4px 8px;}
"""


def paint_thumbnail_highlight(painter, art_rect, option):
    """Highlight only the image, identically in all three library sections."""
    selected = bool(option.state & QStyle.State_Selected)
    hovered = bool(option.state & QStyle.State_MouseOver)
    if not selected and not hovered:
        return
    painter.save()
    accent = option.palette.color(QPalette.Highlight)
    fill = accent.toRgb()
    fill.setAlpha(60 if selected else 28)
    painter.setBrush(fill)
    painter.setPen(QPen(accent, 2) if selected else Qt.NoPen)
    painter.drawRoundedRect(art_rect.adjusted(-4, -4, 4, 4), 5, 5)
    painter.restore()


# Qt caches palette(...) values when polishing a stylesheet. Re-polish on an
# OS palette change so already-open controls do not keep the previous colors.


class SystemThemeBinding(QObject):
    def __init__(self, app):
        super().__init__(app)
        self.app = app
        self.use_system = True
        from .native_decoration import native_decoration_binding
        self.decoration = native_decoration_binding(app)
        self.timer = QTimer(self)
        self.timer.setSingleShot(True)
        self.timer.timeout.connect(self.refresh)
        app.paletteChanged.connect(lambda *_: self.timer.start(0))
        self.shown_windows = WeakSet()
        self.window_timer = QTimer(self)
        self.window_timer.setSingleShot(True)
        self.window_timer.timeout.connect(self.refresh_shown_windows)
        app.installEventFilter(self)

    def eventFilter(self, watched, event):
        if (event.type() == QEvent.Hide and isinstance(watched, QWidget)
                and watched.isWindow() and self.decoration):
            self.decoration.release(watched.windowHandle())
        # Menus and dialogs created after a theme switch otherwise start with
        # the application's OS palette (which we deliberately keep intact).
        if event.type() == QEvent.Show and isinstance(watched, QWidget) and watched.isWindow():
            watched.setPalette(theme_palette())
            self.shown_windows.add(watched)
            self.window_timer.start(0)
        return super().eventFilter(watched, event)

    def refresh_shown_windows(self):
        # Opening a popup should not repolish the whole library/player UI.
        widgets = set()
        for window in self.shown_windows:
            if isValid(window):
                widgets.add(window)
                widgets.update(window.findChildren(QWidget))
        self.shown_windows.clear()
        self.refresh(widgets)

    def refresh(self, widgets=None):
        widgets = self.app.allWidgets() if widgets is None else widgets
        styled = []
        for widget in widgets:
            current = widget.styleSheet()
            if not current:
                continue
            template = widget.property('_emuluna_theme_template')
            if not template or current != widget.property('_emuluna_theme_applied'):
                template = current
            styled.append((widget, template))
            widget.setStyleSheet('')
        palette = theme_palette()
        # Stylesheet-polished children can retain explicit native palette roles.
        # Update them too, especially when starting directly in the dark theme.
        for widget in widgets:
            widget.setPalette(palette)
            if self.decoration and widget.isWindow():
                scheme = (EMULUNA_COLOR_SCHEME if (widget.property('emuluna.library_window')
                                                   or widget.property('emuluna.library_header'))
                          else EMULUNA_WINDOW_COLOR_SCHEME)
                self.decoration.apply(widget, '' if self.use_system else str(scheme))
        # QSS resolves palette() through the application palette on some Qt
        # styles. Resolve explicitly for the optional local dark theme, without
        # replacing the application's native palette or losing OS updates.
        roles = {'window': QPalette.Window, 'window-text': QPalette.WindowText,
                 'base': QPalette.Base, 'alternate-base': QPalette.AlternateBase,
                 'text': QPalette.Text, 'button': QPalette.Button,
                 'button-text': QPalette.ButtonText, 'highlight': QPalette.Highlight,
                 'highlighted-text': QPalette.HighlightedText, 'mid': QPalette.Mid,
                 'midlight': QPalette.Midlight, 'placeholder-text': QPalette.PlaceholderText}
        colors = {name: palette.color(role).name() for name, role in roles.items()}
        colors['disabled-text'] = palette.color(QPalette.Disabled, QPalette.WindowText).name()
        for widget, template in styled:
            applied = re.sub(r'palette\(([-a-z]+)\)',
                             lambda match: colors.get(match[1], match[0]), template)
            widget.setStyleSheet(applied)
            widget.setProperty('_emuluna_theme_template', template)
            widget.setProperty('_emuluna_theme_applied', applied)


def follow_system_theme(library=None):
    app = QApplication.instance()
    if not hasattr(app, '_emuluna_system_theme'):
        app._emuluna_system_theme = SystemThemeBinding(app)
    if library is not None:
        binding = app._emuluna_system_theme
        enabled = library.setting('appearance.use_system_theme', '1') == '1'
        # Opening another dialog does not change the theme. Its Show event
        # styles that window alone; only a real theme change touches the app.
        if binding.use_system != enabled:
            binding.use_system = enabled
            _sync_native_decoration(app, enabled)
            binding.timer.start(0)


def set_system_theme(enabled):
    follow_system_theme()
    binding = QApplication.instance()._emuluna_system_theme
    binding.use_system = enabled
    _sync_native_decoration(QApplication.instance(), enabled)
    binding.refresh()


def _sync_native_decoration(app, use_system):
    """Let KDE/Wayland decorate native title bars with the selected theme."""
    app.setProperty('KDE_COLOR_SCHEME_PATH', None if use_system else str(EMULUNA_COLOR_SCHEME))


def theme_palette():
    app = QApplication.instance()
    binding = getattr(app, '_emuluna_system_theme', None)
    if binding is None or binding.use_system:
        return app.palette()
    palette = QPalette(app.palette())
    for role, color in ((QPalette.Window, '#24252b'), (QPalette.WindowText, '#ebeaf0'),
                        (QPalette.Base, '#24252b'), (QPalette.AlternateBase, '#1b1c21'),
                        (QPalette.Text, '#ebeaf0'), (QPalette.Button, '#383943'),
                        (QPalette.ButtonText, '#ebeaf0'), (QPalette.Highlight, '#b6a0e4'),
                        # Native Qt controls can use Accent independently of
                        # Highlight; otherwise they retain the OS accent here.
                        (QPalette.Accent, '#b6a0e4'),
                        (QPalette.HighlightedText, '#201b2b'), (QPalette.PlaceholderText, '#a4a5b2'),
                        (QPalette.ToolTipBase, '#303139'), (QPalette.ToolTipText, '#ebeaf0'),
                        (QPalette.Mid, '#50515c'), (QPalette.Midlight, '#393a43'),
                        (QPalette.Light, '#555560'), (QPalette.Dark, '#1b1c21'),
                        (QPalette.Link, '#b6a0e4'), (QPalette.LinkVisited, '#c6b2ee')):
        palette.setColor(role, QColor(color))
    for role in (QPalette.Text, QPalette.WindowText, QPalette.ButtonText):
        palette.setColor(QPalette.Disabled, role, QColor('#70717d'))
    return palette
