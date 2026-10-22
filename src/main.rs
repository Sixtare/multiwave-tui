/// Application.
pub mod app;

/// Audio signal processing with realfft.
pub mod audio_processor;

/// Terminal events handler.
pub mod event;

/// Widget renderer.
pub mod ui;

/// Terminal user interface.
pub mod tui;

/// Sound device audio capture with cpal.
pub mod sound_device;

/// Application updater.
pub mod update;

use app::App;
use color_eyre::Result;
use event::{Event, EventHandler};
use ratatui::{Terminal, backend::CrosstermBackend};
use sound_device::{DeviceListUpdate, SoundDevice};
use std::io::BufWriter;
use tui::Tui;
use update::{AppAction, update};

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
    // Initialize the TUI event handler (~60 FPS at 16ms)
    let events = EventHandler::new(16);
    let audio_sender = events.sender();

    // Initialize sound device capture
    let mut sound_device = SoundDevice::new().ok();
    let mut startup_error = None;
    let (device_names, active_idx) = match sound_device.as_mut() {
        Some(dev) => {
            if let Err(e) = dev.start_capture(audio_sender.clone(), 32, 2.5) {
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
    if let Some(error) = startup_error {
        app.set_audio_error(error);
    }

    // Initialize terminal user interface
    // stderr is unbuffered; without a BufWriter every queued crossterm command becomes its own
    // console write, which makes each frame slow on Windows.
    let backend = CrosstermBackend::new(BufWriter::new(std::io::stderr()));
    let terminal = Terminal::new(backend)?;
    let mut tui = Tui::new(terminal, events);
    tui.enter()?;

    // Main application loop
    tui.draw(&mut app)?;
    while !app.should_quit {
        // Handle everything queued since the last iteration before rendering, so a slow draw
        // can never leave a growing backlog of input and audio events behind it.
        let batch = tui.events.next_batch()?;
        let needs_draw = batch.needs_draw();

        if let Some(bars) = batch.latest_bars {
            // Update application state with bars calculated by realfft
            app.update_audio_bars(bars);
        }

        for event in batch.events {
            match event {
                Event::Key(key_event) => {
                    if let Some(action) = update(&mut app, key_event) {
                        match action {
                            AppAction::ChangeDevice(new_idx) => {
                                if let Some(ref mut dev) = sound_device {
                                    match dev.switch_device(new_idx, audio_sender.clone(), 32, 2.5)
                                    {
                                        Ok(()) => {
                                            app.active_device_idx = new_idx;
                                            app.clear_audio_error();
                                        }
                                        Err(e) => {
                                            app.set_audio_error(format!(
                                                "Failed to switch audio device: {e}"
                                            ));
                                        }
                                    }
                                } else {
                                    app.set_audio_error("No audio output device available");
                                }
                            }
                        }
                    }
                }
                Event::AudioError(message) => {
                    app.set_audio_error(message);
                }
                Event::AudioDeviceLost => {
                    if let Some(ref mut dev) = sound_device {
                        let update = dev.recover_from_device_loss(
                            app.selected_device_idx,
                            &audio_sender,
                            32,
                            2.5,
                        );
                        apply_device_update(&mut app, update);
                    } else {
                        app.set_audio_error("No audio output device available");
                    }
                }
                // Collapsed into the batch counters by `EventBatch::push`.
                Event::Tick | Event::AudioBars(_) | Event::Mouse(_) | Event::Resize(_, _) => {}
            }
        }

        for _ in 0..batch.ticks {
            app.tick();
        }
        if batch.ticks > 0
            && let Some(ref mut dev) = sound_device
            && let Some(update) =
                dev.poll_device_changes(app.selected_device_idx, &audio_sender, 32, 2.5)
        {
            apply_device_update(&mut app, update);
        }

        // Render the user interface once per batch, on the tick cadence
        if needs_draw && !app.should_quit {
            tui.draw(&mut app)?;
        }
    }

    // Restore the terminal upon exit
    tui.exit()?;
    Ok(())
}
