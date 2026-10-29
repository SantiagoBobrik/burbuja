//! Safe wrappers over whisper.cpp and parakeet.cpp, through the C layer in `native/engine.c`.
//! Used only by the engine worker process and the benchmark.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};

#[repr(C)]
struct WhisperContext {
    _opaque: [u8; 0],
}
#[repr(C)]
struct ParakeetContext {
    _opaque: [u8; 0],
}
#[repr(C)]
struct VadContext {
    _opaque: [u8; 0],
}
#[repr(C)]
struct VadSegments {
    _opaque: [u8; 0],
}

type AbortFn = unsafe extern "C" fn(user: *mut c_void) -> bool;

unsafe extern "C" {
    fn bj_set_log_level(min_level: c_int);
    fn bj_reserve_stdout() -> isize;
    fn bj_load_backends(cpu_dir: *const c_char, gpu_dir: *const c_char);
    fn bj_device_count() -> c_int;
    fn bj_device_info(
        i: c_int,
        description: *mut c_char,
        description_len: c_int,
        backend: *mut c_char,
        backend_len: c_int,
    ) -> c_int;
    fn bj_pick_gpu(device_index: *mut c_int) -> c_int;

    fn bj_whisper_load(
        path: *const c_char,
        use_gpu: bool,
        gpu_device: c_int,
    ) -> *mut WhisperContext;
    fn bj_whisper_run(
        ctx: *mut WhisperContext,
        pcm: *const f32,
        n_samples: c_int,
        language: *const c_char,
        prompt: *const c_char,
        n_threads: c_int,
        abort_fn: Option<AbortFn>,
        abort_user: *mut c_void,
    ) -> c_int;
    fn whisper_full_n_segments(ctx: *mut WhisperContext) -> c_int;
    fn whisper_full_get_segment_text(ctx: *mut WhisperContext, i: c_int) -> *const c_char;
    fn whisper_full_lang_id(ctx: *mut WhisperContext) -> c_int;
    fn whisper_lang_str(id: c_int) -> *const c_char;
    fn whisper_free(ctx: *mut WhisperContext);

    fn bj_parakeet_load(
        path: *const c_char,
        use_gpu: bool,
        gpu_device: c_int,
    ) -> *mut ParakeetContext;
    fn bj_parakeet_run(
        ctx: *mut ParakeetContext,
        pcm: *const f32,
        n_samples: c_int,
        n_threads: c_int,
        abort_fn: Option<AbortFn>,
        abort_user: *mut c_void,
    ) -> c_int;
    fn parakeet_full_n_segments(ctx: *mut ParakeetContext) -> c_int;
    fn parakeet_full_get_segment_text(ctx: *mut ParakeetContext, i: c_int) -> *const c_char;
    fn parakeet_free(ctx: *mut ParakeetContext);

    fn bj_vad_load(path: *const c_char, n_threads: c_int) -> *mut VadContext;
    fn bj_vad_detect(
        vctx: *mut VadContext,
        pcm: *const f32,
        n_samples: c_int,
        threshold: f32,
        min_speech_ms: c_int,
    ) -> *mut VadSegments;
    fn whisper_vad_segments_n_segments(segments: *mut VadSegments) -> c_int;
    fn whisper_vad_segments_get_segment_t0(segments: *mut VadSegments, i: c_int) -> f32;
    fn whisper_vad_segments_get_segment_t1(segments: *mut VadSegments, i: c_int) -> f32;
    fn whisper_vad_free_segments(segments: *mut VadSegments);
    fn whisper_vad_free(vctx: *mut VadContext);
}

pub const SAMPLE_RATE: u32 = 16_000;
/// The VAD reports times in centiseconds.
const SAMPLES_PER_CS: f32 = SAMPLE_RATE as f32 / 100.0;
/// ggml log levels (ggml.h).
const LOG_LEVEL_WARN: c_int = 3;
/// Folder beside the executable with the GPU backend module (see build.rs).
const GPU_SUBDIR: &str = "gpu";

/// Takes the process's stdout for the worker protocol; everything else printed to stdout from
/// then on (library debug output included) lands on stderr.
pub fn reserve_stdout() -> std::io::Result<std::fs::File> {
    // SAFETY: called once at worker start, before any engine code runs.
    let raw = unsafe { bj_reserve_stdout() };
    if raw == -1 {
        return Err(std::io::Error::other("could not reserve stdout"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::FromRawHandle;
        // SAFETY: a fresh duplicate handle that nothing else owns.
        Ok(unsafe { std::fs::File::from_raw_handle(raw as _) })
    }
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        // SAFETY: a fresh duplicate descriptor that nothing else owns.
        Ok(unsafe { std::fs::File::from_raw_fd(raw as _) })
    }
}

/// Loads the compute backends once per process. CPU-only skips the GPU module entirely, so a
/// broken GPU driver can't crash a CPU worker. Also quiets ggml's info logs.
pub fn init(cpu_only: bool) {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let cpu_dir = path_cstring(&dir);
    let gpu_dir = (!cpu_only).then(|| path_cstring(&dir.join(GPU_SUBDIR)));
    // SAFETY: valid NUL-terminated strings that outlive the calls.
    unsafe {
        bj_set_log_level(LOG_LEVEL_WARN);
        bj_load_backends(
            cpu_dir.as_ptr(),
            gpu_dir.as_ref().map_or(ptr::null(), |d| d.as_ptr()),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Cpu,
    Gpu,
    IntegratedGpu,
    Accelerator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub description: String,
    pub backend: String,
    pub kind: DeviceKind,
}

/// The compute devices the loaded backends registered.
pub fn devices() -> Vec<Device> {
    // SAFETY: plain query.
    let count = unsafe { bj_device_count() };
    (0..count).filter_map(device).collect()
}

fn device(i: c_int) -> Option<Device> {
    let mut description = [0 as c_char; 256];
    let mut backend = [0 as c_char; 64];
    // SAFETY: the buffers are writable for the lengths passed; the C side NUL-terminates.
    let kind = unsafe {
        bj_device_info(
            i,
            description.as_mut_ptr(),
            description.len() as c_int,
            backend.as_mut_ptr(),
            backend.len() as c_int,
        )
    };
    let kind = match kind {
        0 => DeviceKind::Cpu,
        1 => DeviceKind::Gpu,
        2 => DeviceKind::IntegratedGpu,
        3 => DeviceKind::Accelerator,
        _ => return None,
    };
    // SAFETY: NUL-terminated by the C side. CPU names come padded with spaces.
    let text = |buf: &[c_char]| {
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .trim()
            .to_string()
    };
    Some(Device {
        description: text(&description),
        backend: text(&backend),
        kind,
    })
}

/// The GPU a model loads on: the first discrete GPU, else the first integrated one.
struct GpuChoice {
    ordinal: c_int,
    device: Device,
}

fn pick_gpu() -> Option<GpuChoice> {
    let mut index: c_int = -1;
    // SAFETY: `index` is a valid out pointer.
    let ordinal = unsafe { bj_pick_gpu(&mut index) };
    if ordinal < 0 {
        return None;
    }
    device(index).map(|device| GpuChoice { ordinal, device })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Whisper,
    Parakeet,
}

impl Arch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Whisper => "whisper",
            Self::Parakeet => "parakeet",
        }
    }

    /// Only Whisper takes a prompt, which is how the vocabulary reaches the model.
    pub fn supports_vocabulary(self) -> bool {
        self == Self::Whisper
    }

    /// Silence for the warm-up run, long enough to compile the GPU kernels a real dictation
    /// uses. Whisper pads every window to 30 s, so 1 s already runs the same kernels; Parakeet's
    /// kernels depend on the input length, so it gets a dictation-sized clip.
    pub fn warm_up_audio(self) -> Vec<f32> {
        let seconds = match self {
            Self::Whisper => 1,
            Self::Parakeet => 8,
        };
        vec![0.0; seconds * SAMPLE_RATE as usize]
    }
}

enum Context {
    Whisper(NonNull<WhisperContext>),
    Parakeet(NonNull<ParakeetContext>),
}

/// A loaded speech model, resident until dropped.
pub struct Model {
    ctx: Context,
    arch: Arch,
    device: Option<Device>,
    threads: c_int,
}

// SAFETY: the contexts are only used through `&mut self`, one call at a time; whisper.cpp and
// parakeet.cpp allow a context to move between threads.
unsafe impl Send for Model {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("aborted")]
    Aborted,
    #[error("{0}")]
    Failed(String),
}

#[derive(Debug, Default, Clone)]
pub struct RunOptions {
    /// ISO 639-1 code; `None` detects the language (Whisper) or ignores it (Parakeet).
    pub language: Option<String>,
    /// Initial prompt with names and jargon to bias Whisper toward.
    pub prompt: Option<String>,
}

impl Model {
    /// Loads a model, on the GPU when `use_gpu` and one is available.
    pub fn load(path: &Path, arch: Arch, use_gpu: bool) -> Result<Self, String> {
        let gpu = use_gpu.then(pick_gpu).flatten();
        let ordinal = gpu.as_ref().map_or(0, |g| g.ordinal);
        let c_path = path_cstring(path);
        // SAFETY: valid path string; the returned pointer is owned by `Model` and freed in Drop.
        let ctx = unsafe {
            match arch {
                Arch::Whisper => {
                    NonNull::new(bj_whisper_load(c_path.as_ptr(), gpu.is_some(), ordinal))
                        .map(Context::Whisper)
                }
                Arch::Parakeet => {
                    NonNull::new(bj_parakeet_load(c_path.as_ptr(), gpu.is_some(), ordinal))
                        .map(Context::Parakeet)
                }
            }
        }
        .ok_or_else(|| format!("could not load {}", file_name(path)))?;
        Ok(Self {
            ctx,
            arch,
            device: gpu.map(|g| g.device),
            threads: inference_threads(),
        })
    }

    pub fn arch(&self) -> Arch {
        self.arch
    }

    /// The GPU the model runs on, or `None` on the CPU.
    pub fn gpu(&self) -> Option<&Device> {
        self.device.as_ref()
    }

    /// Transcribes 16 kHz mono PCM. Setting `abort` stops it at the next compute step.
    pub fn run(
        &mut self,
        pcm: &[f32],
        options: &RunOptions,
        abort: &AtomicBool,
    ) -> Result<Transcript, RunError> {
        let n =
            c_int::try_from(pcm.len()).map_err(|_| RunError::Failed("audio too long".into()))?;
        let user = ptr::from_ref(abort).cast_mut().cast::<c_void>();
        match self.ctx {
            Context::Whisper(ctx) => {
                let language = options.language.as_deref().map(c_string);
                let prompt = options
                    .prompt
                    .as_deref()
                    .filter(|p| !p.is_empty())
                    .map(c_string);
                // SAFETY: `ctx` is live; the strings and `abort` outlive the call, and
                // `should_abort` only reads the AtomicBool behind `user`.
                let code = unsafe {
                    bj_whisper_run(
                        ctx.as_ptr(),
                        pcm.as_ptr(),
                        n,
                        language.as_ref().map_or(ptr::null(), |l| l.as_ptr()),
                        prompt.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
                        self.threads,
                        Some(should_abort),
                        user,
                    )
                };
                check(code, abort, "whisper")?;
                // SAFETY: `ctx` is live and holds the result of the run above.
                unsafe {
                    let n = whisper_full_n_segments(ctx.as_ptr());
                    let text = join_segments(n, |i| whisper_full_get_segment_text(ctx.as_ptr(), i));
                    let language = c_str(whisper_lang_str(whisper_full_lang_id(ctx.as_ptr())));
                    Ok(Transcript { text, language })
                }
            }
            Context::Parakeet(ctx) => {
                // SAFETY: as above.
                let code = unsafe {
                    bj_parakeet_run(
                        ctx.as_ptr(),
                        pcm.as_ptr(),
                        n,
                        self.threads,
                        Some(should_abort),
                        user,
                    )
                };
                check(code, abort, "parakeet")?;
                // SAFETY: `ctx` is live and holds the result of the run above.
                let text = unsafe {
                    let n = parakeet_full_n_segments(ctx.as_ptr());
                    join_segments(n, |i| parakeet_full_get_segment_text(ctx.as_ptr(), i))
                };
                Ok(Transcript {
                    text,
                    language: None,
                })
            }
        }
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        // SAFETY: each context was returned by its load function and is freed exactly once.
        unsafe {
            match self.ctx {
                Context::Whisper(ctx) => whisper_free(ctx.as_ptr()),
                Context::Parakeet(ctx) => parakeet_free(ctx.as_ptr()),
            }
        }
    }
}

/// Silero voice activity detection (CPU).
pub struct Vad(NonNull<VadContext>);

// SAFETY: used through `&mut self` only, one call at a time.
unsafe impl Send for Vad {}

impl Vad {
    pub fn load(path: &Path) -> Result<Self, String> {
        let c_path = path_cstring(path);
        // SAFETY: valid path string; the context is owned by `Vad` and freed in Drop.
        NonNull::new(unsafe { bj_vad_load(c_path.as_ptr(), 2) })
            .map(Self)
            .ok_or_else(|| format!("could not load {}", file_name(path)))
    }

    /// Speech segments as `(start_sample, end_sample)`, in order.
    pub fn segments(
        &mut self,
        pcm: &[f32],
        threshold: f32,
        min_speech_ms: u32,
    ) -> Result<Vec<(i64, i64)>, String> {
        let n = c_int::try_from(pcm.len()).map_err(|_| "audio too long".to_string())?;
        // SAFETY: the context is live and `pcm` is valid for `n` samples; the segments are
        // freed below, after the last read.
        unsafe {
            let segments = bj_vad_detect(
                self.0.as_ptr(),
                pcm.as_ptr(),
                n,
                threshold,
                min_speech_ms as c_int,
            );
            if segments.is_null() {
                return Err("speech detection failed".into());
            }
            let count = whisper_vad_segments_n_segments(segments);
            let to_sample = |cs: f32| (cs * SAMPLES_PER_CS).round() as i64;
            let out = (0..count)
                .map(|i| {
                    (
                        to_sample(whisper_vad_segments_get_segment_t0(segments, i)),
                        to_sample(whisper_vad_segments_get_segment_t1(segments, i)),
                    )
                })
                .collect();
            whisper_vad_free_segments(segments);
            Ok(out)
        }
    }
}

impl Drop for Vad {
    fn drop(&mut self) {
        // SAFETY: returned by `bj_vad_load`, freed exactly once.
        unsafe { whisper_vad_free(self.0.as_ptr()) }
    }
}

unsafe extern "C" fn should_abort(user: *mut c_void) -> bool {
    // SAFETY: `user` is the `&AtomicBool` passed to `Model::run`, alive for the whole call.
    unsafe { &*user.cast::<AtomicBool>() }.load(Ordering::Relaxed)
}

fn check(code: c_int, abort: &AtomicBool, what: &str) -> Result<(), RunError> {
    match code {
        0 => Ok(()),
        _ if abort.load(Ordering::Relaxed) => Err(RunError::Aborted),
        code => Err(RunError::Failed(format!("{what} failed ({code})"))),
    }
}

/// Concatenates segment texts (each carries its own leading space) and trims the ends.
///
/// # Safety
/// `segment` must return NUL-terminated strings (or null) for `0..n`.
unsafe fn join_segments(n: c_int, segment: impl Fn(c_int) -> *const c_char) -> String {
    let mut text = String::new();
    for i in 0..n {
        // SAFETY: guaranteed by the caller.
        if let Some(part) = unsafe { c_str(segment(i)) } {
            text.push_str(&part);
        }
    }
    text.trim().to_string()
}

/// # Safety
/// `s` must be null or a NUL-terminated string.
unsafe fn c_str(s: *const c_char) -> Option<String> {
    // SAFETY: guaranteed by the caller.
    (!s.is_null()).then(|| unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
}

/// Paths reach the C side as UTF-8; whisper.cpp converts them to UTF-16 on Windows.
fn path_cstring(path: &Path) -> CString {
    c_string(&path.to_string_lossy())
}

fn c_string(s: &str) -> CString {
    CString::new(s.replace('\0', "")).expect("no NUL bytes left")
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Leaves cores for the app and the rest of the system; past 8 threads decoding barely speeds up.
fn inference_threads() -> c_int {
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    cores.saturating_sub(2).clamp(1, 8) as c_int
}
