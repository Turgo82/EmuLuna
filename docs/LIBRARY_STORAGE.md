# Library storage

New libraries group game files by console:

```text
library/
  library.sqlite3
  consoles/
    snes/
      games.json
      roms/
        Super Example.sfc
      saves/<game-id>/
      screenshots/<game-id>/
      states/<game-id>/libretro/<core>/<build>/
  covers/
  cores/
  system/
  themes/
```

`library.sqlite3` is the authoritative index. Each console's `games.json` is a
readable inventory with game titles, IDs, ROM paths, SHA-256 hashes, file sizes,
and save/media locations. Inventories update on imports, file consolidation,
relinking, renaming and removals. **Settings → Library → Rebuild game inventories**
refreshes snapshots for audits. Editing a snapshot does not edit the library.

Single-file ROMs and CHDs share the console's `roms/` folder. Different games
with the same filename get a short ID suffix rather than overwriting one
another. Multi-file CUE, GDI and playlist sets have their own folders underneath
`roms/`, preserving every relative track reference. Game IDs remain content-based;
changing the layout does not change ratings, favorites, collections, cover art,
or save-state core compatibility.

External games remain in their original locations when copying on import is
turned off. Their inventory entries are marked `external`. Copies are managed
inside the library; source downloads are never moved by library organization.

## Moving console files

Console grouping is the default for every new library. **Settings → Library →
Move library…** relocates `consoles` to a selected folder or drive. ROMs, battery
saves, states, screenshots and console inventories move together. The database,
settings, covers, themes, BIOS files and downloaded cores remain in the app's
original data folder. External ROMs outside `consoles` remain external.

The move copies files in the background and verifies every copy with SHA-256,
including unindexed files in the console folder. It works across filesystems and
refuses an existing destination `consoles` folder. After verification, an atomic
pointer switch makes the new location active and removes the original copies.
A journal restores the old pointer before commit or completes cleanup after
commit if interrupted. If old files were changed or cannot be deleted, they are
retained with a recovery record rather than blocking the working new library.

`console-location.json` records the selected location. An app-managed symlink at
`<library>/consoles` preserves the relative ROM and media paths in the index.
Only that recorded location is permitted outside the app data folder. Use the
move button again to change drives; do not edit this pointer by hand. Reconnect
an unplugged console drive before playing, importing or moving its files. The
local game list and audit remain available offline; rebuild inventories after
reconnecting if you edited entries while the drive was missing.

## Existing libraries

Existing libraries keep their layout until you choose
**Settings → Library → Move library…**. Choose a folder or drive that will contain
your `consoles` folder. Older libraries are organized by console before relocation.
Close running games first.
Game locks are checked one at a time, while a library gate prevents new games
from starting during the move. This keeps large libraries within the system’s
open-file limit. Migration opens a library check first. Missing ROMs or disc tracks are listed
by game; resolve them before continuing, then confirm the move. The app backs
up the database into `backups/`, verifies managed ROMs, moves
indexed game files and known media, then commits their new paths. It refuses to
overwrite existing files. Unrelated files and ambiguous legacy screenshots are
left in place; only empty old game folders are cleaned up.

A journal lets the next launch recover an interrupted operation. Before the
database commit, moves are reversed; after commit, cleanup and inventories are
finished. File moves use hard links within the library filesystem, so there is
no second ROM-sized copy. If a library spans filesystems, the move fails safely
and returns to the old layout.

Readers continue to recognize legacy save-state and screenshot locations.
Existing core-build subfolders and game IDs are retained. The top-level `saves/`
folder also holds game locks shared by both layouts.

## OpenEmu comparison

OpenEmu uses a central database and groups ROMs by system. Its save states live
in a separate Save States root, organized by system and game, and screenshots
have their own root. EmuLuna uses the requested console grouping to make each
console easier to audit, with a readable JSON inventory alongside the database.
No OpenEmu application code was copied for this storage implementation.

Sources: [OpenEmu library database](https://github.com/OpenEmu/OpenEmu/blob/master/OpenEmu/OELibraryDatabase.swift),
[ROM records](https://github.com/OpenEmu/OpenEmu/blob/master/OpenEmu/OEDBRom.swift).

## Audit and repair

Unlock Advanced settings and use **Advanced → Library audit → Audit library…**.
The normal scan checks missing ROMs, missing disc tracks and changed disc sizes.
Enable **Verify file contents too** to compare content hashes, including
cartridges whose files still exist. Checks run in the background.

Select an issue and choose **Locate original ROM…**. For a multi-file disc,
choose its CUE, GDI or playlist descriptor with the original tracks beside it.
A replacement must match the existing game's identity; unrelated ROMs are
rejected so save data cannot be silently attached to another game. A successful
repair keeps the game ID, ratings and favorites, updates the inventory and
rescans the library.

**Remove selected entries…** asks for confirmation and removes only the index
entries. It keeps source ROMs, battery saves, states and screenshots. Nothing
is removed automatically by an audit. Reconnect a missing drive before removing
entries if you intend to keep using those games.

The migration check is available even when Advanced settings are locked.
**Continue move** stays disabled until a complete scan has no issues.
The final confirmation shows the destination and explains what moves. Legacy
organization also keeps a database backup.

## First-run setup

New libraries show a five-page Setup Assistant: Welcome, Library location,
Appearance, Emulator cores and Ready to play. Welcome has a native Qt animated
large logo reveal, twinkling stars and shooting stars from varying positions across the screen every seven seconds
(starting after three seconds), with a vivid night sky, layered procedural clouds drifting at different speeds and a gentle logo wobble.
The setup layout adapts to narrow windows and widescreen displays. Fullscreen
or F11 expands it; Escape returns to the window. Page changes fade in. The
logo and sky animations continue across every setup page without restarting and stop when the window closes. Back and Next keep choices while
navigating. The final **Let’s go!** applies them; a failed folder move keeps setup
pending and displays the error. Dismissing setup leaves it pending for next
launch. Existing libraries do not receive a pending flag on update.

**Advanced → Developer options → Preview onboarding…** runs the same pages in
preview mode. It never applies choices, resets first-run completion or moves
files or downloads cores. In normal setup, users can choose immediate core downloads
or automatic recommended downloads when games are subsequently imported.

The step-by-step flow and centered setup panel are inspired by
[OpenEmu’s Setup Assistant](https://github.com/OpenEmu/OpenEmu/wiki/User-guide:-Getting-Started).
No OpenEmu source code or artwork was copied.

When automatic core downloads are off, onboarding shows a console checklist
with icons. Selected recommended cores download on completion; shared cores are
downloaded once and already installed cores are kept. No selections means manual
installation later. Failed downloads keep setup pending and allow retrying or
changing selections. Developer previews never download cores.
