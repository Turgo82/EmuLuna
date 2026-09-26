"""Background import pipeline and persistent, user-resolvable issues."""
import os
from pathlib import Path
import zipfile

from PySide6.QtCore import QThread, Signal, Qt
from PySide6.QtWidgets import (QDialog, QVBoxLayout, QHBoxLayout, QLabel, QTableWidget,
    QTableWidgetItem, QHeaderView, QAbstractItemView, QPushButton, QComboBox, QFileDialog)

from .library import Library, SYSTEMS, ImportProblem
from .content import DESCRIPTORS, content_files


class Importer(QThread):
    result = Signal(int, list)
    progress = Signal(int, int, str)

    def __init__(self, root, paths, restore_names=False, system_override=None, consolidate_ids=None):
        super().__init__()
        self.root, self.paths = root, paths
        self.restore_names = restore_names
        self.system_override = system_override
        self.consolidate_ids = consolidate_ids
        self.duplicates = 0
        self.new_games = 0
        self.imported_systems = set()
        self.cancelled = False
        self.error = None

    def scan(self, library, errors):
        files, seen = [], set()
        # Folder imports ignore common documentation/artwork and hidden folders.
        # Explicitly selected files always receive a result, including unknowns.
        sidecars = {".txt", ".nfo", ".pdf", ".png", ".jpg", ".jpeg", ".webp", ".xml", ".dat", ".sav", ".srm", ".rtc"}
        def add(path):
            path = path.expanduser().absolute()
            if path not in seen:
                seen.add(path)
                files.append(path)
        def scan_error(error):
            path = error.filename or "Folder"
            self.issue(library, path, error, errors)
        for requested in self.paths:
            path = Path(requested).expanduser().absolute()
            if self.isInterruptionRequested():
                break
            if not path.is_dir():
                add(path)
                continue
            for current, directories, names in os.walk(path, onerror=scan_error, followlinks=False):
                if self.isInterruptionRequested():
                    return files
                directories[:] = sorted(name for name in directories if not name.startswith(".") and
                    (Path(current) / name).resolve() != library.root)
                if Path(current).resolve() == library.root:
                    directories[:] = []
                    continue
                for name in sorted(names):
                    candidate = Path(current) / name
                    if not name.startswith(".") and candidate.suffix.lower() not in sidecars:
                        add(candidate)
                library.resolve_import_issue(Path(current).absolute())
                self.progress.emit(0, 0, f"Scanning folders… {len(files)} files found")
                if len(files) >= 100000:
                    self.issue(library, path, ImportProblem("scan_limit", "This folder contains too many files. Import smaller folders separately."), errors)
                    return files[:100000]
        referenced = set()
        for file in files:
            if file.suffix.lower() in DESCRIPTORS:
                try:
                    referenced.update(path.absolute() for path in content_files(file).values() if path.absolute() != file.absolute())
                except (OSError, ValueError):
                    pass  # The descriptor itself will receive a visible issue.
        return [file for file in files if file not in referenced]

    def issue(self, library, path, error, errors):
        if isinstance(error, ImportProblem):
            code, message = error.code, str(error)
        elif isinstance(error, PermissionError):
            code, message = "permission", "This file or folder cannot be read. Check its permissions, then retry."
        elif isinstance(error, FileNotFoundError):
            code, message = "missing_file", "This file is missing. Choose its new location or reconnect the drive, then retry."
        elif isinstance(error, zipfile.BadZipFile):
            code, message = "invalid_archive", "This ZIP is damaged or incomplete. Obtain a complete archive or extract a working ROM first."
        else:
            code, message = "import_error", str(error)
        library.record_import_issue(path, code, message, self.system_override)
        errors.append(f"{Path(path).name}: {message}")

    def run(self):
        try:
            self.perform()
        except Exception as error:
            self.error = str(error)
            self.result.emit(0, [self.error])

    def perform(self):
        library = Library(self.root)
        ids, errors = set(), []
        try:
            if self.restore_names:
                count, unavailable = library.restore_filenames()
                self.result.emit(count, [str(unavailable)] if unavailable else [])
                return
            if self.consolidate_ids is not None:
                for index, game_id in enumerate(self.consolidate_ids):
                    if self.isInterruptionRequested():
                        self.cancelled = True
                        break
                    row = library.get(game_id)
                    if not row:
                        continue
                    self.progress.emit(index, len(self.consolidate_ids), f"Copying {row['title']}…")
                    try:
                        library.consolidate(game_id)
                        ids.add(game_id)
                    except (OSError, ValueError) as error:
                        errors.append(f"{row['title']}: {error}")
            else:
                if library.needs_filename_restore():
                    library.restore_filenames()
                self.progress.emit(0, 0, "Scanning folders…")
                files = self.scan(library, errors)
                known = {row["id"] for row in library.games()}
                for index, file in enumerate(files):
                    if self.isInterruptionRequested():
                        self.cancelled = True
                        break
                    self.progress.emit(index, len(files), f"Importing {file.name}")
                    try:
                        imported = library.import_file(file, system_override=self.system_override)
                        for game_id in imported:
                            game = library.get(game_id)
                            if game:
                                self.imported_systems.add(game['system'])
                            if game_id in known:
                                self.duplicates += 1
                            else:
                                self.new_games += 1
                                known.add(game_id)
                            ids.add(game_id)
                        library.resolve_import_issue(file)
                    except (OSError, ValueError, RuntimeError, zipfile.BadZipFile) as error:
                        self.issue(library, file, error, errors)
                        # A ZIP may contain good entries before a damaged one.
                        # Count copies already committed instead of hiding them.
                        partial = {row[0] for row in library.db.execute("SELECT id FROM games WHERE source=?", (str(file),))}
                        self.new_games += len(partial - known)
                        ids.update(partial)
                        known.update(partial)
                    self.progress.emit(index + 1, len(files), f"Processed {index + 1} of {len(files)} files")
                self.cancelled = self.cancelled or self.isInterruptionRequested()
        finally:
            library.close()
        self.result.emit(len(ids), errors)


class ImportIssuesDialog(QDialog):
    retry = Signal(str, object)
    retry_many = Signal(list, object)

    def __init__(self, library, parent=None):
        super().__init__(parent)
        self.library = library
        self.busy = False
        self.setWindowTitle("Import issues")
        self.resize(800, 460)
        layout = QVBoxLayout(self)
        label = QLabel("Select files that need attention. For unidentified discs, choose their console below. Select multiple discs to import them for the same console. Originals are never removed.")
        label.setWordWrap(True)
        layout.addWidget(label)
        self.table = QTableWidget(0, 2)
        self.table.setHorizontalHeaderLabels(["File", "Problem"])
        self.table.horizontalHeader().setSectionResizeMode(0, QHeaderView.ResizeToContents)
        self.table.horizontalHeader().setSectionResizeMode(1, QHeaderView.Stretch)
        self.table.verticalHeader().hide()
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.table.itemSelectionChanged.connect(self.selection_changed)
        layout.addWidget(self.table, 1)
        self.detail = QLabel()
        self.detail.setWordWrap(True)
        self.detail.setTextInteractionFlags(Qt.TextSelectableByMouse)
        layout.addWidget(self.detail)
        controls = QHBoxLayout()
        self.system = QComboBox()
        self.system.addItem("Detect system automatically", None)
        for key, system in SYSTEMS.items():
            self.system.addItem(system.name, key)
        controls.addWidget(self.system)
        self.retry_button = QPushButton("Retry import")
        self.retry_button.clicked.connect(self.retry_selected)
        controls.addWidget(self.retry_button)
        self.locate_button = QPushButton("Choose replacement file…")
        self.locate_button.clicked.connect(self.choose_file)
        controls.addWidget(self.locate_button)
        self.dismiss_button = QPushButton("Dismiss")
        self.dismiss_button.clicked.connect(self.dismiss)
        controls.addWidget(self.dismiss_button)
        layout.addLayout(controls)
        close = QPushButton("Done")
        close.clicked.connect(self.close)
        layout.addWidget(close)
        self.refresh()

    def refresh(self):
        self.table.blockSignals(True)
        self.rows = self.library.import_issues()
        self.table.setRowCount(len(self.rows))
        for index, row in enumerate(self.rows):
            for column, text in enumerate((Path(row["path"]).name, row["message"])):
                item = QTableWidgetItem(text)
                item.setToolTip(row["path"] + "\n" + row["message"])
                self.table.setItem(index, column, item)
        self.table.blockSignals(False)
        if self.rows:
            self.table.selectRow(0)
        self.selection_changed()

    def selected(self):
        index = self.table.currentRow()
        return self.rows[index] if 0 <= index < len(self.rows) else None

    def selected_rows(self):
        return [self.rows[index.row()] for index in self.table.selectionModel().selectedRows()]

    def selection_changed(self):
        rows = self.selected_rows()
        row = rows[0] if rows else None
        self.detail.setText((row["path"] + "\n" + row["message"] if len(rows) == 1 else
                            f"{len(rows)} files selected. Choose one console for all selected disc games.") if row else "All import issues are resolved.")
        for widget in (self.retry_button, self.dismiss_button):
            widget.setEnabled(bool(rows) and not self.busy)
        self.locate_button.setEnabled(len(rows) == 1 and not self.busy)
        can_choose = bool(rows) and all(r['code'] in ('unknown_system', 'unknown_disc', 'invalid_rom') or
                                       (r['code'] == 'invalid_disc' and r['system']) for r in rows)
        choices = set(SYSTEMS)
        if rows and all(r['code'] == 'unknown_disc' for r in rows):
            for record in rows:
                ext = Path(record['path']).suffix.lower().lstrip('.')
                choices &= {key for key, system in SYSTEMS.items() if system.media == 'disc' and ext in system.extensions}
        self.system.clear()
        self.system.addItem('Choose console…' if row and row['code'] == 'unknown_disc' else 'Detect system automatically', None)
        for key, system in SYSTEMS.items():
            if key in choices:
                self.system.addItem(system.name, key)
        self.system.setEnabled(can_choose and not self.busy)
        self.system.setCurrentIndex(max(0, self.system.findData(row['system'] if row else None)))
        self.retry_button.setText('Import selected discs' if rows and all(r['code'] == 'unknown_disc' for r in rows) else 'Retry import')

    def set_busy(self, busy):
        self.busy = busy
        self.selection_changed()

    def retry_selected(self):
        rows = self.selected_rows()
        if not rows:
            return
        system = self.system.currentData() if self.system.isEnabled() else None
        if any(row['code'] == 'unknown_disc' for row in rows) and system is None:
            self.detail.setText('Choose the console for the selected discs before importing.')
            self.system.setFocus()
            return
        if len(rows) > 1:
            self.retry_many.emit([row['path'] for row in rows], system)
        else:
            self.retry.emit(rows[0]['path'], system)

    def choose_file(self):
        row = self.selected()
        if not row:
            return
        path, _ = QFileDialog.getOpenFileName(self, "Choose a complete ROM file")
        if path:
            # A replacement is a new import, not a hash-checked relink of a game.
            self.library.resolve_import_issue(row["path"], "replaced")
            self.retry.emit(path, self.system.currentData() if self.system.isEnabled() else None)
            self.refresh()

    def dismiss(self):
        for row in self.selected_rows():
            self.library.resolve_import_issue(row["path"], "dismissed")
        self.refresh()
