"""Read small decoded CHD track samples using the bundled, isolated helper."""
import os
from pathlib import Path
import struct
import subprocess

PROBE_LIMIT = 8 * 1024 * 1024


def track_samples(path):
    directory = Path(os.environ.get('EMULUNA_CORE_DIR', Path(__file__).resolve().parents[1] / 'build/cores'))
    helper = directory / 'emuluna-chd-probe'
    if not helper.is_file():
        return []  # Source install without the reader: retain manual selection.
    try:
        result = subprocess.run([str(helper), os.fsencode(path)], capture_output=True, timeout=8)
    except (OSError, subprocess.TimeoutExpired):
        return []
    if result.returncode or len(result.stdout) > PROBE_LIMIT:
        return []  # Unsupported, damaged, parent-dependent or unreadable container.
    samples, position = [], 0
    while position < len(result.stdout):
        if position + 8 > len(result.stdout):
            return []
        size, stride = struct.unpack_from('>II', result.stdout, position)
        position += 8
        if stride not in (2048, 2324, 2336, 2352) or not size or size % stride or position + size > len(result.stdout):
            return []
        samples.append(result.stdout[position:position + size])
        position += size
    return samples
