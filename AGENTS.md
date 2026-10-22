# AGENTS.md

Ratatui terminal audio visualizer (Rust, edition 2024). Captures system output audio with cpal, computes a spectrum with realfft, and renders mirrored bars. There is no README, CI, workspace, or task runner config — Cargo is the source of truth.

## Commands

- `cargo run` — the TUI writes to **stderr** (`CrosstermBackend::new(std::io::stderr())`), so run it in a real terminal and don't pipe stdout to inspect it. It still launches when audio capture is unavailable, just without bars.
- `cargo test --lib` — preferred test command. Inline tests in `src/app.rs` and `src/audio_processor.rs` are reachable from both `main.rs` and `lib.rs`; plain `cargo test` runs them twice (lib + bin harness).
- `cargo test --lib <filter>` — run one test, e.g. `cargo test --lib app::tests::test_app_gain_adjustment`.
- `cargo test --lib -- --skip test_sound_device_filtered_list` — headless-safe run. That test asserts at least one real output device exists and fails on machines without audio hardware.
- Linux builds need ALSA headers and pkg-config (`libasound2-dev`, `pkg-config`) for `alsa-sys`.
- Windows builds use the default cpal WASAPI backend and need no extra system packages beyond the Rust MSVC toolchain.
- Full check: `cargo fmt --check && cargo clippy --all-targets && cargo test --lib -- --skip test_sound_device_filtered_list`.

## Architecture & Code Conventions

- **Language:** All source code comments, docstrings, and error messages must be kept in **English**.
- `src/main.rs` is the entrypoint and redeclares every module itself; `src/lib.rs` repeats that list so tests compile without `main`. When adding a module, register it in **both** files.
- `main.rs` owns the event loop and the `SoundDevice`; `App` only holds pure UI state and device name strings. `update()` mutates `App` and returns `AppAction::ChangeDevice`, and `main` performs the hardware switch afterward. Keep audio hardware side effects out of `App` and `update`.
- `event::EventHandler` runs a crossterm polling thread and emits `Event::Tick` every ~16 ms (60 FPS); the cpal callback sends `Event::AudioBars` through the same mpsc channel.
- `event::forward_terminal_event()` forwards key presses, mouse, and resize events, and deliberately ignores `FocusGained`/`FocusLost`. Windows always emits focus events and they cannot be disabled; never panic on an unhandled crossterm variant, because killing the event thread leaves audio bars flowing while the UI stops responding to keys.
- `Tui::reset()` disables mouse capture before raw mode: on Windows, `DisableMouseCapture` restores the console input-mode snapshot taken while raw mode was active, so it must run before `disable_raw_mode()`.
- `sound_device.rs` captures the output **sink monitor**, not a microphone. On Linux, `init_capture_env()` sets `PIPEWIRE_ALSA` / `PULSE_SOURCE` only if unset; don't overwrite those variables when the user already set them. On Windows, cpal uses WASAPI loopback automatically when a render endpoint is opened as an input stream.
- On Linux, `sound_device.rs` filters out global ALSA plugin aliases (anything without `:CARD=` in its PCM id), technical duplicate drivers (raw `hw:`), dummy sinks ("Discard all samples"), and unconfigured hardware ports. On Windows it deduplicates active render endpoints by cpal `DeviceId`, because different endpoints may share the same friendly name.
- `SoundDevice` polls the default output every second on Windows, rebuilds capture when the selected "Default Output" changes, and falls back to the current default if an explicitly selected endpoint disappears. `Event::AudioDeviceLost` triggers that recovery path.
- `App::tick()` decays targets and applies attack/decay smoothing; `App::displayed_bars()` applies gain. Use these for rendering instead of mutating raw bar values for display-only effects.

## DSP & Audio Pipeline (`src/audio_processor.rs`)

- Interleaved audio is downmixed to mono `(L + R) / 2` and buffered into chunks of 1024 samples. For multichannel layouts, only the front left/right channels are averaged so center/LFE/surround content does not dominate the bars.
- Samples are multiplied by a Hann window before running forward FFT via `realfft`.
- Output magnitudes are grouped into 32 log-spaced frequency bins (40 Hz to 16,000 Hz) using the stream's actual sample rate, not a fixed 44100 Hz value.
- Bins combine average and peak magnitude (`0.5 * avg + 0.5 * max`) to preserve rapid transients (cymbals, snares) across wide treble ranges.
- Frequency compensation curve (treble boost) scales linearly from `1.0x` on the lowest sub-bass bar up to `~6.0x` on the highest treble bar to counterbalance natural 1/f energy roll-off without reducing bass response.

## UI & Rendering (`src/ui.rs`)

- The visualizer occupies 100% of the available vertical height, centered dynamically on the screen without enclosing block borders.
- Symmetrical layout:
  - Upper half: normal bars growing upward using base-anchored Unicode blocks (` ▂▃▄▅▆▇█`) with cyan-green-magenta gradient.
  - Center line: subtle horizontal divider (`──`).
  - Lower half: upside-down mirror effect growing downward using top-anchored blocks (` ▔▀█`), with attenuated amplitude and deeper tones.
- Device selection popup renders a rounded bordered box using `Clear` over the visualizer, highlighting the selected cursor in cyan and tagging the active device with `[Active]`. When the terminal is too short for every device, the popup caps its height, scrolls to keep the selection visible, and shows a vertical scrollbar.

## Keybindings (`src/update.rs`)

- Popup closed: `q`/Esc quit, `s` open device popup, Left/`j` counter--, Right/`l` counter++, Up/`i` gain+, Down/`k` gain-. `j`/`k` are not vim-style navigation here.
- Popup open: Up/`k` and Down/`j` move selection, Enter activates the highlighted device, Esc/`s` closes. Ctrl+C quits in both modes.
- The popup branch returns early and swallows every other key; add new popup keys there explicitly.
