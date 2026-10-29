//! Regenerates `src/lib/ipc.ts` without launching the app:
//! `cargo run --bin export-bindings`.
//!
//! A binary rather than a test: on Windows, test executables don't get Tauri's Common Controls
//! v6 manifest and crash with STATUS_ENTRYPOINT_NOT_FOUND once they link the webview runtime.

fn main() {
    burbuja_lib::export_bindings(&burbuja_lib::specta_builder());
    println!("wrote src/lib/ipc.ts");
}
