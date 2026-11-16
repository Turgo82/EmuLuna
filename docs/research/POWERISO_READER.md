# PowerISO Linux reader investigation

Inspected October 30, 2026. No PowerISO executable was run and no application
code was copied. Source: the official Linux GUI v1.1 archive:
https://www.poweriso.com/poweriso-x64-1.1.tar.gz
GUI executable SHA-256:
503de303157e09d49fc0085f76a674778e4ff10014d5994f18ff31d42fed6dfc

## Observed

The GUI binary retains function symbols. Its dynamic dependencies are Qt5,
libpthread, libstdc++, libgcc and libc; no external disc-reader library is listed.
The official CLI v1.6 executable is statically linked and stripped, which does
not identify its internal libraries.

The GUI contains these connected routines:

- BurnReadToc at 0xf43f0 builds a command with opcode 0x43 (READ TOC).
- BurnReadCDRawBlock at 0xf52e0 builds a 12-byte command with opcode 0xbe
  (READ CD), an LBA and a sector count. Its main-channel selection is 0xf8;
  the subchannel-selection byte remains zero in this routine.
- Both call BurnDeviceIoControlEx, which calls BurnOverlapDeviceIoControl
  at 0xf2430. That wrapper issues Linux ioctl 0x2285 (SG_IO), with the
  sg_io_hdr interface identifier 'S'.

This is direct evidence of embedded MMC/SCSI pass-through reading, including
raw-sector reads without appended subchannels in the inspected routine. It
is not simply a dynamically linked cdrdao front end.

A local function named libburn_lba_to_msf also exists. One address-conversion
helper is insufficient evidence that the extraction engine is libburn; do not
report that identification as established.

## Limits and implication for EmuLuna

Static inspection establishes available code paths, not the exact runtime
sequence used on the user's disc, retry policy, or every pregap/sub-index
handling detail. A live command trace would be required to establish those.

An independently implemented standard MMC read path can read the physical TOC,
stream raw sectors into BIN and produce CUE track starts without requiring
cdrdao's slow fallback sub-index search. That is a concrete candidate for
EmuLuna, not yet an implemented or validated replacement. Preserve sector/error,
track-boundary, cancellation and disc-change validation when pursuing it.

Official Linux make-image documentation supports BIN/CUE output:
https://www.poweriso.com/tutorials/copy-disc-to-iso-from-linux-command-line.htm
Official credits do not identify a disc extraction engine:
https://www.poweriso.com/tutorials/credits.htm

## Independent physical MMC experiment

Tested the inserted Ecco the Dolphin on the CW-8123 using the developer-only
`tools/probe_mmc_reader.py`; the application's default reader was not changed.
No PowerISO code was copied or executed. The experiment uses only read-only
READ CD commands through Linux SG_IO and the existing physical TOC probe.

- 77 boundary/body sectors each read twice identically in the initial probe.
- The first sequential run stopped correctly when a 16-sector request crossed
  the data/audio transition (LBA 25344). It did not pad or accept the short read.
- An adaptive run retried that rejected batch as individual sector reads. Every
  sector succeeded; no missing data was filled. The resulting image contains
  all 279875 sectors, 658266000 bytes, including the 150 pregap sectors omitted
  from the cdrdao compatibility image. A 16-track CUE uses the physical INDEX 01
  positions; detailed sub-indices are not reconstructed.
- Whole experiment: 220.56 seconds, including sector sampling and disc checks.
  This excludes managed import, unlike the earlier 521-second complete-worker
  benchmark. It performed no cdrdao-style five-minute boundary scan.
- Data bytes before the audio pregap match the cdrdao image exactly.
- All 598297052 compared audio bytes match exactly after a consistent four-byte
  (one stereo PCM sample, about 0.0208 ms) shift. The first/last unmatched sample
  is excluded from this comparison. The raw images are therefore NOT bit-identical.
  Which extraction path best reflects an absolute reference offset is unresolved.
- Console detection reports segacd, and the physical CUE has 16 valid tracks.

Artifacts: `/tmp/emuluna-mmc-full-probe-adaptive/{disc.bin,disc.cue,report.json,comparison.json}`.
Repeat extraction, other mixed-mode discs/drives, read-error/cancellation tests
and a declared audio-offset policy are still needed before production integration.
Raw MMC success does not establish archival/Redump correctness.
