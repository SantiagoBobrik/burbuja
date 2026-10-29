// Burbuja's C layer over whisper.cpp and parakeet.cpp.
//
// The engine worker (src/engine/native.rs) calls these instead of the libraries directly, so the
// large parameter structs of whisper.h / parakeet.h never have to be mirrored in Rust. Simple
// getters (segment text, language, VAD segment times, free) are called directly from Rust.
#ifndef BURBUJA_ENGINE_H
#define BURBUJA_ENGINE_H

#include <stdbool.h>
#include <stdint.h>

#include "whisper.h"
#include "parakeet.h"

#ifdef __cplusplus
extern "C" {
#endif

// Returns true to stop the running inference.
typedef bool (*bj_abort_fn)(void * user);

// Only log messages at or above this ggml level reach stderr (GGML_LOG_LEVEL_WARN by default).
void bj_set_log_level(int min_level);

// The worker speaks its protocol on stdout, but whisper.cpp has a few debug printf()s. This
// points C stdout (and the process's standard output) at stderr and returns a private handle
// to the original stdout for the protocol: an OS HANDLE on Windows, a file descriptor
// elsewhere. -1 on failure. Call once, before loading anything.
intptr_t bj_reserve_stdout(void);

// Loads the runtime ggml backend modules (dynamic-backend builds only; a no-op otherwise):
// GPU modules from `gpu_dir` when not NULL, then the best CPU variant from `cpu_dir`.
void bj_load_backends(const char * cpu_dir, const char * gpu_dir);

// Number of compute devices registered so far.
int bj_device_count(void);

// Describes device `i`: writes its description and backend name, returns its type
// (0 CPU, 1 GPU, 2 integrated GPU, 3 accelerator) or -1 if `i` is out of range.
int bj_device_info(int i, char * description, int description_len, char * backend, int backend_len);

// The `gpu_device` ordinal (counted over GPUs and integrated GPUs, as whisper.cpp counts) of
// the device to run on: the first discrete GPU, else the first integrated one. -1 if none.
// Writes the chosen device's position in the full device list to `device_index`.
int bj_pick_gpu(int * device_index);

struct whisper_context * bj_whisper_load(const char * path, bool use_gpu, int gpu_device);

// Transcribes `pcm` (16 kHz mono). `language` NULL means detect; `prompt` may be NULL.
// Returns 0 on success; read the result with whisper_full_n_segments / _get_segment_text.
int bj_whisper_run(struct whisper_context * ctx, const float * pcm, int n_samples,
                   const char * language, const char * prompt, int n_threads,
                   bj_abort_fn abort_fn, void * abort_user);

struct parakeet_context * bj_parakeet_load(const char * path, bool use_gpu, int gpu_device);

// Transcribes `pcm` (16 kHz mono). Returns 0 on success; read the result with
// parakeet_full_n_segments / parakeet_full_get_segment_text.
int bj_parakeet_run(struct parakeet_context * ctx, const float * pcm, int n_samples,
                    int n_threads, bj_abort_fn abort_fn, void * abort_user);

// The Silero VAD always runs on the CPU: it is tiny and stays off the GPU queue.
struct whisper_vad_context * bj_vad_load(const char * path, int n_threads);

// Speech segments of `pcm`; times are in centiseconds. NULL on failure.
struct whisper_vad_segments * bj_vad_detect(struct whisper_vad_context * vctx,
                                            const float * pcm, int n_samples,
                                            float threshold, int min_speech_ms);

#ifdef __cplusplus
}
#endif

#endif
