# Changelog

This file records user-facing changes to EmuLuna releases.

## [0.16.0] - 2026-09-30

### Changed

- Added persistent tiny cover previews for large libraries. Fast scrolling can
  show a cached preview immediately while the full thumbnail loads, and the
  Advanced settings now offer **Rebuild cover cache**.
- Added Beetle PSX and Beetle PSX HW to the PlayStation core download catalog
  and showed uninstalled alternatives in each console's Core menu. Choosing one
  opens its download row. Existing PlayStation core choices and saves are unchanged.
- Let Beetle PSX HW, PPSSPP, and DeSmuME use their OpenGL renderers when the
  required context is available. Software sessions continue to resume with
  software rendering; unavailable OpenGL contexts use the software option.
- Marked Beetle PSX HW's OpenGL path as experimental in core download details.
  Vulkan display presentation remains separate from libretro Vulkan contexts.
- Enabled ParaLLEl N64's GPU plugin by default for new sessions when OpenGL is
  available. The Advanced switch can still disable it, and existing software
  save states continue with the renderer that created them.
- Added an Auto frontend renderer that checks Vulkan initialization, then
  OpenGL, then falls back to software presentation. Advanced settings can
  force a renderer for compatibility testing.
- Separated a core's hardware-context request from the frontend display API.
  Software cores can use Vulkan or OpenGL presentation; supported OpenGL core
  contexts are negotiated explicitly, with unavailable requests declined.
- Corrected the OpenGL core framebuffer readback used by Parallel N64 so
  frames remain upright and are not doubled after loading a state.
- Kept video filters available with Vulkan presentation through a bounded
  offscreen filter pass, and added renderer initialization/fallback diagnostics.
- Simplified the README and refreshed third-party notices for bundled
  dependencies, controller art, branding, the unlock sound, and website assets.
- Removed unused legacy SVG console-icon copies and refined the fallback Neo
  Geo Pocket controller illustration.

## [0.15.0] - 2026-09-28

### Changed

- Replaced the five-minute background cover polling with event-based checks.
  Missing covers are checked after imports and through the manual Artwork menu,
  without starting scheduled downloads during gameplay.
- Added **Check for missing box art when EmuLuna starts** to Library settings.
  It is enabled by default and controls the single startup cover check.
- Added an optional high-confidence closest-title match for automatic artwork,
  including reordered titles such as `007 GoldenEye` and `GoldenEye 007`.
- Removed the redundant **Download missing cover art** and **Download replacement
  cover art…** commands from game context menus. Imports and the optional startup
  check handle missing artwork, while **Find cover art…** handles reviewed
  replacements.
- Replaced runtime-generated toolbar, notification, and Settings icons with packaged,
  palette-aware SVG assets that remain sharp under fractional and high-DPI scaling.
- Kept the expandable library search icon attached to its field without an empty toolbar gap.
- Added an Open folder button to System Files for opening the configured BIOS/system location.
- Moved per-console core selection from Settings to each console's sidebar context menu; Settings now focuses on installing and maintaining cores.

### Added

- Added **Find cover art…** to each game’s context menu. It provides a visual,
  searchable cover picker with likely matches ranked by title.
- Selecting a cover now also applies matching OpenVGDB information for that
  console and regional variant while preserving manually edited fields.
- Added a complete Linux x86_64 Libretro buildbot checklist showing which
  downloadable cores are included in EmuLuna’s curated catalog.

### Fixed

- Removed oversized gaps in the visual cover picker by using compact fixed-size
  cards, bounded labels, and one entry for visually identical cover variants.

## [0.14.0] - 2026-09-27

### Added

- Added a **+** button at the bottom of the console sidebar for importing games,
  importing folders, and creating regular or smart collections.
- Added an animated sidebar activity panel for game scans, cover downloads, and
  automatic core installation. It reports progress, exposes cancellation when
  available, and returns to the compact **+** button when work finishes.
- Added a hidden **Advanced** Settings section. Open **Help → About EmuLuna** and
  click the EmuLuna logo four times to unlock it for the current app session.
- Added a logo spin, unlock message, and bundled pop sound when Advanced Settings
  is unlocked.

### Changed

- Reorganized Settings so gameplay behavior and scaling are under **Gameplay**,
  automatic artwork options are under **Library**, and the BIOS folder is under
  **System Files**.
- Moved the experimental “Minimize the library while a game is running” option
  into the hidden Advanced section. It remains disabled by default.
- Added numbered progress details while installing multiple default cores.
- Updated the supplied Pinapple_Graphics controller artwork to use the locally
  edited, logo-free images while retaining the source credit and updating the
  artwork manifest.

### Fixed

- Kept the AppImage launchable on systems without the optional PulseAudio
  client library; the Advanced unlock effect falls back to the system alert.
- Fixed **Move game files to Trash** so every selected game is processed instead
  of stopping after the first file on PySide versions where Trash returns a
  boolean result.
- Preserved multi-game selections when opening the context menu in both Grid and
  List views.
- Refreshed games, collection counts, selections, placeholders, and thumbnail
  caches immediately after batch removal so deleted games disappear everywhere.
- Added real isolated Linux Trash coverage, including retrying a partially
  completed batch without touching the user’s desktop Trash.
