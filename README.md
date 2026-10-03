<p align="center">
  <img src="emuluna/data/branding/emuluna-logo.png" width="430" alt="EmuLuna logo">
</p>

# EmuLuna

EmuLuna is a desktop library and emulator frontend for classic games. Browse
your collection by console, find cover art, configure controllers, and play
through downloadable libretro cores. It runs on Linux; Windows support is
still in development. EmuLuna is independent of RetroArch and OpenEmu.

<p align="center">
  <img src="screenshots/emuluna-gameplay.png" alt="EmuLuna library with a Sega Mega Drive game running">
</p>

[Website](https://emuluna-retro-library.turgo82.chatgpt.site/) ·
[GitHub releases](https://github.com/Turgo82/EmuLuna/releases) ·
<br>
<a href="https://www.buymeacoffee.com/turgo" target="_blank"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me a Coffee" style="height: 60px !important;width: 217px !important;" ></a>

## Get started

Download the latest Linux AppImage from
[GitHub releases](https://github.com/Turgo82/EmuLuna/releases), make it
executable, and open it. Or, from a source checkout, run `./run.sh` (or
`Launch EmuLuna.sh`). The source version needs Python 3, PySide6, the
native host built with `./build.sh`, and SDL2 for controller support.

Use the **+** button at the bottom of the sidebar to import games or folders.
EmuLuna can download a default core for a supported console when you import a
game. You can also manage cores in **Settings → Cores**, or right-click a console
to choose its installed core. BIOS files, games, and their licenses are yours
to supply; EmuLuna does not include them.

## What you can do

- Organize games by console or collection. Search, rate favorites, switch
  between grid and list views, and adjust cover size.
- Download cover art automatically after imports, optionally check for
  missing covers at startup, or right-click a game to pick a cover visually.
  A small cached preview fills each cover while its full image loads. Use
  **Advanced → Rebuild cover cache** if those previews become stale.
- Play with keyboard or controller, customize controls for each console,
  and use save states, screenshots, video filters, fullscreen, and scaling.
- Browse save states and screenshots separately. Remove a game from the
  library or move its files to Trash, with separate choices for states and
  screenshots.

Most games open in their own gameplay window. Move the pointer over that
window to show its controls. F5 saves a state, F8 loads one, F11 switches
fullscreen, and F12 takes a screenshot. Closing a game creates an automatic
state; starting it again can resume that state. Manual states and in-game
saves are separate.

Library and game windows remember their size and maximized state. On Wayland
desktops, EmuLuna uses native Wayland windows; KDE title bars match its theme.

EmuLuna keeps your library in `~/.local/share/emuluna` by default (or under
`$XDG_DATA_HOME` when set).
If an older `openemu-linux` library exists, EmuLuna opens it in place. It
does not move or rename your games. Use `--data-dir` or `EMULUNA_DATA_DIR`
to choose another location.

## Cores and graphics

EmuLuna's curated catalog contains 29 downloadable libretro cores. Existing
cores are updated only when you request it, and the previous build is kept for
rollback. Installed cores, ROMs, saves, and downloaded artwork live in your
library folder, outside the AppImage.

**Auto** display mode tries Vulkan, then OpenGL, then software presentation.
Software-rendered cores can still be displayed through Vulkan or OpenGL.
Hardware-rendered cores must receive the context they request; EmuLuna
supports libretro OpenGL contexts and Beetle PSX HW's Vulkan context. Beetle
PSX HW tries Vulkan, then OpenGL, then software for new games. Save states
resume with the renderer that created them. Beetle PSX HW also needs a
region-matched PlayStation BIOS supplied by the user.

The video-filter menu includes CRT, LCD, smoothing, and other bundled
community shaders. Some effects require a capable GPU. See the
[shader notes](emuluna/data/shaders/README.md) for sources and compatibility.

## Build and test

For a source build, install Python 3, the packages in `requirements.txt`,
a C++14 compiler, CMake, Ninja, and SDL2. Then run:

```sh
./build.sh
./run.sh
```

To build a Linux AppImage, run `./packaging/appimage/build-appimage.sh`.
The result appears in `dist/`. The first build downloads its packaging tools.
GitHub release tags build an AppImage and checksum through the release
workflow.

Run the automated tests with:

```sh
QT_QPA_PLATFORM=offscreen PYTHONPATH=. python3 -m unittest discover -s tests
```

The project is still in development. Core availability does not guarantee
every game works. Disc switching, wider hardware-core support, and Windows
packaging remain on the [roadmap](ROADMAP.md). See the
[changelog](CHANGELOG.md) for release details and the
[core catalog checklist](docs/CORE_CATALOG_CHECKLIST.md) for supported cores.

## Credits and licenses

**Controller illustrations: Pinapple_Graphics**, from the user-supplied
Controller_Vectors_by_Pinapple_Graphics_(Normal_300ppi)_v2.1 set.
Console icons include OpenEmu artwork. Community shaders, runtime libraries,
downloaded cores, game art, and the user-supplied unlock sound have their own
terms. The logo and Luna mascot were supplied for this project.

EmuLuna's original code is MIT-licensed. The diagnostic cartridges are CC0.
No games or BIOS files are bundled. Read [LICENSE](LICENSE) and
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for credits and provenance.
