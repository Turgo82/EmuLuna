"""User settings and the Libretro core library."""
from pathlib import Path

from PySide6.QtCore import Qt, Signal, QTimer, QSize, QEvent, QByteArray
from PySide6.QtGui import QIcon
from PySide6.QtMultimedia import QMediaDevices
from PySide6.QtWidgets import (QApplication, QDialog, QVBoxLayout, QHBoxLayout, QTabWidget, QWidget, QFormLayout,
    QLabel, QSlider, QLineEdit, QPushButton, QFileDialog, QTableWidget,
    QTableWidgetItem, QHeaderView, QAbstractItemView, QScrollArea, QMessageBox, QInputDialog)

from .theme import set_system_theme, follow_system_theme, theme_palette
from .bios import bios_status, requirements, import_bios
from .core_manager import CATALOG, CoreManager, CoreWorker
from .library import SYSTEMS
from .video_filters import ALL_FILTERS, valid_filter
from .controller_settings import ControlsPage
from .branding import navigation_icon
from .gamepad import Gamepad
from .settings_style import (SETTINGS_STYLE, SettingsCheckBox as QCheckBox,
                             SettingsComboBox as QComboBox, SettingsSpinBox as QSpinBox)


class SettingsDialog(QDialog):
    changed = Signal()

    def __init__(self, library, parent=None):
        super().__init__(parent)
        self.library = library
        follow_system_theme(library)
        self.setPalette(theme_palette())
        self.manager = CoreManager(library.root)
        self.worker = None
        self.close_pending = False
        self.refreshing = False
        self.setWindowTitle("Settings — EmuLuna")
        self.resize(840, 620)
        self.setMinimumSize(740, 550)
        self.setStyleSheet(SETTINGS_STYLE)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 14)
        self.tabs = QTabWidget()
        self.tabs.setIconSize(QSize(23, 23))
        self.tabs.setDocumentMode(True)
        self.tabs.tabBar().setDrawBase(False)
        self.tabs.tabBar().setFocusPolicy(Qt.NoFocus)
        layout.addWidget(self.tabs, 1)
        self.general_tab()
        self.library_tab()
        self.page_keys = ('general', 'library', 'gameplay', 'controls', 'cores', 'bios')
        self.loaded_pages = {'general', 'library'}
        self.page_builders = {'gameplay': self.gameplay_tab, 'controls': self.controls_tab,
                              'cores': self.core_tab, 'bios': self.bios_tab}
        for key, title in zip(self.page_keys[2:], ('Gameplay', 'Controls', 'Cores', 'System Files')):
            page = QWidget()
            page.setObjectName('settings-' + key)
            page_layout = QVBoxLayout(page)
            page_layout.setContentsMargins(0, 0, 0, 0)
            self.tabs.addTab(page, title)
        self.tabs.currentChanged.connect(self.ensure_page)
        self.refresh_navigation_icons()
        self.status = QLabel()
        self.status.setWordWrap(True)
        self.status.hide()
        layout.addWidget(self.status)
        buttons = QHBoxLayout()
        buttons.addStretch()
        close = QPushButton("Done")
        close.clicked.connect(self.accept)
        buttons.addWidget(close)
        layout.addLayout(buttons)
        self.core_watch = QTimer(self)
        self.core_watch.timeout.connect(self.check_core_changes)
        self.core_watch.start(1000)
        geometry = self.library.setting('window.settings.geometry')
        if geometry:
            try:
                # Qt also brings the window back on screen if displays change.
                self.restoreGeometry(QByteArray(bytes.fromhex(geometry)))
            except ValueError:
                pass  # Keep the default size if the saved preference is invalid.

    def show_page(self, key):
        """Select a section, constructing its contents on the first visit."""
        self.tabs.setCurrentIndex(self.page_keys.index(key))

    def ensure_page(self, index):
        if index < 0:
            return
        key = self.page_keys[index]
        if key in self.loaded_pages:
            return
        self.loaded_pages.add(key)
        page = self.page_builders[key]()
        self.tabs.widget(index).layout().addWidget(page)
        # A page created after the window's Show event needs its own palette
        # polish, without restyling the library or already loaded sections.
        binding = QApplication.instance()._emuluna_system_theme
        binding.refresh([page, *page.findChildren(QWidget)])

    def core_tab(self):
        self.core_pages = QTabWidget()
        self.core_pages.setDocumentMode(True)
        self.core_pages.tabBar().setDrawBase(False)
        self.core_pages.tabBar().setFocusPolicy(Qt.NoFocus)
        self.cores_tab()
        self.downloads_tab()
        self.refresh_cores()
        return self.core_pages

    def refresh_navigation_icons(self):
        for index, key in enumerate(('general', 'library', 'gameplay', 'controls', 'cores', 'bios')):
            # Keep the same legible foreground in selected and unselected tabs;
            # the tab background and underline already communicate selection.
            base = navigation_icon(key).pixmap(23, 23, QIcon.Normal, QIcon.Off)
            self.tabs.setTabIcon(index, QIcon(base))

    def changeEvent(self, event):
        super().changeEvent(event)
        if (event.type() in (QEvent.PaletteChange, QEvent.ApplicationPaletteChange)
                and hasattr(self, 'tabs') and self.tabs.count() == 6):
            self.refresh_navigation_icons()

    def set_status(self, message):
        self.status.setText(message)
        self.status.setVisible(bool(message))

    def check_core_changes(self):
        if not self.isVisible() or 'cores' not in self.loaded_pages:
            return
        stamp = self.manager.manifest.stat().st_mtime_ns if self.manager.manifest.exists() else None
        if stamp != getattr(self, 'core_stamp', None) and not self.worker:
            self.refresh_cores()

    def set_setting(self, key, value):
        self.library.set_setting(key, value)
        self.changed.emit()

    def checkbox(self, form, title, key, default):
        box = QCheckBox(title)
        box.setChecked(self.library.setting(key, default) == "1")
        box.toggled.connect(lambda value: self.set_setting(key, int(value)))
        form.addRow(box)
        return box

    def general_tab(self):
        page = QWidget()
        form = QFormLayout(page)
        form.setContentsMargins(20, 22, 20, 20)
        form.setSpacing(14)
        self.use_system_theme = self.checkbox(form, "Use system theme colors", "appearance.use_system_theme", "1")
        self.use_system_theme.setToolTip("Follow OS colors and selection accents. Turn off to use EmuLuna’s dark theme.")
        self.use_system_theme.toggled.connect(set_system_theme)
        self.volume = QSlider(Qt.Horizontal)
        self.volume.setMinimumWidth(240)
        self.volume.setMaximumWidth(360)
        self.volume.setRange(0, 100)
        self.volume.setValue(int(self.library.setting("volume", "80")))
        self.volume.valueChanged.connect(lambda value: self.set_setting("volume", value))
        form.addRow("Game volume", self.volume)
        self.focus_pause = self.checkbox(form, "Pause games when switching to another window", "pause_unfocused", "1")
        self.integer = self.checkbox(form, "Use integer scaling for sharp pixels", "integer_scale", "0")
        self.auto_art = self.checkbox(form, "Automatically download box art", "artwork_auto", "1")
        self.backup_art = self.checkbox(form, "Use backup artwork source when needed", "artwork_backup", "1")
        folder = QWidget()
        row = QHBoxLayout(folder)
        row.setContentsMargins(0, 0, 0, 0)
        self.bios_path = QLineEdit(self.library.setting("bios_directory", str(self.library.root / "system")))
        self.bios_path.setReadOnly(True)
        row.addWidget(self.bios_path, 1)
        browse = QPushButton("Choose…")
        browse.clicked.connect(self.choose_bios)
        row.addWidget(browse)
        form.addRow("BIOS / system folder", folder)
        explanation = QLabel("Use this folder for files required by a core. BIOS files are not downloaded by the app.")
        explanation.setWordWrap(True)
        explanation.setStyleSheet("color:palette(placeholder-text)")
        form.addRow(explanation)
        self.tabs.addTab(page, "General")

    def choose_bios(self):
        path = QFileDialog.getExistingDirectory(self, "Choose BIOS / system folder", self.bios_path.text())
        if path:
            self.bios_path.setText(path)
            self.set_setting("bios_directory", path)
            if 'bios' in self.loaded_pages:
                self.refresh_bios()

    def controls_tab(self):
        self.controls_page = ControlsPage(self.library, self)
        self.controls_page.changed.connect(self.changed)
        return self.controls_page

    def gameplay_tab(self):
        page = QWidget()
        form = QFormLayout(page)
        form.setContentsMargins(20, 22, 20, 20)
        form.setSpacing(16)
        game_mode = QLabel("Game Mode")
        game_mode.setObjectName("controlGroup")
        form.addRow(game_mode)
        self.game_mode_keep_awake = self.checkbox(
            form, "Keep the display awake while playing", "game_mode.keep_awake", "1")
        self.game_mode_keep_awake.setToolTip(
            "Prevent screen dimming, the screen saver, and automatic sleep while a game is open.")
        self.fullscreen_default = self.checkbox(form, "Open games in fullscreen", "fullscreen_default", "0")
        self.hide_cursor = self.checkbox(form, "Hide the pointer when gameplay controls disappear", "hide_game_cursor", "1")
        self.fast_speed = QSpinBox()
        self.fast_speed.setRange(2, 10)
        self.fast_speed.setSuffix("×")
        try:
            self.fast_speed.setValue(int(self.library.setting("fast_forward_speed", "3")))
        except ValueError:
            self.fast_speed.setValue(3)
        self.fast_speed.valueChanged.connect(lambda value: self.set_setting("fast_forward_speed", value))
        form.addRow("Fast-forward speed", self.fast_speed)
        self.aspect = QComboBox()
        self.aspect.addItem("System default", "system")
        self.aspect.addItem("Square pixels", "square")
        self.aspect.setCurrentIndex(max(0, self.aspect.findData(self.library.setting("aspect_mode", "system"))))
        self.aspect.currentIndexChanged.connect(lambda _: self.set_setting("aspect_mode", self.aspect.currentData()))
        form.addRow("Display aspect ratio", self.aspect)
        self.video_filter = QComboBox()
        self.video_filter.setMinimumWidth(240)
        self.video_filter.setMaximumWidth(300)
        for key, name in ALL_FILTERS.items():
            self.video_filter.addItem(name, key)
        self.video_filter.setCurrentIndex(self.video_filter.findData(valid_filter(self.library.setting("video_filter", "nearest"))))
        self.video_filter.currentIndexChanged.connect(lambda: self.set_setting("video_filter", self.video_filter.currentData()))
        filter_row = QWidget()
        filter_layout = QHBoxLayout(filter_row)
        filter_layout.setContentsMargins(0, 0, 0, 0)
        filter_layout.setSpacing(10)
        filter_layout.addWidget(self.video_filter)
        self.reset_filters = QPushButton("Use for all consoles")
        self.reset_filters.clicked.connect(self.reset_video_filters)
        filter_layout.addWidget(self.reset_filters)
        filter_layout.addStretch(1)
        form.addRow("Default video filter", filter_row)
        self.audio_output = QComboBox()
        self.audio_output.currentIndexChanged.connect(self.select_audio_output)
        form.addRow("Audio output", self.audio_output)
        self.audio_devices = QMediaDevices(self)
        self.audio_devices.audioOutputsChanged.connect(self.refresh_audio_outputs)
        self.refresh_audio_outputs()
        self.latency = QSpinBox()
        self.latency.setRange(20, 250)
        self.latency.setSuffix(" ms")
        try:
            self.latency.setValue(int(self.library.setting("audio_latency", "80")))
        except ValueError:
            self.latency.setValue(80)
        self.latency.valueChanged.connect(lambda value: self.set_setting("audio_latency", value))
        form.addRow("Audio buffer target", self.latency)
        self.rumble = self.checkbox(form, "Enable controller vibration", "controller_rumble", "1")
        self.rumble_intensity = QSlider(Qt.Horizontal)
        self.rumble_intensity.setMinimumWidth(240)
        self.rumble_intensity.setMaximumWidth(360)
        self.rumble_intensity.setRange(0, 100)
        try:
            strength = max(0, min(100, int(self.library.setting("controller_rumble_intensity", "100"))))
        except ValueError:
            strength = 100
        self.rumble_intensity.setValue(strength)
        self.rumble_intensity.valueChanged.connect(
            lambda value: self.set_setting("controller_rumble_intensity", value))
        vibration_row = QWidget()
        vibration_layout = QHBoxLayout(vibration_row)
        vibration_layout.setContentsMargins(0, 0, 0, 0)
        vibration_layout.setSpacing(10)
        vibration_layout.addWidget(self.rumble_intensity)
        self.vibration_test = QPushButton("Test controller vibration")
        self.vibration_test.clicked.connect(self.test_vibration)
        vibration_layout.addWidget(self.vibration_test)
        vibration_layout.addStretch(1)
        form.addRow("Vibration strength", vibration_row)
        note = QLabel("Move the mouse over a game to show its controls. Hold Tab to fast-forward; audio is muted while fast-forwarding. Lower audio buffers can reduce delay but may cause crackling on some devices. Fit to window fills the available area while preserving the system aspect ratio; integer scaling keeps whole scanline multiples.")
        note.setWordWrap(True)
        note.setStyleSheet("color:palette(placeholder-text)")
        form.addRow(note)
        experimental = QLabel("Experimental")
        experimental.setObjectName("controlGroup")
        form.addRow(experimental)
        self.minimize_library = self.checkbox(
            form,
            "Minimize the library while a game is running",
            "experimental.minimize_library_during_game",
            "0",
        )
        self.minimize_library.setToolTip(
            "Minimize the library after the game window opens, then restore it when the game closes."
        )
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setWidget(page)
        return scroll

    def test_vibration(self):
        """Test the first connected controller without requiring a game."""
        self.stop_vibration_test()
        pad = Gamepad()
        pad.poll()
        strength = round(0xffff * self.rumble_intensity.value() / 100)
        if not pad.pad:
            pad.close()
            QMessageBox.information(self, "Controller vibration",
                                    "Connect a controller, then try the vibration test again.")
            return
        if not pad.set_rumble(strength, strength):
            detail = pad.rumble_error or "The controller did not accept the vibration command."
            pad.close()
            QMessageBox.warning(self, "Controller vibration unavailable", detail)
            return
        self._vibration_test_pad = pad
        self.vibration_test.setText("Vibrating…")
        self.vibration_test.setEnabled(False)
        self._vibration_test_timer = QTimer(self)
        self._vibration_test_timer.setSingleShot(True)
        self._vibration_test_timer.timeout.connect(self.stop_vibration_test)
        self._vibration_test_timer.start(700)

    def stop_vibration_test(self):
        timer = getattr(self, '_vibration_test_timer', None)
        if timer:
            timer.stop()
            timer.deleteLater()
            self._vibration_test_timer = None
        pad = getattr(self, '_vibration_test_pad', None)
        if pad:
            pad.stop_rumble()
            pad.close()
            self._vibration_test_pad = None
        if hasattr(self, 'vibration_test'):
            self.vibration_test.setText("Test controller vibration")
            self.vibration_test.setEnabled(True)

    def refresh_audio_outputs(self):
        self.audio_output.blockSignals(True)
        selected = self.library.setting("audio_device", "")
        self.audio_output.clear()
        self.audio_output.addItem("System default", "")
        for device in QMediaDevices.audioOutputs():
            self.audio_output.addItem(device.description(), bytes(device.id()).hex())
        index = self.audio_output.findData(selected)
        if index < 0:
            self.audio_output.addItem("Disconnected device (uses system default)", selected)
            index = self.audio_output.count() - 1
        self.audio_output.setCurrentIndex(index)
        self.audio_output.blockSignals(False)

    def reset_video_filters(self):
        with self.library.db:
            self.library.db.execute("DELETE FROM settings WHERE key LIKE 'video_filter.%'")
        self.changed.emit()

    def select_audio_output(self):
        if self.audio_output.currentIndex() >= 0:
            self.set_setting("audio_device", self.audio_output.currentData())

    def library_tab(self):
        page = QWidget()
        form = QFormLayout(page)
        form.setContentsMargins(20, 22, 20, 20)
        form.setSpacing(18)
        location = QLineEdit(str(self.library.root))
        location.setReadOnly(True)
        location.setCursorPosition(0)
        form.addRow("Library location", location)
        self.show_alphabet_index = self.checkbox(form, "Show alphabet index", "library.show_alphabet_index", "1")
        self.show_alphabet_index.setToolTip("Show the #–Z shortcuts beside the game library.")
        self.hide_empty_consoles = self.checkbox(form, "Hide consoles with no games", "library.hide_empty_consoles", "0")
        self.copy_games = self.checkbox(form, "Copy games into library when importing", "copy_games", "1")
        self.auto_metadata = self.checkbox(form, "Automatically look up game information", "metadata_auto", "1")
        note = QLabel("When enabled, copies are organized by system and keep their original filenames.\n\nWhen disabled, games stay in their current folders. Keep those files and drives available to play. ZIP imports always extract a managed copy.\n\nThis setting applies to new imports. To copy an existing external game, right-click it and choose Consolidate files into library. Originals are never moved or deleted.")
        note.setWordWrap(True)
        form.addRow(note)
        self.tabs.addTab(page, "Library")

    def cores_tab(self):
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(20, 22, 20, 20)
        form = QFormLayout()
        form.setSpacing(18)
        self.choices = {}
        for key in SYSTEMS:
            system = SYSTEMS[key]
            label = QWidget()
            row = QHBoxLayout(label)
            row.setContentsMargins(0, 0, 12, 0)
            icon = QLabel()
            icon.setPixmap(QIcon(str(system.icon)).pixmap(24, 24))
            row.addWidget(icon)
            row.addWidget(QLabel(system.name))
            combo = QComboBox()
            combo.currentIndexChanged.connect(lambda _, s=key: self.select_core(s))
            self.choices[key] = combo
            form.addRow(label, combo)
        contents = QWidget()
        contents.setLayout(form)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setWidget(contents)
        layout.addWidget(scroll, 1)
        description = QLabel("Download or import standard libretro cores in Core downloads. Automatic uses the recommended installed core, or another installed core for the system.\n\nBattery saves and save states are kept separately for each core. Save states also belong to the exact core build that created them. States from another emulator may not be compatible.")
        description.setWordWrap(True)
        description.setStyleSheet("color:palette(placeholder-text);padding-top:20px")
        layout.addWidget(description)
        self.core_pages.addTab(page, "Core selection")

    def select_core(self, system):
        if not self.refreshing:
            self.set_setting("core." + system, self.choices[system].currentData())

    def downloads_tab(self):
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(16, 18, 16, 16)
        label = QLabel("Official Linux cores used by RetroArch. Downloads and updates happen only when you choose them. Previous builds are retained for rollback.")
        label.setWordWrap(True)
        layout.addWidget(label)
        self.auto_cores = QCheckBox("Automatically download a default core when importing games")
        self.auto_cores.setChecked(self.library.setting('core_auto_install', '1') == '1')
        self.auto_cores.toggled.connect(lambda checked: self.set_setting('core_auto_install', int(checked)))
        layout.addWidget(self.auto_cores)
        self.table = QTableWidget(0, 3)
        self.table.setHorizontalHeaderLabels(["Consoles", "Core", "Installed build"])
        self.table.horizontalHeader().setSectionResizeMode(0, QHeaderView.ResizeToContents)
        self.table.horizontalHeader().setSectionResizeMode(1, QHeaderView.Stretch)
        self.table.horizontalHeader().setSectionResizeMode(2, QHeaderView.Stretch)
        self.table.verticalHeader().hide()
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.SingleSelection)
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.table.itemSelectionChanged.connect(self.update_buttons)
        layout.addWidget(self.table, 1)
        controls = QHBoxLayout()
        self.download_button = QPushButton("Download / update selected")
        self.download_button.clicked.connect(self.download_selected)
        controls.addWidget(self.download_button)
        self.update_button = QPushButton("Update all installed")
        self.update_button.clicked.connect(lambda: self.start_job(update_all=True))
        controls.addWidget(self.update_button)
        self.install_all_button = QPushButton("Install all cores")
        self.install_all_button.clicked.connect(lambda: self.start_job(install_all=True))
        controls.addWidget(self.install_all_button)
        layout.addLayout(controls)
        controls = QHBoxLayout()
        self.import_button = QPushButton("Import core file…")
        self.import_button.clicked.connect(self.import_core)
        controls.addWidget(self.import_button)
        self.remove_button = QPushButton("Remove core")
        self.remove_button.clicked.connect(self.remove_core)
        controls.addWidget(self.remove_button)
        self.restore_button = QPushButton("Restore previous…")
        self.restore_button.clicked.connect(self.restore_core)
        controls.addWidget(self.restore_button)
        self.cancel_download_button = QPushButton("Cancel download")
        self.cancel_download_button.clicked.connect(lambda: self.worker.requestInterruption() if self.worker else None)
        controls.addWidget(self.cancel_download_button)
        layout.addLayout(controls)
        self.core_details = QLabel()
        note = self.core_details
        note.setStyleSheet("color:palette(placeholder-text);font-size:11px")
        note.setWordWrap(True)
        layout.addWidget(note)
        self.core_pages.addTab(page, "Core downloads")

    def refresh_cores(self):
        self.core_stamp = self.manager.manifest.stat().st_mtime_ns if self.manager.manifest.exists() else None
        self.refreshing = True
        records = self.manager.installed()
        for system, combo in self.choices.items():
            selected = self.library.setting("core." + system, "auto")
            if selected == "builtin":
                selected = "auto"
            combo.clear()
            compatible = [r for r in records.values() if system in r["systems"]]
            combo.addItem("Automatic" if compatible else "No core installed — see Core downloads", "auto")
            for core_id, record in records.items():
                if system in record["systems"]:
                    combo.addItem(f"{record['name']} — Libretro ({record['version']})", core_id)
            index = combo.findData(selected)
            if index < 0:
                combo.addItem("Missing core — choose another", selected)
                index = combo.count() - 1
            combo.setCurrentIndex(index)
        selected_id = self.table.item(self.table.currentRow(), 0).data(Qt.UserRole) if self.table.currentRow() >= 0 else None
        all_cores = dict(CATALOG)
        all_cores.update({key: record for key, record in records.items() if key not in all_cores})
        self.table.setRowCount(len(all_cores))
        def console_names(info):
            return sorted((SYSTEMS[system].name for system in info["systems"]), key=str.casefold)
        ordered = sorted(all_cores.items(), key=lambda pair:
            (console_names(pair[1])[0].casefold(), pair[1]["name"].casefold()))
        for row, (core_id, info) in enumerate(ordered):
            record = records.get(core_id)
            if record:
                build = record.get("build") or ("Imported" if record.get("source") == "local"
                                                 else "retained " + record["sha256"][:12])
                installed = f"{record['version']} · {build}"
            else:
                installed = "Not installed"
            for column, value in enumerate((" / ".join(console_names(info)), info["name"], installed)):
                item = QTableWidgetItem(value)
                item.setData(Qt.UserRole, core_id)
                item.setToolTip(value)
                self.table.setItem(row, column, item)
            if core_id == selected_id:
                self.table.selectRow(row)
        if self.table.currentRow() < 0:
            self.table.selectRow(0)
        self.refreshing = False
        self.update_buttons()

    def update_buttons(self):
        row = self.table.currentRow()
        item = self.table.item(row, 0) if row >= 0 else None
        self.download_button.setEnabled(not self.worker and bool(item) and item.data(Qt.UserRole) in CATALOG)
        if item and hasattr(self, "core_details"):
            core_id = item.data(Qt.UserRole)
            info = CATALOG.get(core_id, {})
            previous = self.manager.history(core_id)
            details = info.get("launch_block") or info.get("notice") or \
                "Standard libretro core · Linux .so file. See BIOS checklist for firmware requirements."
            if previous:
                details += f" {len(previous)} previous build{'s' if len(previous) != 1 else ''} available."
            else:
                details += " An update will retain the current build for rollback."
            self.core_details.setText(details)
        self.remove_button.setEnabled(not self.worker and bool(item) and item.data(Qt.UserRole) in self.manager.installed())
        self.restore_button.setEnabled(not self.worker and bool(item) and bool(self.manager.history(item.data(Qt.UserRole))))
        self.install_all_button.setEnabled(not self.worker)
        self.cancel_download_button.setEnabled(bool(self.worker))
        self.import_button.setEnabled(not self.worker)
        self.update_button.setEnabled(not self.worker and any(r["source"] == "buildbot" for r in self.manager.installed().values()))

    def download_selected(self):
        row = self.table.currentRow()
        if row >= 0:
            self.start_job(core_id=self.table.item(row, 0).data(Qt.UserRole))

    def remove_core(self):
        row = self.table.currentRow()
        if row < 0:
            return
        core_id = self.table.item(row, 0).data(Qt.UserRole)
        try:
            self.manager.remove(core_id, self.library)
            self.set_status("Core removed. Games and saves were kept. Automatic downloads will not reinstall it; use Download to restore it.")
            self.refresh_cores()
            self.changed.emit()
        except (OSError, RuntimeError) as error:
            QMessageBox.warning(self, "Core could not be removed", str(error))

    def restore_core(self):
        row = self.table.currentRow()
        if row < 0:
            return
        core_id = self.table.item(row, 0).data(Qt.UserRole)
        versions = self.manager.history(core_id)
        if not versions:
            return
        labels = [f"{item.get('version') or 'Unknown version'} · {item.get('build') or item['sha256'][:12]}"
                  for item in versions]
        chosen, accepted = QInputDialog.getItem(self, "Restore previous core build",
            "Choose the build to restore. The current build will remain available:", labels, 0, False)
        if not accepted:
            return
        try:
            record = self.manager.restore(core_id, versions[labels.index(chosen)]["sha256"])
            self.set_status(f"Restored {record['name']} {record['version']}. Games and saves were kept.")
            self.refresh_cores()
            self.changed.emit()
        except (OSError, RuntimeError) as error:
            QMessageBox.warning(self, "Core could not be restored", str(error))

    def import_core(self):
        path, _ = QFileDialog.getOpenFileName(self, "Import a Linux Libretro core", "", "Linux cores (*.so)")
        if path:
            names = [system.name for system in SYSTEMS.values()]
            chosen, ok = QInputDialog.getItem(self, "Core system", "Associate this core with a system", names, 0, False)
            if ok:
                system = next(key for key, item in SYSTEMS.items() if item.name == chosen)
                self.start_job(local_file=path, system=system)

    def bios_tab(self):
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(16, 18, 16, 16)
        note = QLabel("Check the BIOS / system folder for files required by each core. Import your own files using the required filename; originals are kept. No BIOS files are downloaded.")
        note.setWordWrap(True)
        layout.addWidget(note)
        self.bios_system = QComboBox()
        self.bios_system.addItem("All systems", None)
        for key, system in SYSTEMS.items():
            self.bios_system.addItem(system.name, key)
        self.bios_system.currentIndexChanged.connect(self.refresh_bios)
        layout.addWidget(self.bios_system)
        self.bios_table = QTableWidget(0, 5)
        self.bios_table.setHorizontalHeaderLabels(["File / status", "System / core", "Requirement", "Status", "Expected MD5"])
        self.bios_table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.bios_table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.bios_table.setSelectionMode(QAbstractItemView.SingleSelection)
        self.bios_table.horizontalHeader().setSectionResizeMode(QHeaderView.Interactive)
        for column, width in enumerate((175, 190, 200, 130, 265)):
            self.bios_table.setColumnWidth(column, width)
        self.bios_table.verticalHeader().hide()
        self.bios_table.itemSelectionChanged.connect(self.bios_selection_changed)
        layout.addWidget(self.bios_table, 1)
        self.bios_summary = QLabel()
        self.bios_summary.setWordWrap(True)
        layout.addWidget(self.bios_summary)
        controls = QHBoxLayout()
        refresh = QPushButton("Check again")
        refresh.clicked.connect(self.refresh_bios)
        controls.addWidget(refresh)
        self.bios_import = QPushButton("Import selected BIOS…")
        self.bios_import.clicked.connect(self.import_selected_bios)
        controls.addWidget(self.bios_import)
        controls.addStretch()
        layout.addLayout(controls)
        self.refresh_bios()
        return page

    def refresh_bios(self):
        selected = self.bios_system.currentData()
        directory = self.bios_path.text()
        rows = []
        for system, details in SYSTEMS.items():
            if selected and selected != system:
                continue
            for core_id, core in sorted(CATALOG.items(), key=lambda pair: pair[1]["name"].casefold()):
                if system not in core["systems"]:
                    continue
                for entry in requirements(core_id):
                    rows.append((system, core_id, entry))
        self.bios_table.setRowCount(len(rows))
        valid_count = 0
        cache = {}
        for row, (system, core_id, entry) in enumerate(rows):
            cache_key = (entry['path'], entry.get('md5'))
            if cache_key not in cache:
                cache[cache_key] = bios_status(directory, entry)
            status, valid = cache[cache_key]
            valid_count += int(valid)
            values = (entry['path'], SYSTEMS[system].name + ' / ' + CATALOG[core_id]['name'],
                      entry.get('requirement', 'Optional' if entry['optional'] else 'Required'), status, entry.get('md5', 'Not published'))
            for column, value in enumerate(values):
                item = QTableWidgetItem(value)
                item.setToolTip(value + ("\n" + entry['description'] if column == 0 else ""))
                if column == 0:
                    item.setData(Qt.UserRole, entry)
                    item.setCheckState(Qt.Checked if valid else Qt.Unchecked)
                    item.setFlags(item.flags() & ~Qt.ItemIsUserCheckable)
                self.bios_table.setItem(row, column, item)
        self.bios_summary.setText(f"{valid_count} of {len(rows)} listed requirements present. Optional files are not needed for every game. Regional alternatives need the BIOS matching the game.\nFolder: {directory}")
        if rows:
            self.bios_table.selectRow(0)
        self.bios_selection_changed()

    def bios_selection_changed(self):
        if hasattr(self, 'bios_import'):
            self.bios_import.setEnabled(self.bios_table.currentRow() >= 0)

    def import_selected_bios(self):
        row = self.bios_table.currentRow()
        if row < 0:
            return
        entry = self.bios_table.item(row, 0).data(Qt.UserRole)
        source, _ = QFileDialog.getOpenFileName(self, "Import " + entry['path'], "", "All files (*)")
        if not source:
            return
        try:
            import_bios(source, self.bios_path.text(), entry)
            self.set_status("BIOS imported. The original file was kept.")
            self.refresh_bios()
        except (OSError, ValueError, RuntimeError) as error:
            QMessageBox.warning(self, "BIOS could not be imported", str(error))

    def start_job(self, **kwargs):
        if self.worker:
            return
        if kwargs.get('install_all'):
            for key in CATALOG:
                self.library.set_setting('core.removed.' + key, '0')
        if kwargs.get('core_id'):
            self.library.set_setting('core.removed.' + kwargs['core_id'], '0')
        self.worker = CoreWorker(self.library.root, **kwargs)
        self.worker.progress.connect(self.set_status)
        self.worker.result.connect(self.job_result)
        self.worker.finished.connect(self.job_finished)
        self.set_status("Contacting the core server…")
        self.update_buttons()
        self.worker.start()

    def job_result(self, message, success):
        self.set_status(message)
        self.status.setStyleSheet("color:palette(window-text)")

    def job_finished(self):
        self.worker.deleteLater()
        self.worker = None
        self.refresh_cores()
        self.changed.emit()
        if self.close_pending:
            self.done(QDialog.Accepted)

    def done(self, result):
        if self.worker:
            self.close_pending = True
            self.worker.requestInterruption()
            self.set_status("Stopping the download…")
            return
        self.stop_vibration_test()
        self.library.set_setting('window.settings.geometry', bytes(self.saveGeometry()).hex())
        self.core_watch.stop()
        super().done(result)
