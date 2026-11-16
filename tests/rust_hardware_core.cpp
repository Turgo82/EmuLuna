// Original CC0 diagnostic: real GL output, duplicate callbacks and CPU fallback.
#include "libretro.h"
#include <cstring>
#include <cstdlib>
static retro_environment_t environment;
static retro_video_refresh_t video;
static unsigned frame;
static unsigned initialization_frames;
static bool context_active;
static retro_audio_sample_batch_t audio_batch;
static retro_hw_render_callback graphics;
static void (*bind_framebuffer)(unsigned,unsigned);
static void (*clear_color)(float,float,float,float);
static void (*clear)(unsigned);
static void lifecycle_probe() {
#ifdef RUST_FIXTURE_GL_LIFECYCLE_PROBE
    auto bind_texture = reinterpret_cast<void (*)(unsigned,unsigned)>(graphics.get_proc_address("glBindTexture"));
    bind_texture(0xffffffffu, 0);
#endif
}
static void context_reset() {
    if (context_active) std::abort();
    context_active = true;
    lifecycle_probe();
    bind_framebuffer = reinterpret_cast<decltype(bind_framebuffer)>(graphics.get_proc_address("glBindFramebuffer"));
    clear_color = reinterpret_cast<decltype(clear_color)>(graphics.get_proc_address("glClearColor"));
    clear = reinterpret_cast<decltype(clear)>(graphics.get_proc_address("glClear"));
}
static void context_destroy() {
    if (!context_active) std::abort();
    auto is_framebuffer = reinterpret_cast<unsigned char (*)(unsigned)>(graphics.get_proc_address("glIsFramebuffer"));
    if (!is_framebuffer(static_cast<unsigned>(graphics.get_current_framebuffer()))) std::abort();
    context_active = false;
}
extern "C" {
void retro_set_environment(retro_environment_t cb) { environment = cb; }
void retro_set_video_refresh(retro_video_refresh_t cb) { video = cb; }
void retro_set_audio_sample(retro_audio_sample_t) {}
void retro_set_audio_sample_batch(retro_audio_sample_batch_t cb) { audio_batch = cb; }
void retro_set_input_poll(retro_input_poll_t) {}
void retro_set_input_state(retro_input_state_t) {}
void retro_set_controller_port_device(unsigned, unsigned) {}
void retro_init() {initialization_frames=0;frame=0;}
void retro_deinit() {}
unsigned retro_api_version() { return RETRO_API_VERSION; }
void retro_get_system_info(retro_system_info *info) {
    *info = {"Rust hardware transport diagnostic", "1", "bin", false, false};
}
void retro_get_system_av_info(retro_system_av_info *info) {
    *info = {{4, 4, 4, 4, 1.0f}, {60.0, 48000.0}};
}
bool retro_load_game(const retro_game_info *) {
    retro_hw_render_callback hw{};
    hw.context_type = RETRO_HW_CONTEXT_OPENGL_CORE;
    hw.version_major = 3; hw.version_minor = 3;
    hw.context_reset = context_reset;
    hw.context_destroy = context_destroy;
    hw.bottom_left_origin = true;
    auto format = RETRO_PIXEL_FORMAT_XRGB8888;
    frame = 0;
    bool success = environment(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, &format) &&
        environment(RETRO_ENVIRONMENT_SET_HW_RENDER, &hw);
    graphics = hw;
    return success;
}
void retro_unload_game() { if (context_active) std::abort(); }
void retro_reset() { frame = 0; lifecycle_probe(); }
void retro_run() {
    ++initialization_frames;
    retro_variable shutdown_at{"fixture_shutdown_frame",nullptr};
    if (environment(RETRO_ENVIRONMENT_GET_VARIABLE,&shutdown_at) && shutdown_at.value && frame==std::strtoul(shutdown_at.value,nullptr,10)){
        environment(RETRO_ENVIRONMENT_SHUTDOWN,nullptr);return;
    }
#ifdef RUST_FIXTURE_CHECK_GL_STATE
    auto get_integer = reinterpret_cast<void (*)(unsigned,int*)>(graphics.get_proc_address("glGetIntegerv"));
    int value = -1;
    get_integer(0x84E0, &value); // GL_ACTIVE_TEXTURE
    if (value != 0x84C0) std::abort(); // GL_TEXTURE0
    get_integer(0x8069, &value); // GL_TEXTURE_BINDING_2D
    if (value != 0) std::abort();
    get_integer(0x8CA7, &value); // GL_RENDERBUFFER_BINDING
    if (value != 0) std::abort();
    get_integer(0x88EF, &value); // GL_PIXEL_UNPACK_BUFFER_BINDING
    if (value != 0) std::abort();
#endif
#ifdef RUST_FIXTURE_GL_DIAGNOSTIC
    if (frame == 0) {
        auto bind_texture = reinterpret_cast<void (*)(unsigned,unsigned)>(graphics.get_proc_address("glBindTexture"));
        // Simulate a core capability probe leaving an error while its valid
        // framebuffer and subsequent duplicate frames remain fully usable.
        bind_texture(0xffffffffu, 0);
    }
#endif
    static uint32_t pixels[16];
    static int16_t silence[40000];
    if (frame == 0 || frame >= 5) {
        bind_framebuffer(0x8D40, static_cast<unsigned>(graphics.get_current_framebuffer()));
        clear_color(frame == 0 ? 1.f : 0.f, frame >= 6 ? 1.f : 0.f, frame == 5 ? 1.f : 0.f, 1.f);
        clear(0x4000);
        video(RETRO_HW_FRAME_BUFFER_VALID,4,4,0);
    } else if (frame == 1 || frame == 4) {
        video(nullptr,4,4,0);
    } else if (frame == 3) {
        for (auto &pixel : pixels) pixel = 0x00112233;
        video(pixels,4,4,16);
    }
    // Frame 2 intentionally makes no video callback.
#ifdef RUST_FIXTURE_AUDIO_BURST
    audio_batch(silence,frame == 0 ? 19026 : 800);
#else
    audio_batch(silence,800);
#endif
    ++frame;
}
size_t retro_serialize_size() {
    retro_variable option{"fixture_state_size", nullptr};
    if (environment(RETRO_ENVIRONMENT_GET_VARIABLE, &option) && option.value)
        return std::strtoul(option.value, nullptr, 10);
    return sizeof(frame);
}
bool retro_serialize(void *data, size_t size) {
    lifecycle_probe();
    if (size != retro_serialize_size() || size < sizeof(frame)) return false;
    std::memset(data, 0, size);
    std::memcpy(data, &frame, sizeof(frame)); return true;
}
bool retro_unserialize(const void *data, size_t size) {
#ifdef RUST_FIXTURE_STATE_INITIALIZATION
    if(initialization_frames<2)return false;
#endif

    lifecycle_probe();
    if (size != retro_serialize_size() || size < sizeof(frame)) return false;
    std::memcpy(&frame, data, sizeof(frame)); return true;
}
void *retro_get_memory_data(unsigned) { return nullptr; }
size_t retro_get_memory_size(unsigned) { return 0; }
}
