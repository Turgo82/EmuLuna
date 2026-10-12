"""Journaled, reversible moves from legacy storage to console folders."""
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import time
import uuid

from .library_paths import console_folder, game_folders, unique_rom_path


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".inventory-", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as file:
            json.dump(value, file, indent=2, ensure_ascii=False)
            file.write("\n")
            file.flush()
            os.fsync(file.fileno())
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def safe_path(root, relative):
    path = Path(relative)
    if path.is_absolute() or ".." in path.parts or not path.parts or path.parts[0] not in (
            "roms", "saves", "states", "screenshots", "consoles"):
        raise ValueError("Invalid storage migration path")
    target = root / path
    if target.is_symlink() or not target.parent.resolve().is_relative_to(root):
        raise ValueError("A storage path points outside the library")
    return target


def prune_empty(root, paths):
    for path in paths:
        parent = path.parent
        while parent != root and parent.is_relative_to(root):
            if parent.is_symlink():
                break
            try:
                parent.rmdir()
            except OSError:
                break
            parent = parent.parent


def recover_storage_migration(library):
    journal = library.root / "storage-migration.json"
    if not journal.exists():
        return
    # A separate process can open the database while a migration is underway.
    # The OS lock distinguishes a live migration from an interrupted one.
    from PySide6.QtCore import QLockFile
    lock = QLockFile(str(library.root / "storage-migration.lock"))
    lock.setStaleLockTime(0)
    if not lock.tryLock(0):
        return
    try:
        _recover(library, journal)
    finally:
        lock.unlock()


def _recover(library, journal):
    saved = json.loads(journal.read_text(encoding="utf-8"))
    if saved.get("format") != 1 or not isinstance(saved.get("id"), str) or not isinstance(saved.get("moves"), list):
        raise ValueError("The storage migration journal is invalid. Keep it for recovery.")
    for move in saved['moves']:
        if (not isinstance(move, dict) or not isinstance(move.get('source'), str)
                or not isinstance(move.get('target'), str) or move['source'] == move['target']):
            raise ValueError("The storage migration journal is invalid. Keep it for recovery.")
    pairs = [(safe_path(library.root, move["source"]), safe_path(library.root, move["target"]))
             for move in saved["moves"]]
    committed = library.setting("storage.migration_id", "") == saved["id"]
    if not committed:
        for (source, target), move in reversed(list(zip(pairs, saved["moves"]))):
            if target.exists() and "inode" in move:
                stat = target.stat()
                if [stat.st_dev, stat.st_ino] != move["inode"]:
                    raise ValueError("A migration destination was replaced. Keep the journal for recovery.")
            if source.exists() and target.exists() and source.samefile(target):
                target.unlink()
            elif source.exists() and target.exists():
                raise ValueError("A migration source was replaced. Keep the journal for recovery.")
            elif not source.exists() and target.exists():
                source.parent.mkdir(parents=True, exist_ok=True)
                target.rename(source)
        prune_empty(library.root, [target for _, target in pairs])
    else:
        prune_empty(library.root, [source for source, _ in pairs])
        library.export_inventory()
    journal.unlink()


from contextlib import contextmanager


@contextmanager
def idle_games(library, games):
    """The caller holds the migration gate, so new players cannot start.

    Probe one lock at a time rather than holding thousands of file descriptors.
    An existing player holds its own game lock for its entire lifetime.
    """
    from PySide6.QtCore import QLockFile
    for game in games:
        game_id = game['id']
        if not game_id or Path(game_id).name != game_id or game_id in ('.', '..'):
            raise ValueError('Invalid game identifier.')
        lock = QLockFile(str(library.root / 'saves' / (game_id + '.lock')))
        lock.setStaleLockTime(0)
        if not lock.tryLock(0):
            if lock.error() == QLockFile.LockFailedError:
                raise ValueError(f"Close “{game['title']}” before organizing the library.")
            if lock.error() == QLockFile.PermissionError:
                raise ValueError("The saved games folder is not writable. Check its permissions before organizing the library.")
            raise ValueError("A game lock could not be checked. Check available system resources and try again.")
        lock.unlock()
    yield


def migrate_console_storage(library, progress=lambda message: None):
    """Move only indexed managed files, retaining IDs, core builds and metadata."""
    from PySide6.QtCore import QLockFile
    from .file_management import rom_files
    lock = QLockFile(str(library.root / "storage-migration.lock"))
    lock.setStaleLockTime(0)
    if not lock.tryLock(0):
        raise ValueError("Another library migration is already running.")
    journal = library.root / "storage-migration.json"
    moves, changes, previous_stems, reserved = [], {}, {}, set()
    folders = set()
    token = uuid.uuid4().hex
    try:
        if journal.exists():
            _recover(library, journal)
        if library.console_layout():
            library.export_inventory()
            return 0
        games = library.games()
        with idle_games(library, games):
            backup = library.root / "backups" / f"library-before-console-layout-{time.time_ns()}.sqlite3"
            backup.parent.mkdir(parents=True, exist_ok=True)
            connection = sqlite3.connect(backup)
            try:
                library.db.backup(connection)
            finally:
                connection.close()
            library.db.execute("BEGIN IMMEDIATE")
            try:
                def plan(source, target):
                    source = safe_path(library.root, source.relative_to(library.root))
                    target = safe_path(library.root, target.relative_to(library.root))
                    if target.exists() or target.is_symlink() or target in reserved:
                        raise ValueError(f"Storage destination already exists: {target.name}")
                    reserved.add(target)
                    stat = source.stat()
                    moves.append({"source": str(source.relative_to(library.root)),
                                  "target": str(target.relative_to(library.root)),
                                  "inode": [stat.st_dev, stat.st_ino]})

                for game in games:
                    progress("Checking " + game["title"])
                    console = console_folder(library.root, game["system"])
                    rom = Path(game["rom_path"])
                    if not rom.is_absolute():
                        try:
                            main = library.validate_game(game["id"])
                        except (ValueError, OSError) as error:
                            raise ValueError(f"{game['title']}: {error} Run Library audit in Advanced settings to resolve it.") from error
                        rows = library.content_rows(game["id"])
                        folder = console / "roms"
                        folder.mkdir(parents=True, exist_ok=True)
                        if len(rows) > 1:
                            target_folder = unique_rom_path(folder, main.stem, game["id"])
                            if target_folder in folders or target_folder in reserved:
                                target_folder = folder / (main.stem + " [" + game["id"] + "]")
                            folders.add(target_folder)
                            target = target_folder / main.name
                            for file in sorted(rom_files(library, game)):
                                plan(file, target_folder / file.relative_to(main.parent))
                        else:
                            target = unique_rom_path(folder, main.name, game["id"])
                            if target in reserved:
                                target = unique_rom_path(folder, main.stem + " [" + game["id"][:12] + "]" + main.suffix, game["id"])
                            plan(main, target)
                        changes[game["id"]] = str(target.relative_to(library.root))
                        if target.stem != main.stem:
                            previous_stems[game["id"]] = main.stem
                    for kind in ("saves", "states"):
                        destination, old = game_folders(library.root, kind, game["id"], game["system"])
                        for file in sorted(old.rglob("*")):
                            if file.is_symlink():
                                raise ValueError("A saved game folder contains a symlink. Resolve it before migrating.")
                            if file.is_file():
                                plan(file, destination / file.relative_to(old))
                # Legacy screenshot prefixes can be ambiguous; keep those files
                # in place rather than assigning them to the wrong game.
                by_prefix = {}
                for game in games:
                    by_prefix.setdefault(game["id"][:12], []).append(game)
                for file in sorted((library.root / "screenshots").glob("*.png")):
                    matches = by_prefix.get(file.name.split("-")[0], [])
                    if len(matches) == 1:
                        game = matches[0]
                        destination = game_folders(library.root, "screenshots", game["id"], game["system"])[0]
                        plan(file, destination / file.name)
                write_json(journal, {"format": 1, "id": token, "moves": moves})
                for index, move in enumerate(moves):
                    source = safe_path(library.root, move["source"])
                    target = safe_path(library.root, move["target"])
                    progress(f"Moving files · {index + 1} / {len(moves)}")
                    target.parent.mkdir(parents=True, exist_ok=True)
                    # Hard-link first: atomically refuses overwrite, preserves
                    # bytes, and uses no second ROM-sized allocation on disk.
                    os.link(source, target)
                    source.unlink()
                for game_id, path in changes.items():
                    library.db.execute("UPDATE games SET rom_path=? WHERE id=?", (path, game_id))
                for game_id, stem in previous_stems.items():
                    library.db.execute("UPDATE games SET previous_rom_stem=? WHERE id=?", (stem, game_id))
                for key, value in (("storage.layout", "consoles"), ("storage.migration_id", token)):
                    library.db.execute("INSERT OR REPLACE INTO settings VALUES(?,?)", (key, value))
                library.db.commit()
            except BaseException:
                library.db.rollback()
                if journal.exists():
                    # If interrupted between link and unlink, both names refer
                    # to our exact inode; remove only the extra name we made.
                    for move in moves:
                        source = safe_path(library.root, move["source"])
                        target = safe_path(library.root, move["target"])
                        if source.exists() and target.exists() and source.samefile(target):
                            target.unlink()
                    _recover(library, journal)
                raise
            _recover(library, journal)
            return len(games)
    finally:
        lock.unlock()


# Imports may recursively resolve a track to its CUE. Keep one process lock
# throughout the outer operation, including file publication and JSON indexing.
def storage_operation(method):
    from functools import wraps
    @wraps(method)
    def wrapped(library, *args, **kwargs):
        from PySide6.QtCore import QLockFile
        if getattr(library, '_storage_operation_depth', 0):
            return method(library, *args, **kwargs)
        lock = QLockFile(str(library.root / 'storage-migration.lock'))
        lock.setStaleLockTime(0)
        if not lock.tryLock(0):
            raise ValueError('The library is being organized. Try again when it finishes.')
        library._storage_operation_depth = 1
        try:
            return method(library, *args, **kwargs)
        finally:
            library._storage_operation_depth = 0
            lock.unlock()
    return wrapped


from PySide6.QtCore import QThread, Signal


class StorageWorker(QThread):
    progress = Signal(str)
    result = Signal(str)

    def __init__(self, root, parent=None):
        super().__init__(parent)
        self.root = root

    def run(self):
        from .library import Library
        library = None
        try:
            library = Library(self.root)
            count = migrate_console_storage(library, self.progress.emit)
            self.result.emit(f'Library organized by console · {count} games. A database backup was kept in backups.')
        except Exception as error:
            self.result.emit('The library could not be organized: ' + str(error))
        finally:
            if library:
                library.close()
