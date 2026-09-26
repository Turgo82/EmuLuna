/* SPDX-License-Identifier: MIT
 * EmuLuna reusable emulation host API.
 * Buffers belong to the instance; the caller copies them before the next frame.
 * Each instance must be used from only one thread. Keys use GBA bit order,
 * extended through the standard 16-button RetroPad layout. Ports 1–4 can be
 * supplied independently. Width and height can change after el_frame. Snes9x
 * permits one instance per helper process.
 */
#pragma once
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
void* el_create(const char* rom, const char* save_dir);
void el_destroy(void* instance);
void el_reset(void* instance);
int el_frame(void* instance, unsigned keys);
void el_set_analog(void* instance, int16_t lx, int16_t ly, int16_t rx, int16_t ry);
void el_set_port_input(void* instance, unsigned port, unsigned keys, int16_t lx, int16_t ly, int16_t rx, int16_t ry);
uint16_t el_rumble_strength(void* instance, unsigned port, unsigned effect);
void el_set_runtime_option(void* instance, const char* key, const char* value);
const uint32_t* el_pixels(void* instance);
const int16_t* el_audio(void* instance);
unsigned el_width(void* instance);
unsigned el_height(void* instance);
double el_fps(void* instance);
double el_aspect_ratio(void* instance);
unsigned el_sample_rate(void* instance);
int el_save_state(void* instance, const char* path);
int el_load_state(void* instance, const char* path);
void el_flush(void* instance);
#ifdef __cplusplus
}
#endif
