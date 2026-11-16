#!/usr/bin/env python3
"""Live desktop/audio test using an original CC0 NES diagnostic and disposable saves.

Run from any directory. Requires a built Rust workspace and native host, an
installed Nestopia core, and access to the desktop and audio device. No library
or existing saves are changed. The device receives silence (--mute).
"""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests"))
from nes_rom import nes


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--core", type=Path, required=True, help="Compatible libretro shared library")
    parser.add_argument("--diagnostic", choices=("nes", "snes", "snes-pal", "gb"), default="nes")
    parser.add_argument("--overlay", action="store_true", help="Exercise native gameplay controls during playback")
    parser.add_argument("--preset", help="Optional bundled preset ID")
    parser.add_argument("--shader-pack",type=Path,default=ROOT / "build/rust/shader-pack")
    parser.add_argument("--host", type=Path, default=ROOT / "build/cores/libemuluna_host.so")
    args = parser.parse_args()
    for path in (args.core, args.host):
        if not path.is_file():
            parser.error(f"Missing file: {path}")
    binary = ROOT / "target/release"
    with tempfile.TemporaryDirectory(prefix="emuluna-rust-check-") as temp:
        work = Path(temp)
        if args.diagnostic.startswith("snes"):
            from snes_rom import snes
            content, extension = snes(pal=args.diagnostic == "snes-pal"), "sfc"
        elif args.diagnostic == "gb":
            from roms import gameboy
            content, extension = gameboy(), "gb"
        else:
            content, extension = nes(), "nes"
        rom = work / f"Diagnostic.{extension}"
        rom.write_bytes(content)
        bios = work / "bios"
        bios.mkdir()
        base = [str(args.host.resolve()), str(args.core.resolve()), str(rom)]
        probe = subprocess.run(
            [str(binary / "emuluna-player-probe"), *base, str(work / "probe"), str(bios), "180"],
            capture_output=True, text=True, timeout=15, check=True,
        )
        print(probe.stdout, flush=True)
        assert "buffer_growth_after_warmup=0" in probe.stdout, probe.stdout
        preset_args=["--shader-pack",str(args.shader_pack.resolve()),"--preset",args.preset] if args.preset else []
        if args.overlay:
            preset_args.append("--overlay")
        for name, stall, frames in [("normal", 0, 300), ("ui-stall", 500, 300), ("short", 0, 1)]:
            env = os.environ.copy()
            env["EMULUNA_TEST_UI_STALL_MS"] = str(stall)
            result = subprocess.run(
                [str(binary / "emuluna-native-player"), *base, str(work / name), str(bios), str(frames), "--mute", *preset_args],
                env=env, capture_output=True, text=True, timeout=20,
            )
            print(name, result.stdout, flush=True)
            if result.returncode:
                raise RuntimeError(result.stderr)
            metrics = {key: int(value) for key, value in re.findall(r"(\w+)=(\d+)", result.stdout)}
            assert metrics["displayed"] > (200 if frames > 1 else 0), metrics
            assert metrics["audio_missing_frames"] == 0, metrics
            assert metrics["audio_overflow_frames"] == 0, metrics
            assert metrics["audio_pending_frames"] == 0, metrics
            if stall:
                assert metrics["video_dropped"] > 0, metrics
    print("RUST_PLAYBACK_CHECK_OK", flush=True)


if __name__ == "__main__":
    run()
