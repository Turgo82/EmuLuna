#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
"${CMAKE:-cmake}" -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release "$@"
"${CMAKE:-cmake}" --build build --parallel "${BUILD_JOBS:-4}"
printf '%s\n' 'Build complete. Launch with ./run.sh'
