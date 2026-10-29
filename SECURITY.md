# Security policy

Burbuja captures your microphone, listens for a global shortcut and types into other apps, so we take security reports seriously.

## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's private vulnerability reporting](https://github.com/SantiagoBobrik/burbuja/security/advisories/new) instead of opening a public issue. Include the affected version, steps to reproduce and the impact you expect.

You'll get an acknowledgment within a few days. Once a fix is ready we'll publish an advisory and credit you, unless you prefer to stay anonymous.

## Supported versions

Burbuja is in early development; only the latest commit on `main` receives security fixes.

## Security model

- **No network at rest.** The app makes requests only when you download a speech model (HTTPS, pinned version, SHA-256 verified) and, if you enable AI cleanup, to a server on your own machine. Cleanup addresses must be loopback (`localhost`, `127.0.0.1`, `[::1]`); the client follows no redirects and uses no proxy.
- **Isolated speech engine.** Transcription runs in a separate worker process, so a crash in a GPU driver or a malformed model can't take down the app.
- **Least-privilege windows.** Each webview only gets the commands it uses, under a strict Content Security Policy with no remote resources.
- **Local data.** History is stored in a local SQLite database with secure delete; dictated text is never written to logs. Text Burbuja places on the clipboard is excluded from cloud clipboard sync.
