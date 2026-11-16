# Try the native EmuLuna preview

Run `./launch-preview.sh` from the project folder. This opens the Rust app
without replacing the installed application.

- Choose a console or collection. Switch between the grid and list at the top.
- Double-click a game to play, or use its hover Play button. Right-click for
  details, favorites, rating and its ROM folder. Stars are clickable.
  **Start from boot** skips automatic resume while keeping in-game saves.
- Search applies to the selected console/collection, including saved media.
- Open **Save States** to browse existing and native states. Double-click to
  resume a private copy. The matching core build and renderer are required;
  native Vulkan states are not supported yet.
- Open **Screenshots** to browse existing images. Double-click opens the image;
  its menu can open the containing folder.
- **Settings** offers themes, audio output, filters, native progress/resume,
  installed core selection and the runtime options in the existing catalog.
  **Reload themes** is in General for theme editing.

The gameplay overlay has stop, pause/resume, reset, save/load, ten slots,
screenshots, volume, mute and fullscreen controls. Library launches also expose the
exported screen filters in the overlay. F5/F8 save/load, F6 pauses, F7 resets,
F11 toggles fullscreen and Escape stops the game.

Native progress is kept between normal launches, with automatic resume enabled
by default. Each game's native files have an exclusive OS lock. Closing the
library requests normal game shutdown and waits for saving/cleanup. Gameplay
settings can disable retained progress or automatic resume.

Existing library saves, BIOS/system files and the original SQLite database are
unchanged. The first normal native launch starts with new native battery saves;
existing battery saves have not been migrated. Selecting an existing save state
loads a validated private copy. Native favorites, ratings and play history are
also stored separately during this preview.

Portable libraries store preferences in `settings/settings.json` and native
progress under each console's `saves`, `states`, and `screenshots` folders.
States separate core binary revisions. When the console folder is linked to
another drive, native progress stays inside the chosen library root in
`settings/progress`. Explicit `--preferences FILE` paths can isolate progress
for testing. The finite `--test-play GAME_ID` check always uses temporary saves
and does not record native history or preferences.

Theme files remain independent under `data/themes/`. Edit a theme JSON
and reload it in Settings; newly launched games also read those colors.

For bug reports, include the game, core, filter, window/fullscreen mode and what
happened. The library's Session log button shows the current temporary log;
keep the library open until it has been collected.

This remains a preview. Imports/disc backup, remaining settings/controller tools,
collection management, original battery-save migration, shared database editing,
Vulkan, multiplayer/rumble, committing first-run setup and portable packaging still need work.

Library appearance update: the sidebar stays visible at a fixed width. Open
Settings through the upper-left application menu or Ctrl+,. Settings > Cores
contains the Genesis, Sega CD and 32X low-pass switches; choose the console and
installed core. Sega CD starts off to preserve unfiltered CD audio.

A matching native automatic save changes Play to Resume. The adjacent restart
arrow asks before starting from boot; it keeps in-game saves. Automatic resume
must be enabled in Gameplay. The original application's saved states can still
be resumed through Save States.


Settings and gameplay update — November 3

The application menu now includes Help → About EmuLuna. Settings has General,
Library, Gameplay, Controls, Cores, System Files and Advanced sections with the
existing navigation icons and theme colors. Click the About logo four times to
toggle Advanced; the choice persists. The unlocked notice opens Advanced directly.
About also has the disc-backup legal review with a resettable acknowledgement.

- General: System theme follows desktop light/dark changes. Create theme copy
  makes a standalone editable package under the native settings folder's themes
  directory. Reload themes applies edits. Audio output and volume defaults apply
  to the next game; the game overlay provides volume and mute.
- Gameplay: fullscreen, focus pause, keep-awake, cursor hiding, integer scaling,
  square/system pixel aspect, filters and native progress/resume are connected.
- Controls: per-console Player 1 keyboard and controller remapping, controller
  selection, clearing and restoring defaults. Existing profiles are read; edits
  go to native preferences and apply on the next launch. More players and rumble
  are still pending.
- Cores: selected playback core, catalog runtime options (including all Sega
  low-pass switches), background official downloads, imports, removal from the
  installed list and previous-build restore. Core changes are disabled while a
  game runs. These explicit operations manage the existing cores directory;
  the regression checks use disposable libraries for management operations.
- System Files: defaults to populated consoles and selected cores; optional files
  remain hidden. Imports retain the source and do not overwrite existing BIOS.
  Green verification requires a known matching MD5, not mere file presence.
- Library/Advanced: open index/progress locations, hide empty systems, export
  inventory, audit missing ROMs/disc tracks, checksum-matched single-ROM
  reconnection via native preferences, rebuild previews and open settings folder.

The game bar hides after 2.5 seconds of inactivity. Mouse movement brings it
back; pause, open menus and dragging keep it visible. Save/load feedback is a
separate, non-interactive message inside the game window for 2.4 seconds.
Restart asks before resetting. Ctrl+P/F6 pauses, Ctrl+R/F7 restarts, F5 saves,
F8 loads, F12 screenshots and F11 toggles fullscreen. Escape dismisses restart
confirmation/menus or leaves fullscreen. The bottom-right corner grip resizes
the game client at the core's aspect ratio without a stretch-and-snap cycle.

Remaining original settings/workflows include library relocation/import/backup,
committing first-run setup, multiplayer/rumble and live settings/profile
propagation. These are not represented by inert checkboxes.
Management currently uses Linux curl, unzip, binutils and md5sum utilities;
portable packaging remains pending.


Advanced tools update — November 3

Advanced → Developer options now includes Preview onboarding. It is a read-only,
five-page native scene with fullscreen/windowed controls, continuous stars,
drifting lower clouds, shooting stars and a gently floating high-resolution logo.
Appearance, storage and core-checkbox choices are local to the preview; finishing
or closing it writes no settings and moves/downloads nothing. First-run setup
that commits these choices remains separate work.

Advanced → Developer options → Artwork conversion provides 75/85/95/lossless
WebP choices, defaulting to 85, background progress and cancellation. It converts
full-size native copies under the native settings folder's artwork namespace.
The old artwork files and SQLite index are untouched. Copies are shared by image
checksum and quality; an atomic index selects them when the native library starts.
Changing quality always reads the original source, rather than repeatedly
recompressing the previous copy. Use original artwork switches back immediately;
converted copies remain reusable. FFmpeg with libwebp is required.

Controller capture and compact layouts — November 3

Escape and Cancel binding stop both keyboard and controller capture. Leaving the
Controls page also stops capture. A modifier already held when clicking a binding
is ignored until pressed again. The controller diagram and bindings stay side by
side where they fit and stack in smaller windows; all mappings remain scrollable.
