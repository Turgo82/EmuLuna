"""Review game removal separately from ROM and saved-media disposal."""
from PySide6.QtWidgets import (QDialog, QVBoxLayout, QLabel, QRadioButton,
    QDialogButtonBox, QPlainTextEdit)
from .settings_style import SettingsCheckBox as QCheckBox


class GameRemovalDialog(QDialog):
    def __init__(self, plan, parent=None, *, trash_roms=False):
        super().__init__(parent)
        self.setWindowTitle('Remove games')
        self.resize(530,420)
        layout = QVBoxLayout(self)
        title = (f'Remove “{plan["games"][0]["title"]}” from the library?'
                 if len(plan['games']) == 1 else f'Remove {len(plan["games"])} games from the library?')
        label = QLabel(title); label.setWordWrap(True); layout.addWidget(label)
        self.keep = QRadioButton('Keep ROM files')
        self.trash = QRadioButton('Move ROM files to Trash')
        self.keep.setChecked(not trash_roms); self.trash.setChecked(trash_roms)
        layout.addWidget(self.keep); layout.addWidget(self.trash)
        if plan.get('rom_error'):
            self.keep.setChecked(True)
            self.trash.setEnabled(False)
            problem=QLabel('ROM files must be kept: '+plan['rom_error'])
            problem.setWordWrap(True); layout.addWidget(problem)
        self.states = QCheckBox('Also move save states and their previews to Trash')
        self.screenshots = QCheckBox('Also move screenshots to Trash')
        self.states.setEnabled(bool(plan['states']))
        self.screenshots.setEnabled(bool(plan['screenshots']))
        layout.addWidget(self.states); layout.addWidget(self.screenshots)
        note = QLabel('Battery / in-game saves are kept. Files moved to Trash can be restored using your file manager.')
        note.setWordWrap(True); layout.addWidget(note)
        if plan['shared']:
            shared = QLabel(f'{len(plan["shared"])} shared ROM file(s) will be kept for other library games.')
            shared.setWordWrap(True); layout.addWidget(shared)
        self.details = QPlainTextEdit()
        self.details.setReadOnly(True)
        self.details.setAccessibleName('Files selected for Trash')
        self.details.setMaximumHeight(170)
        layout.addWidget(self.details)
        buttons = QDialogButtonBox(QDialogButtonBox.Cancel)
        self.remove_button = buttons.addButton('Remove from library', QDialogButtonBox.AcceptRole)
        buttons.rejected.connect(self.reject); buttons.accepted.connect(self.accept)
        buttons.button(QDialogButtonBox.Cancel).setDefault(True)
        layout.addWidget(buttons)
        def preview():
            paths = (plan['roms'] if self.trash.isChecked() else []) + (
                plan['states'] if self.states.isChecked() else []) + (
                plan['screenshots'] if self.screenshots.isChecked() else [])
            self.details.setPlainText('Files to move to Trash:\n' + '\n'.join(map(str,paths))
                                      if paths else 'No files will be moved or deleted.')
            self.remove_button.setText('Remove and move files to Trash' if paths else 'Remove from library')
        for option in (self.trash,self.states,self.screenshots): option.toggled.connect(preview)
        preview()
