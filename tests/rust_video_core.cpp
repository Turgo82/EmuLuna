/* SPDX-License-Identifier: CC0-1.0
 * Original libretro contract fixture: padded software formats, duplicate frames,
 * retained content, geometry/timing changes and stereo callback delivery. */
#include "libretro.h"
#include <cstdint>
#include <cstdlib>
#include <cstring>
static retro_environment_t environment;
static retro_video_refresh_t video;
static retro_audio_sample_t sample;
static retro_audio_sample_batch_t batch;
static const uint8_t *content;
static unsigned frame, format;
static retro_perf_callback perf;
static retro_perf_counter timing_counter;
static uint64_t cpu_features;
static bool split_rate;
extern "C" {
void retro_set_environment(retro_environment_t cb) {
    environment=cb;
    static retro_variable definitions[]={{"fixture_format","Pixel format; 1555|565|8888"},{"fixture_split_rate","Split audio rates; disabled|enabled"},{nullptr,nullptr}};
    if (!environment(RETRO_ENVIRONMENT_SET_VARIABLES,definitions)) std::abort();
}
void retro_set_video_refresh(retro_video_refresh_t cb){video=cb;}
void retro_set_audio_sample(retro_audio_sample_t cb){sample=cb;}
void retro_set_audio_sample_batch(retro_audio_sample_batch_t cb){batch=cb;}
void retro_set_input_poll(retro_input_poll_t){}
void retro_set_input_state(retro_input_state_t){}
void retro_set_controller_port_device(unsigned,unsigned){}
void retro_init(){frame=0;timing_counter={"original fixture"};}
void retro_deinit(){}
unsigned retro_api_version(){return RETRO_API_VERSION;}
void retro_get_system_info(retro_system_info *info){*info={"Rust software contract fixture","1","bin",false,false};}
void retro_get_system_av_info(retro_system_av_info *info){*info={{2,2,8,8,1.f},{60.,48000.}};}
bool retro_load_game(const retro_game_info *game){
    if (!game || !game->data || game->size!=2) return false;
    content=static_cast<const uint8_t*>(game->data);
    if(!environment(RETRO_ENVIRONMENT_GET_PERF_INTERFACE,&perf) || !perf.get_time_usec
        || !perf.get_cpu_features || !perf.get_perf_counter || !perf.perf_register
        || !perf.perf_start || !perf.perf_stop || !perf.perf_log) return false;
#if defined(__x86_64__)
    if(!(perf.get_cpu_features() & RETRO_SIMD_SSE2)) return false;
    __builtin_cpu_init();
    if(bool(perf.get_cpu_features() & RETRO_SIMD_AVX2)!=bool(__builtin_cpu_supports("avx2"))) return false;
#endif
    cpu_features=perf.get_cpu_features();
    const auto before=perf.get_perf_counter();
    const auto time=perf.get_time_usec();
    perf.perf_register(&timing_counter);
    if(!timing_counter.registered) return false;
    perf.perf_start(&timing_counter);
    for(volatile unsigned i=0;i<20000;++i){}
    perf.perf_stop(&timing_counter);
    const auto total=timing_counter.total;
    perf.perf_register(&timing_counter); // Re-registering must not reset totals.
    if(timing_counter.call_cnt!=1 || total==0 || timing_counter.total!=total
        || perf.get_perf_counter()<before || perf.get_time_usec()<time) return false;
    retro_variable option{"fixture_format",nullptr};
    if (!environment(RETRO_ENVIRONMENT_GET_VARIABLE,&option) || !option.value) return false;
    format=!std::strcmp(option.value,"565")?RETRO_PIXEL_FORMAT_RGB565:
        !std::strcmp(option.value,"8888")?RETRO_PIXEL_FORMAT_XRGB8888:RETRO_PIXEL_FORMAT_0RGB1555;
    retro_variable split{"fixture_split_rate",nullptr};
    split_rate=environment(RETRO_ENVIRONMENT_GET_VARIABLE,&split) && split.value && !std::strcmp(split.value,"enabled");
    auto selected=static_cast<retro_pixel_format>(format);
    if (!environment(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,&selected)) return false;
    const char *save=nullptr,*system=nullptr;
    if (!environment(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY,&save)||!save||!*save
        ||!environment(RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY,&system)||!system||!*system) return false;
    retro_variable missing{"not_a_fixture_option",reinterpret_cast<const char*>(uintptr_t(1))};
    if(environment(RETRO_ENVIRONMENT_GET_VARIABLE,&missing)||missing.value!=nullptr) return false;
    bool changed=true;
    if(!environment(RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,&changed)||changed) return false;
    return true;
}
void retro_unload_game(){if(content[0]!=0x5a || content[1]!=0xa5) std::abort();perf.perf_log();}
void retro_reset(){frame=0;retro_system_av_info av;retro_get_system_av_info(&av);if(!environment(RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO,&av))std::abort();}
void retro_run(){
    if(content[0]!=0x5a || content[1]!=0xa5) std::abort();
    if(frame==6){environment(RETRO_ENVIRONMENT_SHUTDOWN,nullptr);return;}
    if(frame==3){retro_game_geometry geometry{3,1,8,8,16.f/9.f};if(!environment(RETRO_ENVIRONMENT_SET_GEOMETRY,&geometry))std::abort();}
    if(frame==4){retro_system_av_info av{{3,1,8,8,3.f},{50.,44100.}};if(!environment(RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO,&av))std::abort();}
    const unsigned width=frame<3?2:3,height=frame<3?2:1;
    if(frame==1) video(nullptr,0,0,0);
    else if(frame!=2){
        if(format==RETRO_PIXEL_FORMAT_XRGB8888){
            // Non-opaque source high bytes and row padding must not become
            // visible transparency or extra pixels in the frontend snapshot.
            uint32_t pixels[]={0x00ff0000,0x8000ff00,0x000000ff,0xdeadbeef,0x000000ff,0x00ffffff,0xdeadbeef,0xdeadbeef};
            if(frame==5){pixels[0]=(cpu_features&RETRO_SIMD_SSE2)?0xff0000:0;pixels[1]=(cpu_features&RETRO_SIMD_AVX2)?0x00ff00:0;pixels[2]=(cpu_features&RETRO_SIMD_AES)?0x0000ff:0;}
            video(pixels,width,height,16);
        }else{
            const uint16_t red=format==RETRO_PIXEL_FORMAT_RGB565?0xf800:0xfc00;
            const uint16_t green=format==RETRO_PIXEL_FORMAT_RGB565?0x07e0:0x83e0;
            uint16_t pixels[]={red,green,0x001f,0xdead,0x001f,0xffff,0xdead,0xdead};
            if(frame==5){pixels[0]=(cpu_features&RETRO_SIMD_SSE2)?red:0;pixels[1]=(cpu_features&RETRO_SIMD_AVX2)?green:0;pixels[2]=(cpu_features&RETRO_SIMD_AES)?0x001f:0;}
            video(pixels,width,height,8);
        }
    }
    if(split_rate){
        const double fps=frame>=4?50.:60.;
        for(unsigned half=0;half<2;++half){
            const double rate=half?24000.:48000.;
            retro_system_av_info av{{width,height,8,8,1.f},{fps,rate}};
            if(!environment(RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO,&av))std::abort();
            const unsigned count=unsigned(rate/fps/2);
            if(!half)for(unsigned i=0;i<count;++i)sample(1000,-1000);
            else {
                int16_t audio[960];
                for(unsigned i=0;i<count;++i){audio[i*2]=2000;audio[i*2+1]=-2000;}
                if(batch(audio,count)!=count)std::abort();
            }
        }
        ++frame;return;
    }
    sample(1000,-1000);
    int16_t audio[128];for(unsigned i=0;i<128;i+=2){audio[i]=1000;audio[i+1]=-1000;}
    if(batch(audio,64)!=64)std::abort();
    ++frame;
}
size_t retro_serialize_size(){return 0;}
bool retro_serialize(void*,size_t){return false;}
bool retro_unserialize(const void*,size_t){return false;}
void *retro_get_memory_data(unsigned){return nullptr;}
size_t retro_get_memory_size(unsigned){return 0;}
void retro_cheat_reset(){}
void retro_cheat_set(unsigned,bool,const char*){}
unsigned retro_get_region(){return RETRO_REGION_NTSC;}
bool retro_load_game_special(unsigned,const retro_game_info*,size_t){return false;}
}
