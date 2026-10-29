/// App commands are listed here so each window's capability must grant them explicitly
/// (least privilege per window). Keep in sync with `specta_builder()` in `lib.rs`.
const COMMANDS: &[&str] = &[
    "get_settings",
    "update_settings",
    "list_microphones",
    "start_mic_preview",
    "stop_mic_preview",
    "play_sound",
    "start_hands_free",
    "cancel_dictation",
    "list_models",
    "download_model",
    "cancel_download",
    "delete_model",
    "set_active_model",
    "get_engine_status",
    "test_provider",
    "detect_local_llms",
    "overlay_presence",
    "set_pill_bounds",
    "overlay_menu",
    "open_hub",
    "get_history",
    "delete_history_item",
    "clear_history",
    "copy_history_item",
    "retry_cleanup",
    "get_stats",
    "record_hotkey",
    "cancel_record_hotkey",
    "get_autostart",
    "set_autostart",
    "open_logs_folder",
    "open_permission_settings",
    "get_permissions",
    "request_permission",
    "restart_app",
    "reset_permission",
    "reset_settings",
    "get_last_timings",
];

use std::path::{Path, PathBuf};

fn main() {
    build_engine();
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build");
}

/// The GPU backend module goes in this subfolder beside the executable, so a CPU-only worker
/// can skip it entirely (see `engine::native::load_backends`).
const GPU_SUBDIR: &str = "gpu";

/// Builds whisper.cpp (the `vendor/whisper.cpp` submodule, pinned to a release tag) and the C
/// layer in `native/`, then links them.
///
/// - Windows x64: shared libraries with runtime-loaded backends (Vulkan + every CPU variant),
///   so the app starts on any PC, with or without a working GPU driver.
/// - macOS arm64: static, Metal with the shader library embedded.
/// - Elsewhere: static, CPU only.
///
/// The native code is always built optimized, so dev builds measure real engine speed.
fn build_engine() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let whisper = manifest.join("../vendor/whisper.cpp");
    assert!(
        whisper.join("CMakeLists.txt").exists(),
        "vendor/whisper.cpp is missing: run `git submodule update --init`"
    );
    println!("cargo:rerun-if-changed=native");
    println!("cargo:rerun-if-changed=../.git/modules/vendor/whisper.cpp/HEAD");

    #[cfg(windows)]
    add_visual_studio_cmake_to_path();

    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let dynamic = os == "windows" && arch == "x86_64";

    let mut cmake = cmake::Config::new(manifest.join("native"));
    cmake
        .profile("Release")
        // Keep CMake's own Release flags (/O2, NDEBUG, /EHsc); the crate's would replace them.
        .no_default_flags(true)
        .build_target("burbuja_engine")
        .define("WHISPER_CPP_DIR", &whisper)
        .define("WHISPER_BUILD_TESTS", "OFF")
        .define("WHISPER_BUILD_EXAMPLES", "OFF")
        .define("WHISPER_BUILD_SERVER", "OFF")
        .define("WHISPER_ALL_WARNINGS", "OFF")
        // ggml's own thread pool; OpenMP would add a runtime DLL dependency (vcomp140).
        .define("GGML_OPENMP", "OFF")
        .define("GGML_NATIVE", "OFF");
    if os == "windows" {
        // MSBuild trips over the >260-character paths of ggml's nested Vulkan shader tool.
        cmake.generator("Ninja");
    }
    if dynamic {
        cmake
            .define("BUILD_SHARED_LIBS", "ON")
            .define("GGML_BACKEND_DL", "ON")
            .define("GGML_CPU_ALL_VARIANTS", "ON")
            .define("GGML_VULKAN", "ON");
    } else {
        cmake.define("BUILD_SHARED_LIBS", "OFF");
        if os == "macos" && arch == "aarch64" {
            cmake
                .define("GGML_METAL", "ON")
                .define("GGML_METAL_EMBED_LIBRARY", "ON")
                .define("GGML_BLAS", "OFF");
        } else {
            cmake.define("GGML_METAL", "OFF");
        }
    }
    // A short build folder shared by every profile: the native code is always Release, so dev,
    // release and clippy builds reuse one compile, and ggml's nested Vulkan shader tool stays
    // under Windows' 260-character path limit. The lock serializes concurrent cargo builds.
    let target_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap())
        .ancestors()
        .nth(4)
        .map(PathBuf::from)
        .expect("target dir");
    let native_dir = target_dir.join("wcpp");
    std::fs::create_dir_all(&native_dir).expect("create native build dir");
    let lock = std::fs::File::create(target_dir.join("wcpp.lock")).expect("create build lock");
    lock.lock().expect("lock native build");
    let out = cmake.out_dir(&native_dir).build().join("build/out");
    drop(lock);

    println!(
        "cargo:rustc-link-search=native={}",
        out.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=burbuja_engine");
    if dynamic {
        for lib in ["whisper", "parakeet", "ggml", "ggml-base"] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
        stage_engine_dlls(&manifest, &out.join("bin"));
    } else {
        for lib in ["whisper", "parakeet", "ggml", "ggml-base", "ggml-cpu"] {
            println!("cargo:rustc-link-lib=static={lib}");
        }
        if os == "macos" {
            if arch == "aarch64" {
                println!("cargo:rustc-link-lib=static=ggml-metal");
                for framework in ["Foundation", "Metal", "MetalKit"] {
                    println!("cargo:rustc-link-lib=framework={framework}");
                }
            }
            println!("cargo:rustc-link-lib=framework=Accelerate");
            println!("cargo:rustc-link-lib=c++");
            link_clang_runtime();
        } else {
            println!("cargo:rustc-link-lib=stdc++");
        }
    }
}

/// ggml's Objective-C code uses `@available`, which compiles to `__isPlatformVersionAtLeast`
/// from clang's runtime. rustc links with `-nodefaultlibs`, so link that runtime explicitly.
fn link_clang_runtime() {
    let out = std::process::Command::new("clang")
        .arg("-print-resource-dir")
        .output()
        .expect("run clang -print-resource-dir");
    assert!(
        out.status.success(),
        "clang -print-resource-dir failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let resource_dir = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    println!(
        "cargo:rustc-link-search=native={}",
        resource_dir.join("lib/darwin").display()
    );
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}

/// Copies the engine DLLs into `engine-libs/` (bundled beside `burbuja.exe` by
/// `tauri.windows.conf.json`) and beside the dev and test executables. The GPU module goes in
/// `gpu/`; everything else sits next to the executable, where Windows resolves imports.
fn stage_engine_dlls(manifest: &Path, built: &Path) {
    let bundle = manifest.join("engine-libs");
    let _ = std::fs::remove_dir_all(&bundle);
    // target/<profile>, three levels above OUT_DIR.
    let profile = PathBuf::from(std::env::var("OUT_DIR").unwrap())
        .ancestors()
        .nth(3)
        .map(PathBuf::from)
        .expect("profile dir");
    let mut staged = 0;
    for entry in std::fs::read_dir(built)
        .expect("read engine output")
        .flatten()
    {
        let src = entry.path();
        let Some(name) = src.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if !name.ends_with(".dll") {
            continue;
        }
        let gpu = name.starts_with("ggml-vulkan");
        let mut dests = vec![bundle.clone(), profile.clone()];
        if !gpu {
            // Test binaries run from target/<profile>/deps and import the core DLLs.
            dests.push(profile.join("deps"));
        }
        for dir in dests {
            let dir = if gpu { dir.join(GPU_SUBDIR) } else { dir };
            std::fs::create_dir_all(&dir).expect("create DLL folder");
            if let Err(e) = copy_over_locked(&src, &dir.join(&name)) {
                println!(
                    "cargo:warning=could not stage {name} in {}: {e}",
                    dir.display()
                );
            }
        }
        staged += 1;
    }
    assert!(staged > 0, "no engine DLLs found in {}", built.display());
}

/// The Visual Studio C++ workload ships CMake and Ninja but doesn't put them on `PATH` outside a
/// Developer PowerShell. When `cmake` isn't on `PATH`, find them through `vswhere` (installed
/// with every Visual Studio) and prepend their folders, so a plain terminal builds too.
#[cfg(windows)]
fn add_visual_studio_cmake_to_path() {
    use std::process::Command;

    let on_path = |exe: &str| Command::new(exe).arg("--version").output().is_ok();
    if on_path("cmake") && on_path("ninja") {
        return;
    }
    let Some(program_files) = std::env::var_os("ProgramFiles(x86)") else {
        return;
    };
    let vswhere =
        PathBuf::from(program_files).join("Microsoft Visual Studio/Installer/vswhere.exe");
    let Ok(out) = Command::new(&vswhere)
        .args(["-latest", "-products", "*", "-property", "installationPath"])
        .output()
    else {
        return;
    };
    let install = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let tools = install.join("Common7/IDE/CommonExtensions/Microsoft/CMake");
    let dirs = [tools.join("CMake/bin"), tools.join("Ninja")];
    if !dirs.iter().all(|d| d.is_dir()) {
        return;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let joined = std::env::join_paths(dirs.iter().cloned().chain(std::env::split_paths(&path)))
        .expect("join PATH");
    // SAFETY: the build script is single-threaded here; child processes inherit the new PATH.
    unsafe { std::env::set_var("PATH", joined) };
}

/// Copies `src` over `dest`. A running app keeps its DLLs locked against writes, but Windows
/// still lets them be renamed, so a locked `dest` is moved aside to `<name>.stale` first.
fn copy_over_locked(src: &Path, dest: &Path) -> std::io::Result<()> {
    if std::fs::copy(src, dest).is_ok() {
        return Ok(());
    }
    let mut stale = dest.as_os_str().to_owned();
    stale.push(".stale");
    let _ = std::fs::remove_file(&stale);
    std::fs::rename(dest, &stale)?;
    std::fs::copy(src, dest).map(drop)
}
