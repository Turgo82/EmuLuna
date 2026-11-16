# Performance lessons from Lime Studio

Reviewed October 28, 2026 against the local project behind “Recreate FL Studio
for Linux.” Its current source is LimeStudio, rather than the thread's older
working directory. The review read its native render exchange, native audio
supervisor, Python/Qt audio feeder and Phase 4 validation notes. No Lime Studio
files were changed.

## Applied: cache diagnostic drawing work

Lime Studio caches static artwork and repaints only changing playhead/meter
regions (`limestudio/widgets.py`, `workspace.py`, and `docs/NATIVE_PHASE4_VALIDATION.md`).
EmuLuna now applies the same principle to its optional FPS overlay:

- Aggregate the ten-second history only when the diagnostic snapshot refreshes.
- Reuse the painter path across exposures; rebuild on snapshot, bounds or scale changes.
- Keep every raw timing interval for 1% lows and peak statistics. The display's
  existing smoothing/deadband and refresh cadence are preserved.
- Skip graph preparation when the FPS display is disabled.

An offscreen Qt comparison with the previous committed implementation used 600
frame intervals, including periodic 40 ms drops, then five runs of 200 widget
grabs per implementation. Median complete overlay repaint: **686.66 us before,
404.97 us after (41.0% reduction)**. Captured images were pixel-identical.
Raw measurements are in the ignored `build/benchmarks/diagnostics-cache.json`.
This synthetic result measures overlay drawing only, not game FPS or device
latency. The targeted diagnostics, playback-pacing and library-performance
suites passed all **17 checks**.

## Larger transferable designs

1. **Separate audio delivery from UI stalls.** Lime's optional native JACK path
   consumes prepared blocks using bounded native storage; Python supervision,
   plugin work and resource lifetime changes stay outside the device callback
   (`native/include/limestudio/render_exchange.hpp`, `native_audio.py`). EmuLuna
   currently feeds QAudioSink from Player.tick on the GUI thread, alongside
   retro_run and video presentation. A dedicated audio consumer could help
   preserve an existing audio cushion during UI/GPU stalls. It cannot compensate
   for a core that consistently produces audio too slowly.
2. **Preallocate and bound queues.** Lime's exchange has fixed capacity, explicit
   saturation/reset handling, and no waiting or allocation in its native callback.
   EmuLuna already reuses vector capacity and bounds pending audio, but a dedicated
   consumer should use similarly bounded storage and preserve pause/state-load/
   shutdown semantics. A lower latency setting alone is not an optimization.
3. **Measure deadlines and delivered audio.** Lime records late blocks, worker
   misses and device xruns, and validates actual audio through a private server.
   EmuLuna counts audio underruns internally; correlate them with frame timing
   before claiming a stutter improvement or changing buffer sizes.

## Implemented: independent Qt audio delivery

`emuluna/audio_output.py` now owns a preallocated, frame-aligned stereo PCM ring
and a dedicated Qt thread. The worker creates, feeds, controls and destroys its
QAudioSink on that thread. The GUI submits already-produced audio through short
bounded copies; it does not run the core ahead or move OpenGL/Vulkan contexts.
The existing latency preference and selected/default-device fallback remain.

The pending ring is capped at 125 ms. On overflow the oldest complete sample
frames are discarded and counted, preventing unlimited latency/memory growth.
The worker keeps incomplete device writes until accepted, including odd-byte
partial writes. Queue resets invalidate old pending chunks and restart the sink
before consuming new-generation data. Pause/focus loss and fast-forward suspend
output; save-state loads and device/rate changes clear stale queued audio.
Mute and volume use coalesced shared control values. Shutdown stops the worker's
sink before quitting/joining the thread; no worker calls a GUI widget.

No Python QIODevice read callback is installed in the native device callback.
The feeder is a normal Qt/Python worker, not a hard-real-time or lock-free audio
engine. It can deliver an existing PCM cushion during GUI stalls but cannot
produce missing samples when a core is consistently too slow. It has not
replaced Qt with JACK, changed the host audio server, or changed emulation pace.

The optional performance overlay now includes underruns, queued milliseconds
and overflow sample-frame counts, refreshed with the existing FPS statistics.
An asynchronous device failure produces a notice and an unavailable status.

Validation includes bounded-ring wrap/overflow and concurrent ordering, partial
writes, continued delivery without GUI event processing, pause/reset/resume,
volume, underrun accounting, failed startup, idempotent shutdown, existing
save-state/core/player tests and the NES alternating-frame regression.

A live check reused Lime Studio's disposable private PipeWire null-sink helper,
selected Qt's actual PipeWire backend, configured/linked only test-server ports,
and captured a 440 Hz stereo tone using pw-record. With GUI events withheld,
the worker submitted all 19,200 tone bytes; the capture contained 11,776 stereo
frames with peak 4,000, zero reported underruns/overflow and no output errors.
The capture includes surrounding silence. No physical audio device or host
server configuration was used. This is functional evidence, not a hardware
round-trip latency or real-game FPS/stutter benchmark.

Final regression run: **69 checks passed**, with no skips, including installed
Libretro core coverage. A final packaged AppImage ran the SNES audio fixture
for 120 frames on the private server, connected its actual output ports,
produced the diagnostic capture, and exited with status 0 without thread/timer
ownership warnings. The capture showed 0 reported audio underruns. The final
focused queue/pacing/diagnostic rerun passed 20 checks after startup/shutdown
control refinements. No real-game stutter improvement percentage is claimed.
