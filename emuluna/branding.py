"""Shared application identity and the EmuLuna desktop icon."""
from pathlib import Path
from PySide6.QtCore import QByteArray
from PySide6.QtGui import QIcon, QPixmap, QPalette

ICON = Path(__file__).parent / 'data' / 'branding' / 'emuluna.png'
COLLECTION_ICON_DIR = Path(__file__).parent / 'data' / 'icons' / 'collections'
COLLECTION_ICONS = {
    'collection-all': 'all-games.svg',
    'collection-recent': 'recently-played.svg',
    'collection-favorite': 'favorites.svg',
    'collection-added': 'recently-added.svg',
    'collection': 'collection.svg',
    'smart-collection': 'smart-collection.svg',
}


def navigation_icon(name, foreground=None):
    """Original line icons using the native text and selected-text colors."""
    if name in COLLECTION_ICONS and foreground is None:
        return QIcon(str(COLLECTION_ICON_DIR / COLLECTION_ICONS[name]))
    paths = {
        'menu': '<path d="M3 5h18M3 12h18M3 19h18"/>',
        'search': '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
        'general': '<path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1"/><circle cx="12" cy="12" r="4"/>',
        'gameplay': '<path d="M7 8h10a5 5 0 0 1 4.6 7l-1 2.4a2 2 0 0 1-3.2.8L15 16H9l-2.4 2.2a2 2 0 0 1-3.2-.8L2.4 15A5 5 0 0 1 7 8Z M7 11v4M5 13h4"/><circle cx="16" cy="12" r="1"/><circle cx="18" cy="14" r="1"/>',
        'controls': '<path d="M5 5h14v14H5Z M8 10v4M6 12h4"/><circle cx="16" cy="10" r="1"/><circle cx="17.5" cy="13" r="1"/>',
        'cores': '<path d="M7 7h10v10H7Z M9 1v4M15 1v4M9 19v4M15 19v4M1 9h4M1 15h4M19 9h4M19 15h4"/>',
        'downloads': '<path d="M12 3v12M7 10l5 5 5-5M4 19h16"/>',
        'bios': '<path d="M5 4h14v16H5Z M8 8h8M8 12h8M8 16h5"/>',
        'library': '<path d="M16 3a9 9 0 1 0 5 14A8 8 0 0 1 16 3Z"/>',
        'states': '<path d="M4 3h13l3 3v15H4Z M8 3v6h8V3 M8 21v-8h8v8"/>',
        'screenshots': '<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="8" cy="9" r="1.5"/><path d="m3 18 5-5 4 3 4-6 5 6"/>',
        'grid': ''.join(f'<rect x="{x}" y="{y}" width="4" height="4"/>' for y in (3,10,17) for x in (3,10,17)),
        'list': '<path d="M3 5h18M3 10h18M3 15h18M3 20h18"/>',
        'power': '<path d="M12 2v9M6.3 5.5a8 8 0 1 0 11.4 0"/>',
        'fullscreen': '<path d="M9 4H4v5M15 4h5v5M4 15v5h5M20 15v5h-5"/>',
        'fullscreen-exit': '<path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5"/>',
    }
    icon = QIcon()
    from .theme import theme_palette
    palette = theme_palette()
    colors = ((QIcon.Off, foreground), (QIcon.On, foreground)) if foreground else (
        (QIcon.Off, palette.color(QPalette.WindowText).name()),
        (QIcon.On, palette.color(QPalette.HighlightedText).name()))
    for state, color in colors:
        svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><g fill="none" stroke="{color}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">{paths[name]}</g></svg>'
        pixmap = QPixmap()
        pixmap.loadFromData(QByteArray(svg.encode()), 'SVG')
        icon.addPixmap(pixmap, QIcon.Normal, state)
    selected_color = foreground or palette.color(QPalette.HighlightedText).name()
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><g fill="none" stroke="{selected_color}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">{paths[name]}</g></svg>'
    selected = QPixmap()
    selected.loadFromData(QByteArray(svg.encode()), 'SVG')
    icon.addPixmap(selected, QIcon.Selected, QIcon.Off)
    icon.addPixmap(selected, QIcon.Selected, QIcon.On)
    return icon


def configure_application(app):
    from .theme import follow_system_theme
    follow_system_theme()
    app.setApplicationName('EmuLuna')
    app.setOrganizationName('EmuLuna')
    app.setDesktopFileName('emuluna')
    app.setWindowIcon(QIcon(str(ICON)))
