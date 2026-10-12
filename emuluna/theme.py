"""Native palette styling and the common cover/preview selection treatment."""
import re
from pathlib import Path
from weakref import WeakSet
from .theme_files import ThemeRegistry, ROLE_NAMES
from shiboken6 import isValid
from PySide6.QtCore import Qt, QObject, QTimer, QEvent
from PySide6.QtGui import QPalette, QPen, QColor
from PySide6.QtWidgets import QStyle, QApplication, QWidget

EMULUNA_COLOR_SCHEME = Path(__file__).with_name('data') / 'EmuLuna.colors'
EMULUNA_WINDOW_COLOR_SCHEME = Path(__file__).with_name('data') / 'EmuLuna-Window.colors'

LIBRARY_STYLE = (Path(__file__).with_name("data") / "themes/base/library.qss").read_text(encoding="utf-8")


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
        self.use_system = False
        self.registry = ThemeRegistry()
        self.theme_id = "luna"
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
            component = widget.property('_emuluna_theme_component')
            current = widget.styleSheet()
            if component:
                template = self.registry.get(self.theme_id).stylesheet(component)
                styled.append((widget, template))
                widget.setStyleSheet('')
                continue
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
            widget.setPalette(controls_palette(palette) if on_controls_surface(widget) else palette)
            if self.decoration and widget.isWindow():
                scheme = (native_scheme(True) if (widget.property('emuluna.library_window')
                                                   or widget.property('emuluna.library_header'))
                          else native_scheme(False))
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
        colors.update(self.registry.get(self.theme_id).colors)
        for widget, template in styled:
            applied = re.sub(r'palette\(([-a-z]+)\)',
                             lambda match: colors.get(match[1], match[0]), template)
            applied = re.sub(r'theme\(([-a-z]+)\)', lambda match: colors.get(match[1], match[0]), applied)
            widget.setStyleSheet(applied)
            widget.setProperty('_emuluna_theme_template', template)
            widget.setProperty('_emuluna_theme_applied', applied)


def follow_system_theme(library=None):
    app = QApplication.instance()
    if not hasattr(app, '_emuluna_system_theme'):
        app._emuluna_system_theme = SystemThemeBinding(app)
    if library is not None:
        binding = app._emuluna_system_theme
        registry = ThemeRegistry(library.root)
        saved_id = library.setting('appearance.theme', 'luna')
        theme_id = registry.get(saved_id).id
        if saved_id in ('purple', 'light', 'openemu'):
            library.set_setting('appearance.theme', theme_id)
        changed = binding.theme_id != theme_id or binding.registry.folder != registry.folder
        binding.registry = registry
        binding.theme_id = theme_id
        enabled = library.setting('appearance.use_system_theme', '0') == '1'
        # Opening another dialog does not change the theme. Its Show event
        # styles that window alone; only a real theme change touches the app.
        if binding.use_system != enabled or changed:
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
    app.setProperty('KDE_COLOR_SCHEME_PATH', None if use_system else str(native_scheme(True)))


def theme_palette():
    app = QApplication.instance()
    binding = getattr(app, '_emuluna_system_theme', None)
    if binding is None or binding.use_system:
        return app.palette()
    palette = QPalette(app.palette())
    roles = (QPalette.Window, QPalette.WindowText, QPalette.Base, QPalette.AlternateBase,
             QPalette.Text, QPalette.Button, QPalette.ButtonText, QPalette.Highlight,
             QPalette.Accent, QPalette.HighlightedText, QPalette.PlaceholderText,
             QPalette.ToolTipBase, QPalette.ToolTipText, QPalette.Mid, QPalette.Midlight,
             QPalette.Light, QPalette.Dark, QPalette.Link, QPalette.LinkVisited)
    colors = binding.registry.get(binding.theme_id).palette
    for name, role in zip(ROLE_NAMES, roles):
        palette.setColor(role, QColor(colors[name]))
    for role in (QPalette.Text, QPalette.WindowText, QPalette.ButtonText):
        palette.setColor(QPalette.Disabled, role, QColor(colors['disabled-text']))
    return palette


def apply_theme_style(widget, component):
    follow_system_theme()
    widget.setProperty('_emuluna_theme_component', component)
    binding = QApplication.instance()._emuluna_system_theme
    widget.setStyleSheet(binding.registry.get(binding.theme_id).stylesheet(component))
    binding.refresh([widget])


def theme_color(name, default):
    binding = getattr(QApplication.instance(), '_emuluna_system_theme', None)
    return QColor(binding.registry.get(binding.theme_id).colors.get(name, default) if binding else default)


def select_theme(library, theme_id):
    library.set_setting('appearance.theme', theme_id)
    library.set_setting('appearance.use_system_theme', '0')
    follow_system_theme(library)
    QApplication.instance()._emuluna_system_theme.refresh()


def reload_themes(library):
    follow_system_theme(library)
    binding = QApplication.instance()._emuluna_system_theme
    binding.refresh()
    return binding.registry


def theme_asset(name):
    binding = getattr(QApplication.instance(), '_emuluna_system_theme', None)
    return binding.registry.get(binding.theme_id).assets.get(name) if binding else None


def native_scheme(header=True):
    binding = getattr(QApplication.instance(), '_emuluna_system_theme', None)
    if binding is None or binding.theme_id == 'luna':
        return EMULUNA_COLOR_SCHEME if header else EMULUNA_WINDOW_COLOR_SCHEME
    theme = binding.registry.get(binding.theme_id)
    # Derive native decoration colors from the same package; no extra hand
    # maintained color-scheme file is needed when editing a custom theme.
    import tempfile
    folder = (binding.registry.folder.parent / 'cache/themes' if binding.registry.folder
              else Path(tempfile.gettempdir()) / 'emuluna-themes')
    folder.mkdir(parents=True, exist_ok=True)
    def rgb(name):
        color = QColor(theme.palette[name])
        return f'{color.red()},{color.green()},{color.blue()}'
    background = rgb('alternate-base' if header else 'window')
    foreground, inactive = rgb('window-text'), rgb('placeholder-text')
    value = (f'[General]\nName=EmuLuna {theme.name}\n\n[Colors:Window]\n'
             f'BackgroundNormal={background}\nForegroundNormal={foreground}\n\n[WM]\n'
             f'activeBackground={background}\nactiveForeground={foreground}\n'
             f'inactiveBackground={background}\ninactiveForeground={inactive}\n')
    path = folder / (theme.id + ('-header' if header else '-window') + '.colors')
    if not path.exists() or path.read_text(encoding='utf-8') != value:
        import os
        fd, temporary = tempfile.mkstemp(dir=folder)
        try:
            with os.fdopen(fd, 'w', encoding='utf-8') as file:
                file.write(value)
            os.replace(temporary, path)
        finally:
            Path(temporary).unlink(missing_ok=True)
    return path


def theme_font(name, default):
    binding = getattr(QApplication.instance(), '_emuluna_system_theme', None)
    return binding.registry.get(binding.theme_id).fonts.get(name, default) if binding else default


def on_controls_surface(widget):
    """Include footer buttons and popups owned by the wood Controls page."""
    if widget.objectName() == 'settingsDone' and widget.property('woodControls'):
        return True
    current = widget
    while current is not None:
        if current.objectName() == 'woodControlsPage':
            return True
        current = current.parentWidget()
    return False


def controls_palette(base=None):
    """Text and custom-painted widgets must use the wood surface's local palette."""
    palette = QPalette(base or theme_palette())
    text = theme_color('controls-text', '#fff5e9')
    subtle = theme_color('controls-subtle', '#ead7bc')
    for group in (QPalette.Active, QPalette.Inactive, QPalette.Disabled):
        foreground = subtle if group == QPalette.Disabled else text
        for role in (QPalette.WindowText, QPalette.Text, QPalette.ButtonText):
            palette.setColor(group, role, foreground)
        palette.setColor(group,QPalette.PlaceholderText,subtle)
        palette.setColor(group,QPalette.Base,theme_color('controls-popup','#302015'))
        palette.setColor(group,QPalette.Highlight,theme_color('controls-accent','#e4b874'))
        palette.setColor(group,QPalette.HighlightedText,theme_color('controls-popup','#302015'))
        palette.setColor(group,QPalette.Mid,theme_color('controls-border','#b48a5b'))
    return palette
