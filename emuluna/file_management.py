"""Explicit library removal and recoverable OS Trash operations."""
from contextlib import contextmanager
from pathlib import Path

from PySide6.QtCore import QFile, QLockFile


@contextmanager
def game_locks(library, game_ids):
    locks = []
    try:
        for game_id in sorted(set(game_ids)):
            if not game_id or Path(game_id).name != game_id or game_id in ('.','..'):
                raise ValueError('Invalid game identifier.')
            lock = QLockFile(str(library.root / 'saves' / (game_id + '.lock')))
            lock.setStaleLockTime(0)
            if not lock.tryLock(0):
                raise ValueError('Close the selected game before removing it or its saved media.')
            locks.append(lock)
        yield
    finally:
        for lock in reversed(locks):
            lock.unlock()


def rom_files(library, game):
    """Use the imported disc inventory; never delete an entire ROM directory."""
    main = library.root / game['rom_path']
    result = {main.absolute()}
    for row in library.content_rows(game['id']):
        relative = Path(row['path'])
        if relative.is_absolute() or '..' in relative.parts:
            raise ValueError('Invalid disc inventory. Remove the library reference while keeping files instead.')
        candidate = main.parent / relative
        # A replaced directory symlink must not lead us outside the disc set.
        if not candidate.parent.resolve().is_relative_to(main.parent.resolve()):
            raise ValueError('A disc file points outside its game folder.')
        result.add(candidate.absolute())
    return result


def media_files(library, game_ids, kind):
    selected = set(game_ids)
    root = library.root / kind
    if kind == 'states':
        candidates = (path for game_id in selected
                      for path in (root / game_id).rglob('*')
                      if path.suffix.lower() in ('.oesavestate','.png'))
    elif kind == 'screenshots':
        # Preserve screenshots with ambiguous legacy 12-character prefixes
        # unless all matching games are part of the removal selection.
        remaining = {row['id'][:12] for row in library.db.execute('SELECT id FROM games')
                     if row['id'] not in selected}
        prefixes = {game_id[:12] for game_id in selected} - remaining
        candidates = (path for path in root.glob('*.png') if path.name.split('-')[0] in prefixes)
    else:
        raise ValueError('Unknown media type.')
    return sorted({path for path in candidates if path.is_file() and not path.is_symlink()
                   and path.resolve().is_relative_to(root.resolve())})


def removal_plan(library, game_ids, *, include_roms=True):
    selected = set(game_ids)
    games = [dict(row) for row in library.db.execute('SELECT * FROM games')]
    wanted = [game for game in games if game['id'] in selected]
    candidates, protected = set(), set()
    if include_roms:
        for game in games:
            paths = rom_files(library, game)
            if game['id'] in selected:
                candidates.update(paths)
            else:
                protected.update(path.resolve() for path in paths)
    shared = {path for path in candidates if path.resolve() in protected}
    roms = sorted(path for path in candidates - shared if path.is_file() or path.is_symlink())
    if any(path.is_dir() and not path.is_symlink() for path in candidates - shared):
        raise ValueError('A ROM path is a folder. Keep its files when removing the library entry.')
    return dict(games=wanted, roms=roms, shared=sorted(shared),
                states=media_files(library, selected, 'states'),
                screenshots=media_files(library, selected, 'screenshots'))


def move_to_trash(path):
    # The static overload returns bool on some supported PySide versions,
    # despite its tuple return annotation. The instance overload consistently
    # returns bool, so a successful move cannot fail during tuple unpacking.
    file = QFile(str(path))
    if not file.moveToTrash():
        raise OSError(f'Could not move “{path.name}” to Trash. Check its permissions and available disk space.')


def trash_files(paths):
    moved = 0
    for path in dict.fromkeys(paths):
        if not path.exists() and not path.is_symlink():
            continue
        try:
            move_to_trash(path)
            moved += 1
        except OSError as error:
            detail = f' {moved} file(s) were already moved; they can be restored from Trash.' if moved else ''
            raise OSError(str(error) + detail) from error


def remove_games(library, game_ids, *, trash_roms=False, states=False, screenshots=False):
    with game_locks(library, game_ids):
        plan = removal_plan(library, game_ids, include_roms=trash_roms)
        paths = (plan['roms'] if trash_roms else []) + (plan['states'] if states else []) + (
            plan['screenshots'] if screenshots else [])
        # Retain library entries if a Trash operation fails. Never fall back to
        # unlink/rmtree, which would turn a recoverable action into deletion.
        trash_files(paths)
        ids = [(game['id'],) for game in plan['games']]
        with library.db:
            library.db.executemany('DELETE FROM artwork WHERE game_id=?', ids)
            library.db.executemany("DELETE FROM settings WHERE key LIKE ? ESCAPE '\\'",
                                   [("core\\_option." + game_id + ".%",) for game_id, in ids])
            library.db.executemany('DELETE FROM games WHERE id=?', ids)
        return plan


def remove_media(library, entry):
    with game_locks(library, [entry['game_id']]):
        path = Path(entry['path'])
        allowed = media_files(library, [entry['game_id']], entry['kind'])
        if path not in allowed:
            raise ValueError('This media file is missing or is no longer associated with the game.')
        files = [path]
        if entry['kind'] == 'states' and path.with_suffix('.png') in allowed:
            files.append(path.with_suffix('.png'))
        trash_files(files)
