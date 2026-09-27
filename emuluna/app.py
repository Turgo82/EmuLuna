"""EmuLuna game-library window."""
import argparse
from collections import OrderedDict
import json
import os
from pathlib import Path
import sys

from PySide6.QtCore import (Qt, QSize, QProcess, QProcessEnvironment, QThread, Signal,
                           QTimer, QRect, QUrl, QItemSelectionModel, QEvent)
from PySide6.QtGui import QAction, QColor, QDesktopServices, QFont, QFontMetrics, QIcon, QPainter, QPixmap, QLinearGradient
from PySide6.QtWidgets import (QApplication, QMainWindow, QWidget, QHBoxLayout, QVBoxLayout,
    QLabel, QPushButton, QListWidget, QListWidgetItem, QLineEdit, QFileDialog, QMessageBox, QGridLayout, QToolButton,
    QInputDialog, QMenu, QAbstractItemView, QStackedWidget, QSlider, QButtonGroup, QHeaderView, QProgressBar)

from . import __version__
from .artwork import ArtworkWorker
from .branding import ICON, LOGO, configure_application, navigation_icon
from .core import ROOT, CoreError
from .core_manager import CoreManager, DefaultCoreWorker
from .library import Library, SYSTEMS, EXTENSIONS
from .importing import Importer, ImportIssuesDialog
from .metadata import MetadataWorker
from .media_library import MediaBrowser
from .thumbnails import ThumbnailCache
from .notifications import NotificationBell
from .library_widgets import (GameGrid, GameTable, CoverDelegate, CoverSizeSlider, LibrarySidebar, SortItem, SmartCollectionDialog,
                              GameInfoDialog, date_text, AlphabetIndex, title_initial, LibraryScrollBar, ExpandableSearch,
                              SIDEBAR_COUNT_ROLE)

from .theme import LIBRARY_STYLE as STYLE, follow_system_theme, theme_palette


def cover_pixmap(game, library, dimensions=None):
    if game["cover"]:
        image = QPixmap(str(library.root / game["cover"]))
        if not image.isNull():
            return image.scaled(256, 256, Qt.KeepAspectRatio, Qt.SmoothTransformation)
    dimensions = dimensions or QSize(*SYSTEMS[game["system"]].cover_size)
    dimensions = dimensions.scaled(QSize(256, 256), Qt.KeepAspectRatio)
    pixmap = QPixmap(dimensions)
    pixmap.fill(QColor("#353640"))
    painter = QPainter(pixmap)
    # Lay out the placeholder within its real box shape instead of squeezing a
    # square graphic into a wide or tall card.
    painter.scale(256 / 176, 256 / 176)
    width, height = round(dimensions.width() * 176 / 256), round(dimensions.height() * 176 / 256)
    painter.setRenderHint(QPainter.Antialiasing)
    accent = QColor(SYSTEMS[game["system"]].color)
    gradient = QLinearGradient(0, 0, width, height)
    gradient.setColorAt(0, accent.darker(145))
    gradient.setColorAt(1, accent.darker(360))
    painter.fillRect(QRect(0, 0, width, height), gradient)
    painter.setPen(QColor(255, 255, 255, 28))
    for x in range(-height, width + height, 20):
        painter.drawLine(x, height, x + height, 0)
    painter.setPen(accent.lighter(145))
    painter.setFont(QFont("Noto Sans", 9, QFont.Bold))
    painter.drawText(QRect(12, 10, width - 24, 22), Qt.AlignLeft, game["system"].upper())
    font = QFont("Noto Sans", 17, QFont.Bold)
    text_rect = QRect(12, 38, width - 24, max(18, height - 66))
    title = game["title"][:55]
    while font.pointSize() > 9 and QFontMetrics(font).boundingRect(text_rect, Qt.TextWordWrap, title).height() > text_rect.height():
        font.setPointSize(font.pointSize() - 1)
    painter.setFont(font)
    painter.setPen(QColor("#fff"))
    painter.setClipRect(text_rect)
    painter.drawText(text_rect, Qt.AlignLeft | Qt.AlignVCenter | Qt.TextWordWrap, title)
    painter.setClipping(False)
    painter.setPen(accent)
    painter.drawLine(12, height - 13, min(51, width - 12), height - 13)
    painter.end()
    return pixmap


class Window(QMainWindow):
    def __init__(self, library, *, auto_artwork=True, auto_metadata=None):
        super().__init__()
        self.library = library
        self.worker = None
        self.issue_dialog = None
        self.art_worker = None
        self.art_pending = []
        self.metadata_worker = None
        self.metadata_pending = []
        self.metadata_enabled = auto_artwork if auto_metadata is None else auto_metadata
        self.core_worker = None
        self.core_pending = set()
        self.auto_cores_enabled = auto_artwork
        self.closing = False
        self.processes = {}
        self.game_restore_state = None
        self.library_tab = "library"
        self.library_selection = []
        self.rows = {}
        self.game_items = {}
        self.placeholder_cache = OrderedDict()
        self.thumbnails = ThumbnailCache(library.root, self)
        self.thumbnails.ready.connect(self.thumbnail_ready)
        self.table_dirty = True
        self.view_mode = self.library.setting("library.view", "grid")
        if self.view_mode not in ("grid", "list"):
            self.view_mode = "grid"
        self.setWindowTitle("EmuLuna")
        self.resize(1140, 750)
        self.setMinimumSize(850, 560)
        self.setAcceptDrops(True)
        follow_system_theme(library)
        self.setPalette(theme_palette())
        self.setStyleSheet(STYLE)
        outer = QWidget()
        self.setCentralWidget(outer)
        layout = QHBoxLayout(outer)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)
        sidebar = QWidget()
        sidebar.setObjectName("sidebar")
        sidebar.setFixedWidth(260)
        side = QVBoxLayout(sidebar)
        side.setContentsMargins(0, 18, 0, 12)
        side.setSpacing(10)
        sidebar_header = QHBoxLayout()
        sidebar_header.setContentsMargins(20, 0, 20, 0)
        self.menu_button = QToolButton()
        self.menu_button.setObjectName("applicationMenuButton")
        self.menu_button.setIcon(navigation_icon("menu"))
        self.menu_button.setIconSize(QSize(20, 20))
        self.menu_button.setFixedSize(32, 32)
        self.menu_button.setToolTip("Application menu (F10)")
        self.menu_button.setAccessibleName("Application menu")
        self.menu_button.setPopupMode(QToolButton.InstantPopup)
        sidebar_header.addWidget(self.menu_button)
        sidebar_header.addStretch()
        side.addLayout(sidebar_header)
        self.nav = LibrarySidebar()
        self.nav.setObjectName("nav")
        self.nav.setIconSize(QSize(20, 20))
        self.rebuild_sidebar(self.library.setting("library.section", "all"))
        self.nav.currentRowChanged.connect(self.refresh)
        self.nav.setAcceptDrops(True)
        self.nav.games_dropped.connect(self.add_to_collection)
        self.nav.setContextMenuPolicy(Qt.CustomContextMenu)
        self.nav.customContextMenuRequested.connect(self.collection_menu)
        side.addWidget(self.nav)
        layout.addWidget(sidebar)
        content = QWidget()
        main = QVBoxLayout(content)
        main.setContentsMargins(0, 0, 0, 0)
        main.setSpacing(0)
        self.library_toolbar = QWidget()
        self.library_toolbar.setObjectName("libraryToolbar")
        self.library_toolbar.setStyleSheet("QWidget#libraryToolbar {background:palette(alternate-base);}")
        controls = QGridLayout(self.library_toolbar)
        controls.setContentsMargins(24, 18, 24, 12)
        controls.setHorizontalSpacing(8)
        controls.setColumnStretch(0, 1)
        controls.setColumnStretch(2, 1)
        self.view_controls = QWidget()
        left = QHBoxLayout(self.view_controls)
        left.setContentsMargins(0, 0, 0, 0)
        left.setSpacing(4)
        left.setSizeConstraint(QHBoxLayout.SetFixedSize)
        self.grid_button = QPushButton()
        self.list_button = QPushButton()
        group = QButtonGroup(self)
        for button, mode in [(self.grid_button, "grid"), (self.list_button, "list")]:
            button.setObjectName("viewControl")
            button.setIcon(navigation_icon(mode))
            button.setIconSize(QSize(18, 18))
            button.setFixedSize(30, 30)
            button.setToolTip(mode.title() + " view")
            button.setAccessibleName(mode.title() + " view")
            button.setCheckable(True)
            button.setChecked(self.view_mode == mode)
            group.addButton(button)
            button.clicked.connect(lambda checked=False, mode=mode: self.change_view(mode))
            left.addWidget(button)
        self.cover_size = CoverSizeSlider()
        try:
            self.cover_size.setValue(int(self.library.setting("library.cover_size", "176")))
        except ValueError:
            self.cover_size.setValue(176)
        self.cover_size.setFixedWidth(76)
        self.cover_size.setAccessibleName("Cover size")
        left.addSpacing(16)
        left.addWidget(self.cover_size)
        controls.addWidget(self.view_controls, 0, 0, Qt.AlignLeft)
        self.library_tabs = QWidget()
        self.library_tabs.setObjectName("libraryNavigation")
        tabs = QHBoxLayout(self.library_tabs)
        tabs.setContentsMargins(3, 3, 3, 3)
        tabs.setSpacing(3)
        tab_group = QButtonGroup(self)
        self.section_buttons = {}
        for key, title in [("library", "Library"), ("states", "Save States"), ("screenshots", "Screenshots")]:
            if self.section_buttons:
                divider = QWidget()
                divider.setObjectName("navigationDivider")
                divider.setFixedSize(1, 16)
                tabs.addWidget(divider, 0, Qt.AlignVCenter)
            button = QPushButton(title)
            button.setObjectName("sectionNavigation")
            button.setIcon(navigation_icon(key))
            button.setIconSize(QSize(18, 18))
            button.setAccessibleName(title)
            button.setToolTip(title)
            button.setFixedHeight(28)
            button.setCheckable(True)
            button.setChecked(key == self.library_tab)
            button.clicked.connect(lambda checked=False, key=key: self.change_library_tab(key))
            tab_group.addButton(button)
            tabs.addWidget(button)
            self.section_buttons[key] = button
        controls.addWidget(self.library_tabs, 0, 1, Qt.AlignCenter)
        self.search_control = ExpandableSearch()
        self.search = self.search_control.field
        self.search_action = QAction('Search', self)
        self.search_action.setShortcut('Ctrl+F')
        self.search_action.triggered.connect(self.search_control.expand)
        self.addAction(self.search_action)
        self.search_timer = QTimer(self)
        self.search_timer.setSingleShot(True)
        self.search_timer.setInterval(120)
        self.search_timer.timeout.connect(self.refresh)
        self.search.textEdited.connect(lambda: self.search_timer.start())
        self.search.textChanged.connect(self.search_changed)
        search_area = QWidget()
        right = QHBoxLayout(search_area)
        right.setContentsMargins(0, 0, 0, 0)
        right.setSpacing(6)
        right.addStretch()
        right.addWidget(self.search_control)
        self.notifications = NotificationBell(self)
        right.addWidget(self.notifications)
        controls.setColumnMinimumWidth(0, self.view_controls.minimumSizeHint().width())
        controls.setColumnMinimumWidth(2, self.view_controls.minimumSizeHint().width())
        controls.addWidget(search_area, 0, 2)
        main.addWidget(self.library_toolbar)
        self.pages = QStackedWidget()
        self.games = GameGrid()
        self.games.setObjectName("games")
        self.prefetch_timer = QTimer(self)
        self.prefetch_timer.setSingleShot(True)
        self.prefetch_timer.setInterval(16)
        self.prefetch_timer.timeout.connect(self.prefetch_covers)
        self.games.viewport_changed.connect(lambda: self.prefetch_timer.start())
        self.games.action_info = lambda key: (self.automatic_state(key) is not None, key in self.processes)
        self.games.play_requested.connect(self.play_game)
        self.games.restart_requested.connect(self.restart_game)
        self.games.setViewMode(QListWidget.IconMode)
        self.games.setResizeMode(QListWidget.Adjust)
        self.games.setMovement(QListWidget.Static)
        self.games.setIconSize(QSize(176, 176))
        self.games.setGridSize(QSize())
        self.games.setWordWrap(True)
        self.games.setItemDelegate(CoverDelegate(self.games, self.cover_icon))
        self.games.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.games.setVerticalScrollMode(QAbstractItemView.ScrollPerPixel)
        self.games.setDragDropMode(QAbstractItemView.DragDrop)
        self.games.files_dropped.connect(self.files_dropped)
        self.games.itemDoubleClicked.connect(lambda _: self.play())
        self.games.setContextMenuPolicy(Qt.CustomContextMenu)
        self.games.customContextMenuRequested.connect(self.context_menu)
        self.pages.addWidget(self.games)
        empty = QWidget()
        empty_layout = QVBoxLayout(empty)
        empty_layout.setAlignment(Qt.AlignCenter)
        self.empty_title = QLabel("Your games, together.")
        self.empty_title.setAlignment(Qt.AlignCenter)
        self.empty_title.setStyleSheet("font-size:27px;font-weight:600;")
        empty_layout.addWidget(self.empty_title)
        self.empty_text = QLabel()
        self.empty_text.setAlignment(Qt.AlignCenter)
        self.empty_text.setWordWrap(True)
        self.empty_text.setObjectName("subtle")
        empty_layout.addWidget(self.empty_text)
        self.pages.addWidget(empty)
        self.table = GameTable(0, 8)
        self.table.setHorizontalHeaderLabels(["Game title", "System", "Rating", "Last played", "Date added", "Play count", "Core", "File status"])
        self.table.verticalHeader().hide()
        self.table.setAlternatingRowColors(True)
        self.table.setShowGrid(False)
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.table.setVerticalScrollMode(QAbstractItemView.ScrollPerPixel)
        self.table.setDragDropMode(QAbstractItemView.DragDrop)
        self.table.files_dropped.connect(self.files_dropped)
        self.table.setSortingEnabled(True)
        self.table.sortItems(0, Qt.AscendingOrder)
        self.table.horizontalHeader().setSectionResizeMode(QHeaderView.Interactive)
        self.table.horizontalHeader().setStretchLastSection(True)
        try:
            widths = json.loads(self.library.setting("library.columns", "[230,155,110,145,145,95,160,100]"))
            for i, width in enumerate(widths[:8]):
                self.table.setColumnWidth(i, max(60, min(1000, int(width))))
            sort = json.loads(self.library.setting("library.sort", "[0,0]"))
            if len(sort) == 2 and 0 <= sort[0] < 8 and sort[1] in (0, 1):
                self.table.sortItems(sort[0], Qt.SortOrder(sort[1]))
        except (ValueError, TypeError):
            pass
        self.table.itemDoubleClicked.connect(lambda _: self.play())
        self.table.setContextMenuPolicy(Qt.CustomContextMenu)
        self.table.customContextMenuRequested.connect(lambda position: self.context_menu(position, self.table))
        self.pages.addWidget(self.table)
        self.media_browser = MediaBrowser(self.library, self.thumbnails)
        self.media_browser.media_removed.connect(self.refresh)
        self.media_browser.message.connect(lambda text: self.notifications.post(text))
        self.media_browser.activated.connect(self.activate_media)
        self.pages.addWidget(self.media_browser)
        self.cover_size.valueChanged.connect(self.resize_covers)
        self.resize_covers(self.cover_size.value())
        self.alphabet_index = AlphabetIndex()
        self.alphabet_index.jump_requested.connect(self.jump_to_letter)
        self.letter_targets = {}
        self.library_scrollbar = LibraryScrollBar()
        self.games.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.table.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        library_body = QHBoxLayout()
        library_body.setContentsMargins(24, 12, 0, 0)
        library_body.setSpacing(6)
        library_body.addWidget(self.pages, 1)
        library_body.addWidget(self.alphabet_index)
        library_body.addWidget(self.library_scrollbar)
        main.addLayout(library_body, 1)
        layout.addWidget(content, 1)
        self.setup_menu()
        self.import_progress = QProgressBar()
        self.import_progress.setMaximumWidth(170)
        self.import_progress.hide()
        self.notifications.activity.addWidget(self.import_progress)
        self.cancel_import = QPushButton("Cancel import")
        self.cancel_import.clicked.connect(lambda: self.worker.requestInterruption() if self.worker else None)
        self.cancel_import.hide()
        self.notifications.activity.addWidget(self.cancel_import)
        self.issues_button = QPushButton()
        self.issues_button.clicked.connect(self.show_import_issues)
        self.notifications.activity.addWidget(self.issues_button)
        self.core_status = QLabel()
        self.core_status.setWordWrap(True)
        self.core_status.hide()
        self.notifications.activity.addWidget(self.core_status)
        self.cancel_cores = QPushButton("Cancel core download")
        self.cancel_cores.clicked.connect(lambda: self.core_worker.requestInterruption() if self.core_worker else None)
        self.cancel_cores.hide()
        self.notifications.activity.addWidget(self.cancel_cores)
        for action in (self.cancel_art_action, self.cancel_metadata_action):
            button = QPushButton(action.text())
            button.clicked.connect(action.trigger)
            action.changed.connect(lambda action=action, button=button: button.setVisible(action.isEnabled()))
            button.setVisible(action.isEnabled())
            self.notifications.activity.addWidget(button)
        QTimer.singleShot(0, self.queue_default_cores)
        self.update_issues()
        for widget in (self.games, self.table):
            launch = QAction("Play", widget)
            launch.setShortcut("Return")
            launch.setShortcutContext(Qt.WidgetWithChildrenShortcut)
            launch.triggered.connect(self.play)
            widget.addAction(launch)
        self.refresh()
        self.art_timer = QTimer(self)
        self.art_timer.setInterval(5 * 60 * 1000)
        self.art_timer.timeout.connect(self.start_background)
        if auto_artwork or self.metadata_enabled:
            self.art_timer.start()
            QTimer.singleShot(250, self.start_background)
        if self.library.needs_filename_restore():
            QTimer.singleShot(0, lambda: self.import_paths([], restore_names=True))

    def setup_menu(self):
        self.application_menu = QMenu(self)
        self.menu_button.setMenu(self.application_menu)
        self.menuBar().hide()
        open_menu = QAction("Application menu", self)
        open_menu.setShortcut("F10")
        open_menu.triggered.connect(self.menu_button.showMenu)
        self.addAction(open_menu)
        file = self.application_menu.addMenu("Library")
        actions = [("Import games…", "Ctrl+O", self.choose_files),
                   ("Import folder…", "Ctrl+Shift+O", self.choose_folder),
                   ("Open library folder", "", lambda: QDesktopServices.openUrl(QUrl.fromLocalFile(str(self.library.root)))),
                   ("Quit", "Ctrl+Q", self.close)]
        for title, key, fn in actions:
            a = QAction(title, self)
            if key:
                a.setShortcut(key)
                # Popup menus are normally hidden; register shortcuts on the
                # library window as well so they remain active while closed.
                self.addAction(a)
            a.triggered.connect(fn)
            file.addAction(a)
            if key == "Ctrl+O":
                self.import_action = a
        file.addSeparator()
        file.addAction("New collection…", self.new_collection)
        file.addAction("New smart collection…", self.new_smart_collection)
        file.addAction("Import issues…", self.show_import_issues)
        artwork = self.application_menu.addMenu("Artwork")
        self.download_art_action = artwork.addAction("Download missing box art")
        self.download_art_action.triggered.connect(lambda: self.start_artwork(force=True))
        self.cancel_art_action = artwork.addAction("Cancel cover downloads")
        self.cancel_art_action.setEnabled(False)
        self.cancel_art_action.triggered.connect(self.cancel_artwork)
        artwork.addSeparator()
        self.auto_art_action = artwork.addAction("Automatically download box art")
        self.auto_art_action.setCheckable(True)
        self.auto_art_action.setChecked(self.library.setting("artwork_auto", "1") == "1")
        self.auto_art_action.toggled.connect(self.toggle_auto_artwork)
        self.backup_art_action = artwork.addAction("Use backup source when needed")
        self.backup_art_action.setCheckable(True)
        self.backup_art_action.setChecked(self.library.setting("artwork_backup", "1") == "1")
        self.backup_art_action.toggled.connect(lambda enabled: self.library.set_setting("artwork_backup", int(enabled)))
        information = self.application_menu.addMenu("Game information")
        self.lookup_action = information.addAction("Look up game information")
        self.lookup_action.triggered.connect(lambda: self.start_metadata(force=True))
        self.cancel_metadata_action = information.addAction("Cancel information lookup")
        self.cancel_metadata_action.setEnabled(False)
        self.cancel_metadata_action.triggered.connect(self.cancel_metadata)
        self.auto_metadata_action = information.addAction("Automatically look up game information")
        self.auto_metadata_action.setCheckable(True)
        self.auto_metadata_action.setChecked(self.library.setting("metadata_auto", "1") == "1")
        self.auto_metadata_action.toggled.connect(self.toggle_auto_metadata)
        self.application_menu.addSeparator()
        self.settings_action = QAction("Settings…", self)
        self.settings_action.setShortcut("Ctrl+,")
        self.settings_action.triggered.connect(self.open_settings)
        self.addAction(self.settings_action)
        self.application_menu.addAction(self.settings_action)
        help_menu = self.application_menu.addMenu("Help")
        a = help_menu.addAction("About EmuLuna")
        a.triggered.connect(self.show_about)

    def changeEvent(self, event):
        super().changeEvent(event)
        if event.type() in (QEvent.PaletteChange, QEvent.ApplicationPaletteChange) and hasattr(self, "notifications"):
            for key, button in self.section_buttons.items():
                button.setIcon(navigation_icon(key))
            self.menu_button.setIcon(navigation_icon("menu"))
            self.grid_button.setIcon(navigation_icon("grid"))
            self.list_button.setIcon(navigation_icon("list"))
            self.notifications.update_badge()
            self.refresh_sidebar_icons()
            self.games.viewport().update()
            if hasattr(self, "media_browser"):
                self.media_browser.grid.viewport().update()

    def resizeEvent(self, event):
        super().resizeEvent(event)
        if hasattr(self, "section_buttons"):
            # Keep navigation centered without crowding search on small windows.
            compact = self.width() < 1000
            for button in self.section_buttons.values():
                button.setText("" if compact else button.accessibleName())

    def show_about(self):
        self.about_dialog = QMessageBox(self)
        self.about_dialog.setWindowTitle("About EmuLuna")
        self.about_dialog.setIconPixmap(QPixmap(str(LOGO)).scaled(220, 220, Qt.KeepAspectRatio, Qt.SmoothTransformation))
        self.about_dialog.setText(f"EmuLuna {__version__}")
        self.about_dialog.setInformativeText("An independent game library and emulator frontend.\nRuns standard libretro cores directly.\n\n32 systems and 27 downloadable cores.\nArtwork: OpenVGDB and Libretro thumbnails.\n\nSee README.md and THIRD_PARTY_NOTICES.md for capabilities and credits.")
        self.about_dialog.setStandardButtons(QMessageBox.Ok)
        self.about_dialog.setAttribute(Qt.WA_DeleteOnClose)
        self.about_dialog.open()

    def queue_default_cores(self, systems=None):
        if self.closing or not self.auto_cores_enabled:
            return
        # Import completion already knows which systems changed. Avoid scanning
        # and reconsidering a large library after every small import.
        if systems is None:
            systems = (row['system'] for row in self.library.games())
        self.core_pending.update(systems)
        self.start_default_cores()

    def start_default_cores(self):
        if self.core_worker or self.closing or not self.core_pending:
            return
        systems = self.core_pending.copy()
        self.core_pending.clear()
        if not CoreManager(self.library.root).missing_defaults(self.library, sorted(systems)):
            return
        self.core_worker = DefaultCoreWorker(self.library.root, sorted(systems))
        self.core_worker.progress.connect(self.set_core_status)
        self.core_worker.result.connect(lambda message, success: self.notifications.finish("cores", message))
        self.core_worker.finished.connect(self.default_cores_finished)
        self.cancel_cores.show()
        self.set_core_status("Preparing a default core for your games…")
        self.core_worker.start()

    def set_core_status(self, message):
        self.core_status.setText(message)
        self.core_status.setToolTip(message)
        self.core_status.show()
        self.notifications.post(message, key="cores")

    def default_cores_finished(self):
        self.core_worker.deleteLater()
        self.core_worker = None
        self.cancel_cores.hide()
        self.core_status.hide()
        if self.closing:
            self.close()
            return
        # Installing a core changes launch availability and the optional Core
        # column, but it does not change the game grid. Rebuilding hundreds of
        # cover items here made the library appear frozen at download finish.
        self.table_dirty = True
        if self.library_tab == "library" and self.view_mode == "list":
            self.populate_table(list(self.rows.values()))
        self.start_default_cores()
        if not self.core_worker:
            self.drain_lookups()

    def open_settings(self, checked=False, *, system=None):
        from .settings import SettingsDialog
        dialog = SettingsDialog(self.library, self)
        if system in SYSTEMS:
            dialog.show_page('controls')
            page = dialog.controls_page
            page.system.setCurrentIndex(page.system.findData(system))
        def sync():
            self.auto_art_action.setChecked(self.library.setting("artwork_auto", "1") == "1")
            self.backup_art_action.setChecked(self.library.setting("artwork_backup", "1") == "1")
            self.auto_metadata_action.setChecked(self.library.setting("metadata_auto", "1") == "1")
            self.refresh()
        dialog.changed.connect(sync)
        try:
            dialog.exec()
        finally:
            # A QObject parent otherwise retains every closed Settings window
            # and its widgets, timers and signal connections for the app's life.
            dialog.deleteLater()
        self.refresh()


    def toggle_auto_artwork(self, enabled):
        self.library.set_setting("artwork_auto", int(enabled))
        if enabled:
            self.start_artwork()
        else:
            self.cancel_artwork()

    def cancel_artwork(self):
        self.art_pending.clear()
        if self.art_worker:
            self.art_worker.requestInterruption()
        self.cancel_art_action.setEnabled(False)

    def cancel_metadata(self):
        self.metadata_pending.clear()
        if self.metadata_worker:
            self.metadata_worker.requestInterruption()
        self.cancel_metadata_action.setEnabled(False)

    def toggle_auto_metadata(self, enabled):
        self.library.set_setting("metadata_auto", int(enabled))
        if enabled:
            self.start_metadata()
        else:
            self.cancel_metadata()

    def start_background(self):
        self.start_metadata()
        self.start_artwork()

    def start_metadata(self, *, force=False, game_ids=None):
        if self.closing or (not force and (not self.metadata_enabled or self.library.setting("metadata_auto", "1") != "1")):
            return
        # Core installation gets first use of network, disk and Python work.
        # Running all three jobs together can starve Qt's GUI thread on a large
        # library even though each job has its own QThread.
        if self.metadata_worker or self.art_worker or self.core_worker:
            if force or not self.metadata_pending:
                self.metadata_pending.append({"force": force, "game_ids": game_ids})
            self.cancel_metadata_action.setEnabled(True)
            return
        if not self.library.metadata_candidates(force, game_ids):
            return
        self.metadata_worker = MetadataWorker(self.library.root, force=force, game_ids=game_ids)
        self.metadata_worker.progress.connect(lambda message: self.notifications.post(message, key="metadata"))
        self.metadata_worker.result.connect(self.metadata_done)
        self.metadata_worker.finished.connect(self.metadata_finished)
        self.lookup_action.setEnabled(False)
        self.cancel_metadata_action.setEnabled(True)
        self.metadata_worker.start()

    def metadata_done(self, summary):
        if self.closing:
            return
        self.refresh()
        if summary["cancelled"]:
            message = "Game information lookup cancelled."
        elif summary["error"]:
            message = "Game information is temporarily unavailable. Retry from the Game information menu."
        else:
            message = f"Identified {summary['matched']} game(s) · {summary['not_found']} without a match"
            if summary["failed"]:
                message += f" · {summary['failed']} unavailable; retry from Game information"
        self.notifications.finish("metadata", message)

    def metadata_finished(self):
        self.metadata_worker.deleteLater()
        self.metadata_worker = None
        self.lookup_action.setEnabled(True)
        self.cancel_metadata_action.setEnabled(bool(self.metadata_pending))
        if self.closing:
            self.close()
        else:
            self.drain_lookups()

    def drain_lookups(self):
        # One catalog consumer at a time avoids duplicate initial downloads.
        # Keep explicit per-game requests separate so replacement never spreads
        # to unrelated covers when an automatic lookup is also queued.
        # A default-core job also owns this lane. Without this check, finishing
        # metadata while a core was downloading repeatedly popped and requeued
        # the same lookup on the GUI thread, creating an infinite loop.
        while not self.art_worker and not self.metadata_worker and not self.core_worker:
            if self.metadata_pending:
                self.start_metadata(**self.metadata_pending.pop(0))
            elif self.art_pending:
                self.start_artwork(**self.art_pending.pop(0))
            else:
                break
        self.cancel_art_action.setEnabled(bool(self.art_worker or self.art_pending))
        self.cancel_metadata_action.setEnabled(bool(self.metadata_worker or self.metadata_pending))

    def start_artwork(self, *, force=False, game_ids=None, replace=False):
        if self.closing or (not force and self.library.setting("artwork_auto", "1") != "1"):
            return
        if self.art_worker or self.metadata_worker or self.core_worker:
            if force or not self.art_pending:
                self.art_pending.append({"force": force, "game_ids": game_ids, "replace": replace})
            self.cancel_art_action.setEnabled(True)
            return
        if not self.library.artwork_candidates(force, game_ids, replace=replace):
            if force:
                self.notifications.post("There are no missing covers to download.", 5000)
            return
        self.art_worker = ArtworkWorker(self.library.root, force=force, game_ids=game_ids, replace=replace)
        self.art_worker.progress.connect(lambda message: self.notifications.post(message, key="artwork"))
        self.art_worker.changed.connect(self.cover_downloaded)
        self.art_worker.result.connect(self.artwork_done)
        self.art_worker.finished.connect(self.artwork_finished)
        self.download_art_action.setEnabled(False)
        self.cancel_art_action.setEnabled(True)
        self.art_worker.start()

    def cover_downloaded(self, game_id):
        row = self.library.get(game_id)
        if not row:
            return
        if game_id in self.rows:
            self.rows[game_id] = row
        self.games.cover_dimensions[game_id] = self.thumbnails.dimensions(row)
        self.layout_cards(self.cover_size.value())
        self.thumbnail_ready(game_id)

    def prefetch_covers(self):
        if self.closing or self.library_tab != "library" or self.view_mode != "grid" or not self.games.isVisible():
            return
        # Two extra rows above and below; prioritize anything already on screen.
        # Inspecting item rectangles is cheap, and avoids decoding the entire library.
        viewport = self.games.viewport().rect()
        margin = (self.cover_size.value() + 66) * 2
        nearby = viewport.adjusted(0, -margin, 0, margin)
        candidates = []
        for key, item in self.game_items.items():
            rect = self.games.visualItemRect(item)
            if rect.intersects(nearby):
                distance = max(0, viewport.top() - rect.bottom(), rect.top() - viewport.bottom())
                candidates.append((distance, key))
        for _, key in sorted(candidates)[:160]:
            self.thumbnails.icon(self.rows[key])

    def thumbnail_ready(self, game_id):
        if not self.closing:
            # Repaint only the viewport; its delegate requests visible covers.
            self.games.viewport().update()
            self.media_browser.grid.viewport().update()
            if not self.prefetch_timer.isActive():
                self.prefetch_timer.start()

    def cover_icon(self, game_id):
        row = self.rows.get(game_id)
        if row is None:
            return QIcon()
        icon = self.thumbnails.icon(row)
        if icon and not icon.isNull():
            return icon
        dimensions = self.thumbnails.dimensions(row)
        key = (game_id, row['title'], row['system'], dimensions.width(), dimensions.height())
        if key not in self.placeholder_cache:
            placeholder = dict(row)
            placeholder['cover'] = None
            self.placeholder_cache[key] = QIcon(cover_pixmap(placeholder, self.library, dimensions))
            while len(self.placeholder_cache) > 64:
                self.placeholder_cache.popitem(last=False)
        self.placeholder_cache.move_to_end(key)
        return self.placeholder_cache[key]

    def search_changed(self, text):
        if not text or not self.search_timer.isActive():
            self.refresh()

    def artwork_done(self, summary):
        if self.closing:
            return
        self.refresh()
        if summary["error"]:
            message = "Box art is temporarily unavailable. Use Artwork → Download missing box art to retry."
        elif summary["cancelled"]:
            message = "Box art downloads paused."
        else:
            parts = [f"{summary['downloaded']} cover{'s' if summary['downloaded'] != 1 else ''} downloaded"]
            if summary["not_found"]:
                parts.append(f"{summary['not_found']} without a matching cover")
            if summary["failed"]:
                parts.append(f"{summary['failed']} temporarily unavailable; retry from Artwork")
            message = " · ".join(parts)
        self.notifications.finish("artwork", message)

    def artwork_finished(self):
        self.art_worker.deleteLater()
        self.art_worker = None
        self.download_art_action.setEnabled(True)
        self.cancel_art_action.setEnabled(bool(self.art_pending))
        if self.closing:
            self.close()
        else:
            self.drain_lookups()

    def rebuild_sidebar(self, selected=None):
        selected = selected or (self.nav.currentItem().data(Qt.UserRole) if self.nav.currentItem() else "all")
        self.nav.blockSignals(True)
        self.nav.clear()
        def heading(title):
            item = QListWidgetItem(title)
            item.setFlags(Qt.NoItemFlags)
            font = item.font()
            font.setBold(True)
            item.setFont(font)
            item.setSizeHint(QSize(0, 32))
            self.nav.addItem(item)

        heading("Consoles")
        populated = self.populated_systems()
        hide_empty = self.library.setting("library.hide_empty_consoles", "0") == "1"
        self.sidebar_systems = populated if hide_empty else frozenset(SYSTEMS)
        for key, system in SYSTEMS.items():
            if key not in self.sidebar_systems:
                continue
            item = QListWidgetItem(QIcon(str(system.icon)), system.name)
            item.setData(Qt.UserRole, key)
            self.nav.addItem(item)
        divider_item = QListWidgetItem()
        divider_item.setFlags(Qt.NoItemFlags)
        divider_item.setData(Qt.UserRole + 2, "section-divider")
        divider_item.setSizeHint(QSize(0, 13))
        self.nav.addItem(divider_item)
        heading("Collections")
        counts = self.library.sidebar_counts()
        for title, key, icon in [("All Games", "all", "collection-all"),
                                 ("Recently Played", "recent", "collection-recent"),
                                 ("Favorites", "favorites", "collection-favorite"),
                                 ("Recently Added", "added", "collection-added")]:
            item = QListWidgetItem(navigation_icon(icon), title)
            item.setData(Qt.UserRole, key)
            item.setData(Qt.UserRole + 1, "builtin")
            item.setData(SIDEBAR_COUNT_ROLE, counts[key])
            item.setToolTip("Built-in collection")
            self.nav.addItem(item)
        for record in self.library.collections():
            icon = "smart-collection" if record["rules"] is not None else "collection"
            item = QListWidgetItem(navigation_icon(icon), record["name"])
            item.setData(Qt.UserRole, f"collection:{record['id']}")
            item.setData(Qt.UserRole + 1, "smart" if record["rules"] is not None else "regular")
            item.setData(SIDEBAR_COUNT_ROLE, counts[f"collection:{record['id']}"])
            item.setToolTip("Matches rules automatically" if record["rules"] is not None else "Drop games here to add them")
            self.nav.addItem(item)
        rows = {self.nav.item(i).data(Qt.UserRole): i for i in range(self.nav.count())}
        self.nav.setCurrentRow(rows.get(selected, rows["all"]))
        self.nav.blockSignals(False)

    def refresh_sidebar_icons(self):
        icons = {"all": "collection-all", "recent": "collection-recent",
                 "favorites": "collection-favorite", "added": "collection-added"}
        for row in range(self.nav.count()):
            item = self.nav.item(row)
            key = item.data(Qt.UserRole)
            kind = item.data(Qt.UserRole + 1)
            if key in icons:
                item.setIcon(navigation_icon(icons[key]))
            elif isinstance(key, str) and key.startswith("collection:"):
                item.setIcon(navigation_icon("smart-collection" if kind == "smart" else "collection"))

    def refresh_sidebar_counts(self):
        counts = self.library.sidebar_counts()
        for row in range(self.nav.count()):
            item = self.nav.item(row)
            key = item.data(Qt.UserRole)
            if key in counts:
                item.setData(SIDEBAR_COUNT_ROLE, counts[key])

    def populated_systems(self):
        return frozenset(row[0] for row in self.library.db.execute("SELECT DISTINCT system FROM games"))

    def current_collection(self):
        key = self.nav.currentItem().data(Qt.UserRole) if self.nav.currentItem() else "all"
        return int(key.split(":")[1]) if key and key.startswith("collection:") else None

    def new_collection(self, checked=False, game_ids=None):
        name, ok = QInputDialog.getText(self, "New collection", "Collection name")
        if ok and name.strip():
            collection_id = self.library.save_collection(name)
            if game_ids:
                self.library.add_to_collection(collection_id, game_ids)
            self.rebuild_sidebar(f"collection:{collection_id}")
            self.refresh()

    def new_smart_collection(self, checked=False, record=None):
        dialog = SmartCollectionDialog(self, record)
        if dialog.exec():
            collection_id = self.library.save_collection(dialog.name.text(), dialog.rules(), record["id"] if record else None)
            self.rebuild_sidebar(f"collection:{collection_id}")
            self.refresh()

    def add_to_collection(self, collection_id, game_ids):
        self.library.add_to_collection(collection_id, game_ids)
        self.refresh()
        self.notifications.post(f"Added {len(game_ids)} game(s) to {self.library.collection(collection_id)['name']}.", 5000)

    def collection_menu(self, position):
        item = self.nav.itemAt(position)
        if item and item.data(Qt.UserRole) in SYSTEMS:
            system = item.data(Qt.UserRole)
            menu = QMenu(self)
            menu.addAction('Configure Controls…', lambda: self.open_settings(system=system))
            menu.exec(self.nav.viewport().mapToGlobal(position))
            return
        if not item or not (item.data(Qt.UserRole) or "").startswith("collection:"):
            return
        record = self.library.collection(int(item.data(Qt.UserRole).split(":")[1]))
        menu = QMenu(self)
        rename = menu.addAction("Rename collection…")
        edit = menu.addAction("Edit rules…") if record["rules"] is not None else None
        delete = menu.addAction("Delete collection (keep games)")
        action = menu.exec(self.nav.mapToGlobal(position))
        if action is None:
            return
        if action == rename:
            name, ok = QInputDialog.getText(self, "Rename collection", "Name", text=record["name"])
            if ok and name.strip():
                rules = json.loads(record["rules"]) if record["rules"] is not None else None
                self.library.save_collection(name, rules, record["id"])
        elif action == edit:
            self.new_smart_collection(record=record)
        elif action == delete:
            self.library.delete_collection(record["id"])
        self.rebuild_sidebar()
        self.refresh()

    def active_view(self):
        return self.table if self.view_mode == "list" else self.games

    def selected_ids(self):
        return self.active_view().game_ids()

    def restore_selection(self, game_ids):
        ids = set(game_ids)
        self.games.blockSignals(True)
        self.table.blockSignals(True)
        self.games.clearSelection()
        self.table.clearSelection()
        first = True
        for i in range(self.games.count()):
            item = self.games.item(i)
            if item.data(Qt.UserRole) in ids:
                item.setSelected(True)
                if first:
                    self.games.setCurrentItem(item, QItemSelectionModel.NoUpdate)
                    first = False
        first = True
        for i in range(self.table.rowCount()):
            if self.table.item(i, 0).data(Qt.UserRole) in ids:
                self.table.selectionModel().select(self.table.model().index(i, 0),
                    QItemSelectionModel.Select | QItemSelectionModel.Rows)
                if first:
                    self.table.setCurrentCell(i, 0, QItemSelectionModel.NoUpdate)
                    first = False
        self.games.blockSignals(False)
        self.table.blockSignals(False)

    def change_library_tab(self, key):
        if self.library_tab == "library":
            self.library_selection = self.selected_ids()
        self.library_tab = key
        self.section_buttons[key].setChecked(True)
        self.refresh()
        if key == "library":
            self.restore_selection(self.library_selection)

    def activate_media(self):
        entry = self.media_browser.selected_entry()
        if not entry:
            return
        if entry["kind"] == "screenshots":
            QDesktopServices.openUrl(QUrl.fromLocalFile(str(entry["path"])))
        else:
            self.launch_game(entry["game_id"], entry["path"])

    def change_view(self, mode):
        ids = self.selected_ids()
        if mode == "list" and self.table_dirty and self.library_tab == "library":
            self.populate_table(list(self.rows.values()))
        self.view_mode = mode
        self.library.set_setting("library.view", mode)
        self.grid_button.setChecked(mode == "grid")
        self.list_button.setChecked(mode == "list")
        self.cover_size.setEnabled(mode == "grid")
        self.restore_selection(ids)
        if self.library_tab == "library":
            self.pages.setCurrentIndex((2 if mode == "list" else 0) if self.rows else 1)
        else:
            self.media_browser.change_view(mode)
        self.update_library_scrollbar()

    def update_library_scrollbar(self):
        self.library_scrollbar.set_view(self.active_view()
            if self.library_tab == "library" and self.rows else None)

    def jump_to_letter(self, letter):
        if self.library_tab != "library" or letter not in self.letter_targets:
            return
        if self.view_mode == "list":
            # Respect the user's current column sorting, including descending
            # titles or play history; do not reorder or filter the library.
            for row in range(self.table.rowCount()):
                item = self.table.item(row, 0)
                game = self.rows.get(item.data(Qt.UserRole))
                if game and title_initial(game["title"]) == letter:
                    self.table.setCurrentCell(row, 0, QItemSelectionModel.ClearAndSelect | QItemSelectionModel.Rows)
                    self.table.scrollToItem(item, QAbstractItemView.PositionAtTop)
                    self.table.setFocus(Qt.OtherFocusReason)
                    break
        else:
            item = self.game_items[self.letter_targets[letter]]
            self.games.setCurrentItem(item, QItemSelectionModel.ClearAndSelect)
            self.games.scrollToItem(item, QAbstractItemView.PositionAtTop)
            self.games.setFocus(Qt.OtherFocusReason)
            self.prefetch_timer.start()

    def resize_covers(self, size):
        self.games.setIconSize(QSize(size, size))
        self.games.hide_actions()
        self.layout_cards(size)
        self.library.set_setting("library.cover_size", size)
        self.media_browser.resize_items(size)

    def layout_cards(self, size):
        metrics = self.games.fontMetrics()
        for key, item in self.game_items.items():
            dimensions = self.games.cover_dimensions.get(key, QSize(256, 256)).scaled(QSize(size, size), Qt.KeepAspectRatio)
            # Pad the displayed artwork, not the square thumbnail bounding box:
            # portrait and landscape covers should have the same visible gap.
            card_width = dimensions.width() + 40
            text_height = min(34, metrics.boundingRect(QRect(0, 0, card_width - 14, 34), Qt.TextWordWrap, item.data(Qt.UserRole + 1)).height())
            item.setSizeHint(QSize(card_width, dimensions.height() + text_height + 16 + 32))
        self.games.doItemsLayout()
        self.prefetch_timer.start()

    def refresh(self, *_):
        self.search_timer.stop()
        wanted = self.populated_systems() if self.library.setting("library.hide_empty_consoles", "0") == "1" else frozenset(SYSTEMS)
        if wanted != self.sidebar_systems:
            self.rebuild_sidebar()
        else:
            self.refresh_sidebar_counts()
        previous = self.selected_ids() if self.library_tab == "library" else self.library_selection
        scroll_grid = self.games.verticalScrollBar().value()
        scroll_list = self.table.verticalScrollBar().value()
        key = self.nav.currentItem().data(Qt.UserRole)
        if not key:
            return
        self.library.set_setting("library.section", key)
        rows = self.library.games(system=key if key in SYSTEMS else None, search=self.search.text(),
                                  favorites=key == "favorites", recent=key == "recent", added=key == "added",
                                  never=key == "never", collection=self.current_collection())
        self.rows = {r["id"]: r for r in rows}
        self.letter_targets = {}
        for row in rows:
            self.letter_targets.setdefault(title_initial(row["title"]), row["id"])
        self.alphabet_index.set_letters(self.letter_targets)
        self.alphabet_index.setVisible(self.library_tab == "library" and bool(rows)
            and self.library.setting("library.show_alphabet_index", "1") == "1")
        self.games.blockSignals(True)
        self.games.clear()
        self.game_items.clear()
        self.games.cover_dimensions.clear()
        artwork = {row['game_id']: row for row in self.library.db.execute('SELECT * FROM artwork')}
        for index, row in enumerate(rows):
            name = ("★ " if row["favorite"] else "") + row["title"]
            if row["rating"]:
                name += "\n" + "★" * row["rating"] + "☆" * (5 - row["rating"])
            item = QListWidgetItem(name)
            item.setData(Qt.UserRole, row["id"])
            item.setData(Qt.UserRole + 1, ("★ " if row["favorite"] else "") + row["title"])
            item.setData(Qt.UserRole + 2, row["rating"])
            item.setToolTip(f"{row['title']}\n{SYSTEMS[row['system']].name}\nFile: {Path(row['rom_path']).name}\nDouble-click to play")
            info = artwork.get(row["id"])
            if info and info["status"] == "downloaded" and row["cover"]:
                source = "Libretro (backup)" if "libretro-thumbnails/" in (info["source_url"] or "") else "OpenVGDB"
                item.setToolTip(item.toolTip() + f"\nBox art: {source}")
            elif info and not row["cover"]:
                item.setToolTip(item.toolTip() + ("\nNo matching box art found" if info["status"] == "not_found"
                                                else "\nBox art unavailable; use Download missing cover to retry"))
            self.games.addItem(item)
            self.game_items[row["id"]] = item
            self.games.cover_dimensions[row["id"]] = self.thumbnails.dimensions(row)
        self.games.blockSignals(False)
        self.layout_cards(self.cover_size.value())
        self.table_dirty = True
        if self.view_mode == "list" and self.library_tab == "library":
            self.populate_table(rows)
        else:
            self.table.blockSignals(True)
            self.table.setRowCount(0)
            self.table.blockSignals(False)
        self.restore_selection(previous)
        self.games.verticalScrollBar().setValue(scroll_grid)
        self.table.verticalScrollBar().setValue(scroll_list)
        self.cover_size.setEnabled(self.view_mode == "grid")
        self.pages.setCurrentIndex((2 if self.view_mode == "list" else 0) if rows else 1)
        if self.search.text() or key != "all":
            self.empty_title.setText("No games here yet")
            self.empty_text.setText("Try another view, change your search, or import a game.")
        else:
            self.empty_title.setText("Your games, together.")
            self.empty_text.setText("Drop your ROMs or disc playlists here.\n\nOr choose ☰ → Library → Import games to start your library.\nZIP archives are supported.")
        if self.library_tab != "library":
            scope = self.library.games(system=key if key in SYSTEMS else None,
                favorites=key == "favorites", recent=key == "recent", added=key == "added",
                never=key == "never", collection=self.current_collection())
            self.media_browser.refresh(self.library_tab, scope, self.search.text(), set(self.rows))
            self.media_browser.change_view(self.view_mode)
            self.pages.setCurrentWidget(self.media_browser)
        self.update_library_scrollbar()

    def populate_table(self, rows):
        self.table.blockSignals(True)
        self.table.setSortingEnabled(False)
        self.table.setRowCount(len(rows))
        try:
            installed = CoreManager(self.library.root).installed()
        except (ValueError, OSError, CoreError):
            installed = {}
        choices = {key: self.library.setting("core." + key, "auto") for key in SYSTEMS}
        for row_index, row in enumerate(rows):
            core_id = choices[row["system"]]
            core_name = "Automatic" if core_id in ("builtin", "auto") else installed.get(core_id, {}).get("name", core_id)
            values = [(row["title"], None), (SYSTEMS[row["system"]].name, None),
                      ("★" * row["rating"] or "—", row["rating"]),
                      (date_text(row["last_played"]), row["last_played"] or 0),
                      (date_text(row["added"]), row["added"]), (str(row["play_count"]), row["play_count"]),
                      (core_name, None), ("Available" if (self.library.root / row["rom_path"]).is_file() else "Missing", None)]
            for column, (text, order) in enumerate(values):
                cell = SortItem(text, row["id"], order)
                cell.setToolTip(self.game_items[row["id"]].toolTip())
                self.table.setItem(row_index, column, cell)
        self.table.setSortingEnabled(True)
        self.table.blockSignals(False)
        self.table_dirty = False

    def choose_files(self):
        formats = " ".join("*" + ext for ext in (*EXTENSIONS, ".zip"))
        paths, _ = QFileDialog.getOpenFileNames(self, "Import games", "", f"Game ROMs ({formats});;All files (*)")
        if paths: self.import_paths(paths)

    def choose_folder(self):
        path = QFileDialog.getExistingDirectory(self, "Import a ROM folder")
        if path: self.import_paths([path])

    def import_paths(self, paths, *, restore_names=False, system_override=None, consolidate_ids=None):
        if self.closing:
            return
        if self.worker and self.worker.isRunning():
            self.notifications.post("An import is already running. Please wait.")
            return
        self.notifications.post("Restoring original ROM filenames…" if restore_names else "Importing games…", key="import")
        self.import_action.setEnabled(False)
        self.worker = Importer(self.library.root, paths, restore_names=restore_names,
                               system_override=system_override, consolidate_ids=consolidate_ids)
        self.worker.progress.connect(self.import_progress_changed)
        self.worker.result.connect(self.import_done)
        if self.issue_dialog:
            self.issue_dialog.set_busy(True)
        if not restore_names:
            self.import_progress.setRange(0, 0)
            self.import_progress.show()
            self.cancel_import.show()
        self.worker.start()

    def import_progress_changed(self, current, total, message):
        self.import_progress.setRange(0, total)
        self.import_progress.setValue(current)
        self.notifications.post(message, key="import")

    def update_issues(self):
        count = len(self.library.import_issues())
        self.issues_button.setText(f"{count} import issue{'s' if count != 1 else ''}")
        self.issues_button.setVisible(count > 0)
        if count:
            self.notifications.post(f"{count} import issue{'s' if count != 1 else ''} need attention. Open Import issues to resolve them.", key="issues")
        elif "issues" in self.notifications.active:
            self.notifications.finish("issues", "Import issues resolved.")
        if self.issue_dialog:
            self.issue_dialog.refresh()

    def show_import_issues(self):
        self.notifications.panel.hide()
        if self.issue_dialog is None:
            self.issue_dialog = ImportIssuesDialog(self.library, self)
            self.issue_dialog.retry.connect(lambda path, system: self.import_paths([path], system_override=system))
            self.issue_dialog.retry_many.connect(lambda paths, system: self.import_paths(paths, system_override=system))
            self.issue_dialog.finished.connect(self.update_issues)
        self.issue_dialog.refresh()
        self.issue_dialog.show()
        self.issue_dialog.raise_()
        self.issue_dialog.activateWindow()

    def import_done(self, count, errors):
        if self.closing:
            return
        self.import_action.setEnabled(True)
        self.import_progress.hide()
        self.cancel_import.hide()
        if self.issue_dialog:
            self.issue_dialog.set_busy(False)
        self.refresh()
        if self.worker.error:
            self.notifications.finish("import", "Import could not finish: " + self.worker.error)
            self.update_issues()
            return
        if self.worker.restore_names:
            message = f"Restored {count} original ROM filename{'s' if count != 1 else ''}."
            if errors:
                message += f" Reimport {errors[0]} original ROM(s) or ZIP(s) to recover their filenames."
            self.notifications.finish("import", message)
            self.start_background()
            return
        if self.worker.consolidate_ids is not None:
            message = f"Copied {count} game(s) into the library. Original files were kept."
            if self.worker.cancelled:
                message = "Copying cancelled. " + message
            self.notifications.finish("import", message)
            if errors:
                self.notifications.post("Some games could not be copied:\n" + "\n".join(errors[:12]))
        else:
            message = f"{self.worker.new_games} new game(s) · {self.worker.duplicates} duplicate(s) kept once"
            if self.worker.cancelled:
                message = "Import cancelled. " + message
            self.notifications.finish("import", message)
            self.update_issues()
        systems = set(getattr(self.worker, 'imported_systems', ()))
        if systems:
            self.queue_default_cores(systems)
        self.start_background()
        if errors and not self.worker.cancelled and any(issue['code'] == 'unknown_disc' for issue in self.library.import_issues()):
            self.show_import_issues()

    def demo(self):
        self.import_paths([ROOT / "demos"])

    def play(self):
        ids = self.selected_ids()
        if len(ids) != 1: return
        self.play_game(ids[0])

    def automatic_state(self, game_id):
        game = self.rows.get(game_id) or self.library.get(game_id)
        if not game:
            return None
        try:
            manager = CoreManager(self.library.root)
            selected = manager.choice(self.library, game['system'])
            path = self.library.root / 'states' / game_id / 'libretro' / selected['id'] / selected['sha256'] / 'auto.oesavestate'
            if path.is_file():
                return path
            candidates = sorted((self.library.root / 'states' / game_id / 'libretro').glob('*/*/auto.oesavestate'),
                                key=lambda candidate: candidate.stat().st_mtime, reverse=True)
            for candidate in candidates:
                try:
                    manager.state_build(candidate, game_id, game['system'])
                    return candidate
                except (CoreError, ValueError, OSError, KeyError):
                    continue
            return None
        except (CoreError, ValueError, OSError, KeyError):
            return None

    def play_game(self, game_id):
        self.launch_game(game_id, self.automatic_state(game_id))

    def restart_game(self, game_id):
        if game_id in self.processes:
            self.notifications.post("Close this game's window before restarting it.", 6000)
            return
        game = self.library.get(game_id)
        if not game:
            return
        answer = QMessageBox.question(self, "Restart game?",
            f"Restart {game['title']}?\n\nThe automatic save used by Resume will be cleared. Manual save states and normal in-game saves will be kept.",
            QMessageBox.Yes | QMessageBox.Cancel, QMessageBox.Cancel)
        if answer == QMessageBox.Yes:
            self.launch_game(game_id, restart_auto=True)

    def launch_game(self, game_id, state_path=None, *, restart_auto=False):
        if game_id in self.processes:
            self.notifications.post("This game is already running in its game window.", 6000)
            return
        try:
            game = self.library.get(game_id)
            manager = CoreManager(self.library.root)
            selected = manager.selection(self.library, game['system'])
        except CoreError as error:
            self.notifications.post(str(error), 15000)
            self.queue_default_cores()
            if not self.core_worker:
                self.open_settings()
            return
        if state_path:
            try:
                selected = manager.state_build(state_path, game_id, game['system'])
            except (CoreError, ValueError, OSError, KeyError) as error:
                self.notifications.post(str(error), 15000)
                return
        process = QProcess(self)
        process.setProgram(sys.executable)
        frozen = getattr(sys, "frozen", False)
        args = (["--player-process"] if frozen
                else ["-m", "emuluna.player"])
        args.extend(["--data-dir", str(self.library.root), "--game", game_id])
        if restart_auto:
            args.append("--restart-auto")
        if state_path:
            args.extend(["--state-file", str(state_path)])
            args.extend(["--core-id", selected["id"], "--core-sha256", selected["sha256"]])
        process.setArguments(args)
        if frozen:
            # PyInstaller 6.9+ otherwise assumes that a second invocation of
            # sys.executable is a worker belonging to this frozen process. A
            # game player is a complete, independent Qt application and needs
            # a fresh bootloader environment so its top-level window is mapped.
            environment = QProcessEnvironment.systemEnvironment()
            environment.insert("PYINSTALLER_RESET_ENVIRONMENT", "1")
            process.setProcessEnvironment(environment)
        process.setWorkingDirectory(str(ROOT))
        process.setProcessChannelMode(QProcess.MergedChannels)
        process.log = bytearray()
        process.ready_notified = False
        process.focus_notified = False
        process.readyReadStandardOutput.connect(lambda: self.process_output(process))
        process.finished.connect(lambda code, status: self.game_closed(game_id, code))
        process.errorOccurred.connect(lambda error: self.notifications.post(f"Game process error: {process.errorString()}"))
        self.processes[game_id] = process
        process.start()
        self.notifications.post(f"Opening {game['title']}…", 5000)

    def process_output(self, process):
        process.log.extend(bytes(process.readAllStandardOutput()))
        del process.log[:-16000]
        if not process.ready_notified and b"EMULUNA_GAME_READY" in process.log:
            process.ready_notified = True
            self.notifications.post("Game opened in its own window.", 5000)
        if not process.focus_notified and b"EMULUNA_GAME_NEEDS_FOCUS" in process.log:
            process.focus_notified = True
            if self.game_restore_state is None and self.isVisible() and not self.isMinimized():
                self.game_restore_state = self.windowState()
                self.showMinimized()

    def game_closed(self, game_id, code):
        process = self.processes.pop(game_id)
        self.process_output(process)
        if code:
            self.notifications.post("The game stopped: " + (process.log.decode(errors="replace")[-2000:] or "The emulator process exited unexpectedly."))
        process.deleteLater()
        self.refresh()
        if not self.processes and self.game_restore_state is not None and not self.closing:
            state, self.game_restore_state = self.game_restore_state, None
            self.setWindowState(state)
            self.show()
            self.raise_()
            self.activateWindow()

    def context_menu(self, position, view=None):
        view = view or self.games
        item = view.itemAt(position)
        if not item: return
        if not item.isSelected():
            self.restore_selection([item.data(Qt.UserRole)])
        menu = self.game_menu(self.selected_ids())
        menu.exec(view.viewport().mapToGlobal(position))
        self.refresh()

    def game_menu(self, game_ids):
        game_id = game_ids[0]
        row = self.rows[game_id]
        single = len(game_ids) == 1
        menu = QMenu(self)
        if single:
            menu.addAction("Resume" if self.automatic_state(game_id) else "Play", self.play)
            if self.automatic_state(game_id):
                menu.addAction("Restart game…", lambda: self.restart_game(game_id))
        all_favorites = all(self.rows[key]["favorite"] for key in game_ids)
        def favorite():
            for key in game_ids:
                if bool(self.rows[key]["favorite"]) == all_favorites:
                    self.library.favorite(key)
        menu.addAction("Remove from favorites" if all_favorites else "Add to favorites", favorite)
        ratings = menu.addMenu("Rating")
        for value in range(6):
            action = ratings.addAction("★" * value if value else "Unrated")
            action.setCheckable(True)
            action.setChecked(all(self.rows[key]["rating"] == value for key in game_ids))
            action.triggered.connect(lambda checked=False, value=value: self.library.rate(game_ids, value))
        collections = menu.addMenu("Add to collection")
        regular = [record for record in self.library.collections() if record["rules"] is None]
        collections.setEnabled(bool(regular))
        for record in regular:
            action = collections.addAction(record["name"])
            action.triggered.connect(lambda checked=False, collection_id=record["id"]: self.add_to_collection(collection_id, game_ids))
        menu.addAction("Create collection from selection…", lambda: self.new_collection(game_ids=game_ids))
        collection_id = self.current_collection()
        if collection_id and self.library.collection(collection_id)["rules"] is None:
            menu.addAction("Remove from collection", lambda: self.library.remove_from_collection(collection_id, game_ids))
        menu.addSeparator()
        if single:
            menu.addAction("Rename game…", lambda: self.rename_game(game_id))
            menu.addAction("Add cover art from file…", lambda: self.choose_cover(game_id))
        download_cover = menu.addAction("Download missing cover art", lambda: self.start_artwork(force=True, game_ids=set(game_ids)))
        download_cover.setEnabled(not self.art_worker and any(not self.rows[key]["cover"] or
            not (self.library.root / self.rows[key]["cover"]).is_file() for key in game_ids))
        if any(self.rows[key]["cover"] for key in game_ids):
            menu.addAction("Download replacement cover art…", lambda: self.download_replacement(game_ids))
        if single:
            menu.addAction("Open ROM folder", lambda: QDesktopServices.openUrl(QUrl.fromLocalFile(str((self.library.root / row["rom_path"]).parent))))
        external = [key for key in game_ids if Path(self.rows[key]["rom_path"]).is_absolute()]
        if external:
            menu.addAction("Consolidate files into library", lambda: self.import_paths([], consolidate_ids=external))
        if single:
            menu.addAction("Locate missing ROM…", lambda: self.locate_rom(game_id))
        menu.addSeparator()
        menu.addAction("Remove from library…", lambda: self.remove_games(game_ids))
        if single:
            menu.addAction("Game information…", lambda: GameInfoDialog(self.library, game_id, self).exec())
        return menu

    def remove_games(self, game_ids, *, trash_roms=False):
        from .file_management import removal_plan, remove_games
        from .removal_dialog import GameRemovalDialog
        if any(key in self.processes for key in game_ids):
            self.notifications.post('Close the selected games before removing them.')
            return
        try:
            try:
                plan = removal_plan(self.library, game_ids)
            except ValueError as error:
                # A damaged disc inventory must not prevent removing just the
                # database reference. Disable file disposal until it is fixed.
                plan = removal_plan(self.library, game_ids, include_roms=False)
                plan['rom_error'] = str(error)
            if not plan['games']:
                return
            dialog = GameRemovalDialog(plan, self, trash_roms=trash_roms)
            try:
                if not dialog.exec():
                    return
                result = remove_games(self.library, game_ids, trash_roms=dialog.trash.isChecked(),
                                      states=dialog.states.isChecked(), screenshots=dialog.screenshots.isChecked())
            finally:
                dialog.deleteLater()
            self.refresh()
            self.notifications.post(f'Removed {len(result["games"])} game(s) from the library.')
        except (OSError, ValueError) as error:
            self.notifications.post('Could not finish removal: ' + str(error))
            self.refresh()

    def download_replacement(self, game_ids):
        answer = QMessageBox.question(self, "Replace cover art?",
            "Download new covers for the selected games? Their current covers will remain visible unless a replacement downloads successfully.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No)
        if answer == QMessageBox.Yes:
            self.start_artwork(force=True, game_ids=set(game_ids), replace=True)

    def locate_rom(self, game_id):
        if game_id in self.processes:
            self.notifications.post("Close this game's window before changing its ROM location.", 7000)
            return
        path, _ = QFileDialog.getOpenFileName(self, "Locate the original ROM file")
        if path:
            try:
                self.library.relink(game_id, path)
                self.refresh()
            except (OSError, ValueError) as error:
                self.notifications.post("Could not reconnect ROM: " + str(error))

    def rename_game(self, game_id):
        row = self.library.get(game_id)
        name, ok = QInputDialog.getText(self, "Rename game", "Title", text=row["title"])
        if ok:
            self.library.rename(game_id, name)

    def choose_cover(self, game_id):
        path, _ = QFileDialog.getOpenFileName(self, "Choose cover", "", "Images (*.png *.jpg *.jpeg *.webp)")
        if path:
            self.replace_cover(game_id, path)

    def replace_cover(self, game_id, path):
        image = QPixmap(path)
        if image.isNull():
            self.notifications.post("This image could not be opened. Choose a PNG, JPEG or WebP file.", 8000)
            return
        target = Path("covers") / (game_id + ".png")
        if image.scaled(700, 700, Qt.KeepAspectRatio, Qt.SmoothTransformation).save(str(self.library.root / target)):
            self.library.set_manual_cover(game_id, target)
            self.refresh()

    def files_dropped(self, paths, game_id):
        if game_id and len(paths) == 1 and Path(paths[0]).suffix.lower() in (".png", ".jpg", ".jpeg", ".webp"):
            self.replace_cover(game_id, paths[0])
        else:
            self.import_paths(paths)

    def dragEnterEvent(self, event):
        if event.mimeData().hasUrls(): event.acceptProposedAction()

    def dropEvent(self, event):
        self.import_paths([u.toLocalFile() for u in event.mimeData().urls() if u.isLocalFile()])
        event.acceptProposedAction()

    def closeEvent(self, event):
        if (self.worker and self.worker.isRunning()) or self.processes:
            self.notifications.post("Close the game windows and let imports finish before closing the library.")
            event.ignore()
            return
        self.closing = True
        self.art_timer.stop()
        self.search_timer.stop()
        if self.core_worker:
            self.core_pending.clear()
            self.core_worker.requestInterruption()
        if self.art_worker or self.metadata_worker or self.core_worker:
            self.cancel_artwork()
            self.cancel_metadata()
            self.notifications.post("Stopping background lookups…")
            event.ignore()
            return
        header = self.table.horizontalHeader()
        self.library.set_setting("library.columns", json.dumps([self.table.columnWidth(i) for i in range(8)]))
        self.library.set_setting("library.sort", json.dumps([header.sortIndicatorSection(), header.sortIndicatorOrder().value]))
        if self.issue_dialog:
            self.issue_dialog.close()
        self.notifications.panel.close()
        self.prefetch_timer.stop()
        self.thumbnails.close()
        self.library.close()
        event.accept()


def main():
    parser = argparse.ArgumentParser(description="EmuLuna game library")
    parser.add_argument("roms", nargs="*")
    parser.add_argument("--data-dir")
    parser.add_argument("--screenshot", help=argparse.SUPPRESS)
    args = parser.parse_args()
    app = QApplication(sys.argv[:1])
    configure_application(app)
    window = Window(Library(args.data_dir), auto_artwork=not args.screenshot)
    window.show()
    if args.roms: window.import_paths(args.roms)
    if args.screenshot:
        def capture():
            window.grab().save(args.screenshot)
            window.close()
        QTimer.singleShot(1000, capture)
    return app.exec()
