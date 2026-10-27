# multiwave-tui

A terminal audio visualizer written in Rust. It captures whatever your system is playing, computes a spectrum and draws it right in the terminal, built with [ratatui](https://ratatui.rs).

https://github.com/user-attachments/assets/7141ff78-5363-49f3-b749-9905f9e704f7

## Features

- **System audio capture**: records the output sink monitor on Linux (PulseAudio/PipeWire) and WASAPI loopback on Windows.
- **Per-application capture**: pick a single app that is playing audio and visualize only it.
- **Spectrum analysis**: two Hann-windowed FFTs (long for the bass, short for the treble), 32 to 512 log-spaced bars from 40 Hz to 16 kHz, dB scaling and automatic sensitivity.
- **Two render modes**: block glyphs (` ▂▃▄▅▆▇█`) or braille dots.
- **Colors**: single color, gradient along the bars, vertical gradient or rainbow, with free 24-bit colors, a base fade and an optional mirrored reflection.
- **Adjustable smoothing, frame rate (30/60/90/120 FPS), bar count, gap between bars and sensitivity**, all from the keyboard.
- **Persistent options**: your settings are saved on exit and restored on the next run.

## Requirements

- A recent stable Rust toolchain (edition 2024).
- **Linux**: ALSA headers and `pkg-config` to build (`libasound2-dev`, `pkg-config` on Debian/Ubuntu). At runtime, `pactl` and `parec` (from `pulseaudio-utils` or `pipewire-pulse`) are used for capture; without them the app falls back to cpal/ALSA.
- **Windows**: the Rust MSVC toolchain. Per-application capture needs Windows 10 version 2004 or newer.

## Run

```sh
cargo run --release
```

It needs a real interactive terminal. If audio capture is unavailable, the app still starts, just without bars.

## Keys

| Key | Action |
| --- | --- |
| `Esc` | Open the main menu |
| `o` | Choose the audio output or application |
| `c` | Bar style: colors, fade, render mode, gap, mirror |
| `←` / `→` | Fewer / more bars (steps of 8) |
| `↑` / `↓` | Raise / lower the sensitivity |
| `s` | Switch smoothing on or off |
| `g` | Toggle the gap between bars |
| `m` | Toggle the mirror |
| `f` | Step the frame rate (30/60/90/120) |
| `r` | Switch between blocks and braille |
| `q` / `Ctrl+C` | Quit |

## Configuration

Options are stored in a plain `key = value` text file:

- Linux: `~/.config/multiwave-tui/config`
- Windows: `%USERPROFILE%\Documents\multiwave-tui\config`

Missing or invalid lines fall back to the defaults.

## Development

```sh
cargo fmt --check && cargo clippy --all-targets && cargo test --lib -- --skip test_sound_device_filtered_list
```

The skipped test needs a real audio output device; drop the `--skip` on a machine that has one.

## License

MIT, see [LICENSE](LICENSE).
