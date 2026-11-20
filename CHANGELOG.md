# Changelog

This file records user-facing changes to EmuLuna releases.

## [0.21.5] - 2026-11-19

- Group ROM revisions and alternate versions under one library entry. Choose a preferred version from the Versions menu or the optional cover and list badges.
- Show a translucent Play or Resume button that fades in on cover hover. Start from beginning is available in the right-click menu.
- Highlight save states and screenshots with the same selection outline as box art, with Load and Open buttons on previews.
- Match gameplay notices to the translucent toolbar.
- Fix the search field border clipping and align the Scaling submenu with its parent row.

## [0.21.4] - 2026-11-19

- Use a PC mouse as an SNES Mouse with Snes9x or a Mega Mouse with Genesis Plus GX in compatible games. Choose the port, speed, and acceleration per game. Grab the mouse from the gameplay toolbar and release it with Esc.
- Open Game information in a docked sidebar with box art and metadata. Match the ROM against the local catalog when metadata is missing.
- Show the filename in Find cover art.
- List supported libretro cores by console in the README.

## [0.21.3] - 2026-11-19

- Increase the visible size of the EmuLuna icon in launchers, docks, and window title bars.

## [0.21.2] - 2026-11-19

- Fix held keyboard buttons being treated as taps during gameplay.
- Prompt when a newer release is available, with an option to verify, install, and restart from the new AppImage.
- Choose whether to keep or remove the saved state when turning off autosave. When turning it back on, load an existing autosave before resuming automatic saving.
- Show the reset warning only when autosave is on.
- Use the EmuLuna icon on separate windows and label the console selector in Cores settings.

## [0.21.1] - 2026-11-16

- Scroll the cover grid half a row per wheel notch, with smooth slowdown.
- Preserve quick keyboard taps through to the next emulated frame.
- Open Settings in a separate window. Enable startup update checks and library minimization by default.
- List Sega six-button controls in A, B, C, X, Y, Z order.
- Fix the rating menu checkmark and keep the selected library tab highlight consistent.

## [0.21.0] - 2026-11-15

- Rewrote the application in Rust, with a native libretro host.
- Added recovery of compatible save states from retained core builds.
- Reduced routine game-session notifications.

### Added

- Stop repainting the full raster window underneath each OpenGL gameplay frame.
  Let the GPU surface clear its own black borders, preserving software fallback
  and normal overlay updates while reducing shared rendering/audio overhead.

- Retain a bounded recovery window after brief gameplay scheduling delays,
  rather than immediately discarding missed audio-production time. Continue
  presenting each normal frame and rebase after longer stalls.
- Update the audio-clock target when changing the buffer or reconnecting an
  output during play, clearing correction from the previous stream.

- Smooth audio clock drift with bounded, gradual resampling correction, keeping
  the playback queue balanced without changing core speed or skipping frames.

- Correct audio-state/error comparisons across Qt versions so a healthy output
  is not stopped as a failure. Retry interrupted streams up to three times,
  preserving the chosen route, volume and pause state; log actual device errors.

- Blend the disc-backup scroll area into the surrounding theme background.

- Put **System** in the compact Theme dropdown instead of a separate checkbox;
  follow OS sidebar/navigation colors without retaining the prior named palette.
- Keep controller dropdown text readable on its light wood popup background in
  every named theme and OS light/dark mode. Match library margins to the grid.
- Name completed disc files after the reviewed game title, including CHD and
  matching BIN/CUE references, while retaining game/save identity and duplicate
  recognition. Sanitize filenames and refresh the stored file inventory.

- Add Sega CD and 32X low-pass toggles to gameplay audio menus, using the core's
  native filters. Keep Sega CD off initially to avoid filtering CD music; use
  the Genesis-style 60% default for 32X. Save choices per console and core.
- Expose the same audio toggle for Genesis when PicoDrive is selected, with
  explanatory tooltips and no changes to shared Master System/Game Gear defaults.

- Label the success-screen completion action **Done**; save the verified backup
  to the library and close only after import succeeds.
- Remember the disc-backup agreement and add **Disc backup legal agreement** to
  About, where users can review it or clear acceptance to restore the prompt.

- Open web links, library/ROM folders, themes, system files and screenshots
  through the host desktop with AppImage library/plugin paths removed from the
  child environment. Keep the UI responsive and report rejected launch requests.

- Keep the disc-backup window at a fixed screen-fitting size, with its action
  buttons pinned below a scrollable form so accepting the agreement or revealing
  track details cannot shrink or hide the backup controls.

- Replace the completed backup form with a green Success screen, larger centered
  cover animation, **Add to library** and **Back up another** actions. Keep
  verified images staged until import; retain them for retry after import errors.
- Show clear dropdown arrows in the disc wizard and match progress-bar colors
  to the selected theme, including Classic and Daylight.

- Rename the menu and Plus action to **Back up disc…**. Require an unchecked
  ownership/backup agreement before opening the workflow, with relevant
  Section 117 excerpts, conditions and official Copyright Office links.

- Default disc backups to the faster direct MMC reader, retaining cdrdao as a
  compatibility option with adjustable audio checking.
- Offer lossless CHD output alongside BIN/CUE, with compression and verification
  before managed library import. Bundle the CHD compressor in AppImages.

- Show boundary-scan elapsed/activity times and retain complete bounded reader
  diagnostics and final backup outcomes, including conversion/import failures.

- Put the audio output selector in General settings. Follow system-default and
  saved output changes in running games without restarting unrelated audio routes.
- Fall back to the system default when a selected device disconnects and return
  to the selected device when it reconnects, preserving pause and mute state.
- Add **Back up disc…** to the Library menu and Plus menu. A guided popup detects
  the optical drive, shows a blank CD while identifying the inserted game,
  looks up its cover and title, and offers **Back up to library**.
- Preserve Sega CD / Mega-CD data and audio tracks in a complete BIN/CUE
  backup, including stored pregaps and corrected CD-audio sample byte order.
  Also support standard Saturn, PlayStation and PC Engine CD game discs.
- Add cancellable reads, track/size validation, disc-change checks, artwork
  fallback and verified managed library import. Bundle the CD reader and
  converter in AppImages, with their licenses and matching source packages.
- Show the current data/audio track, transferred bytes, read speed and estimated
  time remaining. Animate the filled progress area while bytes advance; pause
  the ETA and show a waiting message when the drive stops delivering data.
  Retain the last reader log for troubleshooting after a failed or cancelled rip.
- Use audio correction mode 2, retaining overlapping reads and verification
  without mode 3's additional scratch repair, to reduce prolonged audio retries.
- Let users choose audio checking levels 0–3 before backing up a disc, with
  explanations of speed and verification tradeoffs and level 2 selected initially.
- Use separate audio/subchannel reads on the MATSHITA CW-8123, retaining track
  boundaries and pregaps instead of relying on its advertised combined-read support.
- Validate explicit CUE pregaps against the physical track timeline so valid
  compatibility-mode images are accepted while truncated audio is rejected.
- Stop failed CD read commands immediately so a disconnected drive cannot
  continue showing misleading progress or produce an imported image.

### Performance

- Schedule gameplay wake-ups against frame deadlines instead of polling the
  GUI every millisecond; keep normal-speed frame presentation intact.

- Refill available audio-device space in bounded batches after a delayed feeder
  callback, retaining partial writes and stopping immediately on backpressure.
- Show emulation work time and the frame budget in the performance display to
  distinguish core load from presentation/audio stalls.

- Deliver already-produced game audio from a dedicated Qt feeder thread using
  a bounded, reusable PCM ring, reducing dependence on GUI repaint timing.
- Preserve partial writes and pause/reset/device-change/shutdown behavior; show
  audio underruns, queued time and overflow counts in the performance display.

- Cache FPS graph history buckets and drawing paths between diagnostic refreshes,
  reducing redundant overlay work while preserving frame-time and 1% low metrics.

## [0.20.2] - 2026-10-20

### Changed

- Increase mouse-wheel scrolling distance by 25% in library grids, lists,
  save states and screenshots, while keeping the same smooth animation.
  Touchpad pixel scrolling remains unchanged.

## [0.20.1] - 2026-10-14

This release fixes missing damage-blink frames and uneven presentation by using
OpenGL in Auto display mode and preserving normal-speed frame submissions.
These frontend fixes apply across all cores; a core's internal renderer remains
independent of the display mode.

### Fixed

- Use direct OpenGL presentation in Auto mode. Avoid the Qt Quick Vulkan path
  rendering at half rate on Wayland and hiding one phase of blinking sprites
  despite full-speed emulation. Keep explicit Vulkan available experimentally.
- Present every normal-speed frame, preserving alternating-frame effects such
  as NES damage blinking. Limit late display skipping to fast-forward and reset
  missed normal-speed deadlines to avoid catch-up bursts and FPS spikes.

## [0.20.0] - 2026-10-11

This release adds independent themes, an animated Setup Assistant, console-based
library storage with move and audit tools, smoother library scrolling, expanded
performance diagnostics, and N64 compatibility and border improvements.

### Fixed

- Smooth mouse-wheel scrolling in library grids, lists, save states and screenshots.
  Keep touchpad pixel scrolling direct and cancel wheel animation when dragging
  the scrollbar, using keys or jumping to another game. Prefetch nearby covers
  during scrolling without scanning the entire library or restarting the queue
  on every animation frame.
- Stop delayed startup lookups after the library window closes.

- Fix 1080° Snowboarding starting to a black screen with ParaLLEl N64 by
  selecting Cached Interpreter for that game in software and OpenGL modes.
  Identify it from the cartridge header as well as its library title; leave
  other games' CPU defaults unchanged.

- Automatically align ParaLLEl N64 pictures in software and OpenGL: remove
  small, entirely black outer margins after three seconds of stable padding
  in a sufficiently bright scene. Ignore mostly black scenes and fades during
  detection; lock the first adjustment for the rest of the play session, including
  resolution changes. Toggle off and on to recalibrate.
  Preserve core display proportions.
  Add a toggle under Scaling in the game overlay that applies immediately, plus a
  per-game switch under Screen borders; manual native cropping takes priority.

- Feed audio before core work and video submission for every core, including
  fullscreen Saturn playback. Drain pending sound between emulation frames.
- When software-frame playback falls behind, skip up to two display submissions
  to catch up while preserving every emulation frame, audio, state and screenshot.
  Resume normal presentation immediately once the deadline recovers.

### Added

- Enlarge settings navigation icons from 30 to 44 pixels and tighten surrounding
  padding, keeping labels beneath the icons.

- Rename the built-in themes to Luna (default), Daylight and Classic. Rename
  folders and IDs to `luna`, `daylight` and `classic`, migrating saved old selections.

- Expand the FPS display with a rolling ten-second line graph, 1% low FPS,
  current frame interval and peak frame interval. Exclude pauses and fast-forward;
  show 1% low after 100 samples. Flatten graph fluctuations within 2% of the
  typical FPS for readability; keep raw timing statistics unchanged.
  Metrics describe core frame pacing.

- Make every built-in theme and newly created custom theme standalone, with its
  own complete palette, fonts, four stylesheets and controller wood asset. Editing
  one theme no longer changes another; new copies have no parent references.

- Add a five-step first-run Setup Assistant with Welcome, Library, Appearance,
  Emulator cores and Ready pages, Back/Next navigation and a final Let’s go action.
  Animate a large, smoothly scaled welcome logo with twinkling stars and
  shooting stars crossing the screen on varying paths every seven seconds, plus a gentle logo wobble. Add a vivid night sky, layered procedural clouds with faster parallax drift,
  responsive side-by-side or stacked layouts, and Fullscreen / F11 with Escape to return;
  offer an icon-based console checklist when automatic core downloads are disabled.
  Download selected cores once during setup, and retain selections for retries.
  Fade between setup pages while keeping sky, cloud and logo animations continuous.
  Stop animation when closing setup.
  Choose console storage, import copying, appearance and recommended core downloads.
  Keep setup pending when dismissed. Add a side-effect-free **Preview onboarding…**
  under Advanced → Developer options; existing libraries do not show setup after updates.
- Add a Library audit section in Advanced settings, with background checks for
  missing ROMs, missing disc tracks, changed sizes and optional content verification.
  Locate matching originals or explicitly remove selected library references
  while keeping ROM files, saves, states and screenshots.
- Check the library before migration, list issues by game, require them to be
  resolved, and ask for confirmation before moving files. Keep this preflight
  available without unlocking Advanced settings.
- Add editable theme packages with JSON colors, independent QSS styles and optional
  controller textures. Create, select, open and reload custom themes in Settings.
- Ship Luna as the default, plus Daylight and Classic themes. Match OpenEmu’s
  source red accent, charcoal surfaces, gray sidebar/navigation selections,
  lighter title typography and native-style preferences labels. Use the original
  OpenEmu wood with dark readable labels in all three themes. Apply the Controls palette to custom-painted inputs and
  regenerated binding rows so white text cannot leak onto the light wood.
- Replace Organize library by console with **Move library…**. Console folders
  remain the default; users can store `consoles` in a chosen folder or drive.
  Copy and verify all console files before switching locations, preserve index
  paths and saved-game identities, and recover interrupted moves. Audit missing
  ROMs and confirm the destination first; never overwrite an existing folder.
- Group new libraries into console folders with ROMs, battery saves, screenshots
  and save states. Keep single-file games together and multi-file disc tracks isolated.
- Write readable per-console `games.json` inventories with titles, game IDs,
  paths, file hashes and save/media locations. Add an inventory rebuild button.
- Add an explicit organization action for existing libraries, with a database
  backup, game locks, collision protection and interrupted-operation recovery.
  Preserve metadata, external ROM locations, source downloads and legacy media access.

### Library fixes

- Check game locks individually during library organization, avoiding a false
  “close the game” error when large libraries exhaust the open-file limit.
  Prevent game launches during organization and name an actually running game
  in the message.

### Changed

- Make rated stars yellow and unrated stars dark, and bring ratings closer to
  game titles. Keep theme-controlled title typography and styled sidebar console/collection
  labels and library section buttons.
- Keep native KDE decoration colors in step with selected theme palettes.

## [0.19.0] - 2026-10-08

This release keeps gameplay windows in the core's proportions, simplifies the
game window, and adds optional per-game N64 overscan controls for software
and OpenGL. Software mode uses the core's automatic overscan crop; OpenGL
offers individual adjustments for each edge.

### Changed

- Add per-game Screen borders controls under Gameplay options → Scaling for
  ParaLLEl N64. Software mode can automatically hide overscan through Angrylion;
  OpenGL mode can trim each edge through GLideN64's own options. Remember
  adjustments separately for each game and renderer, and restore uncropped
  output without affecting other games. Changes apply when the game reopens.
- Use pure black behind gameplay and in letterbox borders with Vulkan,
  OpenGL and software presentation, including before the first frame appears.
- Fit normal game windows to the core's display aspect ratio and preserve it
  while dragging the bottom-right resize grip. Remembered sizes keep their scale
  while adopting the correct ratio; fullscreen and maximized windows retain black
  borders as needed. Square-pixel mode and integer scaling remain available.
- Resize gameplay in 16-pixel height steps with both dimensions calculated before
  applying each size. Lock native border resizing for normal game windows so they
  cannot stretch during a drag or snap back afterward, including on Wayland.
- Keep maximizing and restoring available through Gameplay options → Scaling,
  even when the desktop disables its maximize button for a size-locked window.
- Remove the bottom status and keyboard-hint bar from game windows. Keep the
  resize grip over the bottom-right corner of gameplay and use temporary in-game
  notices for screenshots, audio issues and save errors.
- Align the gameplay resize grip's clickable area with the bottom-right window
  corner, enlarge it, and preserve its target size through delayed desktop events.

## [0.18.0] - 2026-10-03

This release adds Dreamcast, identifies CHD discs from their contents, redesigns
settings, and refreshes library layout and artwork storage. It also fixes
Dreamcast saves and disc re-imports after files have been moved to Trash.

### Changed

- Make the library grid responsive to window width: resizing changes horizontal
  spacing and wrapping while box art keeps its size.
- Give every console grid the same left starting point, matching the Genesis
  layout. Full rows, short final rows and single-game results stay left aligned.
- Space the visible cover edges evenly, including Saturn libraries that mix
  narrow cases and square Japanese covers. Gaps remain compact at wider sizes.
- Cap box art at 256 pixels on its longest edge and preserve its proportions.
  Match ordinary cover heights within a console without enlarging landscape
  SNES and N64 boxes to the height of portrait boxes.
- Remove the manual library cover-size slider; the grid now handles window
  width automatically.
- Increase cached cover previews from 40 to 96 pixels for sharper artwork
  during fast scrolling. Older previews are regenerated automatically.
- Draw cached previews at the full displayed cover size while the larger
  thumbnail loads, preserving the original artwork's shape.
- Save automatically downloaded box art and artwork chosen through the visual
  cover picker as WebP, using quality 85 by default.
- Offer WebP quality 75, 85, 95 or lossless in Advanced settings under the new
  Developer options section. The selected quality applies to downloads and
  bulk conversion.
- Match the settings header and title bar to the main library's darker toolbar
  color without contrasting side gaps. Center the tabs with labels below their
  icons and add a full-width dark divider beneath them.
- Cover Controls settings below the navigation with a continuous walnut
  texture, including the controller illustration and binding area. Translucent
  panels keep labels readable in light and dark themes.
- Mark verified System Files with green status text and crisp green SVG checks,
  including selected rows and both light and dark themes.
- Simplify System Files to show the selected cores for consoles in the library.
  Combine shared BIOS files, separate optional files and accessories, and keep
  regional choices and checksum details available without a crowded table.
- Show firmware only under its relevant console, rather than repeating Sega CD
  BIOS files under cartridge consoles that share the same core.
- Simplify Library, Save States and Screenshots navigation into a text-first
  segmented control inspired by OpenEmu, with compact SVG icons on narrow windows.
- Refresh the README, core catalog checklist and third-party notices for
  Dreamcast, CHD decoding, new artwork assets and artwork conversion.

### Added

- Sega Dreamcast as a library console, with downloadable Flycast in the
  curated core catalog. The library now supports 33 systems and 30 downloadable
  cores.
- Flycast hardware-context negotiation with Vulkan and OpenGL. New Dreamcast
  sessions can try OpenGL when Vulkan initialization fails; existing states
  keep the renderer they were created with.
- Dreamcast GDI, CDI, CHD, CUE, M3U and ELF imports, including multi-disc
  playlists. GDI imports validate and copy every referenced track.
- Four-player Dreamcast control profiles with keyboard and controller
  bindings, sticks, digital triggers and controller illustration hotspots.
- Per-game Dreamcast VMU saves and optional user-supplied BIOS guidance for
  `system/dc/dc_boot.bin`.
- A Dreamcast Video option for accurate per-pixel transparency sorting in
  Flycast, adjustable during gameplay and remembered across launches. This is
  an optional compatibility setting and can require more GPU resources.
- Colorful, shaded SVG illustrations for settings sections, with a larger
  icon size and crisp rendering on scaled displays.
- Keep the secret Advanced settings menu unlocked across restarts until the
  user clicks the About logo four times again to lock it. The unlock animation
  and sound remain on the About logo.
- Make the About screen's unlocked notice clickable so it opens Advanced
  settings directly.
- Automatically identify supported CHD discs by their decoded data tracks,
  including Dreamcast, PlayStation, Sega CD, Saturn and TurboGrafx-CD
  signatures. Unrecognized discs retain manual console selection. The bundled
  reader requires no additional tools or installed emulator cores.
- A generated pixel-art Dreamcast console icon with a clearer disc lid and
  four controller ports, and a controller illustration
  without branding, with clickable button/stick/trigger mappings in Settings.
- Add **Advanced → Developer options → Convert existing covers to WebP**.
  Conversion runs in the background with progress, a stop button and a summary;
  the library and cover cache update automatically.
- Preserve cover resolution, game metadata and manually selected replacement
  artwork during bulk conversion. External originals and shared artwork are
  retained; unused older files inside the managed covers folder are removed.
- Skip artwork already converted at the selected quality, and support
  recompressing earlier WebP covers when a different quality is selected.

### Fixed

- Re-import discs after removing their managed files to Trash. Leftover game
  folders now receive verified missing files instead of reporting that the
  original ROM is missing; intact copies and unrelated files are preserved.
- Remove empty managed game folders after their ROMs are moved to Trash,
  including empty track subfolders. Keep external source folders and any
  directories that still contain files.
- Dreamcast manual save states and auto-save now support Flycast states larger
  than 31 MiB. Save and load share a bounded 128 MiB payload limit.
- Retain the previous hardware framebuffer when a core repeats a frame,
  preventing black frames in Flycast's OpenGL output.
- Keep Flycast rendering on the game thread for compatibility with EmuLuna's
  graphics context and frame timing.
- Keep cover titles, ratings, selection highlights and hover actions attached
  to the correct artwork after the grid wraps or changes between consoles.
- Keep missing or unreadable artwork in the console's expected box shape, and
  refresh the shape when replacement artwork is selected.
- Isolate missing or corrupt images during bulk WebP conversion so one bad
  cover does not stop the rest of the library. Cancelling leaves unconverted
  originals available, and a newer manual cover choice wins over a conversion.

### Known issues

- Ecco the Dolphin: Defender of the Future can still show polygon artifacts
  with Flycast's Vulkan renderer. Accurate transparency is available to try,
  but it does not resolve every rendering issue.
- Rayman 2 can still show a strip of incorrect image data with ParaLLEl N64's
  experimental OpenGL path. Software rendering remains a working alternative
  for this title.

### Validation

All 238 automated tests passed, including CHD identification, disc re-imports,
managed file cleanup, controller mappings, WebP conversion and cancellation,
large save states, and retained hardware frames. Grid checks cover all 33
systems and All Games at window widths from 850 to 2048 pixels, including mixed
box shapes, missing artwork, short rows, single-game and empty results.

The Linux AppImage passed an offscreen startup and screenshot check. Its bundled
CHD reader decoded five console signatures, its Flycast and Snes9x metadata
probes passed, and the packaged host exposes the 128 MiB save-state limit.

## [0.17.0] - 2026-10-02

### Changed

- Remember library and per-console game window sizes and maximized state.
  Fullscreen play does not overwrite the saved normal window size.
- Native KDE Wayland title bars now receive EmuLuna's matching color scheme,
  including game and settings windows, without replacing the desktop controls.
- Beetle PSX HW now uses its Vulkan renderer when a Vulkan device is available,
  with OpenGL and software fallbacks for new sessions. Existing save states keep
  the renderer that created them.

### Fixed

- Pause-on-window-switch now follows application focus and minimized windows,
  and changes to this setting take effect in games that are already running.
- The experimental library-minimize option now applies when any game window
  opens, stays off by default, and restores the library when disabled or when
  the last game closes.
- Quitting the last game also restores a manually minimized library, preserving
  its previous maximized or windowed state.
- Restore the library's native Wayland surface after the last game exits, even
  when Qt no longer reports that it is minimized. On Wayland desktops, both
  library and game windows now prefer Wayland with X11 as a fallback.
- Improved Beetle PSX HW Vulkan speed by reading frames through CPU-cached
  staging memory when the GPU supports it.
- F-Zero X now uses the N64 core's accurate RSP with OpenGL, restoring tracks
  that were black with the core's automatic RSP choice.
- Read Beetle PSX HW's 15-bit Vulkan frames correctly instead of stopping games
  with an unsupported image format error.
- Removed seams between 2D image tiles in N64 OpenGL rendering, including
  Mischief Makers' save-selection screen.

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
