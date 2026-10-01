# EmuLuna third-party notices

EmuLuna's original Python/Qt frontend, C++ libretro host, UI and collection
SVG icons, and fallback controller illustrations are covered by the project
LICENSE. Original diagnostic cartridges are dedicated to CC0-1.0 as stated
there. Historical project copyright notices are retained. The project does
not contain the OpenEmu macOS application or its emulator adapters, and is
not endorsed by OpenEmu or any console manufacturer.

The source checkout, AppImage, and separately published website contain
different material. The project license does **not** relicense the following
dependencies or artwork.

## Artwork and sound in the application

| Material | Origin and status |
| --- | --- |
| Console PNG icons in emuluna/data/icons/ | OpenEmu and its contributors. openemu-icons.json records hashes and matching upstream paths where known. Ownership remains with the original creators; reuse rights have not been independently verified. [OpenEmu system plugins](https://github.com/OpenEmu/OpenEmu/tree/master/OpenEmu/SystemPlugins). |
| 24 imported controller WebP illustrations in emuluna/data/controllers/ | Artwork by **Pinapple_Graphics**, from the user-supplied Controller_Vectors_by_Pinapple_Graphics_(Normal_300ppi)_v2.1 set. Files were resized/converted, and some retain local edits. artwork_manifest.json records source names, hashes, and processing. No license accompanied the supplied set; these images are not covered by EmuLuna's MIT license. The remaining fallback SVG controller illustrations are original project artwork. |
| Full EmuLuna logo and Luna mascot in emuluna/data/branding/ | User-supplied project artwork. The compact app icon was generated for EmuLuna; provenance is recorded in branding/README.md. No separate third-party license is asserted for the user-supplied images. |
| emuluna/data/sounds/unlock-pop.wav | User-supplied pop sound for the Advanced-settings unlock animation. Its creator and reuse terms were not supplied; the project MIT license does not cover it. |

## Runtime and packaging

| Component | Use and upstream terms |
| --- | --- |
| native/vendor/libretro.h | Bundled libretro API header; its upstream MIT notice remains at the top of the file. [Libretro](https://github.com/libretro/libretro-common). |
| Python | The AppImage bundles a Python runtime. Python is distributed under the [Python Software Foundation License](https://docs.python.org/3/license.html), with additional licenses for some incorporated software. Source installs use the system's Python. |
| Qt for Python / PySide6, Shiboken and Qt libraries | Used by the interface and bundled in the AppImage. Community Qt for Python is offered under [LGPLv3/GPLv3](https://doc.qt.io/qtforpython-6/) and commercial terms; individual Qt modules and incorporated components can have additional notices. See [Qt for Python's license inventory](https://doc.qt.io/qtforpython-6/licenses.html) and the licenses for the exact packaged version. |
| certifi | Certificate-authority bundle used for verified downloads; included with the AppImage. [MPL-2.0](https://github.com/certifi/python-certifi/blob/master/LICENSE). |
| SDL2 | Controller input and rumble. The current AppImage includes libSDL2-2.0.so.0; source installs load the system library. SDL2 uses the [zlib license](https://github.com/libsdl-org/SDL/blob/SDL2/LICENSE.txt). |
| PyInstaller | Build tool and bootloader used for the AppImage; its [license and bootloader exception](https://pyinstaller.org/en/stable/license.html) are separate from EmuLuna's license. |
| AppImage runtime / appimagetool | Used to package and launch the AppImage. [AppImageKit is MIT-licensed](https://docs.appimage.org/packaging-guide/distribution.html); the build downloads appimagetool if it is not already cached. |

The AppImage includes native libraries and Qt plugins resolved at build time.
The exact binary contents and applicable license files should be checked
when preparing each release; this source-level list does not claim that every
transitive binary dependency has the same license as its parent package.

## Downloaded separately into the user's library

| Material | Source and status |
| --- | --- |
| Libretro cores | Downloaded on demand from the [Libretro Linux buildbot](https://buildbot.libretro.com/nightly/linux/x86_64/latest/), or imported by the user. Cores are not embedded in the source or AppImage. Each core keeps its own upstream license; the curated catalog and upstream information are in emuluna/data/cores.json. |
| Game metadata | OpenVGDB catalog downloaded from [OpenVGDB](https://github.com/OpenVGDB/OpenVGDB). External provider identifiers are retained. |
| Box art | Downloaded from OpenVGDB and, when configured, [Libretro thumbnail repositories](https://github.com/libretro-thumbnails). Individual artwork and game trademarks remain with their owners. |
| BIOS, games, save files | Not supplied or downloaded by EmuLuna. |

System/core and firmware facts were cross-checked against
[libretro-core-info](https://github.com/libretro/libretro-core-info). A downloaded
core or image is not relicensed under the EmuLuna project license.

## Bundled community shaders

The four GLSL presets from
[libretro/glsl-shaders](https://github.com/libretro/glsl-shaders) at commit
235448f244bf676d135f7b25ea6b8e1eae41c4e4 are:

- zfast CRT and zfast LCD: copyright 2017 Greg Hogan (SoltanGris42), GPL
  version 2 or later.
- CRT EasyMode: EasyMode, GPL (version unspecified in its upstream source).
- Sharp Bilinear: Themaister, public domain.

The 19 Slang preset directories under emuluna/data/shaders/presets/ came
from [OpenEmu/Shaders](https://github.com/OpenEmu/OpenEmu/tree/1d205104640d8410659d321809889cbfd06b99a9/OpenEmu/Shaders)
at commit 1d205104640d8410659d321809889cbfd06b99a9. Their shader programs,
lookup textures, and author/license notices remain in the source files. Terms
vary by preset, including GPL, MIT, BSD, and public-domain notices. The source
shader files are not relicensed under EmuLuna's MIT license. The renderer
adapts shader stages and uniforms at compile time; it does not replace the
bundled source notices.

See emuluna/data/shaders/README.md, manifest.json, original source notices,
and COPYING-GPL-2 for the preset inventory and hashes.

## Website and reference material

The separately published static site is in website/dist/. It contains the
user-supplied EmuLuna logo and mascot, and screenshots of the application.
Screenshots can show game box art, gameplay, console icons and
Pinapple_Graphics controller illustrations; those underlying works remain
credited above and with their respective owners. The website contains no ROMs,
BIOS files, user library database or saves. See website/README.md.

The independent disc detector uses factual header signatures checked against
[OpenEmu system-plugin probes](https://github.com/OpenEmu/OpenEmu/tree/master/OpenEmu/SystemPlugins)
for PlayStation, PSP, Sega CD, Saturn and PC Engine CD. No Cocoa/AppKit importer
or emulator code was copied. Disc-set behavior also follows the documented
need to preserve cue sheets and audio tracks in the
[OpenEmu CD-based games guide](https://github.com/OpenEmu/OpenEmu/wiki/User-guide:-CD-based-games).
