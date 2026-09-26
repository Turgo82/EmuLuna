/* SPDX-License-Identifier: MIT
 * Local filesystem backend for libretro VFS v3. Kept outside the Qt/frontend
 * layer so unmodified cores can inspect ROMs and manage their own sidecars.
 */
#pragma once
#include "vendor/libretro.h"
#include <cerrno>
#include <climits>
#include <cstdio>
#include <cstring>
#include <dirent.h>
#include <limits>
#include <new>
#include <string>
#include <sys/stat.h>
#include <unistd.h>

struct retro_vfs_file_handle {
    FILE *file;
    std::string path;
    unsigned access;
};
struct retro_vfs_dir_handle {
    DIR *directory;
    std::string path, name;
    bool include_hidden, is_directory = false;
};

namespace emuluna_vfs {
static const char *RETRO_CALLCONV get_path(retro_vfs_file_handle *file) {
    return file ? file->path.c_str() : nullptr;
}
static retro_vfs_file_handle *RETRO_CALLCONV open(const char *path, unsigned mode, unsigned) {
    unsigned access = mode & RETRO_VFS_FILE_ACCESS_READ_WRITE;
    if (!path || !*path || !access || (mode & ~7u)) return nullptr;
    struct stat info{};
    if (::stat(path, &info) == 0 && S_ISDIR(info.st_mode)) return nullptr;
    const bool writing = access & RETRO_VFS_FILE_ACCESS_WRITE;
    const char *flags = !writing ? "rb" : (mode & RETRO_VFS_FILE_ACCESS_UPDATE_EXISTING)
        ? "r+b" : access == RETRO_VFS_FILE_ACCESS_WRITE ? "wb" : "w+b";
    FILE *stream = std::fopen(path, flags);
    if (!stream) return nullptr;
    try { return new retro_vfs_file_handle{stream, path, access}; }
    catch (...) { std::fclose(stream); return nullptr; }
}
static int RETRO_CALLCONV close(retro_vfs_file_handle *file) {
    if (!file) return -1;
    int result = std::fclose(file->file);
    delete file;
    return result == 0 ? 0 : -1;
}
static int64_t RETRO_CALLCONV tell(retro_vfs_file_handle *file) {
    return file ? ::ftello(file->file) : -1;
}
static int64_t RETRO_CALLCONV seek(retro_vfs_file_handle *file, int64_t offset, int origin) {
    if (!file || origin < RETRO_VFS_SEEK_POSITION_START || origin > RETRO_VFS_SEEK_POSITION_END) return -1;
    const int origins[] = {SEEK_SET, SEEK_CUR, SEEK_END};
    if (::fseeko(file->file, offset, origins[origin]) != 0) return -1;
    return tell(file); // Libretro filestream consumers expect the new position.
}
static int64_t RETRO_CALLCONV size(retro_vfs_file_handle *file) {
    int64_t position = tell(file);
    if (position < 0) return -1;
    int64_t length = seek(file, 0, RETRO_VFS_SEEK_POSITION_END);
    return seek(file, position, RETRO_VFS_SEEK_POSITION_START) < 0 ? -1 : length;
}
static int64_t RETRO_CALLCONV read(retro_vfs_file_handle *file, void *buffer, uint64_t count) {
    if (!file || !(file->access & RETRO_VFS_FILE_ACCESS_READ) || (!buffer && count) ||
        count > static_cast<uint64_t>(std::numeric_limits<ptrdiff_t>::max())) return -1;
    if (!count) return 0;
    size_t result = std::fread(buffer, 1, static_cast<size_t>(count), file->file);
    return std::ferror(file->file) ? -1 : static_cast<int64_t>(result);
}
static int64_t RETRO_CALLCONV write(retro_vfs_file_handle *file, const void *buffer, uint64_t count) {
    if (!file || !(file->access & RETRO_VFS_FILE_ACCESS_WRITE) || (!buffer && count) ||
        count > static_cast<uint64_t>(std::numeric_limits<ptrdiff_t>::max())) return -1;
    if (!count) return 0;
    size_t result = std::fwrite(buffer, 1, static_cast<size_t>(count), file->file);
    return std::ferror(file->file) ? -1 : static_cast<int64_t>(result);
}
static int RETRO_CALLCONV flush(retro_vfs_file_handle *file) {
    return file && std::fflush(file->file) == 0 ? 0 : -1;
}
static int64_t RETRO_CALLCONV truncate(retro_vfs_file_handle *file, int64_t length) {
    if (!file || !(file->access & RETRO_VFS_FILE_ACCESS_WRITE) || length < 0 || flush(file) != 0) return -1;
    return ::ftruncate(::fileno(file->file), length) == 0 ? 0 : -1;
}
static int RETRO_CALLCONV remove(const char *path) {
    return path && ::unlink(path) == 0 ? 0 : -1;
}
static int RETRO_CALLCONV rename(const char *from, const char *to) {
    return from && to && std::rename(from, to) == 0 ? 0 : -1;
}
static int RETRO_CALLCONV stat(const char *path, int32_t *size) {
    if (size) *size = 0;
    struct stat info{};
    if (!path || ::stat(path, &info) != 0) return 0;
    // VFS v3 cannot represent sizes over 2 GiB. File handles remain 64-bit.
    if (size) *size = static_cast<int32_t>(info.st_size > INT32_MAX ? INT32_MAX : info.st_size);
    return RETRO_VFS_STAT_IS_VALID | (S_ISDIR(info.st_mode) ? RETRO_VFS_STAT_IS_DIRECTORY : 0)
        | (S_ISCHR(info.st_mode) ? RETRO_VFS_STAT_IS_CHARACTER_SPECIAL : 0);
}
static int RETRO_CALLCONV mkdir(const char *path) {
    if (!path) return -1;
    if (::mkdir(path, 0777) == 0) return 0;
    return errno == EEXIST && (stat(path, nullptr) & RETRO_VFS_STAT_IS_DIRECTORY) ? -2 : -1;
}
static retro_vfs_dir_handle *RETRO_CALLCONV opendir(const char *path, bool hidden) {
    if (!path) return nullptr;
    DIR *directory = ::opendir(path);
    if (!directory) return nullptr;
    try { return new retro_vfs_dir_handle{directory, path, "", hidden, false}; }
    catch (...) { ::closedir(directory); return nullptr; }
}
static bool RETRO_CALLCONV readdir(retro_vfs_dir_handle *directory) {
    if (!directory) return false;
    directory->name.clear(); directory->is_directory = false;
    while (auto entry = ::readdir(directory->directory)) {
        if (!std::strcmp(entry->d_name, ".") || !std::strcmp(entry->d_name, "..") ||
            (!directory->include_hidden && entry->d_name[0] == '.')) continue;
        directory->name = entry->d_name;
        const std::string path = directory->path + "/" + directory->name;
        directory->is_directory = stat(path.c_str(), nullptr) & RETRO_VFS_STAT_IS_DIRECTORY;
        return true;
    }
    return false;
}
static const char *RETRO_CALLCONV dirent_get_name(retro_vfs_dir_handle *directory) {
    return directory && !directory->name.empty() ? directory->name.c_str() : nullptr;
}
static bool RETRO_CALLCONV dirent_is_dir(retro_vfs_dir_handle *directory) {
    return directory && directory->is_directory;
}
static int RETRO_CALLCONV closedir(retro_vfs_dir_handle *directory) {
    if (!directory) return -1;
    int result = ::closedir(directory->directory);
    delete directory;
    return result == 0 ? 0 : -1;
}
static bool interface(retro_vfs_interface_info *request) {
    if (!request || request->required_interface_version > 3) return false;
    static retro_vfs_interface api{
        get_path, open, close, size, tell, seek, read, write, flush, remove, rename,
        truncate, stat, mkdir, opendir, readdir, dirent_get_name, dirent_is_dir, closedir, nullptr
    };
    request->required_interface_version = 3;
    request->iface = &api;
    return true;
}
} // namespace emuluna_vfs
