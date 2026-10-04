// Original libretro fixture: a hardware frame, duplicates, then a software frame.
#include "libretro.h"
#include <cstring>
#include <cstdlib>
static retro_environment_t environment;
static retro_video_refresh_t video;
static unsigned frame;
static void context_reset() {}
extern "C" {
void retro_set_environment(retro_environment_t cb) { environment = cb; }
void retro_set_video_refresh(retro_video_refresh_t cb) { video = cb; }
void retro_set_audio_sample(retro_audio_sample_t) {}
void retro_set_audio_sample_batch(retro_audio_sample_batch_t) {}
void retro_set_input_poll(retro_input_poll_t) {}
void retro_set_input_state(retro_input_state_t) {}
void retro_set_controller_port_device(unsigned, unsigned) {}
void retro_init() {}
void retro_deinit() {}
unsigned retro_api_version() { return RETRO_API_VERSION; }
void retro_get_system_info(retro_system_info *info) {
    *info = {"Hardware duplicate fixture", "1", "bin", false, false};
}
void retro_get_system_av_info(retro_system_av_info *info) {
    *info = {{4, 4, 4, 4, 1.0f}, {60.0, 48000.0}};
}
bool retro_load_game(const retro_game_info *) {
    retro_hw_render_callback hw{};
    hw.context_type = RETRO_HW_CONTEXT_OPENGL_CORE;
    hw.version_major = 3; hw.version_minor = 3;
    hw.context_reset = context_reset;
    auto format = RETRO_PIXEL_FORMAT_XRGB8888;
    frame = 0;
    return environment(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, &format) &&
        environment(RETRO_ENVIRONMENT_SET_HW_RENDER, &hw);
}
void retro_unload_game() {}
void retro_reset() { frame = 0; }
void retro_run() {
    static uint32_t pixels[16];
    if (frame == 0) video(RETRO_HW_FRAME_BUFFER_VALID, 4, 4, 0);
    else if (frame == 1 || frame >= 4) video(nullptr, 4, 4, 0);
    else if (frame == 3) {
        for (auto &pixel : pixels) pixel = 0x00112233;
        video(pixels, 4, 4, 16);
    }
    // Frame 2 intentionally makes no video callback: retain the previous frame.
    ++frame;
}
size_t retro_serialize_size() {
    retro_variable option{"fixture_state_size", nullptr};
    if (environment(RETRO_ENVIRONMENT_GET_VARIABLE, &option) && option.value)
        return std::strtoul(option.value, nullptr, 10);
    return sizeof(frame);
}
bool retro_serialize(void *data, size_t size) {
    if (size != retro_serialize_size() || size < sizeof(frame)) return false;
    std::memset(data, 0, size);
    std::memcpy(data, &frame, sizeof(frame)); return true;
}
bool retro_unserialize(const void *data, size_t size) {
    if (size != retro_serialize_size() || size < sizeof(frame)) return false;
    std::memcpy(&frame, data, sizeof(frame)); return true;
}
void *retro_get_memory_data(unsigned) { return nullptr; }
size_t retro_get_memory_size(unsigned) { return 0; }
}
