"""Library controls shared by the grid and list; no emulator implementation here."""
import json
import unicodedata
from datetime import datetime
from pathlib import Path

from PySide6.QtCore import (Qt, QMimeData, Signal, QRect, QSize, QEvent, QPointF,
                            QItemSelectionModel, QSignalBlocker, QTimer)
from PySide6.QtGui import QDrag, QColor, QPen, QPixmap, QPainter, QIcon, QPolygonF, QCursor, QPalette
from PySide6.QtWidgets import (QApplication, QAbstractItemView, QCheckBox, QComboBox, QDialog,
    QDialogButtonBox, QFormLayout, QLabel, QLineEdit, QListWidget, QSpinBox,
    QTableWidget, QTableWidgetItem, QVBoxLayout, QStyledItemDelegate, QStyle, QStyleOptionViewItem,
    QScrollArea, QWidget, QPlainTextEdit, QPushButton, QMessageBox, QHBoxLayout, QToolButton, QSlider, QSizePolicy, QScrollBar)

from .library import SYSTEMS
from .theme import paint_thumbnail_highlight
from .branding import navigation_icon

GAME_MIME = "application/x-emuluna-library-games"
SIDEBAR_COUNT_ROLE = Qt.UserRole + 3


class ExpandableSearch(QWidget):
    """Keep a query visible; collapse an unused search field back to its icon."""
    def __init__(self):
        super().__init__()
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(4)
        self.field = QLineEdit()
        self.field.setObjectName('librarySearch')
        self.field.setFixedHeight(34)
        self.field.setPlaceholderText('Search')
        self.field.setAccessibleName('Search library')
        self.field.setToolTip('Search games, systems, filenames and information · Escape to clear and close')
        self.field.setMinimumWidth(100)
        self.field.setMaximumWidth(180)
        self.field.setClearButtonEnabled(True)
        self.field.installEventFilter(self)
        self.field.textChanged.connect(lambda text: self.field.show() if text else None)
        layout.addWidget(self.field)
        self.field.hide()
        self.button = QToolButton()
        self.button.setObjectName('searchButton')
        self.button.setFixedSize(30, 30)
        self.button.setIconSize(QSize(18, 18))
        self.button.setIcon(navigation_icon('search'))
        self.button.setAccessibleName('Search library')
        self.button.setToolTip('Search (Ctrl+F)')
        self.button.clicked.connect(self.toggle)
        layout.addWidget(self.button)

    def expand(self):
        self.field.show()
        self.field.setFocus(Qt.ShortcutFocusReason)
        self.field.selectAll()

    def toggle(self):
        if not self.field.isHidden() and not self.field.text():
            self.field.hide()
            self.button.setFocus()
        else:
            self.expand()

    def collapse_if_idle(self):
        if not self.field.hasFocus() and not self.field.text():
            self.field.hide()

    def eventFilter(self, watched, event):
        if watched is self.field:
            if event.type() == QEvent.KeyPress and event.key() == Qt.Key_Escape:
                self.field.clear()
                self.field.hide()
                self.button.setFocus()
                return True
            if event.type() == QEvent.FocusOut:
                QTimer.singleShot(0, self.collapse_if_idle)
        return super().eventFilter(watched, event)

    def changeEvent(self, event):
        super().changeEvent(event)
        if event.type() in (QEvent.PaletteChange, QEvent.ApplicationPaletteChange) and hasattr(self, 'button'):
            self.button.setIcon(navigation_icon('search'))


def date_text(timestamp):
    return datetime.fromtimestamp(timestamp).strftime("%Y-%m-%d %H:%M") if timestamp else "Never"


def title_initial(title):
    """Index the actual title, folding accents and grouping numbers under #."""
    title = unicodedata.normalize('NFKD', title)
    first = next((character for character in title if character.isalnum()), '')
    initial = first.upper()[:1]
    return initial if 'A' <= initial <= 'Z' else '#'


class AlphabetIndex(QWidget):
    jump_requested = Signal(str)

    def __init__(self):
        super().__init__()
        self.setObjectName('alphabetIndex')
        self.setAccessibleName('Jump to games by title')
        self.setFixedWidth(28)
        self.setStyleSheet("""
            QToolButton {background:transparent;border:0;border-radius:4px;padding:0;color:palette(window-text);}
            QToolButton:hover,QToolButton:focus {background:palette(midlight);}
            QToolButton:pressed {background:palette(highlight);color:palette(highlighted-text);}
            QToolButton:disabled {color:palette(disabled-text);}
        """)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(2, 4, 2, 4)
        layout.setSpacing(0)
        layout.addStretch()
        self.buttons = {}
        for letter in '#ABCDEFGHIJKLMNOPQRSTUVWXYZ':
            button = QToolButton()
            button.setText(letter)
            button.setAccessibleName('Jump to ' + ('numbers and other titles' if letter == '#' else letter))
            button.setFocusPolicy(Qt.StrongFocus)
            button.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
            button.setMinimumHeight(12)
            button.setMaximumHeight(24)
            button.clicked.connect(lambda checked=False, letter=letter: self.jump_requested.emit(letter))
            layout.addWidget(button, 1)
            self.buttons[letter] = button
        layout.addStretch()

    def set_letters(self, letters):
        for letter, button in self.buttons.items():
            button.setEnabled(letter in letters)
            button.setToolTip(button.accessibleName() if letter in letters else 'No matching games: ' + letter)


class LibraryScrollBar(QScrollBar):
    """A shared outer scrollbar, after the alphabet index, for both game views."""
    def __init__(self):
        super().__init__(Qt.Vertical)
        self.view = None
        self.setAccessibleName('Scroll game library')
        self.valueChanged.connect(self.scroll_view)

    def set_view(self, view):
        if self.view is not view:
            if self.view is not None:
                bar = self.view.verticalScrollBar()
                bar.rangeChanged.disconnect(self.sync)
                bar.valueChanged.disconnect(self.sync)
            self.view = view
            if view is not None:
                bar = view.verticalScrollBar()
                bar.rangeChanged.connect(self.sync)
                bar.valueChanged.connect(self.sync)
        self.sync()

    def sync(self, *_):
        if self.view is None:
            self.hide()
            return
        source = self.view.verticalScrollBar()
        with QSignalBlocker(self):
            self.setRange(source.minimum(), source.maximum())
            self.setPageStep(source.pageStep())
            self.setSingleStep(source.singleStep())
            self.setValue(source.value())
        self.setVisible(source.maximum() > source.minimum())

    def scroll_view(self, value):
        if self.view is not None:
            self.view.verticalScrollBar().setValue(value)


class GameDragMixin:
    def selectionCommand(self, index, event=None):
        # Qt normally collapses an extended selection to the item under a
        # right-click before customContextMenuRequested is emitted. Preserve
        # the complete selection when the menu is opened on one of its rows.
        if (event is not None and event.type() == QEvent.MouseButtonPress and
                event.button() == Qt.RightButton and self.selectionModel().isSelected(index)):
            return QItemSelectionModel.NoUpdate
        return super().selectionCommand(index, event)

    def startDrag(self, supported_actions):
        mime = QMimeData()
        mime.setData(GAME_MIME, json.dumps(self.game_ids()).encode())
        drag = QDrag(self)
        drag.setMimeData(mime)
        drag.exec(Qt.CopyAction)

    def dragEnterEvent(self, event):
        if event.mimeData().hasUrls():
            event.acceptProposedAction()

    def dragMoveEvent(self, event):
        if event.mimeData().hasUrls():
            event.acceptProposedAction()

    def dropEvent(self, event):
        if event.mimeData().hasUrls():
            paths = [url.toLocalFile() for url in event.mimeData().urls() if url.isLocalFile()]
            item = self.itemAt(event.position().toPoint())
            game_id = item.data(Qt.UserRole) if item else None
            self.files_dropped.emit(paths, game_id)
            event.acceptProposedAction()


class CoverSizeSlider(QSlider):
    default_size = 176

    def __init__(self):
        super().__init__(Qt.Horizontal)
        self.setRange(96, 256)
        self.setTickPosition(QSlider.NoTicks)
        self.setSingleStep(8)
        self.setPageStep(16)
        self.setToolTip("Cover size · double-click to reset")

    def mouseDoubleClickEvent(self, event):
        if event.button() == Qt.LeftButton:
            self.setValue(self.default_size)
            event.accept()
        else:
            super().mouseDoubleClickEvent(event)


class GameGrid(GameDragMixin, QListWidget):
    files_dropped = Signal(list, object)

    viewport_changed = Signal()
    play_requested = Signal(str)
    restart_requested = Signal(str)

    def __init__(self):
        super().__init__()
        self.action_info = lambda game_id: (False, False)
        self.hovered_id = None
        self.cover_dimensions = {}
        self.setMouseTracking(True)
        self.viewport().setMouseTracking(True)
        self.actions_bar = QWidget(self.viewport())
        self.actions_bar.setObjectName("coverActions")
        self.actions_bar.setStyleSheet("QWidget#coverActions {background:palette(window);border-radius:7px;} QToolButton {color:palette(window-text);background:transparent;border:0;padding:5px;} QToolButton:hover {background:palette(highlight);color:palette(highlighted-text);border-radius:5px;}")
        row = QHBoxLayout(self.actions_bar)
        row.setContentsMargins(3, 2, 3, 2)
        row.setSpacing(1)
        self.cover_play = QToolButton()
        self.cover_play.setFocusPolicy(Qt.NoFocus)
        self.cover_play.clicked.connect(lambda: self.play_requested.emit(self.hovered_id) if self.hovered_id else None)
        row.addWidget(self.cover_play)
        self.cover_restart = QToolButton()
        self.cover_restart.setFocusPolicy(Qt.NoFocus)
        self.cover_restart.setAccessibleName("Restart game")
        self.cover_restart.setToolTip("Restart game and clear automatic save…")
        self.update_restart_icon()
        self.cover_restart.clicked.connect(lambda: self.restart_requested.emit(self.hovered_id) if self.hovered_id else None)
        row.addWidget(self.cover_restart)
        self.actions_bar.hide()
        for widget in (self.viewport(), self.actions_bar, self.cover_play, self.cover_restart):
            widget.installEventFilter(self)
        self.verticalScrollBar().valueChanged.connect(self.hide_actions)
        self.verticalScrollBar().valueChanged.connect(self.viewport_changed)
        self.horizontalScrollBar().valueChanged.connect(self.hide_actions)

    def update_restart_icon(self):
        icon = QPixmap(24, 24)
        icon.fill(Qt.transparent)
        painter = QPainter(icon)
        painter.setRenderHint(QPainter.Antialiasing)
        painter.setPen(QPen(self.palette().color(QPalette.WindowText), 2.2))
        painter.drawArc(QRect(4, 5, 16, 16), 0, 180 * 16)
        painter.setPen(Qt.NoPen)
        painter.setBrush(self.palette().color(QPalette.WindowText))
        painter.drawPolygon(QPolygonF([QPointF(4, 17), QPointF(0, 10), QPointF(8, 10)]))
        painter.end()
        self.cover_restart.setIcon(QIcon(icon))

    def changeEvent(self, event):
        super().changeEvent(event)
        if event.type() in (QEvent.PaletteChange, QEvent.ApplicationPaletteChange) and hasattr(self, 'cover_restart'):
            self.update_restart_icon()

    def hide_actions(self, *_):
        self.hovered_id = None
        self.actions_bar.hide()

    def clear(self):
        self.hide_actions()
        super().clear()

    def show_actions(self, item):
        if item is None:
            self.hide_actions()
            return
        key = item.data(Qt.UserRole)
        if key != self.hovered_id:
            self.hovered_id = key
            self.can_resume, running = self.action_info(key)
            self.cover_play.setEnabled(not running)
            self.cover_restart.setEnabled(not running)
        area = self.itemDelegate().art_rect(self.visualItemRect(item), item.icon(), key)
        label = "Resume" if self.can_resume else "Play"
        self.cover_play.setText("▶ " + label if area.width() >= 115 else "▶")
        self.cover_play.setAccessibleName(label + " " + item.data(Qt.UserRole + 1))
        self.cover_play.setToolTip(label)
        self.cover_restart.setVisible(self.can_resume)
        self.actions_bar.adjustSize()
        self.actions_bar.move(area.center().x() - self.actions_bar.width() // 2,
                              area.center().y() - self.actions_bar.height() // 2)
        self.actions_bar.show()
        self.actions_bar.raise_()

    def eventFilter(self, watched, event):
        if event.type() == QEvent.MouseMove and watched is self.viewport():
            self.show_actions(self.itemAt(event.position().toPoint()))
        elif event.type() == QEvent.Leave:
            # Entering an overlaid button also sends Leave to the viewport.
            point = self.viewport().mapFromGlobal(QCursor.pos())
            if not self.actions_bar.geometry().contains(point):
                self.hide_actions()
        elif event.type() in (QEvent.Hide, QEvent.Resize, QEvent.Show) and watched is self.viewport():
            self.hide_actions()
            if event.type() != QEvent.Hide:
                self.viewport_changed.emit()
        return super().eventFilter(watched, event)

    def game_ids(self):
        return [item.data(Qt.UserRole) for item in self.selectedItems()]


class CoverDelegate(QStyledItemDelegate):
    """Keep covers, two title lines and ratings aligned at every grid size."""
    def __init__(self, parent, icon_provider=None):
        super().__init__(parent)
        self.icon_provider = icon_provider

    def sizeHint(self, option, index):
        return index.data(Qt.SizeHintRole) or QSize(192, 230)

    def art_rect(self, item_rect, icon, game_id):
        rect = item_rect.adjusted(5, 5, -5, -5)
        size = self.parent().iconSize().width()
        image_size = self.parent().cover_dimensions.get(game_id, QSize(size, size)).scaled(QSize(size, size), Qt.KeepAspectRatio)
        return QRect(rect.left() + (rect.width() - image_size.width()) // 2,
                     rect.top() + 4, image_size.width(), image_size.height())

    def paint(self, painter, option, index):
        painter.save()
        rect = option.rect.adjusted(5, 5, -5, -5)
        icon = self.icon_provider(index.data(Qt.UserRole)) if self.icon_provider else index.data(Qt.DecorationRole)
        art_rect = self.art_rect(option.rect, icon, index.data(Qt.UserRole))
        paint_thumbnail_highlight(painter, art_rect, option)
        icon.paint(painter, art_rect, Qt.AlignCenter)
        title_rect = QRect(rect.left() + 2, art_rect.bottom() + 5, rect.width() - 4, 34)
        title = index.data(Qt.UserRole + 1)
        title_height = option.fontMetrics.boundingRect(title_rect, Qt.TextWordWrap, title).height()
        title_rect.setHeight(min(34, title_height))
        painter.setFont(option.font)
        painter.setPen(option.palette.color(QPalette.Text))
        painter.setClipRect(title_rect)
        painter.drawText(title_rect, Qt.AlignHCenter | Qt.TextWordWrap, title)
        painter.setClipping(False)
        rating = index.data(Qt.UserRole + 2)
        painter.setPen(option.palette.color(QPalette.Highlight if rating else QPalette.PlaceholderText))
        painter.drawText(QRect(rect.left(), title_rect.bottom() + 2, rect.width(), 16), Qt.AlignCenter,
                         "★" * rating + "☆" * (5 - rating))
        painter.restore()


class GameTable(GameDragMixin, QTableWidget):
    files_dropped = Signal(list, object)

    def game_ids(self):
        return [self.item(index.row(), 0).data(Qt.UserRole) for index in self.selectionModel().selectedRows()]


class SortItem(QTableWidgetItem):
    def __init__(self, text, game_id, sort_value=None):
        super().__init__(text)
        self.setData(Qt.UserRole, game_id)
        self.setData(Qt.UserRole + 1, text.casefold() if sort_value is None else sort_value)

    def __lt__(self, other):
        return self.data(Qt.UserRole + 1) < other.data(Qt.UserRole + 1)


class SidebarDelegate(QStyledItemDelegate):
    """Keep collection names and right-aligned counts clear at every width."""
    def paint(self, painter, option, index):
        count = index.data(SIDEBAR_COUNT_ROLE)
        if count is None:
            return super().paint(painter, option, index)
        styled = QStyleOptionViewItem(option)
        self.initStyleOption(styled, index)
        title, icon = styled.text, QIcon(styled.icon)
        styled.text, styled.icon = "", QIcon()
        style = styled.widget.style() if styled.widget else QApplication.style()
        style.drawControl(QStyle.CE_ItemViewItem, styled, painter, styled.widget)
        selected = bool(styled.state & QStyle.State_Selected)
        enabled = bool(styled.state & QStyle.State_Enabled)
        mode = QIcon.Selected if selected else QIcon.Normal
        group = QPalette.Active if enabled else QPalette.Disabled
        title_role = QPalette.HighlightedText if selected else QPalette.Text
        count_role = QPalette.HighlightedText if selected else QPalette.PlaceholderText
        icon_size = QSize(20, 20)
        left = styled.rect.left() + 28
        icon_rect = QRect(left, styled.rect.center().y() - icon_size.height() // 2,
                          icon_size.width(), icon_size.height())
        # Theme the glyph at paint time. The sidebar can follow a changed system
        # palette without retaining the color used when its QIcon was created.
        device_ratio = painter.device().devicePixelRatioF()
        glyph = icon.pixmap(icon_size, device_ratio, mode, QIcon.Off)
        tinted = QPixmap(glyph.size())
        tinted.setDevicePixelRatio(glyph.devicePixelRatio())
        tinted.fill(Qt.transparent)
        glyph_painter = QPainter(tinted)
        glyph_painter.drawPixmap(0, 0, glyph)
        glyph_painter.setCompositionMode(QPainter.CompositionMode_SourceIn)
        glyph_painter.fillRect(tinted.rect(), styled.palette.color(group, title_role))
        glyph_painter.end()
        painter.drawPixmap(icon_rect.topLeft(), tinted)
        title_left = icon_rect.right() + 10
        right = styled.rect.right() - 18
        count_text = f"{int(count):,}"
        metrics = painter.fontMetrics()
        count_width = metrics.horizontalAdvance(count_text)
        title_rect = QRect(title_left, styled.rect.top(), max(0, right - count_width - 12 - title_left), styled.rect.height())
        count_rect = QRect(right - count_width, styled.rect.top(), count_width, styled.rect.height())
        painter.save()
        painter.setFont(styled.font)
        painter.setPen(styled.palette.color(group, title_role))
        painter.drawText(title_rect, Qt.AlignVCenter | Qt.AlignLeft,
                         metrics.elidedText(title, Qt.ElideRight, title_rect.width()))
        painter.setPen(styled.palette.color(group, count_role))
        painter.drawText(count_rect, Qt.AlignVCenter | Qt.AlignRight, count_text)
        painter.restore()


class LibrarySidebar(QListWidget):
    games_dropped = Signal(int, list)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setItemDelegate(SidebarDelegate(self))

    def section_divider_rect(self):
        """Return a divider centered in the visible sidebar, outside item padding."""
        for row in range(self.count()):
            item = self.item(row)
            if item.data(Qt.UserRole + 2) == "section-divider":
                item_rect = self.visualItemRect(item)
                return QRect(20, item_rect.center().y(), max(0, self.viewport().width() - 40), 1)
        return QRect()

    def paintEvent(self, event):
        super().paintEvent(event)
        rect = self.section_divider_rect()
        if not rect.isEmpty():
            painter = QPainter(self.viewport())
            painter.fillRect(rect, self.palette().color(QPalette.Mid))

    def dragEnterEvent(self, event):
        if event.mimeData().hasFormat(GAME_MIME):
            event.acceptProposedAction()

    def dragMoveEvent(self, event):
        item = self.itemAt(event.position().toPoint())
        if item and item.data(Qt.UserRole + 1) == "regular" and event.mimeData().hasFormat(GAME_MIME):
            event.acceptProposedAction()
        else:
            event.ignore()

    def dropEvent(self, event):
        item = self.itemAt(event.position().toPoint())
        if not item or item.data(Qt.UserRole + 1) != "regular":
            event.ignore()
            return
        try:
            ids = json.loads(bytes(event.mimeData().data(GAME_MIME)))
            if not isinstance(ids, list) or not all(isinstance(key, str) for key in ids):
                raise ValueError("Invalid games")
            self.games_dropped.emit(int(item.data(Qt.UserRole).split(":")[1]), ids)
            event.acceptProposedAction()
        except (ValueError, TypeError):
            event.ignore()


class SmartCollectionDialog(QDialog):
    def __init__(self, parent=None, record=None):
        super().__init__(parent)
        self.setWindowTitle("Edit smart collection" if record else "New smart collection")
        self.setMinimumWidth(440)
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("Games appear automatically when they match all selected rules."))
        form = QFormLayout()
        self.name = QLineEdit(record["name"] if record else "")
        form.addRow("Name", self.name)
        rules = json.loads(record["rules"]) if record else {}
        self.system = QComboBox()
        self.system.addItem("Any system", None)
        for key, system in SYSTEMS.items():
            self.system.addItem(system.name, key)
        self.system.setCurrentIndex(max(0, self.system.findData(rules.get("system"))))
        form.addRow("System", self.system)
        self.rating = QComboBox()
        self.rating.addItem("Any rating", None)
        for value in range(1, 6):
            self.rating.addItem("★" * value + " or higher", value)
        self.rating.setCurrentIndex(max(0, self.rating.findData(rules.get("minimum_rating"))))
        form.addRow("Rating", self.rating)
        self.favorite = QCheckBox("Favorites only")
        self.favorite.setChecked(rules.get("favorite", False))
        form.addRow(self.favorite)
        self.never = QCheckBox("Never played")
        self.never.setChecked(rules.get("never_played", False))
        form.addRow(self.never)
        self.days = {}
        for key, label in [("added_days", "Added in last"), ("played_days", "Played in last")]:
            spin = QSpinBox()
            spin.setRange(0, 36500)
            spin.setSpecialValueText("Any time")
            spin.setSuffix(" days")
            spin.setValue(rules.get(key, 0))
            form.addRow(label, spin)
            self.days[key] = spin
        layout.addLayout(form)
        self.error = QLabel()
        self.error.setWordWrap(True)
        layout.addWidget(self.error)
        buttons = QDialogButtonBox(QDialogButtonBox.Save | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.validate)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def rules(self):
        result = {}
        if self.system.currentData(): result["system"] = self.system.currentData()
        if self.rating.currentData(): result["minimum_rating"] = self.rating.currentData()
        if self.favorite.isChecked(): result["favorite"] = True
        if self.never.isChecked(): result["never_played"] = True
        for key, spin in self.days.items():
            if spin.value(): result[key] = spin.value()
        return result

    def validate(self):
        if not self.name.text().strip() or not self.rules():
            self.error.setText("Enter a name and select at least one rule.")
        elif self.never.isChecked() and self.days["played_days"].value():
            self.error.setText("Never played cannot be combined with a recent play date.")
        else:
            self.accept()


class GameInfoDialog(QDialog):
    def __init__(self, library, game_id, parent=None):
        super().__init__(parent)
        self.library, self.game_id = library, game_id
        game = library.get(game_id)
        self.setWindowTitle("Game information")
        self.resize(620, 780)
        layout = QVBoxLayout(self)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        body = QWidget()
        body_layout = QVBoxLayout(body)
        scroll.setWidget(body)
        layout.addWidget(scroll, 1)
        if game["cover"]:
            cover = QPixmap(str(library.root / game["cover"]))
            if not cover.isNull():
                preview = QLabel()
                preview.setAlignment(Qt.AlignCenter)
                preview.setPixmap(cover.scaled(110, 110, Qt.KeepAspectRatio, Qt.SmoothTransformation))
                body_layout.addWidget(preview)
        form = QFormLayout()
        self.title = QLineEdit(game["title"])
        self.original_title = game["title"]
        self.original_rating = game["rating"]
        form.addRow("Title", self.title)
        form.addRow("System", QLabel(SYSTEMS[game["system"]].name))
        self.rating = QComboBox()
        self.rating.addItems(["Unrated"] + ["★" * i for i in range(1, 6)])
        self.rating.setCurrentIndex(game["rating"])
        form.addRow("Rating", self.rating)
        metadata = library.metadata(game_id)
        self.original_metadata = metadata
        self.fields = {}
        for key, label in [("region", "Region"), ("developer", "Developer"), ("publisher", "Publisher"),
                           ("release_date", "Release date"), ("genre", "Genre"), ("players", "Players"), ("notes", "Notes")]:
            field = QLineEdit(metadata.get(key, ""))
            self.fields[key] = field
            form.addRow(label, field)
        self.description = QPlainTextEdit(metadata.get("description", ""))
        self.description.setMaximumHeight(115)
        form.addRow("Description", self.description)
        lookup = library.metadata_lookup(game_id)
        if lookup:
            source = QLabel(f"{lookup['provider']} · Last checked {date_text(lookup['checked'])}")
            source.setTextFormat(Qt.PlainText)
            source.setWordWrap(True)
            form.addRow("Information source", source)
            if lookup["source_url"]:
                reference = QLineEdit(lookup["source_url"])
                reference.setReadOnly(True)
                form.addRow("Reference", reference)
        for label, value in [("ROM filename", Path(game["rom_path"]).name),
                             ("ROM path", str(library.root / game["rom_path"])), ("SHA-256", game["id"]),
                             ("Imported", date_text(game["added"])), ("Last played", date_text(game["last_played"])),
                             ("Play count", str(game["play_count"]))]:
            field = QLineEdit(value)
            field.setReadOnly(True)
            field.setCursorPosition(0)
            form.addRow(label, field)
        for label, key in [("MD5", "md5"), ("SHA-1", "sha1"), ("CRC32", "crc32")]:
            value = library.hashes(game_id).get(key)
            if value:
                field = QLineEdit(value)
                field.setReadOnly(True)
                form.addRow(label, field)
        body_layout.addLayout(form)
        automatic = QPushButton("Use downloaded information…")
        automatic.setEnabled(bool(lookup and json.loads(lookup["payload"] or "{}")))
        automatic.clicked.connect(self.use_downloaded)
        layout.addWidget(automatic)
        buttons = QDialogButtonBox(QDialogButtonBox.Save | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.save)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def save(self):
        if not self.title.text().strip():
            self.title.setFocus()
            return
        # Only publish edited controls. An automatic result arriving while this
        # dialog is open must not be replaced with stale, untouched text.
        if self.title.text() != self.original_title:
            self.library.rename(self.game_id, self.title.text())
        if self.rating.currentIndex() != self.original_rating:
            self.library.rate([self.game_id], self.rating.currentIndex())
        changes = {key: field.text() for key, field in self.fields.items()
                   if field.text() != self.original_metadata.get(key, "")}
        if self.description.toPlainText() != self.original_metadata.get("description", ""):
            changes["description"] = self.description.toPlainText()
        if changes:
            self.library.update_metadata(self.game_id, changes)
        self.accept()

    def use_downloaded(self):
        if QMessageBox.question(self, "Use downloaded information?",
                "Replace your edits to fields that have downloaded information? Your notes and fields without downloaded values will be kept.",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No) == QMessageBox.Yes:
            self.library.use_automatic_metadata(self.game_id)
            self.accept()
