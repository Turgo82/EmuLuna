cmake_minimum_required(VERSION 3.20)
project(EmuLuna VERSION 0.13.3 LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 14)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/cores")
# Only the frontend middleware is built here. Emulator cores are standard
# libretro binaries installed independently by the Core Manager.
add_library(emuluna_host SHARED native/libretro.cpp)
target_link_libraries(emuluna_host PRIVATE ${CMAKE_DL_LIBS})
target_link_options(emuluna_host PRIVATE -Wl,--no-undefined)
install(TARGETS emuluna_host LIBRARY DESTINATION lib/emuluna/cores)
