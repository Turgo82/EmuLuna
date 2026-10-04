# Vendored CHD reader

Source: https://github.com/rtissera/libchdr
Commit: 607694ca0812edfc9cc2030c64634fc2393668de
Retrieved: 2026-10-03

Source, headers, bundled decoder dependencies and CMake files are unchanged.
Only upstream test/contrib/documentation directories are omitted. EmuLuna
builds the static reader with the default dr_flac backend, without network
fetches or upstream tests. Notices remain in LICENSE.txt and the decoder
sources; binary distribution notices are also in emuluna/data/licenses/.
