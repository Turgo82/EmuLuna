# Native playback architecture and implementation handoff

Date: November 2, 2026. Status: independent Rust software/shared-GL playback,
full GPU presets and native audio/input implemented; application integration pending.
The existing application still uses its current player.

## Decision

User decision, November 1: implement the replacement in **Rust**.

Target a native Rust playback engine and an egui-based native UI. Reuse the
unmodified C/C++ libretro cores through a small audited C ABI boundary. Keep
unsafe core/GPU interfaces isolated from the safe session and application code.

The Rust workspace is under `rust/`. It executes software cores on their owning
worker, delivers native device audio through a bounded stereo ring and presents
reusable owned frames through a Rust OpenGL presenter in an SDL diagnostic
window. Simple nearest/smooth/scanline modes run without CPU readback. Session contracts
also define lifecycle state and frame generations. See `docs/RUST_DEVELOPMENT.md` for
reproducible checks and current limitations. Shared OpenGL transport and the complete bundled preset chain are now implemented
in the independent prototype. Vulkan and the final egui UI remain pending. See
`docs/VALIDATION.md` for measured results and their limits.

Prove same-API GPU transport before committing the player to wgpu: OpenGL core
textures and Vulkan images require compatible contexts/devices and explicit
synchronization. PhotoCraft's egui/wgpu design is a reference, not proof that
arbitrary libretro GPU resources can be imported without copies.

The existing application can launch the Rust player as a separate process during
migration. Move timing, input, resampling, audio delivery, shader dispatch and
frame transport out of Python. Then migrate the remaining UI with feature parity.
The architecture and acceptance requirements below remain applicable. References
to the earlier C++/Qt target in implementation details are superseded by this
Rust decision; Qt integration is now a legacy comparison, not the target design.

## Confirmed architecture costs and uncertainties

These are source observations. Their relative cost must be measured; do not
claim Muse's suggested 20–30% saving without a controlled measurement.

| Area | Current path / source | Consequence |
|---|---|---|
| Scheduling | `Player.tick()` and `advance_playback()` in `emuluna/player.py` | Core execution, frame handling, input and presentation requests share the Qt GUI event loop. Moving only the audio sink did not isolate audio production. |
| Software frames | `native/libretro.cpp::video`, `Core.frame`, `Player.tick` | Native format expansion, a `ctypes.string_at` snapshot and an owned QImage copy precede upload. The safety copy must not simply be removed. |
| GL cores | `HardwareCoreDisplay.capture` | Synchronous `glReadPixels`, CPU buffer/bytes ownership copy, QImage copy, possible vertical mirror, RGB32 conversion, then GPU upload to the display context. Contexts are not currently explicitly shared. |
| Vulkan cores | `VulkanHost::capture`, called inside the native video callback | Image-to-buffer transfer, per-frame command allocation/free, queue-wide idle wait, mapping/invalidation, scalar pixel conversion, Python bytes/QImage handling and display upload. The buffer/memory are already reused when capacity is sufficient; do not claim they allocate every frame. |
| Vulkan synchronization | `native/vulkan_host.h/.cpp` | A single sync index/mask and queue-idle-based `wait_sync` underpin the current synchronous design. Making just capture asynchronous would invalidate its lifetime assumptions. |
| Display API crossing | `vulkan_view.py`, `filter_processing.py`, `Screen.update` | The Vulkan display accepts QImages. Its filtered route runs the GL filter, reads its FBO back to an image, then uploads through Qt Quick. Choosing Vulkan in both settings does not currently establish a shared device or GPU-only path. |
| Filter CPU dispatch | `shader_renderer.py` | Python constructs per-frame/per-pass parameter and texture dictionaries and issues many ctypes GL calls. One presentation uniform location is looked up repeatedly. Compile-on-selection, sampler, texture and FBO caches already exist; preserve them. Measure FFI/GIL/driver costs instead of assuming shader compilation happens every frame. |
| Shader work | `prepare_outputs` / preset pass scales | Intermediate pass sizes, float formats, feedback/history and output resolution can change work substantially. A filter can be GPU-bound even after transport is fixed. |
| Input/rumble | `gamepad.py`, `Player.tick` | SDL global updates occur via each pad object; Python performs repeated button/axis/native rumble calls. Poll globally once per input epoch and coalesce unchanged rumble commands in the native runtime. Magnitude unmeasured. |
| Audio | `audio_output.py`, `audio_clock.py`, native resampling loop | PCM crosses Python objects, locks and a 4 ms Qt/Python feeder timer. Resampling is native but scalar and accumulates a source frame before conversion, including high-rate cores. Distinguish delivery, production and resampler costs. |
| Qt/compositor | `video_screen.py`, player overlays and native surface presentation | The redundant parent repaint fix was measured, but composition, additional FBOs, driver waits and GUI-thread jitter still require accounting. Native C++ alone does not remove Qt composition. |
| Other frame consumers | N64 automatic crop, screenshots, save previews | These currently depend on CPU images. They must not silently force the new GPU path to download every frame. |
| Diagnostics | `GameplayDiagnostics` | Current “Emulation” time includes frame capture/snapshot. Vulkan readback even happens inside `retro_run`'s video callback. Existing numbers cannot identify pure core work or GPU execution time. |

Prior Panzer Dragoon tests established that frontend overhead could starve sound
despite core headroom. The parent-repaint fix improved a measured opening
sequence, not every combat scene. Sega CD stutter reproduced by the user in
RetroArch and two cores at fixed video timestamps remains a separate unresolved
content/core/system comparison. Do not attribute every symptom to one mechanism.

## Target ownership and data flow

```text
Rust/egui application / player UI
    commands, keyboard snapshots, low-rate status only
                       |
Native session owner thread
    core lifecycle + input epoch + retro_run + native resampling
             |                              |
    bounded native PCM ring           leased video frame
             |                              |
Native audio device callback     Native graphics/presentation owner
                                        GPU filter chain
                                        native window surface
```

Use one native player process per game, as today. A single session owner thread
serializes core init/load/run/reset/options/save/load/flush/unload/deinit. Do not
create a core on the GUI thread and casually move only `retro_run` elsewhere.
No UI callback may mutate or destroy a core concurrently with execution.

An independent native audio consumer drains a preallocated PCM ring. GUI pauses,
notifications, settings repaint and window dragging must not be responsible for
producing or delivering its next block. No allocation, logging, file IO, Qt UI
calls or Python callback is allowed on a device callback. Retain selected/default
output routing, reconnect behavior, mute, volume and explicit pause semantics.

Use a monotonic, fractional core-rate schedule with bounded recovery. Audio
occupancy/consumption informs gradual resampling correction; display completion
must not become an accidental second emulation clock. Preserve PAL/NTSC cadence
and normal emulation speed. Do not hide sustained overload with large buffers,
overclocking, repeated silence injection or silent resolution reductions.

### Frame ownership contract

Define native `FrameDescriptor`/`FrameLease` types before renderer integration:

- Generation, emulation sequence, video-content sequence and monotonic timestamp.
- Dimensions, pitch, native pixel format, active source rectangle, aspect,
  orientation and color-space metadata.
- Storage kind: owned CPU slot, shared GL texture or Vulkan image on its device.
- Producer-ready synchronization and consumer-complete release.
- Explicit ownership: consumers cannot free a core-owned texture/image, and the
  producer cannot overwrite a resource still in use by the GPU/presenter/history.

Start with a bounded three-slot transport. Three allocated slots do not mean
three frames should always be queued; target shallow presentation latency.
Specify reset, resize, context-loss and shutdown transitions with generations
and deferred resource retirement. No raw QImage borrowed from reusable native
memory without an owner. A previous local experiment showed that this PySide
build can mutate supplied Python bytes via QImage; removing `.copy()` is not a
safe memory-ownership design.

Software cores get one ownership copy of ephemeral callback pixels into a
reusable native slot. Keep source format/stride where practical and convert on
the GPU, then upload once. CPU-only fallback remains supported.

For GL cores, use a validated share group and producer-owned FBO textures. Native
shader rendering consumes a leased texture directly. GPU-side copies/blits for
immutable history or ownership are acceptable; “GPU-only” does not require a
literally copy-free pipeline. Producer/consumer fences and flushing must prevent
simultaneous sampling/overwrite. No steady-state `glReadPixels`, CPU mirror or
QImage is permitted on this path. Verify compatibility/core profile sharing on
the actual driver and fall back explicitly if unsupported.

For Vulkan, use the same logical device for core output and its filter/present
path. Bootstrap presentation-capable instance/device/queues with the required
surface extensions and libretro device-negotiation contract. The current
headless device is not automatically presentation-ready. Implement rotating sync
indices, resource lifetime, semaphore consumption, layout transitions, queue
family transfers and externally synchronized queue submissions. Replace
queue-wide waits with per-slot synchronization. Global idle waits are reserved
for exceptional lifecycle operations, not ordinary playback.

Do not feed a Vulkan image to an unrelated GL context or a second Vulkan device
by reinterpreting its handle. CPU readback remains an explicitly diagnosed
compatibility path until native interop is proven.

### Window and filter boundary

The native player should own its graphics surface and presentation lifecycle.
Prototype native Rust OpenGL/Vulkan presentation with explicit surface ownership.
Keep platform-window creation/destruction on the required window thread and
emulation independent of its event loop. Pin the graphics/window dependencies.
The SDL software diagnostic is a temporary transport test, not the final UI.

The Vulkan prototype must explicitly own swapchain submissions and queue
synchronization. Do not hand an arbitrary core device to egui/wgpu and
assume its submissions participate in our queue mutex. Test surface/device
negotiation before porting player controls.

Plan HUD integration as a bounded graphics input: native GPU controls, or a small
UI texture refreshed only on changes, composited by the presenter. Avoid a full
window CPU raster overlay or always-on full-frame backing-store upload. Native
popup dialogs remain UI operations. Preserve keyboard/controller navigation,
focus-pause behavior, fullscreen, aspect lock and resize handling.

Move existing filter dispatch into native code: compile programs on selection,
cache uniform locations/bindings/pass layouts, update only dynamic values and
batch GL work without Python FFI per operation. Measure native submission cost
separately from GPU pass cost. Use GPU timestamps without waiting synchronously
for each result. Preserve temporal feedback, history, LUTs, sRGB/float formats,
pass resolution and preset parameters.

Evaluate librashader's C API for the native Slang GL/Vulkan chains instead of
inventing a new Slang compiler. It supports those APIs, but this is not a claim
that it accepts every bundled legacy GLSL preset. First test the actual preset
catalog, packaging and public-API requirements; retain the native GL legacy
adapter for GLSL presets where needed. Keep existing filter IDs stable.

### Backpressure, timing and compatibility constraints

Use bounded frame notifications and native ownership; never queue an unbounded UI event
carrying a full image for every frame. Normal-speed operation must meet the
video cadence without systematic drops. If presentation truly cannot keep up,
report distinct emulation/presentation rates and dropped/repeated frames. Audio
must not wait indefinitely for a GUI consumer to release a video slot. Define a
safe discard/scratch render target when transport slots are occupied.

Do not adopt a naive latest-frame-only mailbox without testing NES damage
blinking: sampling every second frame can permanently hide one phase. Require
real rendered-color checks under transient stalls and tests for the known Qt
Vulkan half-rate case. Under sustained physical 30 Hz presentation, showing all
60 Hz phases is impossible without visual resampling. Diagnose/fallback from a
broken presenter; do not silently introduce frame blending or promise otherwise.

NULL/omitted libretro video callbacks retain the last valid image while audio
continues. Keep emulation, content, presentation and temporal-filter sequences
separate; do not advance history twice from a widget repaint or reset it on a
duplicate. Specify and test duplicate-frame shader timing explicitly.

Screenshots/save thumbnails request an owned snapshot at a defined session/frame
boundary; asynchronous data must carry the matching generation and sequence.
No permanent readback to support occasional previews. N64 border detection uses
GPU crop coordinates plus a bounded small asynchronous probe during calibration,
or an explicit temporary compatibility path, then stops probing once settled.
Preserve the current dark-scene gating/session lock. Color/orientation and
max-FBO-versus-active-image UVs must be validated, not guessed.

Keep existing save/VMU/battery/state identities and renderer compatibility.
Starting a new GL session is not interchangeable with resuming a Vulkan-tagged
state. Library migration and disc backup are out of scope for the runtime change.

## Implementation order for the next coding model

Each numbered item is a reviewable change with its own acceptance evidence.
The first task is instrumentation, not the full rewrite. Do not merge stages
into a single replacement that cannot be compared against the working player.

1. **Record a reproducible baseline.** Add a bounded trace mode to the existing
   player/native host, disabled by default. Record native run wall time, host
   video-callback/capture time, resampling, Python frame/input work, image copies,
   shader submission, GPU execution where available, Qt raster/composition,
   presentation waits, audio ring/sink occupancy and post-startup starvation.
   Vulkan callback work is nested inside run time: do not add it twice or label
   inclusive time “core CPU.” Record thread CPU separately where supported.
   Quantify FFI counts/bytes transferred and avoid per-frame console logging.
2. **Create the Rust native session skeleton.** Build a separate native player target
   and versioned launch/command/status protocol. Reuse core loading and file
   contracts. Implement lifecycle ownership, CPU frame leases, native PCM path,
   input snapshots and low-rate statistics. Use the original diagnostic cores
   and temporary libraries before running user games. Prototype Rust/AppImage packaging early; keep the legacy Qt player isolated.
3. **Prove both GPU transports.** GL shared-texture test first; Vulkan same-device
   output/filter/present test next. Initially use nearest presentation and an
   original test shader. Trace must show zero steady-state GPU-to-CPU video
   transfers. Validate resize, fence reuse, duplicates, device/context loss and
   consumer shutdown. Treat this as a required architecture review gate, not a
   detail for the implementation model to improvise during a UI port.
4. **Integrate the native filter chains and scheduling.** Implement the full
   source-to-filter-to-window path on each API. Keep an explicit readback fallback
   for unsupported combinations. PBO/staging rings belong there or to on-demand
   snapshots; they are not the default performance target. Immediately mapping
   a newly filled PBO, or replacing `vkQueueWaitIdle` with an equally blocking
   wait every frame, does not satisfy this stage.
5. **Match playback features.** Port player commands/HUD, state/thumbnail flow,
   audio device routing, profiles, rumble, aspect/crop and pause/fullscreen.
   Send configuration commands/status, not video or PCM, across the existing
   player-process boundary. Keep the legacy player selectable for comparison
   until correctness and performance gates pass.
6. **Promote the player, then migrate the remaining UI if desired.** Preserve the
   current library/schema, standalone theme assets and user workflows. Port the
   library/settings UI to Rust/egui incrementally with feature-parity checks.
   Its language percentage is not a playback acceptance criterion.

## Benchmark and acceptance contract

Dreamcast plus filters is a demanding benchmark for the shared architecture,
not a Dreamcast-only fix. Compare EmuLuna and RetroArch on the same machine with
the same ROM hash, core binary, BIOS/HLE choice, state/scene, internal resolution,
output resolution, filter pass settings, transparency accuracy and refresh mode.
Log all settings; never obtain parity by quietly lowering quality. Treat the
user's current RetroArch result as a report until this controlled baseline exists.

Measure at native, 2x and 3x internal resolutions where both players/core support
them; use matching 1080p/1440p outputs where available. Include filters off, CRT
Geom and a bundled multi-pass/history preset. Distinguish 30 fps game content
from the libretro core's nominal run rate. Warm up before measuring, but report
shader-switch/startup stalls separately rather than hiding them.

Use at least three 120-second runs per performance-critical configuration. Log
median/p95/p99/max wall time, actual presentations, native audio starvation,
overflow, transfer bytes and bounded CPU/VRAM usage. Record profiler overhead in
an uninstrumented comparison. A preset whose shader GPU work alone exceeds the
frame budget cannot be fixed by removing Python copies; record that distinction.

Required additional coverage: Saturn Panzer Dragoon combat and opening video,
Christmas NiGHTS video, NES Wizards & Warriors blinking, N64 GL and software
modes/crop, Sega CD movie and CD audio, and a high-native-audio-rate core such as
SameBoy. Exercise GUI load/resize, diagnostics on/off, filter changes, pause,
mute, audio rerouting, save/load, fast-forward and shutdown with frames in flight.

Go/no-go requirements:

- Stable nominal emulation rate where the matched RetroArch run achieves it.
- No growing post-startup audio starvation/overflow in those stable scenes.
- GPU core fast paths have no steady-state readback or full-frame Python/IPC
  payload; software frames perform one ownership copy and one upload.
- p99/presentation results move toward the measured reference; investigate
  material gaps instead of declaring success from average FPS alone.
- A deliberate 100 ms UI-thread stall does not stop native audio production when
  the core itself has headroom; video interruptions are counted honestly.
- Blinking, duplicate frames, shader history, color, orientation and borders
  remain correct. Memory stays bounded and lifecycle tests reveal no stale GPU
  handles, races or state/save corruption.

No “all performance issues fixed” claim until the matrix passes. A source-video
artifact, an expensive core, a driver fault and frontend overhead are different
failure classes even if all sound like stuttering to the user.

## Sources consulted

- PhotoCraft architecture: https://github.com/storytold/photocraft/blob/main/docs/architecture.md
- PhotoCraft desktop configuration: https://github.com/storytold/photocraft/blob/main/apps/photocraft/Cargo.toml
- PhotoCraft GPU device/version coupling: https://github.com/storytold/photocraft/blob/main/crates/gpu/Cargo.toml
- Libretro OpenGL ownership: https://docs.libretro.com/development/cores/opengl-cores/
- Libretro lifecycle/thread-safety: https://docs.libretro.com/development/cores/developing-cores/
- Libretro Vulkan image/sync contract: `native/vendor/libretro_vulkan.h`
- Qt shared-context/thread rules: https://doc.qt.io/qt-6/qopenglwidget.html
- Audio/display synchronization: https://docs.libretro.com/guides/optimal-vsync/
- Native Slang library and C API: https://github.com/SnowflakePowered/librashader

These sources establish supported mechanisms, not measured speedups for EmuLuna.

### Hands-on developer build

The independent native library now launches the native player with installed
core verification, runtime option validation, saved first-player controls and
runtime theme palettes. Sessions use copied system files and fresh temporary
saves, keeping the existing library read-only. A native egui overlay handles
playback commands and reuses the last completed GPU image for paused redraw;
hardware source leases remain held through every UI read. This is an isolated
hands-on build, not the installed app's replacement. See `docs/HANDS_ON.md` and
`docs/VALIDATION.md` for instructions, evidence and outstanding parity work.
