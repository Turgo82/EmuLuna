/* SPDX-License-Identifier: MIT
 * Libretro frontend for the EmuLuna helper process.
 * The official API header and its license are retained in vendor/libretro.h.
 */
#include "core.h"
#include "vendor/libretro.h"
#include "vfs.h"
#include "vulkan_host.h"
#include <dlfcn.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iterator>
#include <map>
#include <memory>
#include <string>
#include <vector>

struct Host {
    void *module = nullptr;
    bool initialized = false, loaded = false, shutdown = false;
    bool hardware_allowed = false, vulkan_allowed = false, hardware_active = false;
    unsigned gl_major = 0, gl_minor = 0, core_major = 0, core_minor = 0;
    bool hardware_reset = false, last_frame_hardware = false;
    uintptr_t hardware_framebuffer = 0;
    retro_hw_render_callback hardware{};
    std::unique_ptr<VulkanHost> vulkan;
    int16_t axes[4][4] = {};
    unsigned keys[4] = {};
    bool input_ports[4] = {true, false, false, false};
    unsigned pixel_format = RETRO_PIXEL_FORMAT_0RGB1555;
    unsigned width = 256, height = 224, max_width = 256, max_height = 224;
    double fps = 60, rate = 48000, aspect = 0;
    double phase = 0;
    double accumulated = 0, sum_left = 0, sum_right = 0;
    int16_t previous_left = 0, previous_right = 0;
    bool have_previous = false;
    std::string rom, save, system, core, error;
    std::vector<char> content;
    std::vector<uint32_t> pixels;
    std::vector<int16_t> audio;
    std::vector<int16_t> output;
    std::map<std::string,std::string> options;
    std::atomic<uint16_t> rumble[4][2];
    std::atomic<bool> variables_updated{false};
    Host() {
        for (auto &port : rumble) for (auto &motor : port) motor.store(0);
    }
#define API(name) decltype(&retro_##name) name = nullptr
    API(init); API(deinit); API(api_version); API(get_system_info); API(get_system_av_info);
    API(set_environment); API(set_video_refresh); API(set_audio_sample); API(set_audio_sample_batch);
    API(set_input_poll); API(set_input_state); API(set_controller_port_device);
    API(load_game); API(unload_game); API(run); API(reset); API(serialize_size);
    API(serialize); API(unserialize); API(get_memory_data); API(get_memory_size);
#undef API
};
static Host *active = nullptr;
static std::map<std::string, std::string> launch_options;
static bool launch_hardware_allowed = false;
static bool launch_vulkan_allowed = false;
static unsigned launch_gl_major = 0, launch_gl_minor = 0, launch_core_major = 0, launch_core_minor = 0;
static std::string last_error;

static uintptr_t current_framebuffer() {
    return active ? active->hardware_framebuffer : 0;
}

static retro_proc_address_t hardware_proc_address(const char *name) {
    if (!name || !*name) return nullptr;
    void *symbol = dlsym(RTLD_DEFAULT, name);
    if (symbol) return reinterpret_cast<retro_proc_address_t>(symbol);
    // Qt may create either a GLX or EGL context. Load both dispatchers lazily;
    // they resolve functions against whichever context is current.
    using GLXGetProc = void *(*)(const unsigned char *);
    using EGLGetProc = void *(*)(const char *);
    static void *gl = dlopen("libGL.so.1", RTLD_LAZY | RTLD_GLOBAL);
    static void *egl = dlopen("libEGL.so.1", RTLD_LAZY | RTLD_GLOBAL);
    static auto glx = gl ? reinterpret_cast<GLXGetProc>(dlsym(gl, "glXGetProcAddressARB")) : nullptr;
    static auto egl_get = egl ? reinterpret_cast<EGLGetProc>(dlsym(egl, "eglGetProcAddress")) : nullptr;
    symbol = glx ? glx(reinterpret_cast<const unsigned char*>(name)) : nullptr;
    if (!symbol && egl_get) symbol = egl_get(name);
    return reinterpret_cast<retro_proc_address_t>(symbol);
}
static void log_message(enum retro_log_level, const char *format, ...) {
    va_list args; va_start(args, format); vfprintf(stderr, format, args); va_end(args);
}
static bool set_rumble_state(unsigned port, enum retro_rumble_effect effect, uint16_t strength) {
    if (!active || port >= 4 || effect > RETRO_RUMBLE_WEAK) return false;
    active->rumble[port][effect].store(strength, std::memory_order_relaxed);
    return true;
}
static bool valid_av(const retro_system_av_info &info) {
    return info.geometry.base_width > 0 && info.geometry.base_width <= 1024 &&
        info.geometry.base_height > 0 && info.geometry.base_height <= 1024 &&
        std::isfinite(info.timing.fps) && info.timing.fps >= 10 && info.timing.fps <= 240 &&
        std::isfinite(info.timing.sample_rate) && info.timing.sample_rate >= 8000 && info.timing.sample_rate <= 4194304;
}
template<class T> static void defaults(const T *definitions) {
    if (!definitions) return;
    for (unsigned i = 0; i < 2048 && definitions[i].key; ++i) {
        auto &d = definitions[i];
        const char *value = d.default_value ? d.default_value : d.values[0].value;
        if (value) active->options.emplace(d.key, value);
    }
}
static bool environment(unsigned command, void *data) {
    if (!active) return false;
    switch (command) {
    case RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER:
        if (!active->hardware_allowed || !data) return false;
        if (active->vulkan_allowed) *static_cast<unsigned*>(data) = RETRO_HW_CONTEXT_VULKAN;
        else if (active->core_major) *static_cast<unsigned*>(data) = RETRO_HW_CONTEXT_OPENGL_CORE;
        else if (active->gl_major) *static_cast<unsigned*>(data) = RETRO_HW_CONTEXT_OPENGL;
        else return false;
        return true;
    case RETRO_ENVIRONMENT_SET_HW_RENDER: {
        if (!data) return false;
        auto requested = static_cast<retro_hw_render_callback*>(data);
        const char *kind = requested->context_type == RETRO_HW_CONTEXT_OPENGL_CORE ? "OpenGL Core" :
                           requested->context_type == RETRO_HW_CONTEXT_OPENGL ? "OpenGL" :
                           requested->context_type == RETRO_HW_CONTEXT_VULKAN ? "Vulkan" : "unsupported type";
        fprintf(stderr, "Core requested hardware context: %s\n", kind);
        if (requested->context_type == RETRO_HW_CONTEXT_VULKAN) {
            if (!active->hardware_allowed || !active->vulkan_allowed || !requested->context_reset) {
                fprintf(stderr, "Core Vulkan context unavailable.\n");
                return false;
            }
            active->vulkan.reset(new VulkanHost());
            active->hardware = *requested;
            active->hardware_active = true;
            fprintf(stderr, "Creating Vulkan context for core\n");
            return true;
        }
        unsigned major = requested->context_type == RETRO_HW_CONTEXT_OPENGL_CORE ? active->core_major :
                         requested->context_type == RETRO_HW_CONTEXT_OPENGL ? active->gl_major : 0;
        unsigned minor = requested->context_type == RETRO_HW_CONTEXT_OPENGL_CORE ? active->core_minor : active->gl_minor;
        if (!active->hardware_allowed || !major || !requested->context_reset ||
                requested->version_major > major ||
                (requested->version_major == major && requested->version_minor > minor)) {
            fprintf(stderr, "Core hardware context unavailable: %s %u.%u\n", kind,
                    requested->version_major, requested->version_minor);
            return false;
        }
        fprintf(stderr, "Creating %s context for core\n", kind);
        active->hardware = *requested;
        active->hardware.get_current_framebuffer = current_framebuffer;
        active->hardware.get_proc_address = hardware_proc_address;
        requested->get_current_framebuffer = current_framebuffer;
        requested->get_proc_address = hardware_proc_address;
        active->hardware_active = true;
        return true;
    }
    case RETRO_ENVIRONMENT_GET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_SUPPORT: {
        if (!data) return false;
        auto *requested = static_cast<retro_hw_render_context_negotiation_interface*>(data);
        requested->interface_version = requested->interface_type ==
            RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN && active->vulkan ? 1 : 0;
        return true;
    }
    case RETRO_ENVIRONMENT_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE:
        return active->vulkan && active->vulkan->set_negotiation(
            static_cast<const retro_hw_render_context_negotiation_interface*>(data));
    case RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE:
        if (!data || !active->vulkan || !active->vulkan->interface()->device) return false;
        *static_cast<const retro_hw_render_interface**>(data) =
            reinterpret_cast<const retro_hw_render_interface*>(active->vulkan->interface());
        return true;
    case RETRO_ENVIRONMENT_GET_VFS_INTERFACE:
        return emuluna_vfs::interface(static_cast<retro_vfs_interface_info*>(data));
    case RETRO_ENVIRONMENT_GET_CAN_DUPE: *static_cast<bool*>(data) = true; return true;
    case RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: {
        unsigned format = *static_cast<unsigned*>(data);
        if (format > RETRO_PIXEL_FORMAT_RGB565) return false;
        active->pixel_format = format; return true;
    }
    case RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY:
    case RETRO_ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY: *static_cast<const char**>(data) = active->system.c_str(); return true;
    case RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY: *static_cast<const char**>(data) = active->save.c_str(); return true;
    case RETRO_ENVIRONMENT_GET_LIBRETRO_PATH: *static_cast<const char**>(data) = active->core.c_str(); return true;
    case RETRO_ENVIRONMENT_GET_LOG_INTERFACE: static_cast<retro_log_callback*>(data)->log = log_message; return true;
    case RETRO_ENVIRONMENT_GET_LANGUAGE: *static_cast<unsigned*>(data) = RETRO_LANGUAGE_ENGLISH; return true;
    case RETRO_ENVIRONMENT_GET_INPUT_MAX_USERS: *static_cast<unsigned*>(data) = 4; return true;
    case RETRO_ENVIRONMENT_GET_INPUT_BITMASKS: return true;
    case RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE: *static_cast<int*>(data) = 3; return true;
    case RETRO_ENVIRONMENT_GET_FASTFORWARDING: *static_cast<bool*>(data) = false; return true;
    case RETRO_ENVIRONMENT_GET_TARGET_REFRESH_RATE: *static_cast<float*>(data) = active->fps; return true;
    case RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION: *static_cast<unsigned*>(data) = 2; return true;
    case RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE:
        static_cast<retro_rumble_interface*>(data)->set_rumble_state = set_rumble_state;
        return true;
    case RETRO_ENVIRONMENT_SET_VARIABLES: {
        auto vars = static_cast<const retro_variable*>(data);
        for (unsigned i = 0; vars && i < 2048 && vars[i].key; ++i) {
            std::string desc = vars[i].value ? vars[i].value : "";
            auto start = desc.find("; ");
            if (start != std::string::npos) {
                auto value = desc.substr(start + 2); value = value.substr(0, value.find('|'));
                active->options.emplace(vars[i].key, value);
            }
        }
        return true;
    }
    case RETRO_ENVIRONMENT_GET_VARIABLE: {
        auto var = static_cast<retro_variable*>(data);
        auto found = active->options.find(var->key ? var->key : "");
        var->value = found == active->options.end() ? nullptr : found->second.c_str();
        return var->value != nullptr;
    }
    case RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE:
        *static_cast<bool*>(data) = active->variables_updated.exchange(false, std::memory_order_relaxed);
        return true;
    case RETRO_ENVIRONMENT_SET_CORE_OPTIONS: defaults(static_cast<retro_core_option_definition*>(data)); return true;
    case RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL: defaults(static_cast<retro_core_options_intl*>(data)->us); return true;
    case RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2: defaults(static_cast<retro_core_options_v2*>(data)->definitions); return true;
    case RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL: {
        auto intl = static_cast<retro_core_options_v2_intl*>(data);
        if (intl->us) defaults(intl->us->definitions);
        return true;
    }
    case RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO: {
        auto info = static_cast<retro_system_av_info*>(data);
        if (!valid_av(*info)) return false;
        const unsigned maximum_width = std::max(info->geometry.base_width, info->geometry.max_width);
        const unsigned maximum_height = std::max(info->geometry.base_height, info->geometry.max_height);
        if (maximum_width > 4096 || maximum_height > 4096) return false;
        if (active->rate != info->timing.sample_rate) {
            active->phase = active->sum_left = active->sum_right = active->accumulated = 0;
            active->have_previous = false;
        }
        active->fps = info->timing.fps; active->rate = info->timing.sample_rate;
        active->aspect = info->geometry.aspect_ratio;
        active->max_width = maximum_width;
        active->max_height = maximum_height;
        return true;
    }
    case RETRO_ENVIRONMENT_SET_GEOMETRY: {
        auto geometry = static_cast<retro_game_geometry*>(data);
        active->aspect = geometry->aspect_ratio;
        return true; // The video callback supplies actual dimensions.
    }
    case RETRO_ENVIRONMENT_SHUTDOWN: active->shutdown = true; return true;
    case RETRO_ENVIRONMENT_SET_MESSAGE: return true;
    case RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS:
    case RETRO_ENVIRONMENT_SET_CONTROLLER_INFO:
    case RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME:
    case RETRO_ENVIRONMENT_SET_SERIALIZATION_QUIRKS:
    case RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY: return true;
    default: return false; // Hardware rendering and optional interfaces are not advertised.
    }
}
static void video(const void *data, unsigned width, unsigned height, size_t pitch) {
    if (!active || !data) return; // NULL duplicates the previous frame.
    if (data == RETRO_HW_FRAME_BUFFER_VALID) {
        if (!active->hardware_active || !active->hardware_reset || !width || !height ||
                width > active->max_width || height > active->max_height) {
            active->error = "The core returned an invalid hardware-rendered frame.";
            return;
        }
        active->width = width;
        active->height = height;
        if (active->hardware.context_type == RETRO_HW_CONTEXT_VULKAN &&
            (!active->vulkan || !active->vulkan->capture(width, height, active->pixels, active->error)))
            return;
        active->last_frame_hardware = true;
        return;
    }
    if (!width || !height || width > 1024 || height > 1024) {
        active->error = "This core requires unsupported video output."; return;
    }
    unsigned bytes = active->pixel_format == RETRO_PIXEL_FORMAT_XRGB8888 ? 4 : 2;
    if (pitch < width * bytes || pitch > 1024 * 16) { active->error = "Invalid core video stride."; return; }
    active->width = width; active->height = height; active->pixels.resize(width * height);
    for (unsigned y = 0; y < height; ++y) {
        auto row = static_cast<const uint8_t*>(data) + y * pitch;
        for (unsigned x = 0; x < width; ++x) {
            uint32_t color;
            if (bytes == 4) { memcpy(&color, row + x * 4, 4); color |= 0xff000000; }
            else {
                uint16_t value; memcpy(&value, row + x * 2, 2);
                unsigned r, g, b = value & 31;
                if (active->pixel_format == RETRO_PIXEL_FORMAT_RGB565) {
                    r = (value >> 11) & 31; g = (value >> 5) & 63; g = (g << 2) | (g >> 4);
                } else { r = (value >> 10) & 31; g = (value >> 5) & 31; g = (g << 3) | (g >> 2); }
                r = (r << 3) | (r >> 2); b = (b << 3) | (b >> 2);
                color = 0xff000000 | (r << 16) | (g << 8) | b;
            }
            active->pixels[y * width + x] = color;
        }
    }
}
static size_t audio_batch(const int16_t *samples, size_t frames) {
    if (active && samples && frames <= 524288 && active->audio.size() + frames * 2 <= 1048576)
        active->audio.insert(active->audio.end(), samples, samples + frames * 2);
    else if (active) active->error = "The core produced too much audio in one frame.";
    return frames;
}
static void audio_sample(int16_t left, int16_t right) { int16_t samples[2] = {left, right}; audio_batch(samples, 1); }
static void poll() {}
static int16_t input(unsigned port, unsigned device, unsigned index, unsigned id) {
    if (!active || port >= 4) return 0;
    if ((device & RETRO_DEVICE_MASK) == RETRO_DEVICE_ANALOG)
        return index < 2 && id < 2 ? active->axes[port][index * 2 + id] : 0;
    if ((device & RETRO_DEVICE_MASK) != RETRO_DEVICE_JOYPAD || index) return 0;
    const unsigned mapping[16] = {2,2048,4,8,64,128,32,16,1,1024,512,256,4096,8192,16384,32768};
    unsigned mask = 0;
    for (unsigned i = 0; i < 16; ++i) if (active->keys[port] & mapping[i]) mask |= 1u << i;
    return id == RETRO_DEVICE_ID_JOYPAD_MASK ? static_cast<int16_t>(mask) : (id < 16 && (mask & (1u << id)) ? 1 : 0);
}
static void memory_file(Host *h, unsigned id, const char *name, bool write) {
    size_t size = h->get_memory_size(id); void *memory = h->get_memory_data(id);
    if (!memory || !size || size > 32 * 1024 * 1024) return;
    std::string path = h->save + "/" + name;
    if (write) {
        std::ofstream file(path + ".tmp", std::ios::binary | std::ios::trunc);
        file.write(static_cast<const char*>(memory), size); file.close();
        if (!file || std::rename((path + ".tmp").c_str(), path.c_str())) h->error = "Could not write battery save.";
    } else {
        std::ifstream file(path, std::ios::binary | std::ios::ate);
        if (file && file.tellg() == static_cast<std::streamoff>(size)) {
            file.seekg(0); file.read(static_cast<char*>(memory), size);
        }
    }
}
extern "C" {
void el_set_analog(void *instance, int16_t lx, int16_t ly, int16_t rx, int16_t ry) {
    auto h = static_cast<Host*>(instance);
    h->axes[0][0] = lx; h->axes[0][1] = ly; h->axes[0][2] = rx; h->axes[0][3] = ry;
}
void el_set_port_input(void *instance, unsigned port, unsigned keys, int16_t lx, int16_t ly, int16_t rx, int16_t ry) {
    auto h = static_cast<Host*>(instance);
    if (!h || port >= 4) return;
    if (!h->input_ports[port]) {
        h->set_controller_port_device(port, RETRO_DEVICE_JOYPAD);
        h->input_ports[port] = true;
    }
    h->keys[port] = keys;
    h->axes[port][0] = lx; h->axes[port][1] = ly; h->axes[port][2] = rx; h->axes[port][3] = ry;
}
uint16_t el_rumble_strength(void *instance, unsigned port, unsigned effect) {
    auto h = static_cast<Host*>(instance);
    if (!h || port >= 4 || effect > RETRO_RUMBLE_WEAK) return 0;
    return h->rumble[port][effect].load(std::memory_order_relaxed);
}
void el_set_runtime_option(void *instance, const char *key, const char *value) {
    auto h = static_cast<Host*>(instance);
    if (!h || !key || !value) return;
    h->options[key] = value;
    h->variables_updated.store(true, std::memory_order_relaxed);
}
void el_set_hardware_allowed(int allowed) {
    if (!active) launch_hardware_allowed = allowed != 0;
}
void el_set_vulkan_allowed(int allowed) {
    if (!active) launch_vulkan_allowed = allowed != 0;
}
int el_vulkan_available() { return VulkanHost::available(); }
int el_vulkan_initialize(void *instance) {
    auto *h = static_cast<Host*>(instance);
    if (!h || !h->vulkan || h->hardware.context_type != RETRO_HW_CONTEXT_VULKAN) return 0;
    if (!h->vulkan->initialize(h->error)) return 0;
    return 1;
}
void el_set_hardware_capabilities(unsigned gl_major, unsigned gl_minor,
                                  unsigned core_major, unsigned core_minor) {
    if (active) return;
    launch_gl_major = gl_major; launch_gl_minor = gl_minor;
    launch_core_major = core_major; launch_core_minor = core_minor;
}
int el_hardware_active(void *instance) {
    auto h = static_cast<Host*>(instance); return h && h->hardware_active;
}
unsigned el_hardware_context_type(void *instance) {
    auto h = static_cast<Host*>(instance); return h ? h->hardware.context_type : RETRO_HW_CONTEXT_NONE;
}
unsigned el_hardware_version_major(void *instance) {
    auto h = static_cast<Host*>(instance); return h ? h->hardware.version_major : 0;
}
unsigned el_hardware_version_minor(void *instance) {
    auto h = static_cast<Host*>(instance); return h ? h->hardware.version_minor : 0;
}
unsigned el_hardware_max_width(void *instance) {
    auto h = static_cast<Host*>(instance); return h ? h->max_width : 0;
}
unsigned el_hardware_max_height(void *instance) {
    auto h = static_cast<Host*>(instance); return h ? h->max_height : 0;
}
int el_hardware_bottom_left(void *instance) {
    auto h = static_cast<Host*>(instance); return h && h->hardware.bottom_left_origin;
}
void el_set_hardware_framebuffer(void *instance, uintptr_t framebuffer) {
    auto h = static_cast<Host*>(instance); if (h) h->hardware_framebuffer = framebuffer;
}
int el_hardware_context_reset(void *instance) {
    auto h = static_cast<Host*>(instance);
    if (!h || !h->hardware_active || !h->hardware.context_reset ||
        (h->hardware.context_type != RETRO_HW_CONTEXT_VULKAN && !h->hardware_framebuffer)) return 0;
    h->hardware.context_reset(); h->hardware_reset = true; return 1;
}
void el_hardware_context_destroy(void *instance) {
    auto h = static_cast<Host*>(instance);
    if (!h || !h->hardware_reset) return;
    if (h->hardware.context_destroy) h->hardware.context_destroy();
    h->hardware_reset = false;
}
int el_last_frame_hardware(void *instance) {
    auto h = static_cast<Host*>(instance); return h && h->last_frame_hardware;
}
void el_clear_options() { if (!active) launch_options.clear(); }
void el_set_option(const char *key, const char *value) {
    if (!active && key && value) launch_options[key] = value;
}
const char *el_error() { return active && !active->error.empty() ? active->error.c_str() : last_error.c_str(); }
void el_flush(void *instance) {
    auto h = static_cast<Host*>(instance);
    if (h && h->loaded) { memory_file(h, RETRO_MEMORY_SAVE_RAM, "battery.srm", true); memory_file(h, RETRO_MEMORY_RTC, "clock.rtc", true); }
}
int el_import_battery(void *instance, const char *path) {
    auto h = static_cast<Host*>(instance);
    auto memory = h->get_memory_data(RETRO_MEMORY_SAVE_RAM);
    size_t size = h->get_memory_size(RETRO_MEMORY_SAVE_RAM);
    if (!memory || !size || size > 32 * 1024 * 1024) return 0;
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    if (!file || file.tellg() != static_cast<std::streamoff>(size)) return 0;
    file.seekg(0);
    std::vector<char> bytes(size);
    if (!file.read(bytes.data(), size)) return 0;
    memcpy(memory, bytes.data(), size);
    el_flush(h);
    return h->error.empty();
}
void el_destroy(void *instance) {
    auto h = static_cast<Host*>(instance); if (!h) return;
    if (h->loaded) { el_flush(h); h->unload_game(); }
    if (h->initialized) h->deinit();
    if (h->module) dlclose(h->module);
    if (active == h) active = nullptr;
    delete h;
}
void *el_libretro_create(const char *core, const char *rom, const char *save, const char *system) {
    if (active) { last_error = "A Libretro core is already running in this process."; return nullptr; }
    auto h = new Host(); h->options = launch_options; launch_options.clear();
    h->hardware_allowed = launch_hardware_allowed; launch_hardware_allowed = false;
    h->vulkan_allowed = launch_vulkan_allowed; launch_vulkan_allowed = false;
    h->gl_major = launch_gl_major; h->gl_minor = launch_gl_minor;
    h->core_major = launch_core_major; h->core_minor = launch_core_minor;
    launch_gl_major = launch_gl_minor = launch_core_major = launch_core_minor = 0;
    active = h; last_error.clear();
    h->core = core; h->rom = rom; h->save = save; h->system = system;
    h->module = dlopen(core, RTLD_NOW | RTLD_LOCAL);
    if (!h->module) { last_error = dlerror(); el_destroy(h); return nullptr; }
#define LOAD(name) h->name = reinterpret_cast<decltype(h->name)>(dlsym(h->module, "retro_" #name)); if (!h->name) { last_error = "Missing Libretro function: " #name; el_destroy(h); return nullptr; }
    LOAD(init); LOAD(deinit); LOAD(api_version); LOAD(get_system_info); LOAD(get_system_av_info);
    LOAD(set_environment); LOAD(set_video_refresh); LOAD(set_audio_sample); LOAD(set_audio_sample_batch);
    LOAD(set_input_poll); LOAD(set_input_state); LOAD(set_controller_port_device); LOAD(load_game);
    LOAD(unload_game); LOAD(run); LOAD(reset); LOAD(serialize_size); LOAD(serialize); LOAD(unserialize);
    LOAD(get_memory_data); LOAD(get_memory_size);
#undef LOAD
    if (h->api_version() != RETRO_API_VERSION) { last_error = "Unsupported Libretro API version."; el_destroy(h); return nullptr; }
    h->set_environment(environment); h->set_video_refresh(video); h->set_audio_sample(audio_sample);
    h->set_audio_sample_batch(audio_batch); h->set_input_poll(poll); h->set_input_state(input);
    h->init(); h->initialized = true;
    retro_system_info info{}; h->get_system_info(&info);
    if (!info.need_fullpath) {
        std::ifstream file(rom, std::ios::binary);
        h->content.assign(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
    }
    retro_game_info game{h->rom.c_str(), info.need_fullpath ? nullptr : h->content.data(), info.need_fullpath ? 0 : h->content.size(), nullptr};
    if (!h->load_game(&game)) { last_error = "The Libretro core could not load this ROM. Check its format and any required BIOS files."; el_destroy(h); return nullptr; }
    h->loaded = true;
    h->set_controller_port_device(0, RETRO_DEVICE_JOYPAD);
    retro_system_av_info av{}; h->get_system_av_info(&av);
    if (!valid_av(av)) {
        last_error = "Unsupported core video dimensions or audio timing: " + std::to_string(av.geometry.base_width) + "x" +
            std::to_string(av.geometry.base_height) + ", " + std::to_string(av.timing.fps) + " fps, " + std::to_string(av.timing.sample_rate) + " Hz.";
        el_destroy(h); return nullptr;
    }
    h->width = av.geometry.base_width; h->height = av.geometry.base_height;
    h->max_width = std::max(av.geometry.base_width, av.geometry.max_width);
    h->max_height = std::max(av.geometry.base_height, av.geometry.max_height);
    if (!h->max_width || !h->max_height || h->max_width > 4096 || h->max_height > 4096) {
        last_error = "Unsupported hardware render target size."; el_destroy(h); return nullptr;
    }
    h->aspect = av.geometry.aspect_ratio;
    h->fps = av.timing.fps; h->rate = av.timing.sample_rate; h->pixels.resize(h->width * h->height, 0xff000000);
    memory_file(h, RETRO_MEMORY_SAVE_RAM, "battery.srm", false); memory_file(h, RETRO_MEMORY_RTC, "clock.rtc", false);
    return h;
}
void *el_create(const char*, const char*) { last_error = "Select a Libretro core file first."; return nullptr; }
void el_reset(void *instance) {
    auto h = static_cast<Host*>(instance);
    for (auto &port : h->rumble) for (auto &motor : port) motor.store(0, std::memory_order_relaxed);
    h->reset();
}
int el_frame(void *instance, unsigned keys) {
    auto h = static_cast<Host*>(instance); h->keys[0] = keys; h->audio.clear();
    h->last_frame_hardware = false; h->run();
    if (!h->error.empty() || h->shutdown) return -1;
    // Convert each core's native rate (including SameBoy's MHz-rate audio) to the
    // desktop's 48 kHz stream, preserving fractional position between frames.
    h->output.clear();
    double step = h->rate / 48000.0;
    for (size_t i = 0; i + 1 < h->audio.size(); i += 2) {
        int16_t left = h->audio[i], right = h->audio[i + 1];
        if (step >= 1) {
            // Average the complete source interval instead of discarding
            // high-rate samples, so downsampling also provides a low-pass filter.
            double remaining = 1;
            while (remaining > 1e-9) {
                double part = std::min(remaining, step - h->accumulated);
                h->sum_left += left * part; h->sum_right += right * part;
                h->accumulated += part; remaining -= part;
                if (h->accumulated >= step - 1e-9) {
                    h->output.push_back(std::lround(h->sum_left / step));
                    h->output.push_back(std::lround(h->sum_right / step));
                    h->sum_left = h->sum_right = h->accumulated = 0;
                }
            }
            continue;
        }
        if (!h->have_previous) { h->previous_left = left; h->previous_right = right; h->have_previous = true; }
        while (h->phase < 1.0) {
            h->output.push_back(std::lround(h->previous_left + (left - h->previous_left) * h->phase));
            h->output.push_back(std::lround(h->previous_right + (right - h->previous_right) * h->phase));
            h->phase += step;
        }
        h->phase -= 1.0; h->previous_left = left; h->previous_right = right;
    }
    return h->output.size() / 2;
}
const uint32_t *el_pixels(void *h) { return static_cast<Host*>(h)->pixels.data(); }
const int16_t *el_audio(void *h) { return static_cast<Host*>(h)->output.data(); }
unsigned el_width(void *h) { return static_cast<Host*>(h)->width; }
unsigned el_height(void *h) { return static_cast<Host*>(h)->height; }
double el_fps(void *h) { return static_cast<Host*>(h)->fps; }
double el_aspect_ratio(void *instance) {
    auto h = static_cast<Host*>(instance);
    return std::isfinite(h->aspect) && h->aspect > 0 && h->aspect < 10 ? h->aspect : double(h->width) / h->height;
}
unsigned el_sample_rate(void*) { return 48000; }
int el_save_state(void *instance, const char *path) {
    auto h = static_cast<Host*>(instance); size_t size = h->serialize_size();
    if (!size || size > 31 * 1024 * 1024) return 0;
    std::vector<char> state(size); if (!h->serialize(state.data(), size)) return 0;
    std::ofstream file(path, std::ios::binary); file.write(state.data(), size); return file.good();
}
int el_load_state(void *instance, const char *path) {
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    if (!file || file.tellg() <= 0 || file.tellg() > 31 * 1024 * 1024) return 0;
    size_t size = file.tellg(); file.seekg(0); std::vector<char> state(size); file.read(state.data(), size);
    auto h = static_cast<Host*>(instance);
    // Persistent saves and memory cards are independent of emulator states.
    // Preserve them when restoring an older state so an in-game save cannot be
    // silently rolled back and written to disk on the next battery flush.
    auto snapshot = [h](unsigned id) {
        size_t size = h->get_memory_size(id); void *memory = h->get_memory_data(id);
        if (!memory || !size || size > 32 * 1024 * 1024) return std::vector<char>{};
        return std::vector<char>(static_cast<char*>(memory), static_cast<char*>(memory) + size);
    };
    auto battery = snapshot(RETRO_MEMORY_SAVE_RAM);
    auto rtc = snapshot(RETRO_MEMORY_RTC);
    bool ok = file.good() && h->unserialize(state.data(), state.size());
    auto restore = [h](unsigned id, const std::vector<char> &saved) {
        if (saved.empty()) return;
        size_t size = h->get_memory_size(id); void *memory = h->get_memory_data(id);
        if (memory && size == saved.size()) memcpy(memory, saved.data(), size);
    };
    restore(RETRO_MEMORY_SAVE_RAM, battery);
    restore(RETRO_MEMORY_RTC, rtc);
    h->audio.clear(); h->output.clear(); h->have_previous = false; h->phase = 0;
    h->sum_left = h->sum_right = h->accumulated = 0; return ok;
}
}
