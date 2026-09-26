# Community video filters

## Slang preset collection

The 19 directories under `presets/` are the **unmodified shader sources,
presets and lookup textures bundled by OpenEmu**, from commit
[`1d205104640d8410659d321809889cbfd06b99a9`](https://github.com/OpenEmu/OpenEmu/tree/1d205104640d8410659d321809889cbfd06b99a9/OpenEmu/Shaders).
They were copied from the existing source archive in this workspace, not from
OpenEmu's macOS renderer. These community shaders originate from the
[libretro Slang collection](https://github.com/libretro/slang-shaders).
File hashes are recorded in `manifest.json`.

Included presets: Blinky, CRT Geom, CRT Geom Deluxe, CRT Royale Kurozumi,
Dither, Halftone, LCD PSP, Linear, MAME HLSL, Motion Blur, Nearest Neighbor,
NTSC, NTSC VCR, Pixellate, SABR, Smooth, VHS, xBRZ Freescale and xBRZ Multipass
Freescale. Original author and license notices remain in each file. Contributors
include cgwg, Themaister, DOLLS, TroggleMonkey, hunterk, Martins Upitis, Pokefan531,
Fes, Hyllian, the DeSmuME team and the MAME contributors; consult each source for
its complete credits and terms. These assets are not relicensed under EmuLuna's
frontend MIT license. GPL version 2 is included as `COPYING-GPL-2`.

## Additional GLSL presets

Original shader and preset sources from https://github.com/libretro/glsl-shaders
at commit `235448f244bf676d135f7b25ea6b8e1eae41c4e4`:

| Preset | Author | Upstream license |
| --- | --- | --- |
| zfast CRT / zfast LCD | Greg Hogan (SoltanGris42) | GPL version 2 or later |
| CRT EasyMode | EasyMode | GPL (source does not specify a version) |
| Sharp Bilinear | Themaister | Public domain |

## Rendering and compatibility

EmuLuna's independent OpenGL renderer handles the bundled presets: multipass,
source/viewport/absolute scaling, named pass outputs, previous input frames,
feedback, LUTs, sampler modes, mipmaps, floating-point and sRGB targets. A small
compile-time adapter selects shader stages and replaces Vulkan-style uniform
blocks with desktop GLSL uniforms. Source files on disk remain unchanged.
OpenGL 3.3 is the baseline; shaders using newer GLSL features also need the
corresponding `ARB_shading_language_420pack`, `ARB_arrays_of_arrays` or
`ARB_gpu_shader5` extensions. Unsupported drivers fail with a readable notice.
This is not an unrestricted Slang or GLSL preset importer.

Native-resolution frames upload directly in their existing BGRA layout. The
GPU handles scaling and filtering, then presents directly to Qt's display FBO.
No per-frame readback, image flip, CPU filter or CPU scaling is used. The last
filtered frame is cached for paused/HUD repaints, and textures/targets are reused.
Changing a preset releases its targets/history. Intermediate video memory has
a 512 MiB limit. A failure does not replace the saved user preference.

CRT Royale Kurozumi's bloom degenerates at very small viewport sizes. Its
manifest requests a minimum internal viewport of 640 × 480 (aspect preserved),
then the GPU downsamples to the window. The configuration dialog explains this.
The effect source/default parameters remain unchanged. Heavy filters are marked
as such, with optional output-resolution limits; performance depends on the GPU
and output resolution. Known software drivers are detected and filters disabled
for that session rather than treating a software GL context as acceleration.

Tests run the actual pipeline in headless EGL when available, checking all
presets, orientation, parameters, history, feedback, resizing and source hashes.
A software EGL test does not certify physical GPU performance.
