// Original CC0 libretro input echo diagnostic; no commercial ROM required.
#include "libretro.h"
#include <cstdint>
#include <cstring>
static uint32_t frame;
static unsigned initialization_frames;
static uint8_t battery[4];
static retro_environment_t environment;
static retro_video_refresh_t video;
static retro_input_poll_t poll;
static retro_input_state_t input;
extern "C" {
unsigned retro_api_version() { return RETRO_API_VERSION; }
void retro_set_environment(retro_environment_t fn) { environment = fn; }
void retro_set_video_refresh(retro_video_refresh_t fn) { video = fn; }
void retro_set_audio_sample(retro_audio_sample_t) {}
void retro_set_audio_sample_batch(retro_audio_sample_batch_t) {}
void retro_set_input_poll(retro_input_poll_t fn) { poll = fn; }
void retro_set_input_state(retro_input_state_t fn) { input = fn; }
void retro_set_controller_port_device(unsigned,unsigned) {}
void retro_init() { initialization_frames=0;frame=0; uint8_t initial[4]={7,8,9,10}; std::memcpy(battery,initial,4); }
void retro_deinit() {}
void retro_get_system_info(retro_system_info *info) { *info = {"Rust input echo", "1", "bin", false, false}; }
void retro_get_system_av_info(retro_system_av_info *info) { *info = {{4,4,4,4,1.f},{60.,48000.}}; }
bool retro_load_game(const retro_game_info *) {
    auto format = RETRO_PIXEL_FORMAT_XRGB8888;
    return environment(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,&format);
}
void retro_unload_game() {}
void retro_reset() { frame=0; }
void retro_run() {
    ++initialization_frames;
    static uint32_t pixels[16];
    poll();
    for (unsigned axis=0; axis<4; ++axis)
        pixels[axis] = static_cast<uint16_t>(input(0,RETRO_DEVICE_ANALOG,axis/2,axis%2));
    pixels[4] = static_cast<uint16_t>(input(0,RETRO_DEVICE_JOYPAD,0,RETRO_DEVICE_ID_JOYPAD_MASK));
    pixels[5]=frame++;pixels[6]=++battery[0];
    for (unsigned port=1; port<4; ++port) {
        pixels[5 + port * 2] = static_cast<uint16_t>(input(port,RETRO_DEVICE_JOYPAD,0,RETRO_DEVICE_ID_JOYPAD_MASK));
        pixels[6 + port * 2] = static_cast<uint16_t>(input(port,RETRO_DEVICE_ANALOG,0,0));
    }
    video(pixels,4,4,16);
}
size_t retro_serialize_size() { return 8; }
bool retro_serialize(void *data,size_t size) {
    if (size!=8) return false; std::memcpy(data,&frame,4);std::memcpy(static_cast<uint8_t*>(data)+4,battery,4);return true;
}
bool retro_unserialize(const void *data,size_t size) {
#ifdef RUST_FIXTURE_STATE_INITIALIZATION
    if(initialization_frames<2)return false;
#endif

    if (size!=8) return false;std::memcpy(&frame,data,4);std::memcpy(battery,static_cast<const uint8_t*>(data)+4,4);return true;
}
void *retro_get_memory_data(unsigned id) { return id==RETRO_MEMORY_SAVE_RAM ? battery : nullptr; }
size_t retro_get_memory_size(unsigned id) { return id==RETRO_MEMORY_SAVE_RAM ? 4 : 0; }
}
