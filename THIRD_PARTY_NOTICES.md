# EmuLuna source and license notices

EmuLuna's Python/Qt frontend and C++ libretro host are original project code,
licensed under the included MIT license. Historical copyright notices are
retained. This distribution does not include the original OpenEmu macOS
application or adapters, and does not claim endorsement by that project.
Console icons from OpenEmu are included separately as described below.

| Component | Provenance / license |
| --- | --- |
| OpenEmu console icons | Restored PNG artwork from OpenEmu and its contributors; provenance and hashes in `emuluna/data/icons/`; reuse rights not independently verified; not relicensed under the frontend MIT license |
| Pinapple_Graphics controller artwork | User-supplied `Controller_Vectors_by_Pinapple_Graphics_(Normal_300ppi)_v2.1`; optimized copies in `emuluna/data/controllers/`, with source names/hashes in its manifest; no license file accompanied the supplied folder; not relicensed under the frontend MIT license |
| Libretro API header | `native/vendor/libretro.h`; original upstream MIT notice retained; https://github.com/libretro/libretro-common |
| Qt for Python / PySide6 | Separately installed runtime; https://doc.qt.io/qtforpython-6/licenses.html |
| SDL2 | Separately installed controller runtime; https://github.com/libsdl-org/SDL/tree/SDL2 |
| OpenVGDB | Separately downloaded catalog; https://github.com/OpenVGDB/OpenVGDB; external provider identifiers remain unchanged |
| Libretro thumbnail indexes/artwork | Separately downloaded; https://github.com/libretro-thumbnails/libretro-thumbnails; images retain their respective copyrights |
| Standard libretro cores | Downloaded into the user's library from https://buildbot.libretro.com/; each core keeps its upstream license, including GPL/MPL and other core-specific terms |
| System/core and firmware information | Factual configuration cross-checked against https://github.com/libretro/libretro-core-info |

No RetroArch application, third-party core binary, commercial game, or BIOS is
bundled with this source. Downloading cores and artwork does not relicense them
under the frontend MIT license. Diagnostic cartridges are original CC0-1.0 code.
The alternative console badges are original SVG artwork; the active PNG icons
come from OpenEmu. The generated EmuLuna app icon
has its prompt and provenance recorded in `emuluna/data/branding/README.md`.

## Community GLSL shaders

Four original single-pass presets from [libretro/glsl-shaders](https://github.com/libretro/glsl-shaders)
at commit `235448f244bf676d135f7b25ea6b8e1eae41c4e4` are bundled unchanged:

- zfast CRT and zfast LCD: copyright 2017 Greg Hogan (SoltanGris42), GPL version 2 or later.
- CRT EasyMode: EasyMode, GPL (version unspecified in the upstream source).
- Sharp Bilinear: Themaister, public domain.

See `emuluna/data/shaders/README.md`, the original source notices, the
manifest of hashes, and `COPYING-GPL-2` in that directory. Shader compilation
selects the original vertex/fragment branches and parameter uniforms; it does
not rewrite the bundled source. The former CPU-painted filter approximations
have been retired.

## Community Slang shaders

The 19 preset directories in `emuluna/data/shaders/presets/` are copied unchanged
from `OpenEmu/Shaders` at OpenEmu commit
`1d205104640d8410659d321809889cbfd06b99a9`. These are community/libretro shader
programs and their LUTs, not the Cocoa/Metal renderer. Original author and
copyright/license notices remain in the sources. Their individual terms
(including GPL, MIT, BSD and public-domain notices) are separate from the
frontend MIT license. Every file is recorded by SHA-256 in the shader manifest.
See the shader README for provenance and the list of presets.

## Disc-identification reference

The independent Python disc detector uses factual console header signatures
checked against OpenEmu’s system-plugin probes (PlayStation, PSP, Sega CD,
Saturn and PC Engine CD). No Cocoa/AppKit importer or emulator code was copied.
Reference: https://github.com/OpenEmu/OpenEmu/tree/master/OpenEmu/SystemPlugins
Disc-set behavior also follows the documented requirement to preserve cue sheets
and audio tracks: https://github.com/OpenEmu/OpenEmu/wiki/User-guide:-CD-based-games
