#!/bin/sh
# Compatibility launcher; Rust is now the main application.
set -eu
project_root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$project_root/run.sh" "$@"
