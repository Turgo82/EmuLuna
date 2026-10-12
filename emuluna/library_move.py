"""Verified console-folder relocation while keeping the index and logical paths stable."""
import ctypes
import errno
import hashlib
import json
import os
from pathlib import Path
import shutil
import uuid

from PySide6.QtCore import QThread, Signal, QLockFile

from .hashing import file_hexdigest
from .library_paths import console_root
from .storage_migration import write_json, storage_operation, idle_games, migrate_console_storage


def _owned_destination(record):
    destination = Path(record['destination'])
    marker = destination / '.emuluna-move-id'
    return marker.is_file() and marker.read_text() == record['id']


def _backup(library, record):
    return library.root / ('.consoles-before-move-' + record['id'])


def _remove_old(library, record):
    backup = _backup(library, record)
    if not backup.exists() and not backup.is_symlink():
        return
    old = backup.resolve() if backup.is_symlink() else backup
    if backup.is_symlink():
        previous = record.get('previous')
        if not previous or str(old) != previous.get('path'):
            raise ValueError('The previous library location changed; its files were kept.')
    if not old.exists() and record.get('cleanup_started'):
        backup.unlink(missing_ok=True)
        return
    # Do not discard files edited or added outside EmuLuna during the copy.
    cleanup_started = record.get('cleanup_started', False)
    actual = set()
    for file in old.rglob('*'):
        if file.is_symlink():
            raise ValueError('The old library changed during the move; its files were kept.')
        if file.is_file():
            relative = file.relative_to(old).as_posix()
            actual.add(relative)
            expected = record['files'].get(relative)
            if not expected or [file.stat().st_size, file.stat().st_mtime_ns] != expected:
                raise ValueError('The old library changed during the move; its files were kept.')
    if not cleanup_started and actual != set(record['files']):
        raise ValueError('The old library changed during the move; its files were kept.')
    record['cleanup_started'] = True
    write_json(library.root / 'console-move.json', record)
    shutil.rmtree(old)
    if backup.is_symlink():
        backup.unlink()


def _recover(library):
    journal = library.root / 'console-move.json'
    if not journal.exists():
        return
    record = json.loads(journal.read_text(encoding='utf-8'))
    if (record.get('format') != 1 or len(record.get('id', '')) != 32
            or any(c not in '0123456789abcdef' for c in record['id'])):
        raise ValueError('Invalid console move journal. Keep it for recovery.')
    pointer = library.root / 'consoles'
    location = library.root / 'console-location.json'
    current = json.loads(location.read_text()) if location.exists() else {}
    committed = current.get('move_id') == record['id']
    destination = Path(record['destination'])
    stage = Path(record['staging'])
    if (not destination.is_absolute() or stage.parent != destination.parent
            or stage.name != '.emuluna-console-copy-' + record['id']):
        raise ValueError('Invalid console move destination.')
    backup = _backup(library, record)
    if committed:
        if (not pointer.is_symlink() or pointer.resolve() != destination
                or not destination.is_dir()
                or (not _owned_destination(record) and (backup.exists() or backup.is_symlink()))):
            raise ValueError('The new console library is unavailable. Reconnect it before cleanup.')
        try:
            _remove_old(library, record)
        except (OSError, ValueError) as error:
            # A working new library must still open if old copies cannot be cleaned.
            journal.rename(library.root / ('console-move-retained-' + record['id'] + '.json'))
            (destination / '.emuluna-move-id').unlink(missing_ok=True)
            return 'Library moved to ' + str(destination) + '. Previous copies were kept: ' + str(error)
        journal.unlink()
        (destination / '.emuluna-move-id').unlink(missing_ok=True)
        return
    else:
        if backup.exists() or backup.is_symlink():
            if pointer.is_symlink() and pointer.resolve() == destination:
                pointer.unlink()
            if pointer.exists() or pointer.is_symlink():
                raise ValueError('The original console path changed. Keep the move journal for recovery.')
            backup.rename(pointer)
        if record.get('previous'):
            write_json(location, record['previous'])
        else:
            location.unlink(missing_ok=True)
        if _owned_destination(record):
            shutil.rmtree(destination)
        if stage.is_dir() and not stage.is_symlink():
            shutil.rmtree(stage)
    journal.unlink()


def _publish(stage, destination):
    """Linux atomic rename without replacing a concurrently created folder."""
    libc = ctypes.CDLL(None, use_errno=True)
    rename = libc.renameat2
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(-100, os.fsencode(stage), -100, os.fsencode(destination), 1):
        error = ctypes.get_errno()
        if error == errno.EEXIST:
            raise ValueError('The destination changed during the move. The original library was kept.')
        raise OSError(error, os.strerror(error), str(destination))


def recover_console_move(library):
    if not (library.root / 'console-move.json').exists():
        return
    gate = QLockFile(str(library.root / 'storage-migration.lock'))
    gate.setStaleLockTime(0)
    if not gate.tryLock(0):
        return
    try:
        _recover(library)
    finally:
        gate.unlock()


@storage_operation
def move_console_library(library, parent, progress=lambda message: None):
    """Copy/verify across filesystems, then atomically switch the folder pointer."""
    if not library.console_layout():
        raise ValueError('Organize the existing library before moving its console folder.')
    parent = Path(parent).expanduser().resolve()
    if not parent.is_dir():
        raise ValueError('Choose an existing destination folder.')
    source = console_root(library.root).resolve()
    destination = parent / 'consoles'
    if source == destination:
        return False
    if destination.exists() or destination.is_symlink():
        raise ValueError('This destination already contains a consoles folder. Choose another folder to avoid overwriting it.')
    if destination.is_relative_to(source) or source.is_relative_to(destination):
        raise ValueError('Choose a destination outside the current console folder.')
    # Keep application data and its index in place; only game storage moves.
    if destination.is_relative_to(library.root):
        raise ValueError('Choose a destination outside the application data folder.')
    if (library.root / 'consoles').is_symlink() and not source.is_dir():
        raise ValueError('The console library is disconnected. Reconnect its drive before moving it.')
    source.mkdir(parents=True, exist_ok=True)
    token = uuid.uuid4().hex
    stage = parent / ('.emuluna-console-copy-' + token)
    location = library.root / 'console-location.json'
    previous = json.loads(location.read_text()) if location.exists() else None
    record = {'format':1, 'id':token, 'destination':str(destination), 'staging':str(stage),
              'previous':previous, 'files':{}}
    journal = library.root / 'console-move.json'
    with idle_games(library, library.games()):
        for game in library.games():
            path = Path(game['rom_path'])
            if path.is_absolute() and path.resolve().is_relative_to(source):
                raise ValueError('Consolidate “' + game['title'] + '” into the library before moving it.')
        files = sorted(source.rglob('*'))
        if any(file.is_symlink() for file in files):
            raise ValueError('The console folder contains a symlink. Resolve it before moving the library.')
        marker = source / '.emuluna-move-id'
        if marker.exists():
            if previous and marker.read_text() == previous.get('move_id'):
                marker.unlink()
                files.remove(marker)
            else:
                raise ValueError('The console folder contains a reserved move marker. Keep it for recovery.')
        for file in files:
            if not file.is_file() and not file.is_dir():
                raise ValueError('The console folder contains an unsupported file type.')
            if file.is_file():
                record['files'][file.relative_to(source).as_posix()] = [file.stat().st_size, file.stat().st_mtime_ns]
        write_json(journal, record)
        try:
            stage.mkdir()
            (stage / '.emuluna-move-id').write_text(token)
            for index, file in enumerate(files):
                target = stage / file.relative_to(source)
                if file.is_dir():
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                progress(f'Copying console files · {index + 1} / {len(files)} · {file.name}')
                target.parent.mkdir(parents=True, exist_ok=True)
                digest = hashlib.sha256()
                with file.open('rb') as original, target.open('xb') as output:
                    for block in iter(lambda: original.read(1024 * 1024), b''):
                        output.write(block)
                        digest.update(block)
                    output.flush()
                    os.fsync(output.fileno())
                with target.open('rb') as copied:
                    if file_hexdigest(copied, 'sha256') != digest.hexdigest():
                        raise ValueError('A copied file failed verification. The original library was kept.')
                shutil.copystat(file, target)
            current_files = list(source.rglob('*'))
            actual = {file.relative_to(source).as_posix(): [file.stat().st_size, file.stat().st_mtime_ns]
                      for file in current_files if file.is_file()}
            if any(file.is_symlink() for file in current_files) or actual != record['files']:
                raise ValueError('Source files changed during the move. The original library was kept.')
            if destination.exists() or destination.is_symlink():
                raise ValueError('The destination changed during the move. The original library was kept.')
            _publish(stage, destination)
            (library.root / 'consoles').rename(_backup(library, record))
            (library.root / 'consoles').symlink_to(destination, target_is_directory=True)
            write_json(location, {'format':1, 'path':str(destination), 'move_id':token})
        except BaseException:
            _recover(library)
            raise
        progress('Finishing the library move…')
        warning = _recover(library)
        return warning or True


class LibraryMoveWorker(QThread):
    progress = Signal(str)
    result = Signal(str)

    def __init__(self, root, destination, parent=None):
        super().__init__(parent)
        self.root, self.destination = root, destination
        self.succeeded = False

    def run(self):
        from .library import Library
        library = None
        try:
            library = Library(self.root)
            if not library.console_layout():
                migrate_console_storage(library, self.progress.emit)
            moved = move_console_library(library, self.destination, self.progress.emit)
            self.succeeded = True
            self.result.emit(moved if isinstance(moved, str) else
                             'Library moved to ' + str(Path(self.destination) / 'consoles') if moved else
                             'The library already uses this folder.')
        except Exception as error:
            self.result.emit('The library could not be moved: ' + str(error))
        finally:
            if library:
                library.close()
