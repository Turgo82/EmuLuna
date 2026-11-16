# Rust migration working notes — November 1, 2026

This is an independent prototype; the installed Python application has not been
switched to it. No commits, releases or AppImages were requested for this stage.
User asked to continue between milestones without stopping for confirmation.

Implemented since the initial software slice:

- Native desktop GL presenter, immutable sampling objects, cached uniforms and
  conditional context rebinding. Simple nearest/linear/scanline modes and F11.
- Separate hidden worker GL surface sharing textures with the display context.
  Three leased textures; producer-ready and consumer-complete fences; no CPU
  waits on ordinary GPU work. Scoped worker/window ownership, bounded shutdown
  waits and automatic return of unconsumed queued leases.
- Stable core framebuffer with depth/stencil and a default VAO. GPU snapshots
  preserve NULL and absent video callbacks; runtime CPU callbacks still work.
- Native audio device code shared by software and hardware paths. A finite
  96,000-stereo-frame host snapshot limit admits real Flycast startup bursts;
  the fixed audio ring itself remains 4,800 stereo frames. Device callback never
  allocates or waits for the window. Startup/drain accounting remains explicit.
- Core catalog defaults/runtime defaults/render options and explicit JSON launch
  overrides via `--core-id`, `--system`, `--options`. No separate Rust defaults.
- Explicit diagnostic input scripting, timings, optional single-frame capture.
  Captures/readbacks are test-only hooks and disabled in ordinary playback.

Validation:

- Original software NES, PAL SNES, Game Boy fixtures previously passed audio
  delivery and 500 ms window-stall checks. The unified audio module passed the
  filtered NES check again.
- Five explicit live GL/audio checks passed: colors/orientation/resize/fullscreen;
  cross-context leases under stall; early close/restart; original libretro GL
  fixture with duplicates/CPU fallback; 19,026-sample startup burst delivery.
- Crazy Taxi / installed Flycast, HLE BIOS, disposable saves/system directory:
  1,800 frames at 1280x960 internally, scanline shader, 960x720 window: all 1,800
  displayed, no missing/overflow/pending audio, all 1,799,691 stereo frames played,
  zero CPU video bytes. Captured a real 3D attract scene, not just a static menu.
- New catalog/JSON handoff: 1,800 frames at 1920x1440 internally, scanline shader,
  2560x1440 fullscreen: all 1,800 displayed, no audio missing/overflow/pending,
  all 1,973,456 stereo frames played, zero CPU video bytes. Capture shows character
  selection. This is not a matched RetroArch comparison or full CRT preset test.
- Tests using Flycast core defaults without the catalog profile showed loading
  audio gaps. Explicitly passing the existing threaded-rendering-disabled profile
  removed those gaps in the measured runs. Do not generalize that every core or
  scene is fixed. No sustained combat/racing benchmark claim yet.

Additional completed work (November 2):

- Complete native preset chain (history, feedback, LUTs, named passes, float/sRGB
  targets), bounded GPU allocations and cached per-pass dispatch. All 23 bundled
  presets compile/render; dynamic pixel parity matches the legacy GL renderer
  for effect presets/linear; nearest has only small sampling-edge differences.
- Real Flycast at 1920x1440 / 2560x1440 fullscreen with CRT Geom Deluxe, 1,800
  frames, no missing/overflow/pending PCM and no steady-state CPU video transport.
- Monotonic accumulated deadlines and smooth bounded audio resampler correction.
  Christmas NiGHTS full-CRT fullscreen: 3,600 frames, no missing/overflow/pending
  PCM, mean core/snapshot 5.152 ms, peak 8.659 ms. Initial shorter uncorrected run
  had 70 missing samples and queue drift. Do not claim indefinite stability.
- Console input and controller reconnect on window thread. Bounded coherent
  analog/button snapshots; existing catalogs reused. 2,541 default-input cases
  match the app across 33 systems. Native input echo confirms signed axes/release.
- Worker-owned pause/reset/volume command layer and callback audio discontinuity
  acknowledgement; discarded PCM has separate accounting. Video generations reject
  pre-reset queued frames. F6 pause, F7 reset, minus/equals volume in prototype.

- Seven live GPU checks pass, including pause/reset/resume/close while paused.
  All exported presets reproduce fresh first-frame output after history/feedback
  invalidation on a new video generation.
- First native egui/Glow library window: read-only immutable SQLite snapshot,
  async metadata/header/decode work, virtualized fixed-art grid, existing theme
  JSON palettes, literal search and static collections. No launch/edit integration.
  Existing 2,545-game database SHA-256 unchanged, no sidecars. Warm scan 37–41 ms.
  Eight-second scripted scroll: 484 UI updates, mean App::update 0.133 ms, peak
  1.491 ms, max 16 game cards per update; artwork cache pixel storage <=96 MiB.
  Screenshots in /tmp/emuluna-rust-library-{classic,daylight,luna}.png.

Current work: finishing native library workflows. Grid/list navigation, sidebar
visibility, Library/Save States/Screenshots pages, right-click game actions,
clickable ratings, favorites and native play history are connected. Media indexing
runs on a bounded background worker and respects approved console storage.

General/Gameplay/Core settings persist independently. Normal game sessions retain
native battery/slot/automatic saves with exclusive locks and graceful parent-close
handling. Existing states resume via validated private copies; original saves and
database remain unchanged. The overlay has icon controls, ten slots, live filters
screenshots/state preview images, and audio output selection/recovery.

Remaining major work: imports/disc backup; complete controller/settings and
collection management; original battery-save migration/shared database writes;
Vulkan, multiplayer/rumble, onboarding and portable packaging. Physical unplug/
replug audio testing and broader long sessions remain useful.

User testing reports smooth Dreamcast playback with filters and markedly faster
app launch. Sega CD behavior matches RetroArch in the reported scene; further
Sega CD-specific performance work is deprioritized in favor of library completion.
This feedback is distinct from automated measurements.

Latest API audit: core transport bindings are cleaned and initial targets are
explicitly cleared. Original fixtures cover all software pixel formats/strides,
geometry/rate changes, normal shutdown/reset, content lifetime and CPU feature
reporting. The bridge now supplies the optional performance interface. Ten live
GPU/audio checks pass, including diagnostic injection through context reset,
state save/load and reset. Core-requested shutdown exits normally in both
native players. See LIBRETRO_CONFORMANCE.md for evidence and outstanding gaps.

Latest user-found regressions corrected: floating hover controls no longer alter
virtual row height; hover visibility uses the whole card hit area instead of the
covered parent widget, avoiding alternating-frame blinking. List/media row strides
also match their actual allocation. N64 startup resume now validates before core
creation and runs the established two initialization frames before restoration.
Bomberman Hero reproduced a resume crash before the fix and passed afterward;
GoldenEye, 1080 Snowboarding and Bomberman 64 also resumed copied states with CRT
filters. Original states remained unchanged. Start from boot provides an explicit
resume bypass without deleting battery saves.

## Library appearance pass — November 2, 2026

Compared the retained Qt toolbar, sidebar, menus and collection definitions.
The native library now has a permanent, non-resizable 260-point sidebar,
collection vector icons and counts, the application menu with Settings (Ctrl+,),
compact grid/list controls, and content-centered rounded section tabs using
navigation theme colors. Removed the large toolbar brand and separate Settings
button. Recently Added now uses the existing app's 30-day cutoff.

Cover outlines have 8 points of internal top clearance. Virtual row height and
hover visibility remain stable while the floating control owns the pointer.
A background index validates automatic-state headers against the selected
core/build/game/renderer. Matching states show Resume with an adjacent restart
icon; restart requires confirmation and preserves battery saves. Core hashing
and state-file inspection stay outside painting. Original saves remain read-only.

Sega low-pass controls use the shared catalog in Settings > Cores. Genesis and
32X default on; Sega CD defaults off in both Genesis Plus GX and PicoDrive.
Descriptions explain that enabling the CD core filter affects CD music too.
A handoff test covers both switch values for all five supported core/console
combinations, including imported core build identities.

Validation: 16 targeted tests (library, launch configuration and layout model),
strict Clippy, release build; native captures checked in Classic and Luna.
Imports/backup and the rest of the original settings/menu tools are still
pending; this is a library appearance pass, not full feature parity.

## Settings, About and gameplay window — November 3, 2026

Rebuilt the settings shell around seven icon/text sections and the retained Qt
layout/assets. Modules separate About/legal, bindings, firmware/audit and core
management. Settings are screen-bounded, scrollable, themed and backed by native
preferences. Added standalone user theme copies/discovery and desktop theme
following. Existing library index remains immutable; explicit core management
uses the shared install lock/history, checked ELF exports, content-addressed
binaries and atomic manifests. Firmware imports are non-overwriting and verify
known MD5s. Audit reconnection checks the single-ROM digest and stores a native
path override. Inventory export writes only to the user-selected destination.

Connected volume, fullscreen, focus pause, screen-saver inhibition, cursor hiding,
integer scaling and square/system aspect to software and hardware playback.
The SDL client uses a ratio-locked corner grip, avoiding compositor resize snaps.
The hover bar now follows the old 2.5-second timer; messages are independent
2.4-second non-interactive in-window areas. Added reset confirmation, legacy
Ctrl+P/Ctrl+R/F12 shortcuts, menu Escape handling and Escape to leave fullscreen.
No core pacing, audio-clock or GPU transport changes were introduced.

About reuses the high-resolution logo, version/credits, direct browser links,
legal review and persistent four-click Advanced toggle. The unlocked notice
opens Advanced. Settings > Cores retains all five Sega filter profiles and
console-specific defaults.

Validation: targeted unit tests, strict Clippy and release builds; real UI captures
in build/rust/ui-previews; CC0 core import/retained-build check; live GL checks for
bar timing, independent notices, focus pause, volume and ratio-locked resize;
both software and OpenGL persistent session checks pass. Original commercial
library saves were not used for these regression tests. Network downloads and
interactive file chooser interactions have not been exercised end-to-end.

Pending original workflows: library relocation/import/disc backup, onboarding,
WebP conversion, extra controller ports/rumble, audio-buffer/fast-forward controls
and live settings propagation. Do not call this full feature parity yet.

### Advanced continuation

Implemented the read-only native onboarding preview and WebP conversion tools.
The preview's animation clock survives all five pages; manual core selection has
console icons/checkboxes and the fullscreen state is restored when it closes.
Branding/controller previews use a larger decode limit than ordinary box covers.
The native WebP store is separate and content-addressed: quality-specific files,
atomic/checkpointed index, per-library conversion lock, bounded input/decoded
images, sanitized PNG input for the encoder, cancelable one-frame FFmpeg jobs,
and source-based re-encoding. Library snapshots retain original cover paths and
apply valid native overrides in the loader or when a job finishes. No original
artwork is deleted. A button restores original covers without deleting copies.

Tests: 15 native library tests pass, plus the explicit real-encoder fixture for
all four qualities, lossless RGBA equality, unchanged dimensions/sources and
cancel preserving previous results. The setup preview renders every step in a
headless UI test with an unchanged animation clock and an empty temporary library
folder afterward. Strict Clippy and release build pass. Real screenshots inspect
onboarding, manual console selection and Advanced tools. Actual committing
onboarding/library relocation/import/disc backup remain pending.
