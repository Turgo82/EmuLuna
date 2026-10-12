"""1080's interpreter workaround is scoped to the game across ROM byte orders."""
from pathlib import Path
import tempfile
import unittest
from emuluna.n64_compatibility import n64_compatibility_options


class N64CompatibilityTests(unittest.TestCase):
    def test_cartridge_identification_works_even_with_a_renamed_game(self):
        header = bytearray(64)
        header[:4] = b'\x80\x37\x12\x40'
        header[32:52] = b'1080 SNOWBOARDING    '
        with tempfile.TemporaryDirectory() as temp:
            rom = Path(temp)/'renamed.rom'
            for step in (1, 2, 4):
                data = bytes(header) if step == 1 else b''.join(
                    header[i:i+step][::-1] for i in range(0, len(header), step))
                rom.write_bytes(data)
                self.assertEqual(n64_compatibility_options(rom, 'Renamed game'),
                                 {'parallel-n64-cpucore': 'cached_interpreter'})

    def test_title_fallback_and_unrelated_games(self):
        for title in ('1080 Snowboarding (Japan, USA)', '1080° Snowboarding', '1080º Snowboarding'):
            self.assertEqual(n64_compatibility_options('/missing', title),
                             {'parallel-n64-cpucore': 'cached_interpreter'})
        for title in ('Super Mario 64', 'Snowboard Kids', '1080p Demo', '1080 SnowboardingExtra'):
            self.assertEqual(n64_compatibility_options('/missing', title), {})


if __name__ == '__main__':
    unittest.main()
