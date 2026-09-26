/* SPDX-License-Identifier: MIT
 * Exercise VFS semantics against real temporary files, without a game ROM.
 * Run with one argument: an empty scratch directory owned by the test.
 */
#include "vfs.h"
#include <cassert>
#include <set>

int main(int argc, char **argv) {
    assert(argc == 2);
    retro_vfs_interface_info request{4, nullptr};
    assert(!emuluna_vfs::interface(&request));
    assert(!request.iface);
    assert(!emuluna_vfs::interface(nullptr));
    for (unsigned version = 1; version <= 3; ++version) {
        request = {version, nullptr};
        assert(emuluna_vfs::interface(&request));
        assert(request.required_interface_version == 3 && request.iface);
    }
    auto &v = *request.iface;
    const std::string root = argv[1], path = root + "/Game with spaces — test.a26";
    int32_t length = -1;
    assert(v.stat(root.c_str(), nullptr) & RETRO_VFS_STAT_IS_DIRECTORY);
    assert(!v.open(root.c_str(), RETRO_VFS_FILE_ACCESS_READ, 0));
    assert(!v.open(path.c_str(), 0, 0));
    assert(!v.open(path.c_str(), RETRO_VFS_FILE_ACCESS_READ, 0));
    assert(!v.open(path.c_str(), RETRO_VFS_FILE_ACCESS_WRITE | RETRO_VFS_FILE_ACCESS_UPDATE_EXISTING, 0));
    assert(v.stat(path.c_str(), &length) == 0 && length == 0);
    auto f = v.open(path.c_str(), RETRO_VFS_FILE_ACCESS_READ_WRITE, 0);
    assert(f && std::string(v.get_path(f)) == path);
    assert(v.write(f, "abcdef", 6) == 6);
    assert(v.size(f) == 6 && v.tell(f) == 6); // size must preserve position
    assert(v.seek(f, -3, RETRO_VFS_SEEK_POSITION_END) == 3);
    char buffer[8]{};
    assert(v.read(f, buffer, 8) == 3 && std::string(buffer) == "def");
    assert(v.read(f, buffer, 1) == 0); // EOF is not an I/O error
    assert(v.seek(f, 0, RETRO_VFS_SEEK_POSITION_START) == 0);
    assert(v.seek(f, 2, RETRO_VFS_SEEK_POSITION_CURRENT) == 2);
    assert(v.seek(f, 0, 99) == -1);
    assert(v.flush(f) == 0 && v.close(f) == 0);
    assert(v.stat(path.c_str(), &length) == RETRO_VFS_STAT_IS_VALID && length == 6);

    f = v.open(path.c_str(), RETRO_VFS_FILE_ACCESS_WRITE | RETRO_VFS_FILE_ACCESS_UPDATE_EXISTING, 0);
    assert(f && v.size(f) == 6);
    assert(v.read(f, buffer, 1) == -1);
    assert(v.write(f, "X", 1) == 1 && v.close(f) == 0);
    f = v.open(path.c_str(), RETRO_VFS_FILE_ACCESS_READ, 0);
    assert(f && v.read(f, buffer, 6) == 6 && !std::memcmp(buffer, "Xbcdef", 6));
    assert(v.write(f, "Y", 1) == -1 && v.truncate(f, 1) == -1);
    assert(v.close(f) == 0);
    f = v.open(path.c_str(), RETRO_VFS_FILE_ACCESS_READ_WRITE, 0);
    assert(f && v.size(f) == 0); // ordinary write opens truncate
    assert(v.write(f, "abcdef", 6) == 6 && v.truncate(f, 3) == 0);
    assert(v.size(f) == 3 && v.truncate(f, -1) == -1);
    // Large sparse offsets must not wrap at 32 bits.
    constexpr int64_t large = INT64_C(3) * 1024 * 1024 * 1024;
    assert(v.seek(f, large, RETRO_VFS_SEEK_POSITION_START) == large);
    assert(v.write(f, "Z", 1) == 1 && v.size(f) == large + 1);
    assert(v.stat(path.c_str(), &length) == RETRO_VFS_STAT_IS_VALID && length == INT32_MAX);
    assert(v.truncate(f, 3) == 0 && v.close(f) == 0);

    const std::string child = root + "/child", hidden = root + "/.hidden", renamed = root + "/renamed.a26";
    assert(v.mkdir(child.c_str()) == 0 && v.mkdir(child.c_str()) == -2);
    assert(v.mkdir(path.c_str()) == -1); // a file is not an existing directory
    f = v.open(hidden.c_str(), RETRO_VFS_FILE_ACCESS_WRITE, 0);
    assert(f && v.close(f) == 0);
    assert(v.rename(path.c_str(), renamed.c_str()) == 0);
    assert(v.stat(path.c_str(), nullptr) == 0 && v.stat(renamed.c_str(), &length) == RETRO_VFS_STAT_IS_VALID && length == 3);
    assert(!v.opendir(renamed.c_str(), false));
    for (bool include_hidden : {false, true}) {
        auto directory = v.opendir(root.c_str(), include_hidden);
        assert(directory && !v.dirent_get_name(directory));
        std::set<std::string> names;
        while (v.readdir(directory)) {
            std::string name = v.dirent_get_name(directory);
            names.insert(name);
            assert(v.dirent_is_dir(directory) == (name == "child"));
        }
        assert(!v.dirent_get_name(directory) && !v.dirent_is_dir(directory));
        assert(names.count("child") && names.count("renamed.a26"));
        assert(names.count(".hidden") == static_cast<unsigned>(include_hidden));
        assert(v.closedir(directory) == 0);
    }
    assert(v.remove(renamed.c_str()) == 0 && v.remove(renamed.c_str()) == -1);
    assert(v.remove(hidden.c_str()) == 0);
    assert(v.remove(child.c_str()) == -1); // file removal must not delete directories
    assert(::rmdir(child.c_str()) == 0);
    std::puts("VFS v3 file, directory, large-file, and negotiation checks passed.");
}
