"""Browse existing state and screenshot files alongside the game library."""
from pathlib import Path

from PySide6.QtCore import Qt, QSize, Signal, QUrl, QRect
from PySide6.QtGui import QIcon, QPixmap, QDesktopServices, QAction, QColor, QPainter, QPen, QPalette
from PySide6.QtWidgets import (QWidget, QVBoxLayout, QStackedWidget, QListWidget,
    QListWidgetItem, QTableWidget, QAbstractItemView, QHeaderView, QLabel, QStyle, QMenu, QTreeWidget, QTreeWidgetItem, QStyledItemDelegate, QMessageBox)

from .library_widgets import SortItem, date_text
from .systems import SYSTEMS, CATALOG
from .theme import paint_thumbnail_highlight


def media_entries(library, kind, games):
    """Read the existing on-disk layout; never move files or change save identity."""
    result = []
    root = library.root.resolve()
    if kind == 'states':
        candidates = ((path, game) for game in games
                      for path in (root / 'states' / game['id']).rglob('*.oesavestate'))
    else:
        # Older screenshots carry a 12-character game digest, not a database row.
        prefixes = {}
        for game in games:
            prefixes.setdefault(game['id'][:12], []).append(game)
        candidates = ((path, prefixes[path.name.split('-')[0]][0])
                      for path in (root / 'screenshots').glob('*.png')
                      if len(prefixes.get(path.name.split('-')[0], [])) == 1)
    for path, game in candidates:
        try:
            if path.is_symlink() or not path.resolve().is_relative_to(root) or not path.is_file():
                continue
            core = ''
            if kind == 'states':
                parts = path.relative_to(root / 'states' / game['id']).parts
                core_id = parts[1] if len(parts) == 4 and parts[0] == 'libretro' else None
                core = CATALOG.get(core_id, {}).get('name', core_id or 'Legacy core')
                label = 'Automatic save' if path.stem == 'auto' else (
                    'Slot ' + path.stem[5:] if path.stem.startswith('slot-') else path.stem)
            else:
                label = 'Screenshot'
            preview = path.with_suffix('.png') if kind == 'states' else path
            # Ignore stale sidecars left behind by an older writer or failed capture.
            has_preview = (preview.is_file() and not preview.is_symlink() and
                           preview.stat().st_mtime_ns >= path.stat().st_mtime_ns)
            result.append(dict(preview=str(preview) if has_preview else '',
                preview_revision=preview.stat().st_mtime_ns if has_preview else 0, path=path, game_id=game['id'], title=game['title'],
                system=game['system'], cover=game['cover'], cover_revision=game['cover_revision'], kind=kind, label=label,
                core=core, modified=path.stat().st_mtime))
        except (OSError, ValueError):
            continue
    return sorted(result, key=lambda entry: entry['modified'], reverse=True)


class ConsoleGroupDelegate(QStyledItemDelegate):
    def __init__(self, parent, icon_provider=None):
        super().__init__(parent)
        self.icon_provider = icon_provider

    def paint(self, painter, option, index):
        if index.data(Qt.UserRole + 1) == "console":
            painter.save()
            font = option.font
            font.setBold(True)
            painter.setFont(font)
            painter.setPen(option.palette.color(QPalette.Text))
            painter.drawText(option.rect.adjusted(8, 0, -8, 0), Qt.AlignLeft | Qt.AlignVCenter, index.data())
            painter.restore()
        else:
            painter.save()
            icon = self.icon_provider(index.data(Qt.UserRole)) if self.icon_provider else index.data(Qt.DecorationRole)
            size = icon.actualSize(self.parent().iconSize())
            art_rect = QRect(option.rect.center().x() - size.width() // 2,
                             option.rect.top() + 9, size.width(), size.height())
            paint_thumbnail_highlight(painter, art_rect, option)
            icon.paint(painter, art_rect, Qt.AlignCenter)
            text_rect = QRect(option.rect.left() + 6, art_rect.bottom() + 8,
                              option.rect.width() - 12, option.rect.bottom() - art_rect.bottom() - 12)
            painter.setFont(option.font)
            painter.setPen(option.palette.color(QPalette.Text))
            painter.setClipRect(text_rect)
            painter.drawText(text_rect, Qt.AlignHCenter | Qt.AlignTop | Qt.TextWordWrap, index.data())
            painter.restore()



class ConsoleGrid(QListWidget):
    """Full-width console headings between wrapping rows of media cards."""
    def __init__(self):
        super().__init__()
        self.card_size = QSize(208, 266)
        self.setItemDelegate(ConsoleGroupDelegate(self))

    def setGridSize(self, size):
        self.card_size = size
        self.resize_cards()

    def resize_cards(self):
        for row in range(self.count()):
            item = self.item(row)
            item.setSizeHint(QSize(max(1, self.viewport().width() - 2 * self.spacing()), 36)
                             if item.data(Qt.UserRole + 1) == "console" else self.card_size)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self.resize_cards()


class MediaBrowser(QWidget):
    selection_changed = Signal()
    activated = Signal()
    media_removed = Signal()
    message = Signal(str)

    def __init__(self, library, thumbnails):
        super().__init__()
        self.library = library
        self.thumbnails = thumbnails
        self.entries = {}
        self.entry_count = 0
        self.mode = 'grid'
        self.kind = 'states'
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self.pages = QStackedWidget()
        layout.addWidget(self.pages)
        self.grid = ConsoleGrid()
        self.grid.setItemDelegate(ConsoleGroupDelegate(self.grid, self.preview_icon))
        self.grid.setViewMode(QListWidget.IconMode)
        self.grid.setResizeMode(QListWidget.Adjust)
        self.grid.setMovement(QListWidget.Static)
        self.grid.setWordWrap(True)
        self.grid.setMouseTracking(True)
        self.grid.viewport().setMouseTracking(True)
        self.grid.setVerticalScrollMode(QAbstractItemView.ScrollPerPixel)
        self.grid.setSpacing(8)
        self.pages.addWidget(self.grid)
        self.table = QTableWidget(0, 4)
        self.table.setHorizontalHeaderLabels(['Game', 'Save state', 'Date', 'Core'])
        self.table.verticalHeader().hide()
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.SingleSelection)
        self.table.setAlternatingRowColors(True)
        self.table.setSortingEnabled(True)
        self.table.horizontalHeader().setSectionResizeMode(QHeaderView.Interactive)
        self.table.horizontalHeader().setStretchLastSection(True)
        for column, width in enumerate((240, 150, 155, 170)):
            self.table.setColumnWidth(column, width)
        self.pages.addWidget(self.table)
        self.empty = QLabel()
        self.empty.setAlignment(Qt.AlignCenter)
        self.empty.setWordWrap(True)
        self.empty.setObjectName('subtle')
        self.pages.addWidget(self.empty)
        self.state_tree = QTreeWidget()
        self.state_tree.setColumnCount(4)
        self.state_tree.setHeaderLabels(['Console / Game', 'Save state', 'Date', 'Core'])
        self.state_tree.setSelectionMode(QAbstractItemView.SingleSelection)
        self.state_tree.setAlternatingRowColors(True)
        for column, width in enumerate((250, 145, 155, 160)):
            self.state_tree.setColumnWidth(column, width)
        self.pages.addWidget(self.state_tree)
        for view in (self.grid, self.table, self.state_tree):
            view.itemSelectionChanged.connect(self.selection_changed.emit)
            view.itemDoubleClicked.connect(lambda *_: self.activated.emit())
            view.setContextMenuPolicy(Qt.CustomContextMenu)
            view.customContextMenuRequested.connect(lambda point, view=view: self.context_menu(view, point))
            action = QAction('Open selected item', view)
            action.setShortcut('Return')
            action.setShortcutContext(Qt.WidgetWithChildrenShortcut)
            action.triggered.connect(self.activated.emit)
            view.addAction(action)
        self.resize_items(176)

    def preview_icon(self, key):
        entry = self.entries.get(key)
        if not entry:
            return QIcon()
        if entry['kind'] == 'screenshots':
            icon = self.thumbnails.icon(dict(id='media:' + key, cover=str(entry['path']),
                cover_revision=entry['modified']))
            return icon or self.style().standardIcon(QStyle.SP_FileIcon)
        screenshot = self.thumbnails.icon(dict(id='state-preview:' + key,
            cover=entry['preview'], cover_revision=entry['preview_revision']))
        cover = self.thumbnails.icon(dict(id=entry['game_id'], cover=entry['cover'],
            cover_revision=entry['cover_revision']))
        canvas = QPixmap(256, 192)
        canvas.fill(QColor('#17181d'))
        painter = QPainter(canvas)
        # This composite uses image pixels, independent of desktop scaling.
        # Implicit-DPR pixmaps have physical sizes but paint at logical sizes,
        # leaving a half-filled cover frame on a 2x display.
        if screenshot and not screenshot.isNull():
            picture = screenshot.pixmap(QSize(256, 192), 1.0)
            painter.drawPixmap((256-picture.width())//2, (192-picture.height())//2, picture)
        else:
            painter.setPen(self.palette().color(QPalette.PlaceholderText))
            painter.drawText(QRect(8, 40, 240, 80), Qt.AlignCenter,
                             'Loading preview…' if entry['preview'] and screenshot is None else 'No screenshot')
        if cover and not cover.isNull():
            badge = cover.pixmap(QSize(58, 72), 1.0)
        else:
            badge = QIcon(str(SYSTEMS[entry['system']].icon)).pixmap(QSize(32, 32), 1.0)
        rect = QRect(248-badge.width(), 184-badge.height(), badge.width(), badge.height())
        painter.fillRect(rect.adjusted(-3, -3, 3, 3), self.palette().color(QPalette.Window))
        painter.drawPixmap(rect.topLeft(), badge)
        painter.setPen(QPen(self.palette().color(QPalette.Mid), 1))
        painter.drawRect(rect.adjusted(-2, -2, 2, 2))
        painter.end()
        return QIcon(canvas)

    def selected_entry(self):
        view = (self.state_tree if self.kind == 'states' else self.table) if self.mode == 'list' else self.grid
        selected = view.selectedItems()
        key = selected[0].data(0, Qt.UserRole) if selected and view is self.state_tree else selected[0].data(Qt.UserRole) if selected else None
        return self.entries.get(key)

    def select_path(self, key):
        for group_index in range(self.state_tree.topLevelItemCount()):
            group = self.state_tree.topLevelItem(group_index)
            for index in range(group.childCount()):
                item = group.child(index)
                if item.data(0, Qt.UserRole) == key:
                    self.state_tree.setCurrentItem(item)
        for index in range(self.grid.count()):
            if self.grid.item(index).data(Qt.UserRole) == key:
                self.grid.setCurrentRow(index)
                break
        for row in range(self.table.rowCount()):
            if self.table.item(row, 0).data(Qt.UserRole) == key:
                self.table.selectRow(row)
                break

    def change_view(self, mode):
        selected = self.selected_entry()
        self.mode = mode
        if selected:
            self.select_path(str(selected['path']))
        self.pages.setCurrentIndex((3 if self.kind == 'states' else 1) if mode == 'list' and self.entries else 0 if self.entries else 2)
        self.selection_changed.emit()

    def resize_items(self, size):
        self.cover_size = size
        height = round(size * 3 / 4) if self.kind == 'states' else size
        self.grid.setIconSize(QSize(size, height))
        self.grid.setGridSize(QSize(size + 32, height + 90))

    def refresh(self, kind, games, query='', matching_games=()):
        selected = self.selected_entry()
        self.kind = kind
        self.resize_items(self.cover_size)
        entries = media_entries(self.library, kind, games)
        query = query.strip().casefold()
        if query:
            entries = [entry for entry in entries if entry['game_id'] in matching_games or
                       query in ' '.join((entry['title'], entry['label'], entry['core'], entry['path'].name,
                                         SYSTEMS[entry['system']].name)).casefold()]
        if kind == 'states':
            entries.sort(key=lambda entry: (SYSTEMS[entry['system']].name.casefold(), -entry['modified']))
        self.entries = {str(entry['path']): entry for entry in entries}
        self.entry_count = len(entries)
        self.grid.blockSignals(True)
        self.table.blockSignals(True)
        self.grid.clear()
        self.state_tree.blockSignals(True)
        self.state_tree.clear()
        self.table.setSortingEnabled(False)
        self.table.setRowCount(len(entries))
        self.table.setHorizontalHeaderLabels(['Game', 'Save state' if kind == 'states' else 'Screenshot', 'Date', 'Core'])
        self.table.setColumnHidden(3, kind != 'states')
        last_system = None
        for row, entry in enumerate(entries):
            if kind == 'states' and entry['system'] != last_system:
                last_system = entry['system']
                title = SYSTEMS[last_system].name
                header = QListWidgetItem(title)
                header.setData(Qt.UserRole + 1, "console")
                header.setFlags(Qt.NoItemFlags)
                self.grid.addItem(header)
                group = QTreeWidgetItem([title])
                group.setFlags(Qt.ItemIsEnabled)
                group.setIcon(0, QIcon(str(SYSTEMS[last_system].icon)))
                self.state_tree.addTopLevelItem(group)
                group.setExpanded(True)
            if kind == 'states':
                child = QTreeWidgetItem([entry['title'], entry['label'], date_text(entry['modified']), entry['core']])
                child.setData(0, Qt.UserRole, str(entry['path']))
                group.addChild(child)
            path = str(entry['path'])
            icon = self.style().standardIcon(QStyle.SP_DialogSaveButton if kind == 'states' else QStyle.SP_FileIcon)
            detail = date_text(entry['modified'])
            item = QListWidgetItem(icon, f"{entry['title']}\n{entry['label']} · {detail}")
            item.setData(Qt.UserRole, path)
            item.setToolTip(f"{entry['title']}\n{entry['label']}\n{entry['core']}\n{detail}\n{path}")
            self.grid.addItem(item)
            for column, value in enumerate((entry['title'], entry['label'], detail, entry['core'])):
                cell = SortItem(value, path, entry['modified'] if column == 2 else None)
                cell.setToolTip(item.toolTip())
                self.table.setItem(row, column, cell)
        self.table.setSortingEnabled(True)
        self.grid.resize_cards()
        self.state_tree.blockSignals(False)
        self.grid.blockSignals(False)
        self.table.blockSignals(False)
        if selected and str(selected['path']) in self.entries:
            self.select_path(str(selected['path']))
        self.empty.setText(('No matching save states' if kind == 'states' else 'No matching screenshots') +
            ('\n\nSave a state while playing, then return here to browse it.' if kind == 'states' else
             '\n\nPress F12 while playing to capture a screenshot.') +
            '\nYour selected console or collection and search apply here too.')
        self.pages.setCurrentIndex((3 if kind == 'states' else 1) if self.mode == 'list' and entries else 0 if entries else 2)
        self.selection_changed.emit()

    def context_menu(self, view, point):
        item = view.itemAt(point)
        if not item:
            return
        key = item.data(0, Qt.UserRole) if view is self.state_tree else item.data(Qt.UserRole)
        if not key:
            return
        self.select_path(key)
        entry = self.selected_entry()
        menu = QMenu(self)
        menu.addAction('Load state' if self.kind == 'states' else 'Open screenshot', self.activated.emit)
        menu.addAction('Show in folder', lambda: QDesktopServices.openUrl(QUrl.fromLocalFile(str(entry['path'].parent))))
        menu.addSeparator()
        menu.addAction('Move save state to Trash…' if self.kind == 'states' else 'Move screenshot to Trash…',
                       lambda: self.remove_entry(entry))
        menu.exec(view.viewport().mapToGlobal(point))

    def remove_entry(self, entry):
        from .file_management import remove_media
        label = 'save state and its preview' if entry['kind'] == 'states' else 'screenshot'
        answer = QMessageBox.question(self, 'Move to Trash?',
            f'Move this {label} for “{entry["title"]}” to Trash?\n\n{entry["path"].name}\n\nThe game and its other saves will be kept.',
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No)
        if answer != QMessageBox.Yes:
            return
        try:
            remove_media(self.library, entry)
            self.media_removed.emit()
            self.message.emit('Save state moved to Trash.' if entry['kind'] == 'states' else 'Screenshot moved to Trash.')
        except (OSError,ValueError) as error:
            self.message.emit('Could not remove media: ' + str(error))
