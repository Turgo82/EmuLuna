# Rust migration

This workspace is the active EmuLuna application. The repository root launcher
runs the Rust library and native player. The previous Python/Qt application is
archived in `archive/legacy-python/` for reference. Libretro cores run on a
dedicated owning thread, with native audio delivery and GPU filtering outside
Python and Qt. Full feature parity remains in progress.

Build from the repository root:

```
cargo test --manifest-path Cargo.toml
cargo build --release --manifest-path Cargo.toml
```

SDL2 development/link libraries and the existing `libemuluna_host.so` bridge are
required. Local Unreal SDK linker overrides used for development are **not** a
portable release toolchain or AppImage.

## Diagnostic player

```
emuluna-native-player HOST_SO CORE_SO ROM NEW_SAVE_DIR BIOS_DIR FRAMES \
  [--mute] [--hardware-gl] [--core-id ID --system ID] [--options OPTIONS.json] \
  [--shader-pack DIR --preset ID] [--controls PROFILE.json] \
  [--content-digest EXISTING_GAME_ID] [--catalog-id PROFILE_ID] \
  [--overlay --theme classic|daylight|luna --theme-dir DIR] [--interactive]
emuluna-player-probe HOST_SO CORE_SO ROM NEW_SAVE_DIR BIOS_DIR FRAMES
```

Always use a **new disposable save directory**, temporary BIOS folder, and a
finite frame limit. Finite diagnostics allow 36,000 software frames or 3,600 shared-GL frames.
`--interactive` runs until the window closes instead. The headless probe reports buffer reuse and execution timing.
Commercial games used for developer measurements are never copied or changed;
BIOS files are copied to a temporary folder before testing.

`--core-id` and `--system` apply the existing core catalog's console, runtime,
accessory and renderer defaults. Explicit JSON string options override them.
Supply both IDs together. Without a system the diagnostic uses generic input.
With one, keyboard and the first standard controller use the current console
catalog's defaults, including N64/Dreamcast analog sticks and special buttons.
Controller reconnect is supported. Losing focus releases and suppresses input.
`--controls PROFILE.json` accepts normalized saved keyboard/gamepad bindings and
an optional selected controller index from the existing format. It supports
explicit unbinding, analog directions, modifiers and controller reconnect.
Additional players and rumble remain pending. The production launcher must still
apply legacy profile migrations before handing over a saved profile.

Temporary diagnostic shortcuts:

- Escape: close; F11: desktop fullscreen.
- F1: cycle nearest/linear/simple scanlines (replaces a selected full preset).
- F6: pause/resume; F7: reset; minus/equals: decrease/increase volume.
- F5/F8: save/load `quick.state` in the disposable save directory when a core and
  console identity were supplied. States use the existing checksummed envelope;
  core/game/renderer mismatches are rejected before the core receives state data.

`--overlay` enables the native egui controls for pause, reset, quick states,
volume, fullscreen and close. Messages remain visible in the game window. It
uses the selected built-in theme and redraws the last image while paused without
re-running shader effects or uploading pixels. Shared GL retains the displayed
source lease until a replacement arrives. The overlay is opt-in during migration.

For an existing library game, supply its stored content digest through
`--content-digest`. Omitting it hashes the whole ROM before launch; that warms
file cache and must not be presented as a cold disc-I/O performance measurement.

`--mute` zeros PCM while exercising the real device callback; counters therefore
measure delivery, not audible waveform quality. Native runtime does not invoke
Python. `tools/export_shaders.py` uses the existing preset preprocessing only
at **asset-build time**, retaining preset authorship and license notices.

## Ownership and transport

- Core creation, options, input, execution, reset and teardown occur on one
  worker. The isolated unsafe C ABI adapter makes its core non-Send.
- Software frames use one reusable owned native-pixel snapshot and one GPU
  upload. Input and video queues are bounded; a stalled window cannot wait on
  audio production. Input reads make at most three attempts at a coherent state.
- OpenGL core-profile playback owns a stable core FBO. A separate hidden worker
  surface shares three leased textures with the display context. GPU fences
  protect production and reuse, including dropped notifications and shutdown.
  Hardware playback has no steady-state CPU pixel readback or re-upload.
- The GPU preset renderer supports multipass, named pass textures, lookup
  textures, float/sRGB targets, history and feedback. Program/uniform/sampler
  discovery is cached; resources are reused. Targets and assets have size and
  memory limits. Parameter editing is not yet exposed.
- A preallocated 4,800-stereo-frame ring feeds 48 kHz audio. The callback never
  allocates, logs, waits for a lock, performs file IO or calls the core. Worker
  backpressure and monotonic pacing preserve PCM and the fractional core rate.
- Smooth bounded resampler correction follows audio queue occupancy, without
  changing emulation speed. Pause/reset use an acknowledged callback boundary
  to discard old PCM, with discarded samples separately accounted for. Resume
  reprimes the startup cushion. Frame generations reject queued pre-reset video.
- Audio transport publishes and consumes contiguous stereo chunks instead of
  per-sample queue operations. End-of-session silence is measured separately
  from real playback starvation. Controller state is cached from SDL events;
  routine snapshots avoid repeated controller polling calls.

Desktop GL 3.3 core is required; negotiated core-profile capabilities reflect the
actual context. Legacy/compatibility GL and Vulkan are not supported yet. Do not
assume wgpu can import arbitrary libretro GL textures or Vulkan images.

## Validation

`docs/VALIDATION.md` records actual measurements and their limits. Useful checks:

```
python tools/export_shaders.py
python tools/check_playback.py --core /absolute/path/to/nestopia.so \
  --preset slang:crt-geom-deluxe
python tools/check_shader_parity.py --dynamic
python tools/export_input_reference.py
```

The playback script also supports PAL SNES and Game Boy diagnostics. It checks
normal playback, an intentional 500 ms window stall, a one-frame session and a
headless buffer reuse run. Reference exporters require the existing app's Python
environment during testing; the Rust player does not.

Desktop-only tests are deliberately ignored in ordinary `cargo test`. Compile
the original fixtures in `tests/rust_hardware_core.cpp` (normal and
`-DRUST_FIXTURE_AUDIO_BURST`) and `tests/rust_input_core.cpp`, then set
`EMULUNA_RUST_HOST`, `EMULUNA_RUST_HW_FIXTURE`, `EMULUNA_RUST_HW_BURST_FIXTURE`,
`EMULUNA_RUST_INPUT_FIXTURE`, `EMULUNA_TEST_SHADER_PACK` and
`EMULUNA_TEST_INPUT_REFERENCE` to absolute paths before running the relevant
crate with `-- --ignored --test-threads=1 --nocapture`.

Diagnostic capture uses explicit `EMULUNA_TEST_CAPTURE` or test capture-directory
hooks and performs readback only when requested. `EMULUNA_TRACE=1` logs occasional
worker counters. CPU core/snapshot/upload/present call durations are **not** GPU
execution timestamps or compositor frame counts.

Remaining work includes sustained compatibility testing, multiplayer/rumble,
device routing/reconnect, existing-save launch/locking and autosaves, screenshot
and crop handling, Vulkan ownership, settings/library editing and portable packaging. Current results do not establish matched RetroArch performance parity.

## Native library preview

```
emuluna-native-library --library /absolute/path/to/library \
  [--data /absolute/path/to/data] [--theme classic|daylight|luna] \
  [--system saturn]
```

This developer test build is separate from the installed app. Run
`launch-preview.sh` to open the default library, or pass the arguments above.
Double-click a game or use its hover **Play** button. Choose default screen
filters in Settings or switch filters from the gameplay overlay. The native
player runs until closed; the diagnostic frame limit does not end interactive
sessions. Only one test session runs from each browser instance.

The library snapshot reader stays read-only; native imports and explicitly requested library actions write through separate transactional connections. Installed core selection, checksum
verification, saved controls/options and copied system files feed the native
player. Normal launches retain native battery/quick/automatic saves separately
under the native preferences folder; finite diagnostic launches remain disposable.
An existing state can be resumed from a validated private copy using its exact
installed core build and supported renderer. Existing battery saves have not yet
been migrated. Native favorites, ratings and play history are separate preferences.

The library now provides grid/list navigation, game menus, hover play, clickable
ratings, saved-media browsing and native General/Gameplay/Core settings. The
new overlay provides icon controls, ten save slots, live filter selection and
audio output changes. See [HANDS_ON.md](HANDS_ON.md) for storage and controls.

Console/static collection views, title/filename search, favorites/recent sorting,
the fixed-art responsive grid and standalone theme JSON palettes are available.
Theme selection and **Reload current theme** in Settings apply without a rebuild; new game windows
read that theme folder too. Editing/imports, smart collection rules, full settings,
disc backup, existing-save integration and other application features remain
pending. See `HANDS_ON.md` for the first testing steps.

Library loading and artwork headers/decoding run off the interface thread.
Only visible rows are drawn; an LRU texture cache has a 96 MiB pixel-storage
budget, with bounded request/result queues. Artwork decoding has file/dimension
and allocation limits. Actual driver allocations, font atlases and framebuffer
storage are additional. Fixed cover dimensions and a common 28-point left edge
are independent of window width; spacing between visible covers is bounded
between 35 and 64 points. Native desktop fonts are preferred, with bundled egui
fallback fonts. Portable font discovery/packaging remains to be completed.

The library snapshot reader uses a normal read-only SQLite transaction, including
committed WAL entries. Imports, metadata updates and disc backups use separate
transactional writers. The reader does not migrate or modify game records;
SQLite manages its normal journal/shared-memory files when needed. Model tests
cover literal search, collection filtering, external/relative paths, committed
WAL visibility, fixed covers and consistent alignment. The native importer keeps
existing game identities and source files.

Developer checks: `--capture NEW.png` captures and closes the preview;
`--scroll-test 8` exercises scrolling for eight seconds and reports interface
CPU work and cache usage. Normal browsing does not read pixels back.
The UI uses [eframe/egui's native Glow integration](https://docs.rs/eframe/0.31.1/eframe/).
