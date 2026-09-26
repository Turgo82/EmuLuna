"""Regression: official Nestopia and FCEUmm must render beyond first frame."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from core_fixture import core_options
from nes_rom import nes


class NesIntegration(unittest.TestCase):
    def test_official_cores_video_audio_input_and_states(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            rom = root / 'NES diagnostic.nes'
            rom.write_bytes(nes())
            for core_id in ('nestopia', 'fceumm'):
                options = core_options(core_id)
                script = '''
from pathlib import Path
from emuluna.core import Core
import sys
rom, binary, core_id, save = sys.argv[1:]
with Core(rom, 'nes', save, libretro_path=binary, core_id=core_id) as core:
    for i in range(180): baseline, audio = core.frame()
    assert core.width == 256 and core.height == 224
    assert len(set(audio)) > 10, 'No pulse tone'
    # Hold each independently, then together. A must be red, B green, both
    # yellow; stable frames also catch accidentally using a turbo button.
    for keys in (1,2,3):
        frames = []
        for i in range(40):
            pressed, _ = core.frame(keys)
            if i > 10: frames.append(pressed)
        assert all(frame == frames[0] for frame in frames), 'Unexpected turbo'
        b,g,r,a = pressed[10000:10004]
        assert (r > 2*g and r > 2*b) if keys == 1 else ((g > 2*r and g > 2*b) if keys == 2 else (r > 2*b and g > 2*b)), (keys,b,g,r)
    for i in range(10): released, _ = core.frame()
    assert released == baseline
    state = Path(save) / 'nes.state'
    core.save_state(state)
    for i in range(5): core.frame(1)
    core.load_state(state)
    for i in range(10): restored, _ = core.frame()
    assert restored == baseline
'''
                with self.subTest(core=core_id):
                    result = subprocess.run([sys.executable, '-c', script, str(rom), str(options['libretro_path']), core_id, str(root/core_id)], capture_output=True, timeout=20)
                    self.assertEqual(result.returncode, 0, (result.stdout+result.stderr).decode(errors='replace')[-2000:])
