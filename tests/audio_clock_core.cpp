// Original diagnostic core: a continuous 1 kHz stereo tone, opposite channels.
#include "libretro.h"
#include <cmath>
#include <cstdlib>
#include <vector>
#include <cstdint>
static retro_environment_t environment;
static retro_video_refresh_t video;
static retro_audio_sample_batch_t audio;
static double rate=48000;
static uint64_t sample=0;
extern "C" {
void retro_set_environment(retro_environment_t cb) { environment=cb; }
void retro_set_video_refresh(retro_video_refresh_t cb) { video=cb; }
void retro_set_audio_sample(retro_audio_sample_t) {}
void retro_set_audio_sample_batch(retro_audio_sample_batch_t cb) { audio=cb; }
void retro_set_input_poll(retro_input_poll_t) {}
void retro_set_input_state(retro_input_state_t) {}
void retro_set_controller_port_device(unsigned,unsigned) {}
void retro_init() {}
void retro_deinit() {}
unsigned retro_api_version() { return RETRO_API_VERSION; }
void retro_get_system_info(retro_system_info *info) {
    *info={"Audio clock fixture","1","bin",false,false};
}
void retro_get_system_av_info(retro_system_av_info *info) {
    *info={{4,4,4,4,1.f},{60.,rate}};
}
bool retro_load_game(const retro_game_info *) {
    retro_variable option{"fixture_rate",nullptr};
    if (environment(RETRO_ENVIRONMENT_GET_VARIABLE,&option) && option.value)
        rate=std::strtod(option.value,nullptr);
    auto format=RETRO_PIXEL_FORMAT_XRGB8888;
    sample=0;
    return environment(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,&format);
}
void retro_unload_game() {}
void retro_reset() { sample=0; }
void retro_run() {
    static uint32_t pixels[16]{};
    video(pixels,4,4,16);
    std::vector<int16_t> pcm;
    for (unsigned n=0;n<unsigned(rate/60);++n,++sample) {
        int16_t value=std::lround(10000*std::sin(2*3.141592653589793*1000*sample/rate));
        pcm.push_back(value);pcm.push_back(-value);
    }
    audio(pcm.data(),pcm.size()/2);
}
size_t retro_serialize_size() { return sizeof(sample); }
bool retro_serialize(void*,size_t) { return false; }
bool retro_unserialize(const void*,size_t) { return false; }
void *retro_get_memory_data(unsigned) { return nullptr; }
size_t retro_get_memory_size(unsigned) { return 0; }
}
