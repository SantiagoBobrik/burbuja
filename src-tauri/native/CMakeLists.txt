# Builds whisper.cpp (vendor/whisper.cpp, pinned to a release tag) and Burbuja's thin C layer
# over it. Driven by build.rs through the `cmake` crate, which passes the platform options.
cmake_minimum_required(VERSION 3.21)
project(burbuja_native C CXX)

if (NOT WHISPER_CPP_DIR)
    message(FATAL_ERROR "WHISPER_CPP_DIR is not set")
endif()

# Every artifact lands in two known folders, whatever the generator or configuration, so
# build.rs can link and stage them without guessing.
foreach(cfg "" _DEBUG _RELEASE _RELWITHDEBINFO _MINSIZEREL)
    set(CMAKE_RUNTIME_OUTPUT_DIRECTORY${cfg} ${CMAKE_BINARY_DIR}/out/bin)
    set(CMAKE_LIBRARY_OUTPUT_DIRECTORY${cfg} ${CMAKE_BINARY_DIR}/out/bin)
    set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY${cfg} ${CMAKE_BINARY_DIR}/out/lib)
endforeach()

add_subdirectory(${WHISPER_CPP_DIR} w)

add_library(burbuja_engine STATIC engine.c)
target_link_libraries(burbuja_engine PRIVATE whisper parakeet ggml)
