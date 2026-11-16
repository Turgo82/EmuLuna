#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
library="$project_root/target/release/emuluna-native-library"
if [[ ! -x "$library" ]]; then
    printf '%s\n' 'Build EmuLuna with ./build.sh first.' >&2
    exit 1
fi
exec "$library" "$@"
