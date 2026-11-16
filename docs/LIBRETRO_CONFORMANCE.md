# Native frontend contract audit

Compatibility is the expected result of implementing libretro correctly. A core
working in RetroArch with the same build, renderer and options is a reason to
investigate EmuLuna first. A title-specific exception is not the default remedy.
The native player still uses the existing C++ libretro bridge behind its Rust
owner-thread adapter; both sides need contract validation during this migration.

| Boundary | Current evidence | Remaining verification |
| --- | --- | --- |
| Core lifecycle/thread ownership | Normal shutdown is a typed outcome, distinct from errors; explicit reset can restart after shutdown; non-Send core owner; repeated software/GL sessions; GL context_destroy before framebuffer deletion; early close and paused close checks | Lost-context recovery and more core implementations |
| CPU/performance services | CPU-feature reporting, high-resolution clocks and a bounded profiler registry; SSE2/AVX2/AES flags checked independently against Rust runtime detection; duplicate registration preserves counter totals | Other architectures/features and actual core-specific speed effects |
| Core options | Existing catalog reused; installed build ID separate from catalog ID; saved values validated; core checksum verified | Broader dynamic options, core-defined option UI and capability accuracy |
| Software video and duplication | Original core tests verify all three pixel formats with padded rows, ignored XRGB high bytes, retained content pointers, geometry/timing updates, stereo callbacks and shutdown; hardware tests cover GL frames, NULL callbacks, no callback and temporary CPU fallback | More geometry changes and unusual first-frame sequences |
| GL state handoff | Transport uses texture unit zero and unbinds its texture, framebuffer, renderbuffer, unpack buffer and VAO after allocation/copy; a core asserts the relevant bindings before retro_run | More GL global state combinations and non-core-profile backends |
| Initial framebuffer contents | Dedicated GL check verifies opaque black across the entire new target despite incoming scissor/write masks | Depth/stencil rendering-sensitive core cases |
| GPU sharing/synchronization | Three leased textures, producer/consumer fences, UI stalls, lease retention for paused UI redraws and shutdown checks | GPU timing instrumentation, context loss and additional drivers |
| GL diagnostics | Core diagnostics counted separately; context/resource loss and frontend errors remain fatal; original core injects a harmless probe error and verifies usable pixels | A synchronous diagnostic backtrace places native Flycast's first-frame invalid-texture call inside its own glsm_ctl → retro_run; the exact cached-texture lifecycle still needs investigation |
| Native audio delivery | Bounded preallocated stereo ring, chunked transport, acknowledged pause/reset boundaries, EOS separated from underruns, burst/stall/drift checks; audio submitted before a mid-run rate change is resampled at its original rate | Audible testing, physical unplug/replug, more rate changes |
| Timing | Core rate read during execution, accumulated deadlines, smooth bounded queue-rate correction; real Saturn/Dreamcast scenes checked | Matched RetroArch pacing/presentation comparison and long sessions |
| Input | 33-system mapping reference, saved profiles, signed analog input echo, focused-device event caching and in-memory legacy migration | Multiplayer, rumble delivery and further special-key/controller cases |
| States | Checksummed existing envelope, core/game/renderer validation, atomic saves, battery-memory preservation and archived-state and current-state read tests | Original save-folder migration/Qt lock interoperability; native persistent locks, auto/explicit resume and ten slots now work separately |
| Files/content | Existing VFS bridge; read-only library snapshot; real CHD content exercised; copied writable BIOS/NVRAM; disposable diagnostics and separately retained native saves | Wider VFS/subsystem/disc-control coverage |
| Other backend interfaces | Native prototype advertises the GL core path it provides; Vulkan playback is not enabled | Vulkan implementation and a complete audit of optional capability reporting, including rumble |

The frontend's GL contract is described in the
[official libretro documentation](https://docs.libretro.com/development/cores/opengl-cores/).
In particular, the core and frontend cooperate over shared GL state; transport
objects must not be left bound for the next caller. RetroArch's GLCore driver
also clears its framebuffer/texture bindings after hardware-target allocation.

## November 2 findings

The audit found two real native frontend gaps, corrected in the worker transport:
frontend texture/framebuffer/renderbuffer bindings remained live between calls,
and newly allocated core color/depth/stencil storage was not explicitly cleared.
These fixes apply to every GL core, rather than a game profile. The updated
original core fixture asserts a clean transport boundary on every retro_run.
Nine explicit live GPU checks passed after these corrections; later lifecycle expansion brought the passing GPU/audio suite to ten.

A separate issue was over-strict error attribution: any pending core GL error
had aborted playback before frontend snapshot validation. Internal diagnostics
now remain visible/countable without automatically treating them as frontend
transport failure. This is not a claim that the underlying invalid call is fixed.

A short diagnostic-only RetroArch check used the identical installed Flycast
binary and per-pixel transparency option, with a temporary configuration, copied
system files, no SRAM load/save and no audio playback. It negotiated GL 4.3 and
ran on Mesa 26.2.5. Mesa reported a legacy GL_EXTENSIONS query and a no-VAO vertex
attribute call. These are **not** the same invalid-texture diagnostic observed
in the native player, and this was not a matched performance or gameplay test.
An initial comparison had loaded RetroArch's existing per-core options despite
an explicit config file, so that run is discarded as a matching-options check.
The corrected run redirected XDG/config/cache paths and disabled per-core
options/overrides. The existing Flycast.opt modification time remained unchanged;
the initial run refreshed RetroArch's regenerable core-info cache.

Next compatibility work should isolate remaining differences against the same
core/options and expand original contract fixtures, rather than accumulating
per-game workarounds or assuming every startup diagnostic is harmless.

The original software contract fixture also passed RGB565, 0RGB1555 and XRGB8888
row-padding/color tests, NULL/no-callback duplicates, retained in-memory content,
dynamic geometry, 60-to-50 Hz timing with a 48-to-44.1 kHz audio change, stereo
callback delivery, declared/default options and shutdown. These tests exercise
the shared bridge directly, without a game-specific profile or the UI.

Reproduce the original-core checks with `python tools/check_contract.py`
after building the host. Add `--gpu` for the desktop/audio tests and exported
preset pack. A C++ compiler can be selected with `--cxx`; pass extra compiler
flags with repeated `--cxx-flag=...`. The script compiles fixtures into a temporary
folder and never opens an installed library or user save.

The GL boundary rule now also covers context initialization, explicit reset,
serialization and restoration. A diagnostic core injects errors in all of these
callbacks, checking that valid frames and later commands still work while every
diagnostic is counted. The 120-frame control run counted seven injected
diagnostics and delivered 96,152 stereo frames: 92,106 played, 4,046 intentionally
discarded at transitions, zero missing/overflow/pending. Paused interactive close
also passed. Normal core-requested shutdown finishes a five-frame GL session
cleanly rather than reporting a frame failure. The software fixture verifies a
normal shutdown remains stopped until explicit reset, then resumes.

The shared bridge now supplies RETRO_ENVIRONMENT_GET_PERF_INTERFACE: runtime CPU
features, microsecond/nanosecond monotonic clocks, registration, start/stop and
logging. Registry operations are bounded and synchronized; counter start/stop
updates have no allocation or frontend lock. The new callback checks and the
existing four native/Python audio-clock tests passed. This improves interface
coverage; no measured speedup is claimed from enabling these optional services.

### Mid-run audio rate changes

The bundled API header explicitly states that SET_SYSTEM_AV_INFO takes effect
inside its callback, including subsequent audio callbacks in the same retro_run.
The old bridge buffered raw PCM and resampled the whole run using the final rate.
An original fixture reproduced 1,200 output stereo frames instead of 800 when
half of a 60 Hz frame arrived at 48 kHz and half at 24 kHz. The bridge now records
bounded rate boundaries and processes spans chronologically. The normal fixed-rate
path keeps its existing fractional resampler; reset/state load clear stale history.
The fixture checks both callback forms, stereo values/order, and 60/50 Hz output
counts. This proves a contract correction, not the cause of a particular game's
stutter. Matched core/scene/settings/filter comparisons are still required.
