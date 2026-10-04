"""Firmware requirements and safe user-supplied imports from core metadata."""
import hashlib
import os
from pathlib import Path
import tempfile

from .core import CoreError
from .hashing import file_hexdigest
from .systems import CATALOG, SYSTEMS

MAX_BIOS = 64 * 1024 * 1024


def requirements(core_id):
    return CATALOG.get(core_id, {}).get('firmware', [])


def checklist(core_systems):
    """One row per file/checksum, with only its relevant consoles and cores."""
    files = {}
    for system, core_id in core_systems:
        for entry in requirements(core_id):
            if system not in entry.get('systems', CATALOG[core_id]['systems']):
                continue
            key = (entry['path'], entry.get('md5'))
            row = files.setdefault(key, {'entry': entry, 'uses': [],
                                         'requirements': [], 'essential': False})
            use = (system, core_id)
            if use not in row['uses']:
                row['uses'].append(use)
            requirement = entry.get('requirement', 'Optional' if entry['optional'] else 'Required')
            if requirement not in row['requirements']:
                row['requirements'].append(requirement)
            row['essential'] |= (not entry['optional'] or
                                 system in entry.get('required_systems', []) or
                                 requirement.startswith('Required') or entry.get('recommended', False))
    return sorted(files.values(), key=lambda row: (
        not row['essential'], min(SYSTEMS[system].name.casefold() for system, _ in row['uses']),
        row['entry']['path'].casefold()))


def bios_status(directory, entry):
    root = Path(directory).resolve()
    path = (root / entry['path']).resolve()
    if not path.is_relative_to(root):
        return 'Invalid path', False
    if not path.is_file():
        return 'Missing', False
    try:
        if not 0 < path.stat().st_size <= MAX_BIOS:
            return 'Invalid size', False
        if entry.get('md5'):
            with path.open('rb') as file:
                digest = file_hexdigest(file, 'md5')
            if digest != entry['md5']:
                return 'Checksum mismatch', False
            return 'Verified', True
        return 'Present (checksum unknown)', True
    except OSError:
        return 'Unreadable', False


def validate_bios(core_id, system, directory):
    missing, groups = [], {}
    for entry in requirements(core_id):
        required = system in entry.get('required_systems', []) or not entry['optional']
        if not required:
            continue
        status, valid = bios_status(directory, entry)
        if group := entry.get('group'):
            groups.setdefault(group, []).append((entry['path'], valid))
        elif not valid:
            missing.append(entry['path'] + ' (' + status.lower() + ')')
    for entries in groups.values():
        if not any(valid for _, valid in entries):
            missing.append('one of ' + ', '.join(path for path, _ in entries))
    if missing:
        raise CoreError('BIOS required: ' + '; '.join(missing) + '. Open Settings → System Files to import your files.')


def import_bios(source, directory, entry):
    root = Path(directory).resolve()
    target = (root / entry['path']).resolve()
    if not target.is_relative_to(root):
        raise CoreError('Invalid BIOS destination.')
    source = Path(source)
    with source.open('rb') as file:
        payload = file.read(MAX_BIOS + 1)
    if not 0 < len(payload) <= MAX_BIOS:
        raise CoreError('The BIOS file is empty or exceeds 64 MiB.')
    if entry.get('md5') and hashlib.md5(payload, usedforsecurity=False).hexdigest() != entry['md5']:
        raise CoreError('This file does not match the required checksum. The existing BIOS was kept.')
    if target.exists():
        if target.read_bytes() == payload:
            return target
        raise CoreError('A different file already uses this BIOS filename. Keep a backup and move it out of the BIOS folder before importing its replacement.')
    target.parent.mkdir(parents=True, exist_ok=True)
    fd, temp = tempfile.mkstemp(dir=target.parent)
    try:
        with os.fdopen(fd, 'wb') as file:
            file.write(payload)
            file.flush()
            os.fsync(file.fileno())
        # Publish without overwriting a concurrent import.
        os.link(temp, target)
    finally:
        Path(temp).unlink(missing_ok=True)
    return target
