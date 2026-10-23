# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

The project's agent instructions live in `AGENTS.md` and are the single source of truth for commands, architecture, DSP pipeline, UI layout, and keybindings. Keep them there rather than duplicating them here.

@AGENTS.md

## Notes for Claude Code

- Inline `#[cfg(test)]` modules exist in `app.rs`, `audio_processor.rs`, `event.rs`, `sound_device.rs`, `update.rs`, and `lib.rs`. Tests in `lib.rs` only run under the lib harness; the others are compiled into both lib and bin, which is why `cargo test --lib` is preferred.
- `cargo run` needs a real interactive terminal, so it can't be meaningfully verified from a non-TTY tool shell. Rely on `cargo test --lib` and `cargo clippy --all-targets` for verification.
- When behavior described in `AGENTS.md` changes (device filtering, key bindings, DSP constants), update `AGENTS.md` in the same change.
