"""Validate and preserve complete local disc sets without changing filenames."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import shlex
import tempfile

from .hashing import file_hexdigest

DESCRIPTORS = {'.cue', '.ccd', '.gdi', '.m3u'}
DISC_FILES = DESCRIPTORS | {'.chd', '.cdi', '.iso', '.cso', '.pbp'}
MAX_DISC = 8 * 1024**3


def gdi_tracks(text):
    """Validate GDI track rows, including quoted filenames and byte offsets."""
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    try:
        count = int(lines[0])
        if not 1 <= count <= 99 or len(lines) != count + 1:
            raise ValueError
        tracks = []
        for number, line in enumerate(lines[1:], 1):
            fields = shlex.split(line)
            if len(fields) != 6:
                raise ValueError
            index, lba, mode, stride = map(int, fields[:4])
            offset = int(fields[5])
            if (index != number or min(lba, offset) < 0 or mode not in (0, 4)
                    or stride not in (2048, 2352) or not fields[4]):
                raise ValueError
            tracks.append((fields[4], mode, offset))
        if not any(mode == 4 for _, mode, _ in tracks):
            raise ValueError
        return tracks
    except (ValueError, IndexError) as error:
        raise ValueError('Invalid GDI descriptor: check its track count, filenames and data tracks.') from error


def references(path):
    path = Path(path)
    suffix = path.suffix.lower()
    if suffix not in DESCRIPTORS:
        return []
    if path.stat().st_size > 1024**2:
        raise ValueError('The disc descriptor exceeds 1 MiB.')
    text = path.read_text(encoding='utf-8-sig')
    if suffix == '.gdi':
        return [name for name, _, _ in gdi_tracks(text)]
    if suffix == '.cue':
        names = re.findall(r'^\s*FILE\s+(?:"([^"]+)"|(\S+))\s+\S+\s*$', text, re.I | re.M)
        if not names or not re.search(r'^\s*TRACK\s+\d+\s+', text, re.I | re.M) or not re.search(r'^\s*INDEX\s+01\s+', text, re.I | re.M):
            raise ValueError('Invalid CUE sheet: FILE, TRACK and INDEX 01 entries are required.')
        return [a or b for a, b in names]
    if suffix == '.ccd':
        if '[CloneCD]' not in text:
            raise ValueError('Invalid CCD descriptor: missing CloneCD header.')
        names = [path.with_suffix('.img').name]
        if path.with_suffix('.sub').is_file():
            names.append(path.with_suffix('.sub').name)
        return names
    names = [line.strip() for line in text.splitlines() if line.strip() and not line.lstrip().startswith('#')]
    if not names:
        raise ValueError('The disc playlist is empty.')
    if any(Path(name).suffix.lower() not in DISC_FILES for name in names):
        raise ValueError('A playlist must reference CUE, CCD, GDI, CHD, CDI, ISO or another supported disc file.')
    return names


def content_files(path):
    path = Path(path).absolute()
    root = path.parent.resolve()
    files, visiting = {}, set()
    def visit(current, depth=0):
        if depth > 8 or len(files) >= 256:
            raise ValueError('This disc set has too many files or nested playlists.')
        resolved = current.resolve()
        if not resolved.is_relative_to(root):
            raise ValueError('Disc references must stay in the game folder. Keep the disc set together and use relative paths.')
        if resolved in visiting:
            raise ValueError('The disc playlist contains a reference cycle.')
        relative = current.relative_to(path.parent).as_posix()
        if relative in files:
            return
        if not current.is_file():
            raise ValueError('Missing referenced file: ' + relative)
        if not 0 < current.stat().st_size <= MAX_DISC:
            raise ValueError('Disc file is empty or exceeds 8 GiB: ' + relative)
        files[relative] = current
        visiting.add(resolved)
        for name in references(current):
            name = name.replace('\\', '/')
            if Path(name).is_absolute() or '..' in Path(name).parts or ':' in name:
                raise ValueError('Use local relative filenames in the disc descriptor: ' + name)
            visit(current.parent / name, depth + 1)
        visiting.remove(resolved)
    visit(path)
    if sum(file.stat().st_size for file in files.values()) > 32 * 1024**3:
        raise ValueError('The disc set exceeds 32 GiB.')
    return files


def inventory(path):
    rows = []
    for relative, file in content_files(path).items():
        with file.open('rb') as stream:
            digest = file_hexdigest(stream, 'sha256')
        rows.append({'path': relative, 'sha256': digest, 'size': file.stat().st_size})
    return rows


def content_id(rows):
    if len(rows) == 1:
        return rows[0]['sha256']
    return hashlib.sha256(json.dumps(sorted(rows, key=lambda row: row['path']), sort_keys=True).encode()).hexdigest()


def copy_content(source, folder, rows):
    source, folder = Path(source), Path(folder)
    folder.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.disc-', dir=folder.parent) as tmp:
        staged = Path(tmp) / 'content'
        staged.mkdir()
        for row in rows:
            target = staged / row['path']
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source.parent / row['path'], target)
            with target.open('rb') as file:
                if file_hexdigest(file, 'sha256') != row['sha256']:
                    raise ValueError('A disc file changed while copying. Close programs editing it, then retry.')
        if folder.exists():
            # Removing a game can leave its folder behind after files go to
            # Trash. Verify every remaining copy before restoring missing files.
            missing = []
            for row in rows:
                target = folder / row['path']
                if not target.parent.resolve().is_relative_to(folder.resolve()):
                    raise ValueError('An existing managed disc folder points outside its game folder.')
                if not target.exists() and not target.is_symlink():
                    missing.append(row)
                    continue
                if not target.is_file():
                    raise ValueError('An existing managed disc file is unreadable. Restore that copy or choose another library location.')
                with target.open('rb') as file:
                    if file_hexdigest(file, 'sha256') != row['sha256']:
                        raise ValueError('An existing managed disc copy differs. Choose another library location or restore that copy.')
            for row in missing:
                target = folder / row['path']
                target.parent.mkdir(parents=True, exist_ok=True)
                # Publish the verified staged copy without overwriting a file
                # that another import might have created in the meantime.
                os.link(staged / row['path'], target)
        else:
            staged.rename(folder)
    return folder / source.name
