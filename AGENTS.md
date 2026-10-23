# AGENTS.md

Ratatui terminal audio visualizer (Rust, edition 2024). Captures system output audio with cpal, computes a spectrum with realfft, and renders mirrored bars. There is no README, CI, workspace, or task runner config — Cargo is the source of truth.

## Commands

- `cargo run` — the TUI writes to **stderr** (`CrosstermBackend` over a buffered `std::io::stderr()`), so run it in a real terminal and don't pipe stdout to inspect it. It still launches when audio capture is unavailable, just without bars.
- `cargo test --lib` — preferred test command. Inline tests in `src/app.rs`, `src/audio_processor.rs`, `src/event.rs`, `src/sound_device.rs`, and `src/update.rs` are reachable from both `main.rs` and `lib.rs`; plain `cargo test` runs them twice (lib + bin harness). Tests in `src/lib.rs` only run under the lib harness.
- `cargo test --lib <filter>` — run one test, e.g. `cargo test --lib app::tests::test_app_gain_adjustment`.
- `cargo test --lib -- --skip test_sound_device_filtered_list` — headless-safe run. That test asserts at least one real output device exists and fails on machines without audio hardware.
- Linux builds need ALSA headers and pkg-config (`libasound2-dev`, `pkg-config`) for `alsa-sys`.
- Windows builds use the default cpal WASAPI backend and need no extra system packages beyond the Rust MSVC toolchain.
- Full check: `cargo fmt --check && cargo clippy --all-targets && cargo test --lib -- --skip test_sound_device_filtered_list`.

## Architecture & Code Conventions

- **Language:** All source code comments, docstrings, and error messages must be kept in **English**.
- `src/main.rs` is the entrypoint and redeclares every module itself; `src/lib.rs` repeats that list so tests compile without `main`. When adding a module, register it in **both** files.
- `main.rs` owns the event loop and the `SoundDevice`; `App` only holds pure UI state and device name strings. `update()` / `apply_batch()` mutate `App` and return `AppAction`s (`ChangeDevice`, `RecoverLostDevice`, `PollDevices`), and `main` performs the hardware work afterward. Keep audio hardware side effects out of `App` and `update`.
- `event::EventHandler` runs a crossterm polling thread and emits `Event::Tick` every ~16 ms (60 FPS); the cpal callback sends `Event::AudioBars` through the same mpsc channel.
- The main loop calls `EventHandler::next_batch()`, which blocks for one event and drains the rest into an `EventBatch` (tick count, latest bars only, `BatchEvent`s in order). `update::apply_batch()` applies it and returns a `BatchOutcome`; `main` runs the actions and draws only when the batch had a tick. Once an event quits, the rest of the batch is discarded. Never go back to drawing per event: when a draw is slower than the tick + audio rate (Windows consoles), the channel backlog grows without bound and input lags further and further.
- The terminal backend writes to a 256 KiB `BufWriter<Stderr>` (`OUTPUT_BUFFER_CAPACITY`); unbuffered stderr turns every crossterm command into its own console write, and the default 8 KiB splits large frames.
- `event::forward_terminal_event()` forwards key presses, mouse, and resize events, and deliberately ignores `FocusGained`/`FocusLost`. Windows always emits focus events and they cannot be disabled; never panic on an unhandled crossterm variant, because killing the event thread leaves audio bars flowing while the UI stops responding to keys.
- `Tui::reset()` disables mouse capture before raw mode: on Windows, `DisableMouseCapture` restores the console input-mode snapshot taken while raw mode was active, so it must run before `disable_raw_mode()`.
- `sound_device.rs` captures the output **sink monitor**, not a microphone. On Linux, `init_capture_env()` sets `PIPEWIRE_ALSA` / `PULSE_SOURCE` only if unset; don't overwrite those variables when the user already set them. On Windows, cpal uses WASAPI loopback automatically when a render endpoint is opened as an input stream.
- On Linux, `sound_device.rs` filters out global ALSA plugin aliases (anything without `:CARD=` in its PCM id), technical duplicate drivers (raw `hw:`), dummy sinks ("Discard all samples"), and unconfigured hardware ports. On Windows it deduplicates active render endpoints by cpal `DeviceId`, because different endpoints may share the same friendly name.
- `SoundDevice` polls the default output every second on Windows, rebuilds capture when the selected "Default Output" changes, and falls back to the current default if an explicitly selected endpoint disappears. `Event::AudioDeviceLost` triggers that recovery path.
- `App::tick()` decays targets and applies attack/decay smoothing; `App::displayed_bars()` applies gain. Use these for rendering instead of mutating raw bar values for display-only effects.

## DSP & Audio Pipeline (`src/audio_processor.rs`)

- Interleaved audio is downmixed to mono `(L + R) / 2` and buffered into chunks of 1024 samples. For multichannel layouts, only the front left/right channels are averaged so center/LFE/surround content does not dominate the bars.
- Samples are multiplied by a Hann window before running forward FFT via `realfft`.
- Output magnitudes are grouped into `bar_count` log-spaced frequency bins (32 to 256, 40 Hz to 16,000 Hz) using the stream's actual sample rate, not a fixed 44100 Hz value. `AudioProcessor::set_num_bars()` rebuilds the layout without dropping buffered samples.
- A bar narrower than one FFT bin (the lowest bars, and more of them as the count grows) is read at its center frequency by linear interpolation between the two surrounding bins, instead of repeating a neighbour's value as a plateau.
- Bins combine average and peak magnitude (`0.5 * avg + 0.5 * max`) to preserve rapid transients (cymbals, snares) across wide treble ranges.
- Frequency compensation curve (treble boost) scales linearly from `1.0x` on the lowest sub-bass bar up to `~6.0x` on the highest treble bar to counterbalance natural 1/f energy roll-off without reducing bass response.

## UI & Rendering (`src/ui.rs`)

- The visualizer occupies 100% of the available vertical height, centered dynamically on the screen without enclosing block borders.
- The visualizer is a `Soundwave` widget that writes cells straight into the `Buffer`. Don't go back to building a `Paragraph` of per-cell `Span`s: the per-cell `String` allocations and centering cost more than half the app's CPU. Each bar takes `COLUMNS_PER_BAR` (2) columns; on a terminal narrower than the row, the treble side is clipped.
- `Tui::draw_if_changed()` skips the draw when `ui::FrameState` (everything `render` reads, plus terminal size) matches the last drawn frame, so silence costs almost nothing. Anything new that `render` reads must be added to `FrameState`, or changes to it won't show until something else redraws.
- Bar count: `App` keeps the count the user chose (`chosen_bar_count`) and the count shown (`bar_count` = chosen clamped to `MIN_BARS`..=`min(MAX_BARS, width / COLUMNS_PER_BAR)`, never below 32 even if clipped), so bars shrink on a narrow terminal and grow back on resize. `apply_batch()` applies the terminal width first and emits one `AppAction::SetBarCount` when the shown count changed; `main` forwards it to `SoundDevice::set_num_bars()`, which updates an `AtomicUsize` the capture callback reads, so changing it never rebuilds the stream. `App::update_audio_bars()` drops frames whose length differs from `bar_count` (stale frames from before a change). A ~1.5 s `N bars` notice appears in the top-right corner after each key press; changing the sensitivity shows a matching `sensitivity N.NNx` notice (cyan text, no background) stacked below it.
- Symmetrical layout:
  - Upper half: normal bars growing upward using base-anchored Unicode blocks (` ▂▃▄▅▆▇█`) with cyan-green-magenta gradient.
  - Center line: subtle horizontal divider (`──`).
  - Lower half: upside-down mirror effect growing downward using top-anchored blocks (` ▔▀█`), with attenuated amplitude and deeper tones.
- Device selection popup renders a rounded bordered box using `Clear` over the visualizer, highlighting the selected cursor in cyan and tagging the active device with `[Active]`. When the terminal is too short for every device, the popup caps its height, scrolls to keep the selection visible, and shows a vertical scrollbar.

## Keybindings (`src/update.rs`)

- Popup closed: `q`/Esc quit, `s` open device popup, Left/`j` bars -8, Right/`l` bars +8 (min 32, max `min(256, width / 2)`), Up/`i` sensitivity (gain) +0.25 up to 20, Down/`k` -0.25 down to 0.1. `j`/`k` are not vim-style navigation here.
- Popup open: Up/`k` and Down/`j` move selection, Enter activates the highlighted device, Esc/`s` closes. Ctrl+C quits in both modes.
- The popup branch returns early and swallows every other key; add new popup keys there explicitly.
