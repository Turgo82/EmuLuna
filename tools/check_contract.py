#!/usr/bin/env python3
"""Compile original test cores and check native libretro boundaries.

Uses only synthetic content and temporary saves. --gpu additionally opens short
GPU/audio test windows; no installed library, commercial game or BIOS is needed.
"""
import argparse
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(command, env):
    subprocess.run([str(item) for item in command], cwd=ROOT, env=env, check=True, timeout=180)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, default=ROOT / "build/cores/libemuluna_host.so")
    parser.add_argument("--cargo", default=os.environ.get("CARGO") or shutil.which("cargo") or str(ROOT / "build/rust/cargo/bin/cargo"))
    parser.add_argument("--cxx", default=os.environ.get("CXX") or shutil.which("c++"))
    parser.add_argument("--cxx-flag", action="append", default=[])
    parser.add_argument("--gpu", action="store_true", help="Run desktop GPU/audio checks, including all exported presets")
    parser.add_argument("--shader-pack", type=Path, default=ROOT / "build/rust/shader-pack")
    args = parser.parse_args()
    if not args.host.is_file():
        parser.error("Build the native host or supply --host first")
    if not args.cxx:
        parser.error("Supply a C++ compiler with --cxx")
    if args.gpu and not (args.shader_pack / "pack.json").is_file():
        parser.error("Export the shader pack before running --gpu")
    env = os.environ.copy()
    env["EMULUNA_RUST_HOST"] = str(args.host.resolve())
    flags = shlex.split(env.get("CXXFLAGS", "")) + args.cxx_flag
    fixtures = [
        ("input", "rust_input_core.cpp", [], "EMULUNA_RUST_INPUT_FIXTURE"),
        ("video", "rust_video_core.cpp", [], "EMULUNA_RUST_VIDEO_FIXTURE"),
    ]
    if args.gpu:
        fixtures.extend([
            ("hardware", "rust_hardware_core.cpp", [], "EMULUNA_RUST_HW_FIXTURE"),
            ("init-input", "rust_input_core.cpp", ["RUST_FIXTURE_STATE_INITIALIZATION"], "EMULUNA_RUST_INIT_INPUT_FIXTURE"),
            ("init-hardware", "rust_hardware_core.cpp", ["RUST_FIXTURE_STATE_INITIALIZATION"], "EMULUNA_RUST_INIT_HW_FIXTURE"),
            ("burst", "rust_hardware_core.cpp", ["RUST_FIXTURE_AUDIO_BURST"], "EMULUNA_RUST_HW_BURST_FIXTURE"),
            ("diagnostic", "rust_hardware_core.cpp", ["RUST_FIXTURE_GL_DIAGNOSTIC", "RUST_FIXTURE_CHECK_GL_STATE", "RUST_FIXTURE_GL_LIFECYCLE_PROBE"], "EMULUNA_RUST_HW_DIAGNOSTIC_FIXTURE"),
        ])
    with tempfile.TemporaryDirectory(prefix="emuluna-contract-fixtures-") as directory:
        for name, source, definitions, variable in fixtures:
            output = Path(directory) / f"{name}.so"
            run([args.cxx, *flags, "-shared", "-fPIC", "-I", ROOT / "native/vendor", *[f"-D{d}" for d in definitions], ROOT / "tests" / source, "-o", output], env)
            env[variable] = str(output)
        run([args.cargo, "test", "--offline", "--manifest-path", "Cargo.toml", "-p", "emuluna-libretro-host", "--", "--ignored", "--test-threads=1", "--nocapture"], env)
        if args.gpu:
            run([args.cargo, "test", "--offline", "--manifest-path", "Cargo.toml", "-p", "emuluna-audio-device", "--", "--ignored", "--test-threads=1", "--nocapture"], env)
            run([sys.executable, ROOT / "tools/check_native_sessions.py"], env)
            env["EMULUNA_TEST_SHADER_PACK"] = str(args.shader_pack.resolve())
            env["EMULUNA_TEST_DYNAMIC_PATTERN"] = "1"
            run([args.cargo, "test", "--offline", "--manifest-path", "Cargo.toml", "-p", "emuluna-gl-presenter", "--", "--ignored", "--test-threads=1", "--nocapture"], env)
    print("NATIVE_CONTRACT_CHECK_OK", flush=True)


if __name__ == "__main__":
    main()
