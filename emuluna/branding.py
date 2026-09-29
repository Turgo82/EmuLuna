"""Shared application identity and scalable, theme-aware UI icons."""
from pathlib import Path

from PySide6.QtCore import QByteArray, QSize, Qt
from PySide6.QtGui import QColor, QIcon, QIconEngine, QPainter, QPalette, QPixmap
from PySide6.QtSvg import QSvgRenderer

ICON = Path(__file__).parent / 'data' / 'branding' / 'emuluna.png'
LOGO = Path(__file__).parent / 'data' / 'branding' / 'emuluna-logo.png'
MASCOT = Path(__file__).parent / 'data' / 'branding' / 'emuluna-mascot.png'
UNLOCK_SOUND = Path(__file__).parent / 'data' / 'sounds' / 'unlock-pop.wav'
UI_ICON_DIR = Path(__file__).parent / 'data' / 'icons' / 'ui'
COLLECTION_ICON_DIR = Path(__file__).parent / 'data' / 'icons' / 'collections'
COLLECTION_ICONS = {
    'collection-all': 'all-games.svg',
    'collection-recent': 'recently-played.svg',
    'collection-favorite': 'favorites.svg',
    'collection-added': 'recently-added.svg',
    'collection': 'collection.svg',
    'smart-collection': 'smart-collection.svg',
}


class PaletteSvgIconEngine(QIconEngine):
    """Render packaged SVG paths at the exact requested scale and palette color."""

    def __init__(self, path, foreground=None):
        super().__init__()
        self.path = Path(path)
        self.foreground = foreground
        self.template = self.path.read_text(encoding='utf-8')

    def clone(self):
        return PaletteSvgIconEngine(self.path, self.foreground)

    def key(self):
        return 'EmuLunaPaletteSvg'

    def color(self, mode, state):
        if self.foreground:
            return QColor(self.foreground)
        from .theme import theme_palette
        palette = theme_palette()
        group = QPalette.Disabled if mode == QIcon.Disabled else QPalette.Active
        role = QPalette.HighlightedText if mode == QIcon.Selected or state == QIcon.On else QPalette.WindowText
        return palette.color(group, role)

    def renderer(self, mode, state):
        color = self.color(mode, state).name(QColor.HexRgb)
        svg = self.template.replace('currentColor', color)
        svg = svg.replace('<svg ',
            f'<svg fill="none" stroke="{color}" stroke-width="1.5" '
            'stroke-linecap="round" stroke-linejoin="round" ', 1)
        return QSvgRenderer(QByteArray(svg.encode('utf-8')))

    def paint(self, painter, rect, mode, state):
        painter.save()
        painter.setRenderHint(QPainter.Antialiasing)
        self.renderer(mode, state).render(painter, rect)
        painter.restore()

    def pixmap(self, size, mode, state):
        pixmap = QPixmap(size)
        pixmap.fill(Qt.transparent)
        painter = QPainter(pixmap)
        self.paint(painter, pixmap.rect(), mode, state)
        painter.end()
        return pixmap

    def scaledPixmap(self, size, mode, state, scale):
        pixel_size = QSize(max(1, round(size.width() * scale)),
                           max(1, round(size.height() * scale)))
        pixmap = self.pixmap(pixel_size, mode, state)
        pixmap.setDevicePixelRatio(scale)
        return pixmap


def navigation_icon(name, foreground=None):
    """Load a packaged vector icon while following native theme colors."""
    filename = COLLECTION_ICONS.get(name, name + '.svg')
    directory = COLLECTION_ICON_DIR if name in COLLECTION_ICONS else UI_ICON_DIR
    path = directory / filename
    if not path.is_file():
        raise KeyError(f'Unknown navigation icon: {name}')
    return QIcon(PaletteSvgIconEngine(path, foreground))


def configure_application(app):
    from .theme import follow_system_theme
    follow_system_theme()
    app.setApplicationName('EmuLuna')
    app.setOrganizationName('EmuLuna')
    app.setDesktopFileName('emuluna')
    app.setWindowIcon(QIcon(str(ICON)))
