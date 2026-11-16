# Sega CD / Saturn performance investigation

October 30, 2026. Measurements used isolated temporary libraries and saves;
the user's original library, games and saves were not changed.

## What was checked

- Sega Classics (Genesis Plus GX), entering the disc from its BIOS boot screen
  with Start and running through the Sega logo into the intro/menu sequence.
- Sonic R (Beetle Saturn) opening sequence; a specific slow race/scene is still
  needed for a gameplay reproduction.
- Native core execution, fullscreen OpenGL presentation with CRT Geom, and
  the packaged application. The display used the AMD hardware renderer.

The first Sega CD measurements were still at the BIOS boot screen and are not
movie-performance evidence. A subsequent 1,800-frame test entered the disc.
Its steady rate was about 59.92 FPS, with ~0.76 ms average core work and ~1 ms
99th-percentile core work, against a 16.69 ms budget. Sonic R's sampled opening
used ~4–6 ms average core work against a 16.72 ms budget. These measurements do
not reproduce or rule out a slowdown in a later scene or under different load.
Startup-inclusive average FPS is lower and should not be mistaken for steady
emulation speed.

Comparing swap intervals 0 and 1 did not establish a major Sega CD performance
benefit, so synchronization and normal-speed frame presentation are unchanged.
No frame skipping, overclocking, or core-accuracy reductions were enabled.

## Changes

The audio feeder previously wrote at most one 4 KiB chunk per callback even
when the device had much more free space. It now refills that space in up to
eight bounded writes. Partial/zero writes stop the batch, and pending bytes
are retained. A controlled test confirms a 16 KiB refill in one callback
without spinning on a blocked device. This applies to all cores.

Real Sega CD intro runs reported zero underruns but roughly 2,300 discarded
sample frames, largely at startup; the refill change did not demonstrate a
reduction in those drops. It is a bounded-recovery improvement, not proof that
the reported stutter is fixed. Further reproduction/clock analysis is needed.

With the performance display enabled, Emulation shows average and peak wall
time for core execution plus its frame/audio snapshot, over the last 120
samples. Frame budget is the core's target period. These are separate from
presentation intervals and 1% lows. No GPU-time claim is made. Timing collection
is disabled when FPS diagnostics are off and clears on pause/fast-forward.

A zero-copy QImage experiment was rejected: this PySide build permits QImage
writes to modify Python bytes supplied to its constructor. The existing owned
frame copy remains, preserving image safety and alternating-frame effects.

The backup wizard's scroll viewport now inherits the dialog surface, avoiding
an inset dark rectangle in light/dark themes. Rendered pixel checks cover the
three named themes; normal backup controls remain fixed outside the viewport.


## Further shared timing work

The subsequent audio-failure report revealed a separate compatibility bug:
PySide 6.11.2 exposes distinct `QAudio` and `QtAudio` enum types. In this build,
the sink's state signal uses `QAudio::State`, while its error getter can return
`QtAudio.Error`. Equal numeric codes from those types compare unequal in Python,
so even `NoError` could be mistaken for a fatal output error. State/error checks
now compare numeric enum values, retaining compatibility with older Qt versions.
Intentional stream resets suppress intermediate stopped notifications and verify
the restarted device. IdleState also counts starvation on newer Qt versions
that no longer set UnderrunError.
Earlier zero-underrun readings used the mismatched enum comparisons and cannot
be used to rule out audio starvation; queue-discard counters are independent.

Running games reopen a failed stream after bounded backoff, up to three attempts
per persistent failure. A device/preference change or ten seconds of healthy
output resets the recovery allowance. Replaced-stream notifications are ignored.
Volume, mute and pause survive recovery. Initial/open/write errors retain their
actual error names in diagnostics.

Validation: 69 runtime/UI checks passed. A muted host-device smoke test delivered
PCM, injected a disconnect notification without changing the system audio service,
reopened the real output once, delivered more PCM, and passed pause/reset/resume
and clean shutdown. This validates recovery from the injected fault; it does not
establish the cause of the system's separate PipeWire routing errors or resolve
the broader stutter report.

Playback now uses a precise single-shot timer scheduled to the next frame
deadline, replacing the repeating 1 ms polling timer. Paused windows wake at
16 ms; late frames have a bounded 1 ms retry. Core speed, normal-speed frame
presentation and fast-forward skip policy are unchanged. This reduces needless
GUI wakeups; it does not implement adaptive audio-rate control.

Additional reports include Christmas NiGHTS videos, Dreamcast and some NES
games. The first isolated Christmas NiGHTS run stopped at its first-run clock
screen, so it is not evidence about that game's videos. Crazy Taxi with CRT
Geom reproduced worse delivery: about 57.94 steady FPS and 28.7 ms p99 frame
intervals, despite ~5.1 ms average core work (15.7 ms p99). The diagnostic audio
counter also reported substantial discarded samples. This points to a shared
scheduling/synchronization investigation, rather than proof of a slow core.
Disabling swap synchronization did not give a sufficient or consistent fix,
so the shipped swap interval remains unchanged.

RetroArch's documented dynamic rate control coordinates audio resampling with
buffer occupancy and video timing. The original investigation found fixed-rate
conversion and a wall-clock frame schedule; the subsequent correction is below:
https://docs.libretro.com/development/cores/dynamic-rate-control/
https://docs.libretro.com/guides/optimal-vsync/

## October 31: audio clock drift correction

A new clean 2,400-frame Sega Classics run entered the Sonic/Sega logo and the
intro/menu without screenshots during playback. It ran at 59.923 steady FPS;
core work averaged 0.76 ms, with 0.94 ms p99. The queue ended at 216 ms with a
100 ms latency preference. Its only counted underrun and 986 discarded sample
frames were at startup. This does not reproduce repeated movie dropouts on
this run, and does not rule out the user's report under different conditions.

The frontend now smooths total queue occupancy over 250 ms and gently adjusts
the native resampling ratio toward the configured latency. Correction is limited
to ±0.5%, slewed gradually, and preserves core/video timing. Correction is frozen
during pause; fast-forward, state loads and output changes reset it. Native audio retains fractional
state between frames; an in-progress averaging interval completes before a new
ratio is adopted. This also handles correction crossing unity for 48 kHz cores.

The matched 2,400-frame run with correction maintained 59.924 steady FPS and
ended at 128 ms queued, approaching the requested latency rather than remaining
near capacity. Average core work was 0.74 ms, p99 0.97 ms. The single startup
underrun and 1,111 startup discards were not eliminated. These measurements show
queue regulation, not proof that the user's audible video stutter is resolved.

Synthetic 120-second tests model device drift in both directions. A diagnostic
native core verifies sample counts, channel separation and continuous 1 kHz
waveforms while repeatedly changing ratios at 44.1, 48 and 96 kHz. Large/invalid
adjustment requests remain bounded. No larger user buffer setting, core accuracy
reduction or frame skipping is applied. Corrections this small cannot compensate
for sustained severe emulation slowdown.

## Panzer Dragoon underrun reproduction

The user reported the newest clock-test build's count increasing during play.
An isolated 100 ms / CRT Geom run reproduced additional underruns. A subsequent
saved-sequence profile averaged 6.56 ms of core work, with 8.65 ms p99 and 9.59 ms
maximum against a 16.72 ms budget. Complete tick work averaged 6.89 ms; GL drawing
averaged 0.40 ms and composition 3.30 ms. Frame delivery still averaged only
59.19 FPS and drained the queue. These runs show scheduling gaps despite core
headroom; they do not identify every source of OS/GUI jitter.

Disabling swap synchronization on the GL widget did not reliably prevent
underruns. Neither did a separate 150 ms buffer trial, so neither setting was
changed for the user. First boot/intro runs ended at the title menu; the later
profile started from that menu and entered the game's opening sequence.

Previously the scheduler rebased as soon as it fell one core period behind,
discarding that audio-production time. The recovery window is now three periods.
Each timer callback still executes one core frame and requests its presentation;
there is no inner catch-up loop, normal-speed frame skipping or core overclock.
Longer stalls still rebase and the timer retains a minimum 1 ms yield. This is
bounded recovery of the nominal timeline, not a guarantee against sustained load.

A regression covers recovery from a 25 ms delay over successive callbacks while
retaining every audio/video submission. Existing long-stall and alternating-frame
checks remain. A live Nestopia diagnostic with a 30 ms GUI stall every 500 ms
displayed both alternating colors throughout five measured one-second windows:
57–59 paints and 56–58 color transitions per window. This checks Qt rendering;
it is not a physical monitor scanout measurement.

The investigation also found that a live buffer/device change recreated the
sink but retained the old clock controller. Setup now rebuilds that controller
with the current latency and clears its previous correction. The regression
changes the buffer to 150 ms during play and checks both the target and reset.

### Redundant parent repaint

A longer test entering Normal Game with Start reproduced 28 total underruns,
including startup, despite ~6.74 ms average core work and 8.69 ms p99. The
three-period scheduling window alone was insufficient. A separate title-menu
run showed no extra underruns but is not gameplay evidence.

Every accelerated `Screen.update()` also called `QWidget.update()` on the
raster parent underneath the GL surface. This needlessly dirtied the backing
store each frame. The GL renderer already clears its entire FBO to opaque black,
including borders, so accelerated frames now update only the GL child. Parent
exposure/resize and overlay paints remain event-driven. Software fallback and
the explicit Vulkan path retain their previous repaint behavior.

A matched 1,800-frame Normal Game sequence with this optimization reached
59.816 steady FPS against the core's 59.827 target, with 6.70 ms average core
work and 8.65 ms p99. It retained only its initial startup underrun, with zero
discarded samples. This is measured evidence for this sequence, not a guarantee
for every Saturn scene or machine. No synchronization setting or buffer-size
increase was applied.

The rendering regression drives repeated frame updates through a live Qt widget
and checks that the raster parent stays cached, then verifies software fallback
resumes painting the image and pure-black borders.

The final 3,600-frame Panzer Dragoon run entered Normal Game's opening sequence
and maintained ~59.87 steady FPS (core target 59.83), averaging 6.79 ms core work
with 8.52 ms p99. The underrun count stayed at one after startup throughout the
60-second sequence; 807 startup sample-frame discards also stayed unchanged.
The preceding matched run without the raster-repaint optimization reached 28
underruns and ~57.48 steady FPS. These are individual live runs, not a universal
performance percentage or proof about later combat scenes. Temporary libraries,
saves and muted playback were used throughout; user settings were not changed.

Final validation: 94 checks passed, including core integration, clock correction,
buffer rerouting, long/short stall pacing, gameplay borders and renderer fallback.
The final GL/Nestopia blink test retained both colors at 59–60 paints per measured
second while injecting 30 ms GUI stalls. The packaged AppImage also loaded the
Panzer Dragoon diagnostic state, rendered through hardware OpenGL and exited
normally without an audio-output failure.


A subsequent deadline-timer test retained approximately 59.82 FPS in the
filtered Sega Classics intro with zero underruns and 1,412 discarded sample
frames. It overlapped other validation work, so this is not a controlled
before/after performance claim. The reported global slowdown remains unresolved;
the test build supplies measurements for reproducing it rather than claiming
that buffering alone solves it.
