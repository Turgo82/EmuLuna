// SPDX-License-Identifier: MIT
// Read-only, bounded CHD track samples. Kept in a child process so a broken
// container or decoder never takes down the importing application.
#include <libchdr/chd.h>
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <memory>
#include <vector>
#include <sys/resource.h>

namespace {
constexpr unsigned frame_bytes = 2448;
constexpr unsigned sample_frames = 128;
constexpr unsigned output_limit = 8 * 1024 * 1024;
struct Track { unsigned frames, stride, pregap; bool audio; };

bool metadata(chd_file *file, std::vector<Track> &tracks) {
    const unsigned tags[] = {GDROM_TRACK_METADATA_TAG, CDROM_TRACK_METADATA2_TAG,
                             CDROM_TRACK_METADATA_TAG, GDROM_OLD_METADATA_TAG};
    for (unsigned index = 0; index < 99; ++index) {
        char text[512]{}; unsigned tag = 0, length = 0;
        for (auto candidate : tags) {
            auto error = chd_get_metadata(file, candidate, index, text, sizeof(text) - 1,
                                          &length, nullptr, nullptr);
            if (error == CHDERR_NONE) { tag = candidate; break; }
            if (error != CHDERR_METADATA_NOT_FOUND) return false;
        }
        if (!tag) return !tracks.empty();
        if (length >= sizeof(text)) return false;
        unsigned number = 0, frames = 0, pregap = 0, postgap = 0, pad = 0;
        char type[32]{}, subtype[32]{}, pgtype[32]{}, pgsub[32]{};
        int fields;
        if (tag == GDROM_TRACK_METADATA_TAG)
            fields = std::sscanf(text, "TRACK:%u TYPE:%31s SUBTYPE:%31s FRAMES:%u PAD:%u PREGAP:%u PGTYPE:%31s PGSUB:%31s POSTGAP:%u",
                &number, type, subtype, &frames, &pad, &pregap, pgtype, pgsub, &postgap);
        else if (tag == CDROM_TRACK_METADATA2_TAG)
            fields = std::sscanf(text, "TRACK:%u TYPE:%31s SUBTYPE:%31s FRAMES:%u PREGAP:%u PGTYPE:%31s PGSUB:%31s POSTGAP:%u",
                &number, type, subtype, &frames, &pregap, pgtype, pgsub, &postgap);
        else
            fields = std::sscanf(text, "TRACK:%u TYPE:%31s SUBTYPE:%31s FRAMES:%u",
                &number, type, subtype, &frames);
        int expected = tag == GDROM_TRACK_METADATA_TAG ? 9 : tag == CDROM_TRACK_METADATA2_TAG ? 8 : 4;
        if (fields != expected || number != index + 1 || !frames || pregap >= frames || frames > 4000000)
            return false;
        unsigned stride = 0;
        bool audio = std::strcmp(type, "AUDIO") == 0;
        if (audio || !std::strcmp(type, "MODE1_RAW") || !std::strcmp(type, "MODE2_RAW")) stride = 2352;
        else if (!std::strcmp(type, "MODE1") || !std::strcmp(type, "MODE2_FORM1")) stride = 2048;
        else if (!std::strcmp(type, "MODE2") || !std::strcmp(type, "MODE2_FORM_MIX")) stride = 2336;
        else if (!std::strcmp(type, "MODE2_FORM2")) stride = 2324;
        if (!stride) return false;
        // A leading V marks a pregap physically present in the CHD.
        tracks.push_back({frames, stride, pgtype[0] == 'V' ? pregap : 0, audio});
    }
    return true;
}

void word(std::vector<unsigned char> &out, unsigned value) {
    for (int shift = 24; shift >= 0; shift -= 8) out.push_back((value >> shift) & 255);
}
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    rlimit memory{256u * 1024u * 1024u, 256u * 1024u * 1024u};
    rlimit cpu{6, 6};
    setrlimit(RLIMIT_AS, &memory); setrlimit(RLIMIT_CPU, &cpu);
    chd_header header{};
    if (chd_read_header(argv[1], &header) != CHDERR_NONE || !header.hunkbytes ||
        header.hunkbytes > 1024 * 1024 || !header.logicalbytes ||
        header.logicalbytes > 16ull * 1024 * 1024 * 1024 || header.totalhunks > 1048576)
        return 2;
    chd_file *opened = nullptr;
    if (chd_open(argv[1], CHD_OPEN_READ, nullptr, &opened) != CHDERR_NONE) return 2;
    std::unique_ptr<chd_file, decltype(&chd_close)> file(opened, chd_close);
    std::vector<Track> tracks;
    bool cd = header.unitbytes == frame_bytes;
    if (cd) { if (!metadata(file.get(), tracks)) return 2; }
    else {
        char dvd[8]{};
        if (header.unitbytes != 2048 || chd_get_metadata(file.get(), DVD_METADATA_TAG, 0,
                dvd, sizeof(dvd), nullptr, nullptr, nullptr) != CHDERR_NONE) return 2;
        tracks.push_back({static_cast<unsigned>(header.logicalbytes / 2048), 2048, 0, false});
    }
    std::vector<unsigned char> hunk(header.hunkbytes), output;
    uint64_t track_start = 0;
    uint32_t cached = ~uint32_t(0);
    for (auto track : tracks) {
        unsigned unit = cd ? frame_bytes : 2048;
        uint64_t track_end = (track_start + track.frames) * unit;
        if (track_end > header.logicalbytes) return 2;
        if (!track.audio) {
            unsigned frames = std::min(sample_frames, track.frames - track.pregap);
            unsigned bytes = frames * track.stride;
            if (output.size() + bytes + 8 > output_limit) return 2;
            word(output, bytes); word(output, track.stride);
            for (unsigned frame = 0; frame < frames; ++frame) {
                uint64_t offset = (track_start + track.pregap + frame) * unit;
                unsigned left = track.stride;
                while (left) {
                    auto number = static_cast<uint32_t>(offset / header.hunkbytes);
                    if (number != cached) {
                        if (chd_read(file.get(), number, hunk.data()) != CHDERR_NONE) return 2;
                        cached = number;
                    }
                    unsigned start = offset % header.hunkbytes;
                    unsigned count = std::min(left, header.hunkbytes - start);
                    output.insert(output.end(), hunk.begin() + start, hunk.begin() + start + count);
                    left -= count; offset += count;
                }
            }
        }
        // chdman aligns every CD/GD track to four stored frames, including audio.
        track_start += cd ? (uint64_t(track.frames) + 3) / 4 * 4 : track.frames;
    }
    return std::fwrite(output.data(), 1, output.size(), stdout) == output.size() ? 0 : 2;
}
