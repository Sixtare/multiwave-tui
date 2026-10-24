/// Application.
pub mod app;

/// Audio signal processing with realfft.
pub mod audio_processor;

/// Terminal events handler.
pub mod event;

/// Custom color picker state.
pub mod color_picker;

/// Bar color palette.
pub mod palette;

/// Widget renderer.
pub mod ui;

/// Terminal user interface.
pub mod tui;

/// Output listing and capture through `pactl`/`parec` (Linux).
pub mod pulse;

/// Sound device audio capture with cpal.
pub mod sound_device;

/// Application updater.
pub mod update;

use app::{App, MIN_BARS};
use color_eyre::Result;
use event::EventHandler;
use ratatui::{Terminal, backend::CrosstermBackend};
use sound_device::{DeviceListUpdate, SoundDevice};
use std::io::BufWriter;
use tui::Tui;
use update::{AppAction, apply_batch};

/// Sized to hold a typical full-screen frame, so a draw reaches the console in a few writes.
/// The default 8 KiB would split a frame full of color escapes into many writes again.
const OUTPUT_BUFFER_CAPACITY: usize = 256 * 1024;

fn apply_device_update(app: &mut App, update: DeviceListUpdate) {
    let device_count = update.names.len();
    app.devices = update.names;
    app.active_device_idx = if device_count == 0 {
        0
    } else {
        update.active_idx.min(device_count - 1)
    };
    app.selected_device_idx = if device_count == 0 {
        0
    } else {
        update.selected_idx.min(device_count - 1)
    };

    if let Some(message) = update.message {
        app.set_audio_error(message);
    } else if update.active_changed {
        app.clear_audio_error();
    }
}

fn main() -> Result<()> {
    // Initialize the TUI event handler (~60 FPS at 16ms; the user can change the rate)
    let events = EventHandler::new(16);
    let audio_sender = events.sender();

    // Initialize sound device capture
    let mut sound_device = SoundDevice::new(MIN_BARS).ok();
    let mut startup_error = None;
    let (device_names, active_idx) = match sound_device.as_mut() {
        Some(dev) => {
            if let Err(e) = dev.start_capture(audio_sender.clone()) {
                startup_error = Some(format!("Failed to start audio stream: {e}"));
            }
            (dev.device_names(), dev.active_idx)
        }
        None => {
            startup_error = Some("No audio output devices available".to_string());
            (Vec::new(), 0)
        }
    };

    // Initialize application state with device names (App does NOT hold SoundDevice)
    let mut app = App::with_devices(device_names, active_idx);
    events.set_fps(app.fps);
    if let Some(error) = startup_error {
        app.set_audio_error(error);
    }

    // Initialize terminal user interface
    // stderr is unbuffered; without a BufWriter every queued crossterm command becomes its own
    // console write, which makes each frame slow on Windows.
    let backend = CrosstermBackend::new(BufWriter::with_capacity(
        OUTPUT_BUFFER_CAPACITY,
        std::io::stderr(),
    ));
    let terminal = Terminal::new(backend)?;
    let mut tui = Tui::new(terminal, events);
    tui.enter()?;
    app.set_terminal_width(tui.width()?);

    // Main application loop
    tui.draw_if_changed(&mut app)?;
    while !app.should_quit {
        // Handle everything queued since the last iteration before rendering, so a slow draw
        // can never leave a growing backlog of input and audio events behind it.
        let outcome = apply_batch(&mut app, tui.events.next_batch()?);

        for action in outcome.actions {
            match action {
                AppAction::ChangeDevice(new_idx) => {
                    if let Some(ref mut dev) = sound_device {
                        match dev.switch_device(new_idx, audio_sender.clone()) {
                            Ok(()) => {
                                app.active_device_idx = new_idx;
                                app.clear_audio_error();
                            }
                            Err(e) => {
                                app.set_audio_error(format!("Failed to switch audio device: {e}"));
                            }
                        }
                    } else {
                        app.set_audio_error("No audio output device available");
                    }
                }
                AppAction::SetBarCount(count) => {
                    if let Some(ref dev) = sound_device {
                        dev.set_num_bars(count);
                    }
                }
                AppAction::SetGain(gain) => {
                    if let Some(ref dev) = sound_device {
                        dev.set_gain(gain);
                    }
                }
                AppAction::SetFps(fps) => tui.events.set_fps(fps),
                AppAction::SetSmoothing(on) => {
                    if let Some(ref dev) = sound_device {
                        dev.set_smoothing(on);
                    }
                }
                AppAction::RecoverLostDevice => {
                    if let Some(ref mut dev) = sound_device {
                        let update =
                            dev.recover_from_device_loss(app.selected_device_idx, &audio_sender);
                        apply_device_update(&mut app, update);
                    } else {
                        app.set_audio_error("No audio output device available");
                    }
                }
                AppAction::PollDevices => {
                    if let Some(ref mut dev) = sound_device
                        && let Some(update) =
                            dev.poll_device_changes(app.selected_device_idx, &audio_sender)
                    {
                        apply_device_update(&mut app, update);
                    }
                }
            }
        }

        // Render the user interface once per batch: on the tick cadence, or right after a key press
        if outcome.draw {
            tui.draw_if_changed(&mut app)?;
        }
    }

    // Restore the terminal upon exit
    tui.exit()?;
    Ok(())
}
