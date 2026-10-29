# Third-party notices

Burbuja uses the following third-party software and assets. Rust crates and npm packages are
pinned in `src-tauri/Cargo.lock` and `pnpm-lock.yaml`, and their licenses are checked with
`cargo deny check`.

| Component | License | Use |
|---|---|---|
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (includes ggml) | MIT | Speech recognition engine (Whisper, Parakeet, VAD), vendored in `vendor/whisper.cpp` |
| [cpal](https://github.com/RustAudio/cpal) | Apache-2.0 | Microphone capture |
| Silero VAD v6.2.0 ([ggml-org/whisper-vad](https://huggingface.co/ggml-org/whisper-vad)) | MIT | Voice activity detection, bundled in `src-tauri/resources/models/` (license next to it) |
| Whisper large-v3-turbo, Whisper base.en (OpenAI, [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp)) | MIT | Speech models, downloaded on demand |
| Parakeet TDT 0.6B v3 (NVIDIA, [ggml-org/parakeet-GGUF](https://huggingface.co/ggml-org/parakeet-GGUF)) | CC-BY-4.0 | Speech model, downloaded on demand |
| DM Sans | SIL OFL 1.1 | UI font, `src/styles/fonts/DMSans-OFL.txt` |
| Geist Mono | SIL OFL 1.1 | Monospace font, `src/styles/fonts/OFL.txt` |

## whisper.cpp

The speech engine is built from [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (including
ggml), distributed under the following license:

```
MIT License

Copyright (c) 2023-2026 The ggml authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
