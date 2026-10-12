"""Background library checks and explicit repair of indexed ROM references."""
from pathlib import Path

from PySide6.QtCore import QThread, Signal, Qt
from PySide6.QtWidgets import (QDialog, QVBoxLayout, QHBoxLayout, QLabel, QPushButton,
    QCheckBox, QTableWidget, QTableWidgetItem, QHeaderView, QAbstractItemView,
    QFileDialog, QMessageBox)

from .library import Library
from .theme import follow_system_theme, apply_theme_style
from .storage_migration import storage_operation
from .systems import SYSTEMS


@storage_operation
def audit_library(library, *, verify=False, progress=lambda message: None, cancelled=lambda: False):
    """Quick checks find missing tracks and size changes; optional hashes verify identity."""
    games = library.games()
    issues = []
    for index, game in enumerate(games):
        if cancelled():
            raise InterruptedError('Audit stopped. Run it again to check the whole library.')
        progress(f'Checking games · {index + 1} / {len(games)} · {game["title"]}')
        main = library.root / game['rom_path']
        problem = ''
        detail = ''
        try:
            if not main.is_file():
                problem = 'Missing ROM'
                detail = 'Choose its new location or reconnect the drive. The library entry and saves are still kept.'
            else:
                rows = library.content_rows(game['id'])
                for row in rows:
                    relative = Path(row['path'])
                    if relative.is_absolute() or '..' in relative.parts:
                        raise ValueError('The stored disc inventory has an invalid path.')
                    file = main if len(rows) == 1 else main.parent / relative
                    if not file.resolve().is_relative_to(main.parent.resolve()):
                        raise ValueError('A disc track points outside the game folder.')
                    if not file.is_file():
                        problem, detail = 'Missing disc files', 'Missing track: ' + row['path']
                        break
                    if file.stat().st_size != row['size']:
                        problem, detail = 'Changed disc files', 'File size changed: ' + file.name
                        break
                if verify and not problem:
                    library.validate_game(game['id'])
        except PermissionError as error:
            problem, detail = 'Unreadable file', str(error)
        except OSError as error:
            problem, detail = 'File access issue', str(error)
        except (ValueError, UnicodeError) as error:
            problem, detail = 'Changed or invalid ROM', str(error)
        if problem:
            issues.append({'game_id': game['id'], 'title': game['title'],
                'system': game['system'], 'path': str(main), 'problem': problem, 'detail': detail})
    return {'checked': len(games), 'verify': verify, 'issues': issues}


class AuditWorker(QThread):
    progress = Signal(str)
    result = Signal(object)

    def __init__(self, root, *, verify=False, repair=None, parent=None):
        super().__init__(parent)
        self.root, self.verify, self.repair = root, verify, repair

    def run(self):
        library = None
        try:
            library = Library(self.root)
            if self.repair:
                game_id, path = self.repair
                self.progress.emit('Checking the replacement file and reconnecting the game…')
                library.relink(game_id, path)
                self.result.emit({'success': True, 'repair': True})
            else:
                report = audit_library(library, verify=self.verify, progress=self.progress.emit,
                                       cancelled=self.isInterruptionRequested)
                self.result.emit({'success': True, 'report': report})
        except Exception as error:
            self.result.emit({'success': False, 'error': str(error)})
        finally:
            if library:
                library.close()


class LibraryAuditDialog(QDialog):
    changed = Signal()

    def __init__(self, library, parent=None, *, migration=False, operation="organize"):
        super().__init__(parent)
        self.library, self.migration = library, migration
        self.operation = operation
        self.worker = None
        self.report = None
        self.close_pending = False
        self.rescan_pending = False
        self.setWindowTitle(('Check library before moving' if operation == 'move' else 'Check library before organizing') if migration else 'Library audit')
        self.resize(900, 580)
        follow_system_theme(library)
        apply_theme_style(self, 'settings')
        layout = QVBoxLayout(self)
        note = QLabel('Find missing ROMs and disc tracks. Locate the original file to reconnect a game, '
                      'or remove its library entry while keeping its files and saves.' +
                      (' Resolve all issues before moving the library.' if operation == 'move' else ' Resolve all issues before organizing the library.' if migration else ''))
        note.setWordWrap(True)
        layout.addWidget(note)
        self.verify = QCheckBox('Verify file contents too (slower)')
        layout.addWidget(self.verify)
        self.status = QLabel('Ready to check the library.')
        self.status.setTextFormat(Qt.PlainText)
        self.status.setWordWrap(True)
        layout.addWidget(self.status)
        self.table = QTableWidget(0, 3)
        self.table.setHorizontalHeaderLabels(['Game', 'Console', 'Issue'])
        self.table.horizontalHeader().setSectionResizeMode(0, QHeaderView.Stretch)
        self.table.horizontalHeader().setSectionResizeMode(1, QHeaderView.ResizeToContents)
        self.table.horizontalHeader().setSectionResizeMode(2, QHeaderView.ResizeToContents)
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.table.verticalHeader().hide()
        self.table.itemSelectionChanged.connect(self.update_selection)
        layout.addWidget(self.table, 1)
        self.detail = QLabel('')
        self.detail.setWordWrap(True)
        self.detail.setTextFormat(Qt.PlainText)
        self.detail.setTextInteractionFlags(Qt.TextSelectableByMouse)
        layout.addWidget(self.detail)
        buttons = QHBoxLayout()
        self.scan = QPushButton('Scan library')
        self.scan.clicked.connect(self.start_scan)
        self.locate = QPushButton('Locate original ROM…')
        self.locate.clicked.connect(self.locate_selected)
        self.remove = QPushButton('Remove selected entries…')
        self.remove.clicked.connect(self.remove_selected)
        for button in (self.scan, self.locate, self.remove):
            buttons.addWidget(button)
        buttons.addStretch()
        self.continue_button = QPushButton(('Continue move' if operation == 'move' else 'Continue migration') if migration else 'Done')
        self.continue_button.clicked.connect(self.accept if migration else self.reject)
        buttons.addWidget(self.continue_button)
        if migration:
            cancel = QPushButton('Cancel')
            cancel.clicked.connect(self.reject)
            buttons.addWidget(cancel)
        layout.addLayout(buttons)
        self.update_selection()
        self.start_scan()

    def selected(self):
        if not self.report:
            return []
        return [self.report['issues'][index.row()] for index in self.table.selectionModel().selectedRows()]

    def update_selection(self):
        selected = self.selected()
        busy = self.worker is not None
        self.scan.setEnabled(not busy)
        self.verify.setEnabled(not busy)
        self.locate.setEnabled(not busy and len(selected) == 1)
        self.remove.setEnabled(not busy and bool(selected))
        self.continue_button.setEnabled(not busy and (not self.migration or
                                        self.report is not None and not self.report['issues']))
        if len(selected) == 1:
            self.detail.setText(selected[0]['path'] + '\n' + selected[0]['detail'])
        elif selected:
            self.detail.setText(f'{len(selected)} library entries selected.')
        else:
            self.detail.setText('')

    def start_scan(self):
        if self.worker:
            return
        self.report = None
        self.table.setRowCount(0)
        self.start_worker(AuditWorker(self.library.root, verify=self.verify.isChecked(), parent=self))

    def start_worker(self, worker):
        self.worker = worker
        worker.progress.connect(self.status.setText)
        worker.result.connect(self.receive_result)
        worker.finished.connect(self.worker_finished)
        self.update_selection()
        worker.start()

    def receive_result(self, result):
        if not result['success']:
            self.status.setText('The library check could not finish: ' + result['error'])
            return
        if result.get('repair'):
            self.changed.emit()
            self.rescan_pending = True
            return
        self.report = result['report']
        issues = self.report['issues']
        self.table.setRowCount(len(issues))
        for row, issue in enumerate(issues):
            for col, value in enumerate((issue['title'], SYSTEMS[issue['system']].name, issue['problem'])):
                self.table.setItem(row, col, QTableWidgetItem(value))
        count = self.report['checked']
        self.status.setText(f'Checked {count} games · {len(issues)} issues.' if issues else
                            f'Checked {count} games · No issues found.' +
                            ((' Ready to move the library.' if self.operation == 'move' else ' Ready to organize the library.') if self.migration else ''))
        if issues:
            self.table.selectRow(0)

    def worker_finished(self):
        self.worker.deleteLater()
        self.worker = None
        self.update_selection()
        if self.close_pending:
            super().done(QDialog.Rejected)
        elif self.rescan_pending:
            self.rescan_pending = False
            self.start_scan()

    def locate_selected(self):
        selected = self.selected()
        if len(selected) != 1 or self.worker:
            return
        issue = selected[0]
        path, _ = QFileDialog.getOpenFileName(self, 'Locate the original ROM or disc descriptor',
                                              str(Path(issue['path']).parent))
        if path:
            self.start_worker(AuditWorker(self.library.root, repair=(issue['game_id'], path), parent=self))

    def remove_selected(self):
        selected = self.selected()
        if not selected or self.worker:
            return
        answer = QMessageBox.question(self, 'Remove library entries?',
            f'Remove {len(selected)} selected entries from the library?\n\n'
            'ROM files, saved games, states and screenshots will be kept.',
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No)
        if answer != QMessageBox.Yes:
            return
        from .file_management import remove_games
        try:
            remove_games(self.library, [issue['game_id'] for issue in selected])
            self.changed.emit()
            self.start_scan()
        except (OSError, ValueError) as error:
            QMessageBox.warning(self, 'Entries could not be removed', str(error))

    def done(self, result):
        if self.worker:
            if result == QDialog.Accepted:
                return
            self.close_pending = True
            self.worker.requestInterruption()
            self.status.setText('Finishing the current file check before closing…')
            return
        if result == QDialog.Accepted and self.migration and (not self.report or self.report['issues']):
            return
        super().done(result)
