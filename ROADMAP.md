# EmuLuna development roadmap

This roadmap tracks the requested feature blueprint, including work still to
ship. Current preview: **0.13.3**, tested on Linux x86_64. Windows is a target,
not yet a working or tested release.

## Architecture to preserve

- Python/Qt owns the desktop interface. SQLite owns persistent library data.
  Additive, versioned upgrades must preserve existing libraries.
- Reusable C++ middleware owns emulation. Standard, unmodified Libretro cores
  run under this application's control. Never launch RetroArch or another
  standalone emulator. Keep the isolated game helper for crash containment.
- Use only standard Libretro cores; the original adapters were retired at the
  user's request. Preserve library data, ROM filenames and existing save files.
- Describe systems, core associations, BIOS requirements and capabilities in
  data. Hide emulator implementation details behind the native API.
- Share Linux/Windows logic and isolate platform services. Avoid Cocoa/AppKit.
  OpenEmu is the interaction reference, not the implementation architecture.
- Keep ROM filenames intact. Separate collection removal, library removal and
  confirmed disk deletion. Never unexpectedly move or delete original files.

## Stage 1 — foundation

**Working:** SQLite, isolated game processes, standard software-rendered
Libretro hosting, audio, keyboard/player-one SDL controls, analog sticks,
battery saves and checked state slots. Six GB/GBC/GBA/SNES choices passed ten
diagnostic combinations. NES Gyruss works with Nestopia and FCEUmm. ParaLLEl N64
software mode passed rendering/save/load checks for Super Mario 64, Pilotwings
64 and Blast Corps; Mario movement/camera controls were checked too.

**Further work:** hardware rendering, more environment interfaces, capability/
error reporting and broader per-system gameplay validation. A downloaded core
alone does not demonstrate full compatibility.

## Stage 2 — library experience (0.5.0)

**Working:** system icons; All Games, Recently Added (30 days), Recently Played,
Favorites; cover grid and sortable/resizable list; shared
multi-selection and context actions; Return/double-click launch; cover size;
current-section search of title, system, filename and stored information;
regular collections with drag/drop, context additions, creation from selection,
rename/delete/remove membership; smart collections combining system, rating,
favorites, never-played and recent-date rules; batch 0–5 ratings; remembered
section, view, cover size, table widths and sort; editable information; original
filename preservation; additive numbered database migrations. Ratings and
favorites are independent.

Version 0.10 centers the section navigation, simplifies the sidebar to protected
built-in/user collections and consoles, and moves import/collection creation to
the menu. Never Played remains a smart rule, not a built-in sidebar entry.
Visible-only background thumbnails, a bounded cache and deferred list rows
improve large-library opening and browsing.

**Polish ahead:** transitions, further large-library model optimization,
saved selection/scroll per section and localization.

Version 0.12 adds aspect-aware compact rows, always-visible rating stars, no
redundant section heading, freely adjustable cover size, nearby-row thumbnail
prefetching, hover Play/Resume and confirmed Restart. Both grid/list activation
resume compatible automatic states, and failed resumes preserve those files.
The bottom labels/status bar have been replaced with a toolbar notification bell,
bounded message history, unread indicator and background-task controls.

## Stage 3 — metadata, artwork and importing

**Already working:** background file/folder/ZIP import for supported cartridges,
SHA-256 duplicate detection, bounded archive reading, managed copies, filename
migration, OpenVGDB artwork identification, cached automatic covers, Libretro
thumbnail fallback, retry/cancellation and image replacement from file/drop.
Version 0.6 adds managed/external storage, system folders for new managed ROMs,
consolidation, scan/import progress and cancellation, persistent issue resolver,
unknown-system selection, empty/missing/permission/archive diagnostics, safe
folder traversal and hash-checked missing-ROM relinking with save-name continuity.
Version 0.7 adds the replaceable metadata-provider interface, automatic OpenVGDB
enrichment with source/retry records, raw ROM hashes, editable descriptions,
per-field manual-edit protection (including upgrades and stale editors), explicit
restoration of downloaded values, independent lookup/artwork settings and cancel
actions, and safe, scoped cover replacement with immutable image cache paths.

Version 0.9 adds complete disc-set validation, hashing and managed/external
imports; safe consolidation; referenced-track suppression; and actionable issues
for ambiguous systems, missing references and malformed disc descriptors.

**Next:** deeper ROM-header identification, ambiguous core selection and
confirmed disk deletion. Preserve multi-file dependencies and original files.

## Stage 4 — controllers and gameplay

**Already working:** keyboard and SDL controllers, hot-plug polling,
pause/reset, hold fast-forward, fullscreen game window, aspect ratio, integer
scaling, mute/volume and focus pause settings.
Version 0.8 adds the responsive auto-hiding HUD, mouse hiding, fullscreen launch
and clean fullscreen menu/status behavior, persistent volume/mute, 2–10×
fast-forward target, aspect preference, audio device selection/disconnect fallback
and buffer targets, audio suspension during pause/fast-forward and flushing on
state transitions. Version 0.9 removes the top Game menu, retains HUD Options
and shortcuts, and adds data-driven N64 analog/C-button/trigger mappings.
The current Controls page covers all catalog consoles, saves independent
keyboard/controller mappings per console and player, assigns connected devices,
captures physical buttons and axes (including shoulders and analog triggers),
and sends up to four players through separate libretro ports.

**Next:** controller-GUID profiles, frontend action bindings, specialized
light-gun/touch/keypad devices, multitap configuration,
embedded/pop-out display choices, deeper audio/video synchronization and broader
device/performance testing. Hide advanced controls from the main library flow.

## Stage 5 — states, screenshots and history

**Already working:** separate battery/state storage, safe writes, silent exit-only
auto state and independent periodic SRAM flush, nine slots, screenshots, last played and play count.
Libretro state compatibility includes core/build identity. Version 0.9 adds
onscreen manual save/load success and failure notices, including in fullscreen.
Automatic saves remain silent. Version 0.10 adds console-grouped state browsing,
matching-build state launch/restore, screenshot browsing/external opening, shared
search and console/collection filtering, with original files retained.

**Next:** continue/start prompt, dedicated quick slot, unlimited named states,
previews and creation/core/version records, state browser rename/
delete and more sorting, screenshot rename/delete and database records,
capture options, battery-save registry, play sessions and total play time. Never load an incompatible
core or ROM state.

## Stage 6 — visual options and advanced play

**Working (0.11):** frontend Sharp/Smooth/Scanlines/CRT/LCD filters, live gameplay
selection and strength adjustment, per-console persistence, default/reset in
Settings and cached overlays. Native screenshots and core buffers stay intact.

**Working (0.12):** original zfast CRT, CRT EasyMode, zfast LCD and Sharp Bilinear
single-pass GLSL shaders, in-game parameter controls and per-console persistence.
Private frontend OpenGL renderer with sharp-pixel fallback on graphics failure;
verified using Mesa software OpenGL. Upstream sources/licenses are retained.
General preset import, multipass/LUT/history support, Slang and hardware/Windows
validation remain pending.

**Next:** reliable serialization capability and bounded rewind buffer; generic
core-option/display-mode interface; palettes and DS layouts via capabilities;
touch/pointer and multiple screens; persistent cheats via Libretro cheat APIs;
rendering interfaces and compatible shaders/presets/parameters where practical;
per-system/game display overrides; dynamic integer-scale choices. Unsupported
features must remain unavailable rather than exposing nonfunctional controls.

## Stage 7 — systems, firmware and discs

**Working (0.9):** portable registry of 32 alphabetically ordered systems and
27 standard cores, extension-based installed-core associations, BIOS checklist
with known checksums and safe user-file import, launch firmware checks, CUE/BIN,
CCD/IMG/SUB, ISO/CHD and M3U import for compatible cores, and complete-set copies.
One playlist represents one game. No copyrighted BIOS files are supplied.

**Next:** per-game core preferences/launch overrides, remembered disc and
Libretro eject/change/insert APIs, cartridge/disc/tape capability interfaces,
hardware rendering and wider gameplay/firmware tests. PSP/Vectrex catalog cores
currently require unsupported hardware graphics; downloads remain available.

## Stage 8 — setup and cross-platform release

**Already working:** General/Core Selection/Core Downloads settings, official
Linux core download/update/import, immutable builds with integrity checks,
system defaults, artwork preferences and BIOS folder. Version 0.9 adds the
expanded flat core list, removal that preserves saves/ROMs, automatic recommended
core downloads on import and a BIOS checklist with safe file import. Version
0.11 adds installation of the entire catalog in one cancellable batch with
per-core progress and failure reporting. Hardware-dependent launch limitations
remain explicit; installation does not establish gameplay compatibility.

**Next:** setup assistant (library/storage/cores/artwork/controllers), safe
library relocation, visible-system preferences, resettable warnings, complete
gameplay settings, update-availability display and SQLite core registry,
Linux/Windows discovery and installers,
platform abstractions for dynamic loading/locking/directories, packaging and
Windows validation, diagnostic log/viewer and actionable errors, localizable
Python strings/native error codes, UI polish. Optional Homebrew catalog later.

## Verification gates

After each stage, check new behavior and existing emulation, saving, importer
and artwork integration. Use diagnostic/homebrew ROMs; do not bundle commercial
games. Inspect rendered Qt windows at usable sizes. Test database upgrades and
verify ROM bytes/names, saves, metadata and IDs survive. Keep pending features
explicit. Shipping one stage does not mean the full blueprint is complete.

## Independent EmuLuna distribution (0.13)

Active Python package, application identity, native host target/API and launchers
are branded EmuLuna. The original macOS tree and adapter checkouts are archived
outside the app. Existing libraries and state formats remain compatible.
Settings can hide empty consoles without hiding any downloadable cores.
