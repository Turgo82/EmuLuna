cmake_minimum_required(VERSION 3.20)
project(EmuLuna VERSION 0.18.0 LANGUAGES C CXX)
set(CMAKE_CXX_STANDARD 14)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/cores")
# Only the frontend middleware is built here. Emulator cores are standard
# libretro binaries installed independently by the Core Manager.
add_library(emuluna_host SHARED native/libretro.cpp native/vulkan_host.cpp)
target_include_directories(emuluna_host PRIVATE native/vendor)
target_link_libraries(emuluna_host PRIVATE ${CMAKE_DL_LIBS})
target_link_options(emuluna_host PRIVATE -Wl,--no-undefined)
install(TARGETS emuluna_host LIBRARY DESTINATION lib/emuluna/cores)

# Compile the reader offline with its bundled codecs. It never runs an emulator.
set(CHDR_WANT_TESTS OFF CACHE BOOL "" FORCE)
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
add_subdirectory(native/vendor/libchdr EXCLUDE_FROM_ALL)
add_executable(emuluna-chd-probe native/chd_probe.cpp)
target_link_libraries(emuluna-chd-probe PRIVATE chdr-static)
set_target_properties(emuluna-chd-probe PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/cores")
install(TARGETS emuluna-chd-probe RUNTIME DESTINATION lib/emuluna/cores)
