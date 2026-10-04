"""Bounded disc-header inspection; never infer a console from a game's name."""
from pathlib import Path
import re
from .content import content_files, references, gdi_tracks

PROBE_BYTES = 2 * 1024 * 1024


def companion_cue(path):
    """Selecting a BIN track should import its existing descriptor and track set."""
    path = Path(path)
    matches = []
    for candidate in path.parent.iterdir():
        if candidate.suffix.lower() != '.cue':
            continue
        try:
            names = references(candidate)
        except (ValueError, UnicodeError):
            continue
        if any((candidate.parent / name.replace('\\', '/')).resolve() == path.resolve() for name in names):
            matches.append(candidate)
    if len(matches) > 1:
        raise ValueError('Several CUE sheets reference this track. Import the CUE sheet for the desired disc instead of the BIN file.')
    return matches[0] if matches else None


def data_tracks(path):
    """Yield track path and byte offset, honoring CUE INDEX 01 pregaps."""
    path = Path(path)
    suffix = path.suffix.lower()
    if suffix == '.m3u':
        for name in references(path):
            yield from data_tracks(path.parent / name.replace('\\', '/'))
    elif suffix == '.cue':
        current = mode = None
        for line in path.read_text(encoding='utf-8-sig').splitlines():
            file = re.match(r'\s*FILE\s+(?:"([^"]+)"|(\S+))\s+', line, re.I)
            track = re.match(r'\s*TRACK\s+\d+\s+(\S+)', line, re.I)
            index = re.match(r'\s*INDEX\s+01\s+(\d+):(\d+):(\d+)', line, re.I)
            if file:
                current = path.parent / (file[1] or file[2]).replace('\\', '/')
            elif track:
                mode = track[1].upper()
            elif index and current and mode and mode.startswith('MODE'):
                stride = int(mode.split('/')[-1]) if '/' in mode else 2352
                if stride not in (2048, 2336, 2352):
                    continue
                minutes, seconds, frames = map(int, index.groups())
                if seconds >= 60 or frames >= 75:
                    raise ValueError('Invalid CUE INDEX time. Seconds must be below 60 and frames below 75.')
                yield current, ((minutes * 60 + seconds) * 75 + frames) * stride
    elif suffix == '.ccd':
        yield path.with_suffix('.img'), 0
    elif suffix == '.gdi':
        for name, mode, offset in gdi_tracks(path.read_text(encoding='utf-8-sig')):
            if mode == 4:
                yield path.parent / name.replace('\\', '/'), offset
    else:
        yield path, 0


def probe_track(path, offset=0):
    with Path(path).open('rb') as stream:
        stream.seek(offset)
        data = stream.read(PROBE_BYTES)
    if data.startswith(b'ECM\0'):
        raise ValueError('This track uses ECM compression. Decompress it first and import its CUE sheet.')
    if Path(path).suffix.lower() == '.chd':
        if len(data) < 16 or data[:8] != b'MComprHD':
            raise ValueError('Invalid CHD header. Choose a complete CHD image.')
        header_size, version = int.from_bytes(data[8:12], 'big'), int.from_bytes(data[12:16], 'big')
        if version not in (3, 4, 5) or header_size != {3:120, 4:108, 5:124}[version] or len(data) < header_size:
            raise ValueError('Unsupported or incomplete CHD header. Use a complete CHD version 3, 4 or 5 image.')
        from .chd import track_samples
        found = set()
        for sample in track_samples(path):
            found.update(probe_bytes(sample))
        return found
    return probe_bytes(data)


def probe_bytes(data):
    """Identify decoded sector contents; filenames and compressed bytes are irrelevant."""
    found = set()
    for start in (0, 16, 24):
        header = data[start:start+16]
        if header.startswith((b'SEGADISCSYSTEM', b'SEGABOOTDISC', b'SEGADATADISC', b'SEGADISC ')):
            found.add('segacd')
        if header.startswith(b'SEGA SEGASATURN'):
            found.add('saturn')
        if header == b'SEGA SEGAKATANA ':
            found.add('dreamcast')
    # PCE has no universal header offset. Limit reads instead of loading a CD
    # into memory. Exclude the known PC-FX signature collision (Battle Heat).
    marker = data.find(b'PC Engine CD-ROM SYSTEM')
    if marker >= 0 and data[marker+74:marker+85] != b'Battle Heat':
        found.add('pcecd')
    # ISO 9660 system identifiers for cooked and raw-sector images.
    for stride, skip in ((2048,0), (2352,16), (2352,24), (2336,8)):
        base = 16 * stride + skip
        if data[base+1:base+6] != b'CD001':
            continue
        system = data[base+8:base+40].rstrip(b' \0')
        if system == b'PSP GAME':
            found.add('psp')
        # PLAYSTATION alone also appears on PS2 discs; use PS1's licensing
        # sector as corroboration, not the shared volume identifier alone.
        if system == b'PLAYSTATION' and b'  Licensed  by  ' in data[:32768] and b'BOOT2' not in data:
            found.add('psx')
    # Original PS1 raw dumps may lack a readable primary volume descriptor.
    if data[0x24e0:0x24f0] == b'  Licensed  by  ' and b'BOOT2' not in data:
        found.add('psx')
    return found


def detect_disc(path):
    files = content_files(path)  # Validate the entire set before opening any references.
    found = ({'dreamcast'} if any(Path(name).suffix.lower() in ('.gdi', '.cdi')
                                 for name in files) else set())
    tracks = list(data_tracks(path))
    if not tracks:
        raise ValueError('No data track found in this CUE sheet. Choose the game disc descriptor, not an audio-only disc.')
    for track, offset in tracks:
        if offset >= track.stat().st_size:
            raise ValueError('CUE INDEX points beyond its track file: ' + track.name)
        found.update(probe_track(track, offset))
    if len(found) > 1:
        raise ValueError('This disc set contains signatures for different consoles. Import each console’s discs separately.')
    return next(iter(found), None)
