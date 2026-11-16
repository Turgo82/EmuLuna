> Native Rust status: the active wizard uses Direct MMC by default, with
> cdrdao compatibility mode, audio-checking levels 0–3, BIN/CUE or verified CHD,
> progress/ETA and cancellation. The USB-drive compatibility profile and
> audio-only byte swapping are ported from the old reader. Full hardware
> validation remains pending. Logs remain in `cache/disc-backups/`.

# Optical game-disc backups

Open **Library → Back up disc…** or **Plus → Back up disc…**. Insert a game CD and
select its drive. Before opening the backup window, review the Section 117
archival-copy conditions and check the ownership agreement to continue. The
agreement includes links to the official law and Copyright Office backup FAQ;
it starts unchecked until explicitly accepted. Acceptance is remembered for
this library and can be cleared from **About → Disc backup legal agreement**.
A blank disc is shown while EmuLuna reads the header and
track list; a matching cover replaces it when artwork is available. Review
the title and console, then choose **Back up disc**.

During reading the wizard shows the current data/audio track, copied MiB,
speed and an approximate ETA based on recent output bytes. A subtle shimmer
moves inside the filled part of the progress bar. If no new bytes arrive for
ten seconds, ETA and shimmer pause and the wizard reports that it is waiting
for the drive. This can occur during retries or connection problems; it does
not automatically mean the read has failed. The ETA resumes when data returns.
Final track preparation and verified import show a finishing stage instead
of an inaccurate read ETA. The last reader output is retained in
`cache/disc-backups/last-reader.log` for troubleshooting, even after staging
files are cleaned up.

Failed SCSI READ CD commands stop the reader immediately, even if the helper
continues writing bytes or returns a successful exit status. Such images are
never imported.

Sega CD / Mega-CD, Sega Saturn, PlayStation and PC Engine / TurboGrafx-CD
standard single-session CDs are supported. Original Dreamcast GD-ROMs, UMDs,
DVDs and audio-only CDs are not supported by this workflow. A normal PC CD
drive cannot read the high-density area of a Dreamcast GD-ROM.

## Identification and artwork

Sega CD and Saturn headers provide game titles and product codes. PlayStation
boot information can provide a serial; cached OpenVGDB metadata resolves known
serials. No large metadata catalog is downloaded just to identify a disc.
Artwork uses the existing OpenVGDB and Libretro thumbnail sources. Unknown
titles can be entered manually; **Find cover** retries artwork with the reviewed
title and console. A missing cover or network connection does not prevent backup.

## Sega CD track preservation

The optional compatibility reader **cdrdao 1.2.4-2** reads raw 2352-byte sectors and the complete
track list. **toc2cue** preserves track boundaries and stored pregap indices.
EmuLuna converts only AUDIO track sample pairs from cdrdao's big-endian order
to the little-endian order expected by BIN/CUE; data sectors are unchanged.
This avoids an ISO-only backup silently losing the game's CD music.

The **Audio checking** selector offers levels **0–3**, with **2** selected by
default. Level 0 copies audio directly without checking (audio errors may
remain); 1 uses overlapping reads to prevent jitter; 2 adds verification;
3 adds scratch detection and repair and may take much longer. The selector
is locked while an operation is running.

Compatibility mode **2** keeps overlapping reads and audio
verification enabled, while the additional scratch detection and repair
of mode 3 is disabled to reduce prolonged retries on difficult drives/discs.
This setting applies when starting a new backup; it does not alter an active read.

The MATSHITA CW-8123 automatically uses cdrdao's compatibility profile
`generic-mmc:0x20`. This avoids combined audio/subchannel transfers and keeps
the selected audio correction level. Track indices and pregaps are searched
separately, which can take several minutes before audio bytes start advancing.
Other drives retain automatic driver selection. `--fast-toc` is not used.
In this profile cdrdao can represent the data-to-audio gap as a CUE `PREGAP`
rather than storing those sectors in the BIN. The emulator supplies that gap's
silence; those omitted gap samples are not a bit-exact capture of the original.

Before import, stored BIN sectors plus explicitly declared CUE pregaps must
match the disc lead-out. Every track number, data/audio type and INDEX 01
position (including preceding declared gaps) must match the drive's table of
contents. Missing audio sectors still fail validation. EmuLuna also rechecks
the disc identity and console signature.
There is no zero-filling, error-ignoring or forced-read option. A failed read
does not create a game entry. This is a normal emulator backup, not a certified
Redump dump; subchannel data and special copy-protection supplements are not
captured.

## Storage and cancellation

The reader runs in a private staging folder under the app's cache. A complete
backup waits on the Success screen. Choose **Done** to copy and
hash-verify it through the normal importer into the selected console library. Both BIN and CUE remain together. Temporary
files are removed on success, cancellation and failure. Backups always become
managed copies, including when normal imports are set to reference external
files. Existing identical games keep their metadata and saved-game identity.

Cancellation stops the reader subprocess and removes staging files. Once the
verified image begins its final library import, the wizard waits for that
commit to finish. It does not burn, erase or eject the original disc.

## Development and packaging

Source runs can use the system `cdrdao` package. AppImages include the reader
and converter, their copyright/license, and corresponding source packages.
`tools/prepare_disc_tools.py` downloads the unmodified Ubuntu 22.04 build and
checksum-verified source into the ignored build folder. Ubuntu build hosts
need `libao4` and `libvorbisfile3` so PyInstaller can collect their runtime
dependencies. The application needs permission to read the optical drive.

See the [cdrdao manual](https://manpages.debian.org/unstable/cdrdao/cdrdao.1.en.html),
[toc2cue audio byte-order note](https://manpages.debian.org/unstable/cdrdao/toc2cue.1.en.html),
and [Linux CD-ROM interface](https://docs.kernel.org/userspace-api/ioctl/cdrom.html).

Automated fixtures cover a Sega CD data track plus two CD-audio tracks,
pregaps, byte-order conversion, verified managed import, duplicate handling,
incomplete reads, changed discs, cancellation and the wizard/menu flow.
A physical drive and real game disc are still required for hardware validation.

## Reading progress and diagnostics

Boundary scanning now shows its elapsed seconds and the time since cdrdao last
reported activity. After a minute without reader output it says so explicitly;
it does not invent copied bytes or automatically abort a potentially recoverable
read. A successful scan still proceeds to audio extraction automatically.

`cache/disc-backups/last-reader.log` retains the full bounded reader output plus
its command, exit code, elapsed time and cancellation/error outcome. This keeps
drive/driver context that was previously lost when only the tail was saved.
`last-backup.json` records the final backup outcome, console, disc identity and
correction level, including failures after extraction (conversion/validation/
import). This distinguishes a cancelled scan from a failed or completed backup.

### Full physical-worker validation (October 30, 2026)

With Ecco the Dolphin in the reconnected MATSHITA CW-8123, the actual DiscWorker
completed extraction, real toc2cue conversion, explicit-pregap validation,
audio byte-order correction, disc-identity recheck and managed library import
into an isolated temporary library. It reported status complete with 16 tracks,
no duplicate, in 521 seconds. Boundary analysis ended at about 331 seconds;
audio extraction ended at 517 seconds. No existing user-library entry was changed.
The targeted reader/wizard suite passed 32 tests, including a real converter and
managed-import compatibility fixture, full-log preservation and final error/
cancellation reporting. The packaged AppImage startup smoke check also passed.
This confirms the tested workflow; it does not establish that every earlier
reported stall had the same cause or guarantee recovery from damaged media.

## Reader and output choices

**Direct MMC** is the default. It reads complete 2352-byte sectors using Linux
SG_IO READ CD commands, retaining the physical track starts in the CUE. It
retries a rejected batch one sector at a time and stops on an unreadable or
short sector; it never fills missing sectors with zeros. Audio stays in the
raw BIN/CUE byte order. Cancellation waits for the current drive command,
which has a ten-second timeout. This mode does not reconstruct additional
sub-indices or perform overlapping audio correction.

**cdrdao compatibility** remains the second choice. Use it for detailed track
index/pregap scanning and selectable audio checking levels 0–3 (initially 2).
The audio-checking selector applies only to this reader. Its separate boundary
scan can take several minutes on the CW-8123.

Choose **BIN / CUE** or **CHD (lossless compression)** before starting. Both
readers first create and validate a raw image. CHD output uses the standalone
MAME chdman tool to compress that image, then verifies the CHD before the
library imports it. Only the CHD is kept in the managed ROM folder after **Done**. Conversion
or verification failures and cancellation leave no new game entry; temporary
raw images are cleaned up. Compression needs additional temporary free space.
Compression and verification show a busy indicator rather than a guessed ETA.
Their diagnostics are `last-compression.log` and `last-verification.log` in
`cache/disc-backups`. `last-backup.json` also records reader and output format.

The AppImage bundles chdman 0.242, matching FLAC and utf8proc libraries, license
notices and the complete matching MAME source archives/build recipe in
`disc-reader-source`. Source checkouts can install `mame-tools` or run
`tools/prepare_chd_tools.py` using the build environment. The pinned CHD package
supports x86-64 and ARM64 Linux. See the official
[chdman documentation](https://docs.mamedev.org/tools/chdman.html).

The direct-reader prototype completed a physical Ecco backup in about 221
seconds, compared with 521 seconds for the compatibility workflow. Data before
the audio pregap matched exactly; audio matched after a constant one-stereo-
sample offset. Neither comparison establishes an archival reference offset.
The production reader/output tests additionally exercise rejected batches,
unreadable sectors, cancellation, a real CHD round trip and verified import.

### Reader/options validation (October 30, 2026)

The combined reader, wizard and progress suite passed 42 tests. A mixed-mode
Sega CD fixture compressed with real chdman, verified and extracted back to
byte-identical raw sectors. The verified CHD imported as a single managed
image; simulated verification failure produced no import. The two real CHD
checks also passed using the tool and libraries collected into the AppImage.
The test AppImage passed its offscreen startup screenshot check, and the
expanded reader/output form was visually checked for overlapping content.
No optical device was visible in this session, so the production direct-reader
plus CHD path has not yet been retested against a physical disc.


## Success screen

After the read and any CHD verification succeed, the form is replaced by a
clean completion page with a green Success heading. Cover art grows from its
review position toward a larger centered view. **Done** performs the
managed import and closes the backup window; the completed image remains in private staging until then.
An import failure keeps it available for retry without rereading the disc.
**Back up another** returns to identification. Leaving an unadded backup asks
whether to add it, discard its temporary files, or cancel. After import, the
staging image is removed and the next backup starts with cleared artwork and
metadata. Dropdown arrows and the progress fill use the selected theme.


The updated completion/agreement/reader suite passes 50 tests, including
unchecked agreement gating, explicit deferred import, retry after import
failure, discard/cancel handling, next-disc reset and progress fill matching
all three theme palettes. The refreshed test AppImage also passes its startup
screenshot check. These checks do not require or change the user's library.


The backup window uses a fixed size bounded by the available screen area.
The review form scrolls when display scaling or extra status text needs more
room; the progress area and action buttons remain pinned outside that form.
A regression check verifies fixed bounds and visible buttons after agreement,
identification and reading-status updates (51 disc-workflow tests pass).


Copyright links use the same external launcher as library folders, ROM folders,
system files, themes and screenshot actions. On Linux it runs the system
`xdg-open` (or `gio open` fallback) with the host's original library path and
without bundled Qt/plugin paths. The app's own environment and disc tools are
unchanged. Desktop session variables remain available to the launched app.
Launch failures and nonzero desktop-opener exits produce an explanatory dialog.
This follows [PyInstaller's external-process guidance](https://pyinstaller.org/en/stable/common-issues-and-pitfalls.html#launching-external-programs-from-the-frozen-application).
The targeted desktop/media/core/disc suite passes 70 tests, including a real
controlled child process that verifies the cleaned environment and filenames
containing spaces, without opening an actual browser during automated testing.


## Agreement preferences and completion

Choose **About → Disc backup legal agreement** to view the same Section 117
information and official links. The checkbox reflects the saved acceptance;
uncheck it and choose **Done** to clear that acceptance, so the next backup
opening asks again. Cancelling the review leaves the saved preference intact.
Acceptance has a version so a future change to the agreement can prompt again.
On the success screen, **Done** imports the verified image and closes the window
only after import succeeds. A failed import keeps the image available for retry.
**Cancel** remains visible during reading and CHD compression; it requests a
safe interruption and waits for cleanup. Final library commit is allowed to
finish rather than being interrupted partway through.


Completed library files use the reviewed title: for example `Sonic CD.chd` or
`Sonic CD.cue` plus `Sonic CD.bin`. The CUE points at the renamed BIN. Reserved
characters and overly long names are made safe, while the displayed game title
stays intact. Stored filenames/hashes and game inventories are refreshed; game
IDs and saved-game identity remain stable, including duplicate imports.
