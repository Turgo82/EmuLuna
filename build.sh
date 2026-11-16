#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
"${CMAKE:-cmake}" -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
"${CMAKE:-cmake}" --build build --parallel "${BUILD_JOBS:-4}"
cargo_bin="${CARGO:-cargo}"
if [[ "$cargo_bin" == cargo && -x build/rust/cargo/bin/cargo ]]; then
    cargo_bin="$PWD/build/rust/cargo/bin/cargo"
    export CARGO_HOME="$PWD/build/rust/cargo" RUSTUP_HOME="$PWD/build/rust/rustup"
fi
"$cargo_bin" build --release --manifest-path Cargo.toml -p emuluna-native-library -p emuluna-native-player "$@"
printf '%s\n' 'Build complete. Launch with ./run.sh'
