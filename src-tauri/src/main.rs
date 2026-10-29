// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Third-party Vulkan implicit layers (overlays, capture tools) can crash the engine.
    // Disable them unless the user explicitly set the variable.
    #[cfg(windows)]
    if std::env::var_os("VK_LOADER_LAYERS_DISABLE").is_none() {
        // SAFETY: single-threaded at this point; nothing else reads the environment yet.
        unsafe { std::env::set_var("VK_LOADER_LAYERS_DISABLE", "~implicit~") };
    }

    // The engine worker never touches Tauri.
    if burbuja_lib::engine::worker::is_worker_process() {
        std::process::exit(burbuja_lib::engine::worker::run());
    }
    burbuja_lib::run()
}
