"""Game-specific defaults for verified ParaLLEl N64 compatibility problems."""
from pathlib import Path
import re


def n64_compatibility_options(rom_path, title=''):
    """1080 boots with the interpreter but stalls with both bundled recompilers."""
    try:
        with Path(rom_path).open('rb') as source:
            header = source.read(64)
        if header[:4] == b'\x37\x80\x40\x12':
            header = b''.join(header[i:i+2][::-1] for i in range(0, len(header), 2))
        elif header[:4] == b'\x40\x12\x37\x80':
            header = b''.join(header[i:i+4][::-1] for i in range(0, len(header), 4))
        name = header[32:52].decode('ascii', errors='ignore').strip(' \x00')
        identified = header[:4] == b'\x80\x37\x12\x40' and name == '1080 SNOWBOARDING'
    except OSError:
        identified = False
    if identified or re.match(r'^1080[°º]?\s+Snowboarding\b', title, re.IGNORECASE):
        return {'parallel-n64-cpucore': 'cached_interpreter'}
    return {}
