# Contributing to Burbuja

Thanks for helping. This guide covers how to report problems and how to send changes.

## Reporting a bug

Open an issue with:

- What you did, what you expected and what happened instead.
- Your Windows version, GPU and the speech model you use (Settings → Transcription).
- The relevant lines from the log (Settings → Advanced → Open logs folder). Logs never contain what you dictated, but check before you paste.

Report security problems privately instead; see [SECURITY.md](SECURITY.md).

## Sending a change

1. Open an issue first for anything bigger than a small fix, so we can agree on the approach.
2. Set up the project with [docs/BUILDING.md](docs/BUILDING.md).
3. Keep the pull request focused on one change, with tests for new logic.
4. Make sure every check in [docs/BUILDING.md](docs/BUILDING.md#checks) passes.
5. Describe what changed and how you tested it, with a screenshot for interface changes.

## Guidelines

- **Local only.** Burbuja never sends audio or text off the computer. Changes that add network calls, telemetry or cloud services won't be accepted. The only allowed requests are the user-initiated model download and the AI cleanup server on `localhost`.
- **Privacy in logs.** Never log transcripts, audio or anything the user typed.
- **Pure logic in `src-tauri/src/core/`**, with unit tests and no Tauri dependency.
- **One owner for dictation state:** the controller in `src-tauri/src/dictation/`. Other modules send it inputs.
- **Typed IPC.** New commands go in `src-tauri/src/commands.rs` with their input validated; regenerate `src/lib/ipc.ts` instead of editing it.
- **Interface.** Use the components in `src/components/ui/kit.tsx` and the color tokens in `src/styles/tokens.css`; no hard-coded colors. Every string goes in both `src/i18n/en.json` and `src/i18n/es.json`.
- **The bubble never takes focus.** Test overlay changes with a text editor focused: the text must still land in it.
- **New dependencies** need a reason in the pull request, and their license must pass `cargo deny check`.

## AI-assisted contributions

Burbuja itself is built with AI coding agents, so AI-assisted pull requests are welcome. You're responsible for what you submit: read every line, run the checks, and be ready to explain why the change is right.

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org): `feat: …`, `fix: …`, `docs: …`, `refactor: …`, `test: …`, `chore: …`. Keep the subject under 72 characters and explain the why in the body.

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
