"""Real standard cores for integration checks; never use removed adapters."""
import os
from pathlib import Path
import unittest
from emuluna.core_manager import CoreManager


def core_options(core_id='snes9x'):
    root = os.environ.get('EMULUNA_TEST_LIBRETRO_DIR')
    if not root:
        raise unittest.SkipTest('Set EMULUNA_TEST_LIBRETRO_DIR to a library with downloaded standard cores')
    manager = CoreManager(root)
    record = manager.installed()[core_id]
    return {'libretro_path': manager.path(record), 'core_id': core_id}


def install_core(library, core_id='snes9x'):
    options = core_options(core_id)
    return CoreManager(library.root).install_bytes(options['libretro_path'].read_bytes(), core_id)
