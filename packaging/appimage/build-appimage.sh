#!/usr/bin/env bash
set -euo pipefail

project="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$project"

architecture="${ARCH:-x86_64}"
case "$architecture" in
    x86_64) tool_arch=x86_64 ;;
    aarch64) tool_arch=aarch64 ;;
    *) printf 'Unsupported AppImage architecture: %s\n' "$architecture" >&2; exit 2 ;;
esac

version="${VERSION:-$(sed -n 's/^project(EmuLuna VERSION \([^ ]*\).*/\1/p' CMakeLists.txt)}"
output="${OUTPUT:-$project/dist/EmuLuna-${version}-${architecture}.AppImage}"
work="${APPIMAGE_WORK_DIR:-$project/build/appimage}"
venv="${APPIMAGE_VENV:-$project/build/appimage-venv}"
appdir="$work/EmuLuna.AppDir"

cmake_command="${CMAKE:-cmake}"
if command -v "$cmake_command" >/dev/null 2>&1; then
    "$cmake_command" -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
    "$cmake_command" --build build --parallel "${BUILD_JOBS:-4}"
elif [[ -f build/cores/libemuluna_host.so ]]; then
    printf '%s\n' 'CMake is unavailable; using the existing native host build.'
else
    printf '%s\n' 'CMake is required because build/cores/libemuluna_host.so does not exist.' >&2
    exit 1
fi

if [[ ! -x "$venv/bin/pyinstaller" ]]; then
    "${PYTHON:-python3}" -m venv "$venv"
    "$venv/bin/python" -m pip install --upgrade pip
    "$venv/bin/pip" install -r requirements.txt 'pyinstaller>=6.11,<7'
fi

rm -rf "$work" "$project/dist/EmuLuna"
mkdir -p "$work" "$project/dist"
"$venv/bin/pyinstaller" --noconfirm --clean \
    --distpath "$project/dist" --workpath "$work/pyinstaller" \
    packaging/appimage/EmuLuna.spec

mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/256x256/apps"
cp -a "$project/dist/EmuLuna" "$appdir/usr/bin/"
install -Dm755 packaging/appimage/AppRun "$appdir/AppRun"
install -Dm644 packaging/appimage/emuluna.desktop "$appdir/emuluna.desktop"
install -Dm644 packaging/appimage/emuluna.desktop \
    "$appdir/usr/share/applications/emuluna.desktop"

icon_source="$project/emuluna/data/branding/emuluna.png"
install -Dm644 "$icon_source" "$appdir/emuluna.png"
install -Dm644 "$icon_source" \
    "$appdir/usr/share/icons/hicolor/256x256/apps/emuluna.png"
ln -sfn emuluna.png "$appdir/.DirIcon"

tool="${APPIMAGETOOL:-$project/build/tools/appimagetool-${tool_arch}.AppImage}"
if [[ ! -x "$tool" ]]; then
    mkdir -p "$(dirname "$tool")"
    curl --fail --location --show-error \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${tool_arch}.AppImage" \
        --output "$tool"
    chmod +x "$tool"
fi

rm -f "$output"
ARCH="$architecture" "$tool" --appimage-extract-and-run "$appdir" "$output"
chmod +x "$output"
printf 'Created %s\n' "$output"
