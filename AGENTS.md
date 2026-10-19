# AGENTS.md

Ratatui terminal audio visualizer (Rust, edition 2024). Captures system output audio with cpal, computes a spectrum with realfft, and renders mirrored bars. There is no README, CI, workspace, or task runner config — Cargo is the source of truth.

## Commands

- `cargo run` — the TUI writes to **stderr** (`CrosstermBackend::new(std::io::stderr())`), so run it in a real terminal and don't pipe stdout to inspect it. It still launches when audio capture is unavailable, just without bars.
- `cargo test --lib` — preferred test command. Inline tests in `src/app.rs` and `src/audio_processor.rs` are reachable from both `main.rs` and `lib.rs`; plain `cargo test` runs them twice (lib + bin harness).
- `cargo test --lib <filter>` — run one test, e.g. `cargo test --lib app::tests::test_app_gain_adjustment`.
- `cargo test --lib -- --skip test_sound_device_filtered_list` — headless-safe run. That test asserts at least one real output device exists and fails on machines without audio hardware.
- Linux builds need ALSA headers and pkg-config (`libasound2-dev`, `pkg-config`) for `alsa-sys`.
- Full check: `cargo fmt --check && cargo clippy --all-targets && cargo test --lib -- --skip test_sound_device_filtered_list`.

## Architecture & Code Conventions

- **Language:** All source code comments, docstrings, and error messages must be kept in **English**.
- `src/main.rs` is the entrypoint and redeclares every module itself; `src/lib.rs` repeats that list so tests compile without `main`. When adding a module, register it in **both** files.
- `main.rs` owns the event loop and the `SoundDevice`; `App` only holds pure UI state and device name strings. `update()` mutates `App` and returns `AppAction::ChangeDevice`, and `main` performs the hardware switch afterward. Keep audio hardware side effects out of `App` and `update`.
- `event::EventHandler` runs a crossterm polling thread and emits `Event::Tick` every ~16 ms (60 FPS); the cpal callback sends `Event::AudioBars` through the same mpsc channel.
- `sound_device.rs` captures the output **sink monitor**, not a microphone: `init_capture_env()` sets `PIPEWIRE_ALSA` / `PULSE_SOURCE` only if unset. Runtime needs PipeWire or PulseAudio; don't overwrite those variables when the user already set them.
- `sound_device.rs` filters out technical duplicate drivers (raw `hw:`), dummy sinks ("Discard all samples"), and unconfigured hardware ports, exposing only clean physical outputs and the auto-following default output.
- `App::tick()` decays targets and applies attack/decay smoothing; `App::displayed_bars()` applies gain. Use these for rendering instead of mutating raw bar values for display-only effects.

## DSP & Audio Pipeline (`src/audio_processor.rs`)

- Interleaved audio is downmixed to mono `(L + R) / 2` and buffered into chunks of 1024 samples.
- Samples are multiplied by a Hann window before running forward FFT via `realfft`.
- Output magnitudes are grouped into 32 log-spaced frequency bins (40 Hz to 16,000 Hz).
- Bins combine average and peak magnitude (`0.5 * avg + 0.5 * max`) to preserve rapid transients (cymbals, snares) across wide treble ranges.
- Frequency compensation curve (treble boost) scales linearly from `1.0x` on the lowest sub-bass bar up to `~6.0x` on the highest treble bar to counterbalance natural 1/f energy roll-off without reducing bass response.

## UI & Rendering (`src/ui.rs`)

- The visualizer occupies 100% of the available vertical height, centered dynamically on the screen without enclosing block borders.
- Symmetrical layout:
  - Upper half: normal bars growing upward using base-anchored Unicode blocks (` ▂▃▄▅▆▇█`) with cyan-green-magenta gradient.
  - Center line: subtle horizontal divider (`──`).
  - Lower half: upside-down mirror effect growing downward using top-anchored blocks (` ▔▀█`), with attenuated amplitude and deeper tones.
- Device selection popup renders a rounded bordered box using `Clear` over the visualizer, highlighting the selected cursor in cyan and tagging the active device with `[Active]`.

## Keybindings (`src/update.rs`)

- Popup closed: `q`/Esc quit, `s` open device popup, Left/`j` counter--, Right/`l` counter++, Up/`i` gain+, Down/`k` gain-. `j`/`k` are not vim-style navigation here.
- Popup open: Up/`k` and Down/`j` move selection, Enter activates the highlighted device, Esc/`s` closes. Ctrl+C quits in both modes.
- The popup branch returns early and swallows every other key; add new popup keys there explicitly.
