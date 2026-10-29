#include "engine.h"

#include <stdio.h>
#include <string.h>

#include "ggml-backend.h"

#ifdef _WIN32
#    include <fcntl.h>
#    include <io.h>
#else
#    include <fcntl.h>
#    include <unistd.h>
#endif

// Points stdout at stderr, or at the null device when the process has no stderr: a release
// (GUI subsystem) app on Windows starts the worker without one.
static int redirect_stdout(void) {
#ifdef _WIN32
    // _dup2 onto fd 1 also updates the process's STD_OUTPUT_HANDLE.
    if (_fileno(stderr) >= 0 && _dup2(_fileno(stderr), 1) == 0) {
        return 0;
    }
    int null_fd = _open("NUL", _O_WRONLY);
    if (null_fd < 0) {
        return -1;
    }
    int rc = _dup2(null_fd, 1);
    _close(null_fd);
    return rc;
#else
    if (dup2(STDERR_FILENO, STDOUT_FILENO) >= 0) {
        return 0;
    }
    int null_fd = open("/dev/null", O_WRONLY);
    if (null_fd < 0) {
        return -1;
    }
    int rc = dup2(null_fd, STDOUT_FILENO) < 0 ? -1 : 0;
    close(null_fd);
    return rc;
#endif
}

intptr_t bj_reserve_stdout(void) {
    fflush(stdout);
#ifdef _WIN32
    int saved = _dup(_fileno(stdout));
    if (saved < 0) {
        return -1;
    }
    if (redirect_stdout() != 0) {
        _close(saved);
        return -1;
    }
    return _get_osfhandle(saved);
#else
    int saved = dup(STDOUT_FILENO);
    if (saved < 0) {
        return -1;
    }
    if (redirect_stdout() != 0) {
        close(saved);
        return -1;
    }
    return saved;
#endif
}

static int g_min_level = GGML_LOG_LEVEL_WARN;
static int g_last_level = GGML_LOG_LEVEL_NONE;

static void log_to_stderr(enum ggml_log_level level, const char * text, void * user) {
    (void) user;
    if (level != GGML_LOG_LEVEL_CONT) {
        g_last_level = level;
    }
    if (g_last_level >= g_min_level) {
        fputs(text, stderr);
    }
}

void bj_set_log_level(int min_level) {
    g_min_level = min_level;
    ggml_log_set(log_to_stderr, NULL);
    whisper_log_set(log_to_stderr, NULL);
    parakeet_log_set(log_to_stderr, NULL);
}

void bj_load_backends(const char * cpu_dir, const char * gpu_dir) {
#ifdef GGML_BACKEND_DL
    if (gpu_dir != NULL) {
        ggml_backend_load_all_from_path(gpu_dir);
    }
    ggml_backend_load_all_from_path(cpu_dir);
#else
    (void) cpu_dir;
    (void) gpu_dir;
#endif
}

int bj_device_count(void) {
    return (int) ggml_backend_dev_count();
}

static void copy_str(char * dst, int len, const char * src) {
    if (dst == NULL || len <= 0) {
        return;
    }
    snprintf(dst, (size_t) len, "%s", src != NULL ? src : "");
}

int bj_device_info(int i, char * description, int description_len, char * backend, int backend_len) {
    if (i < 0 || (size_t) i >= ggml_backend_dev_count()) {
        return -1;
    }
    ggml_backend_dev_t dev = ggml_backend_dev_get((size_t) i);
    copy_str(description, description_len, ggml_backend_dev_description(dev));
    copy_str(backend, backend_len, ggml_backend_reg_name(ggml_backend_dev_backend_reg(dev)));
    return (int) ggml_backend_dev_type(dev);
}

int bj_pick_gpu(int * device_index) {
    int ordinal = 0;
    int igpu_ordinal = -1;
    int igpu_index = -1;
    for (size_t i = 0; i < ggml_backend_dev_count(); ++i) {
        enum ggml_backend_dev_type type = ggml_backend_dev_type(ggml_backend_dev_get(i));
        if (type == GGML_BACKEND_DEVICE_TYPE_GPU) {
            *device_index = (int) i;
            return ordinal;
        }
        if (type == GGML_BACKEND_DEVICE_TYPE_IGPU) {
            if (igpu_ordinal < 0) {
                igpu_ordinal = ordinal;
                igpu_index = (int) i;
            }
        } else {
            continue;
        }
        ++ordinal;
    }
    *device_index = igpu_index;
    return igpu_ordinal;
}

struct whisper_context * bj_whisper_load(const char * path, bool use_gpu, int gpu_device) {
    struct whisper_context_params params = whisper_context_default_params();
    params.use_gpu = use_gpu;
    params.gpu_device = gpu_device < 0 ? 0 : gpu_device;
    return whisper_init_from_file_with_params(path, params);
}

int bj_whisper_run(struct whisper_context * ctx, const float * pcm, int n_samples,
                   const char * language, const char * prompt, int n_threads,
                   bj_abort_fn abort_fn, void * abort_user) {
    struct whisper_full_params params = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
    params.n_threads = n_threads;
    // Every dictation is independent: never condition on the previous one.
    params.no_context = true;
    params.no_timestamps = true;
    params.print_special = false;
    params.print_progress = false;
    params.print_realtime = false;
    params.print_timestamps = false;
    params.language = language != NULL ? language : "auto";
    params.detect_language = false;
    params.initial_prompt = prompt;
    params.suppress_blank = true;
    // Drops non-speech tokens such as "[Music]" or "(applause)".
    params.suppress_nst = true;
    params.abort_callback = abort_fn;
    params.abort_callback_user_data = abort_user;
    return whisper_full(ctx, params, pcm, n_samples);
}

struct parakeet_context * bj_parakeet_load(const char * path, bool use_gpu, int gpu_device) {
    struct parakeet_context_params params = parakeet_context_default_params();
    params.use_gpu = use_gpu;
    params.gpu_device = gpu_device < 0 ? 0 : gpu_device;
    return parakeet_init_from_file_with_params(path, params);
}

int bj_parakeet_run(struct parakeet_context * ctx, const float * pcm, int n_samples,
                    int n_threads, bj_abort_fn abort_fn, void * abort_user) {
    struct parakeet_full_params params = parakeet_full_default_params(PARAKEET_SAMPLING_GREEDY);
    params.n_threads = n_threads;
    params.no_context = true;
    params.abort_callback = abort_fn;
    params.abort_callback_user_data = abort_user;
    return parakeet_full(ctx, params, pcm, n_samples);
}

struct whisper_vad_context * bj_vad_load(const char * path, int n_threads) {
    struct whisper_vad_context_params params = whisper_vad_default_context_params();
    params.n_threads = n_threads;
    params.use_gpu = false;
    return whisper_vad_init_from_file_with_params(path, params);
}

struct whisper_vad_segments * bj_vad_detect(struct whisper_vad_context * vctx,
                                            const float * pcm, int n_samples,
                                            float threshold, int min_speech_ms) {
    struct whisper_vad_params params = whisper_vad_default_params();
    params.threshold = threshold;
    params.min_speech_duration_ms = min_speech_ms;
    return whisper_vad_segments_from_samples(vctx, params, pcm, n_samples);
}
