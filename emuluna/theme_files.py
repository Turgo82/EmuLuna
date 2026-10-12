"""Editable theme packages: JSON colors and optional QSS, never executable code."""
from dataclasses import dataclass
import json
from pathlib import Path
import re
import shutil

from PySide6.QtGui import QColor

DATA = Path(__file__).with_name('data') / 'themes'
BUILTIN_IDS = ('luna', 'daylight', 'classic')
LEGACY_IDS = {'purple': 'luna', 'light': 'daylight', 'openemu': 'classic'}
ROLE_NAMES = ('window', 'window-text', 'base', 'alternate-base', 'text', 'button',
              'button-text', 'highlight', 'accent', 'highlighted-text', 'placeholder-text',
              'tooltip-base', 'tooltip-text', 'mid', 'midlight', 'light', 'dark',
              'link', 'link-visited', 'disabled-text')


@dataclass(frozen=True)
class Theme:
    id: str
    name: str
    palette: dict
    colors: dict
    styles: dict
    assets: dict
    fonts: dict

    def stylesheet(self, component):
        base = DATA / 'base' / (component + '.qss')
        if component in self.styles:
            return self.styles[component]
        return base.read_text(encoding='utf-8') if base.is_file() else ''


def package_path(folder, relative):
    path = Path(relative)
    if path.is_absolute() or '..' in path.parts or not path.parts:
        raise ValueError('Theme files must stay inside their theme folder.')
    result = folder / path
    if not result.resolve().is_relative_to(folder.resolve()):
        raise ValueError('Theme files must stay inside their theme folder.')
    return result


class ThemeRegistry:
    def __init__(self, root=None):
        self.folder = Path(root) / 'themes' if root else None
        self.errors = []
        self.themes = {}
        for folder in (DATA / key for key in BUILTIN_IDS):
            theme = self.load(folder)
            self.themes[theme.id] = theme
        if self.folder and self.folder.is_dir():
            for folder in sorted(self.folder.iterdir()):
                if not folder.is_dir() or not (folder / 'theme.json').is_file():
                    continue
                try:
                    theme = self.load(folder, custom=True)
                    if theme.id in self.themes or theme.id in LEGACY_IDS:
                        raise ValueError('Choose a unique theme id.')
                    self.themes[theme.id] = theme
                except (ValueError, OSError, TypeError, KeyError) as error:
                    self.errors.append(f'{folder.name}: {error}')

    def load(self, folder, custom=False):
        if (folder / 'theme.json').stat().st_size > 128 * 1024:
            raise ValueError('The theme description is too large.')
        value = json.loads((folder / 'theme.json').read_text(encoding='utf-8'))
        if not isinstance(value, dict):
            raise ValueError('The theme description must be a JSON object.')
        if value.get('format') != 1 or not re.fullmatch(r'[a-z0-9][a-z0-9_-]{0,63}', value.get('id', '')):
            raise ValueError('Unsupported theme format or invalid id.')
        if not isinstance(value.get('name'), str) or not value['name'].strip():
            raise ValueError('A theme needs a name.')
        # Legacy partial custom packages remain readable; full packages have no
        # parent dependency. Newly created themes always contain complete data.
        needs_parent = 'extends' in value or (custom and any(
            role not in value.get('palette', {}) for role in ROLE_NAMES))
        parent_id = value.get('extends', 'luna')
        parent_id = LEGACY_IDS.get(parent_id, parent_id)
        parent = self.themes.get(parent_id) if needs_parent and parent_id in BUILTIN_IDS else None
        if needs_parent and not parent:
            raise ValueError('The parent theme was not found.')
        palette = dict(parent.palette) if parent else {}
        colors = dict(parent.colors) if parent else {}
        for field, target in (('palette', palette), ('colors', colors)):
            entries = value.get(field, {})
            if not isinstance(entries, dict):
                raise ValueError(f'{field} must contain named colors.')
            for key, color in entries.items():
                if (not isinstance(key, str) or not re.fullmatch(r'[a-z][a-z-]*', key)
                        or not isinstance(color, str) or not QColor(color).isValid()):
                    raise ValueError('Invalid theme color.')
                if field == 'palette' and key not in ROLE_NAMES:
                    raise ValueError(f'Unknown palette role: {key}')
                target[key] = QColor(color).name()
        if any(key not in palette for key in ROLE_NAMES):
            raise ValueError('The theme palette is incomplete.')
        assets = dict(parent.assets) if parent else {'controls-background': str(DATA.parent / 'textures/controller-walnut.png')}
        for field in ('assets', 'styles', 'fonts'):
            if not isinstance(value.get(field, {}), dict):
                raise ValueError(f'{field} must be a JSON object.')
        fonts = dict(parent.fonts) if parent else {'game-title-weight': 700}
        for key, weight in value.get('fonts', {}).items():
            if key != 'game-title-weight' or type(weight) is not int or not 100 <= weight <= 900:
                raise ValueError('Game title weight must be between 100 and 900.')
            fonts[key] = weight
        for key, relative in value.get('assets', {}).items():
            if key != 'controls-background':
                raise ValueError('Unknown theme asset.')
            asset = package_path(folder, relative) if relative is not None else None
            if asset is not None and not asset.is_file():
                raise ValueError('The theme background image is missing.')
            assets[key] = str(asset) if asset else None
        styles = dict(parent.styles) if parent else {}
        for component, relative in value.get('styles', {}).items():
            if component not in ('library', 'settings', 'gameplay', 'onboarding'):
                raise ValueError('Unknown stylesheet component.')
            path = package_path(folder, relative)
            if path.stat().st_size > 512 * 1024:
                raise ValueError('The stylesheet is too large.')
            style = path.read_text(encoding='utf-8')
            # Relative images are scoped to this package, including symlinks.
            def image_url(match):
                asset = package_path(folder, match[1].strip('\"\''))
                return 'url("' + str(asset).replace('"', '%22') + '")'
            style = re.sub(r'url\(([^)]+)\)', image_url, style)
            styles[component] = styles.get(component, '') + '\n' + style
        return Theme(value['id'], value['name'].strip(), palette, colors, styles, assets, fonts)

    def get(self, theme_id):
        return self.themes.get(LEGACY_IDS.get(theme_id, theme_id), self.themes['luna'])

    def create(self, name, parent='luna'):
        if not self.folder:
            raise ValueError('No library theme folder is available.')
        slug = re.sub(r'[^a-z0-9]+', '-', name.lower()).strip('-')[:50] or 'my-theme'
        candidate = slug
        number = 2
        while candidate in self.themes or candidate in LEGACY_IDS or (self.folder / candidate).exists():
            candidate = f'{slug}-{number}'
            number += 1
        folder = self.folder / candidate
        folder.mkdir(parents=True)
        inherited = self.get(parent)
        value = {'format': 1, 'id': candidate, 'name': name.strip() or 'My theme',
                 'palette': dict(inherited.palette), 'colors': dict(inherited.colors),
                 'fonts': dict(inherited.fonts), 'assets': {}, 'styles': {}}
        copied = {}

        def copy_asset(source):
            if source not in copied:
                target = folder / ('asset-' + str(len(copied) + 1) + Path(source).suffix)
                shutil.copyfile(source, target)
                copied[source] = target.name
            return copied[source]

        for key, asset in inherited.assets.items():
            value['assets'][key] = copy_asset(asset) if asset else None
        for component in ('library', 'settings', 'gameplay', 'onboarding'):
            style = inherited.stylesheet(component)
            style = re.sub(r'url\(([^)]+)\)',
                           lambda match: 'url("' + copy_asset(match[1].strip('\"\'')) + '")', style)
            relative = component + '.qss'
            (folder / relative).write_text(style, encoding='utf-8')
            value['styles'][component] = relative
        (folder / 'theme.json').write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')
        return candidate
