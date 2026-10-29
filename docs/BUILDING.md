# Building Burbuja

Burbuja is a [Tauri 2](https://tauri.app) app: a Rust core in `src-tauri/` and a React + TypeScript interface in `src/`. Windows 10/11 (x64) is the supported platform today; macOS (Apple Silicon) is in progress.

## Requirements (Windows)

| Tool | Version | Install |
|---|---|---|
| Rust (MSVC toolchain) | stable | `winget install Rustlang.Rustup`, then `rustup default stable-x86_64-pc-windows-msvc` |
| Visual Studio 2022 Build Tools | C++ workload, with CMake | `winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` |
| Node.js | 22.20 or newer | `winget install OpenJS.NodeJS.LTS` |
| pnpm | 10+ | `corepack enable` |
| Vulkan SDK | 1.4+ | `winget install KhronosGroup.VulkanSDK` |
| WebView2 | any | ships with Windows 11 |

Enable long paths once: `git config --global core.longpaths true`.

The speech engine is [whisper.cpp](https://github.com/ggml-org/whisper.cpp), vendored as a git submodule in `vendor/whisper.cpp` and pinned to a release tag. Clone with `git clone --recursive`, or run `git submodule update --init` in an existing checkout. Compiling it needs the Vulkan SDK, CMake and Ninja (both ship with the Build Tools C++ workload). If `cmake` or `ninja` isn't on your `PATH`, run the build from a "Developer PowerShell for VS 2022".

## Run

```sh
pnpm install
pnpm tauri dev
```

The first build compiles the native speech engine and takes a few minutes; later builds take seconds and the interface reloads on save. On first launch, onboarding downloads a speech model.

## Package

```sh
pnpm tauri build
```

The installers land in `src-tauri/target/release/bundle/` (`nsis/*-setup.exe` and `msi/*.msi`).

### Releases

`.github/workflows/release.yml` creates a **draft** GitHub release, builds the Windows x64 setup (`.exe`) and the macOS Apple Silicon `.dmg` with [tauri-action](https://github.com/tauri-apps/tauri-action), uploads both to that draft and adds `SHA256SUMS.txt`, computed from the uploaded files. The draft's description comes from `.github/release-notes.md`.

To cut a release:

1. Bump `version` in `src-tauri/tauri.conf.json` and commit it. The workflow refuses a version that is already published.
2. Push a matching tag (`git tag v0.2.0 && git push origin v0.2.0`), or run the workflow by hand from the Actions tab. Running it again for the same version reuses the draft and replaces its files.
3. Open the draft, write the "What's new" section and publish it.

The installers aren't code-signed (the macOS app is ad-hoc signed). [INSTALL.md](INSTALL.md) is the user guide for the warnings this causes; keep it in step with the release notes.

## Checks

Run these before opening a pull request:

| Command | Where | What |
|---|---|---|
| `pnpm lint` | root | ESLint + Prettier |
| `pnpm typecheck` | root | TypeScript |
| `pnpm test` | root | Interface tests (Vitest) |
| `cargo clippy --all-targets -- -D warnings` | `src-tauri/` | Rust lints |
| `cargo fmt --check` | `src-tauri/` | Rust formatting |
| `cargo test --lib` | `src-tauri/` | Rust tests |
| `cargo deny check` | `src-tauri/` | Dependency licenses and advisories |

Use `cargo test --lib`: on Windows, integration test binaries can't load the webview runtime.

## Useful tools

- **Typed IPC:** `src/lib/ipc.ts` is generated from the Rust commands and events. Regenerate it with `cargo run --bin export-bindings` in `src-tauri/` (debug runs of the app also do it). Don't edit it by hand.
- **Benchmark:** `cargo run --bin bench -- [--cpu] [--model file.bin] ../tests/fixtures/audio/*.wav` in `src-tauri/` measures transcription speed on the bundled sample clips.
- **Interface without the backend:** `pnpm dev` serves the interface with sample data at `http://localhost:1420`. Add `?page=history`, `?page=settings&section=cleanup` or `?onboarding=1&step=2` to open a specific screen.

## Project layout

```
src/                 React interface (hub window, bubble overlay, i18n)
src-tauri/src/
  core/              Pure logic: gestures, hotkey parsing, text cleanup rules (no Tauri)
  dictation/         The controller (single owner of dictation state) and the pipeline
  engine/            Speech engine host and worker process, model catalog and downloads
  audio/ hotkeys/    Microphone capture, global shortcuts
  insert/            Text insertion into the focused app
  cleanup/           Optional AI cleanup through a local Ollama / LM Studio server
  overlay/ tray.rs   Bubble window and system tray
tests/fixtures/      Sample audio for tests and the benchmark
```

Speech recognition runs in a separate worker process (`burbuja --engine-worker`), so a crash in a GPU driver or a model never takes the app down.
