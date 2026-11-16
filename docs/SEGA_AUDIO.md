# Sega low-pass audio controls

The gameplay overlay exposes the core's native low-pass toggle under
**Genesis Audio**, **Sega CD Audio**, or **32X Audio**, as appropriate.
Choices are saved separately for each console and core.

- Genesis Plus GX: Genesis retains its Model 1-style filter, enabled at 60%.
  Sega CD gains an optional toggle, initially off, with strength set to 60%.
- PicoDrive: Genesis and 32X enable their Model 1-style filter at 60%.
  Sega CD gains the same optional toggle, initially off.
- Master System, Game Gear and unrelated console defaults are unchanged.

Sega CD is off by default to avoid low-pass filtering CD music. Turning the
option on filters the full mix, including CD-DA music and PCM sounds, rather
than just Genesis FM/PSG. The menu label and tooltip state this explicitly.
There is no frontend post-processing filter and no additional Python audio
processing. A CD-only bypass would require a change inside the core's mixer;
it cannot be recovered from the single mixed stream sent to the frontend.

This behavior was checked against upstream implementation:

- [Genesis Plus GX mixer and filter](https://github.com/libretro/Genesis-Plus-GX/blob/master/core/system.c):
  `audio_update` mixes PCM/CD-DA with FM/PSG, then filters the output buffer.
- [Genesis Plus GX options](https://github.com/libretro/Genesis-Plus-GX/blob/master/libretro/libretro_core_options.h):
  `genesis_plus_gx_audio_filter` and `genesis_plus_gx_lowpass_range`.
- [PicoDrive sound mixing](https://github.com/libretro/picodrive/blob/master/pico/sound/sound.c)
  and [output filter](https://github.com/libretro/picodrive/blob/master/pico/sound/mix.c):
  PCM, CD-DA and PWM enter the mix before the output filter.
- [PicoDrive options](https://github.com/libretro/picodrive/blob/master/platform/libretro/libretro_core_options.h):
  `picodrive_audio_filter` and `picodrive_lowpass_range`.

Tests check console-specific defaults, unaffected shared systems, live overlay
switches, saved choices and the Sega CD tooltip. Listening tests with actual
Sega CD/32X content are separate from these configuration and UI checks.
