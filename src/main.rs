/// Application.
pub mod app;

/// Output devices listed in the device popup.
pub mod device_menu;

/// Animated bar heights.
pub mod bar_motion;

/// Audio signal processing with realfft.
pub mod audio_processor;

/// Terminal events handler.
pub mod event;

/// Settings shared with the running capture.
pub mod capture_settings;

/// Rows of the main menu.
pub mod menu;

/// Options saved between sessions.
pub mod config;

/// Short on-screen announcements.
pub mod notices;

/// Custom color picker state.
pub mod color_picker;

/// State of one visit to the bar color popup.
pub mod color_session;

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

use app::{App, MIN_BARS, Overlay};
use capture_settings::CaptureSettings;
use color_eyre::Result;
use config::Config;
use device_menu::DeviceTab;
use event::EventHandler;
use ratatui::{Terminal, backend::CrosstermBackend};
use sound_device::{AppListUpdate, DeviceListUpdate, SoundDevice};
use std::{io::BufWriter, sync::Arc};
use tui::Tui;
use update::{AppAction, apply_batch};

/// Sized to hold a typical full-screen frame, so a draw reaches the console in a few writes.
/// The default 8 KiB would split a frame full of color escapes into many writes again.
const OUTPUT_BUFFER_CAPACITY: usize = 256 * 1024;

fn apply_device_update(app: &mut App, update: DeviceListUpdate) {
    app.devices
        .replace(update.names, update.active_idx, update.selected_idx);

    if let Some(message) = update.message {
        app.set_audio_error(message);
    } else if update.active_changed {
        app.clear_audio_error();
    }
}

fn refresh_apps(app: &mut App, update: AppListUpdate) {
    app.devices.replace_apps(update.names, update.active);
}

fn main() -> Result<()> {
    // Initialize the TUI event handler (~60 FPS at 16ms; the user can change the rate)
    let events = EventHandler::new(16);
    let audio_sender = events.sender();

    // Initialize sound device capture
    let settings = Arc::new(CaptureSettings::new(MIN_BARS));
    let mut sound_device = SoundDevice::new(Arc::clone(&settings)).ok();
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
    app.apply_config(&Config::load());
    events.set_fps(app.fps);
    settings.set_gain(app.gain);
    settings.set_smoothing(app.smoothing_strength());
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
    settings.set_num_bars(app.bar_count);

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
                                app.devices.active = new_idx;
                                app.devices.apps.active = None;
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
                AppAction::ChangeApp(row) => {
                    if let Some(ref mut dev) = sound_device {
                        match dev.switch_to_app(row, audio_sender.clone()) {
                            Ok(()) => app.clear_audio_error(),
                            Err(e) => {
                                app.set_audio_error(format!("Failed to capture application: {e}"));
                            }
                        }
                        refresh_apps(&mut app, dev.list_apps());
                    }
                }
                AppAction::ListApps => {
                    if let Some(ref mut dev) = sound_device {
                        refresh_apps(&mut app, dev.list_apps());
                    }
                }
                AppAction::SetBarCount(count) => settings.set_num_bars(count),
                AppAction::SetGain(gain) => settings.set_gain(gain),
                AppAction::SetFps(fps) => tui.events.set_fps(fps),
                AppAction::SetSmoothing(strength) => settings.set_smoothing(strength),
                AppAction::RecoverLostDevice => {
                    if let Some(ref mut dev) = sound_device {
                        let update =
                            dev.recover_from_device_loss(app.devices.selected, &audio_sender);
                        apply_device_update(&mut app, update);
                    } else {
                        app.set_audio_error("No audio output device available");
                    }
                }
                AppAction::PollDevices => {
                    if let Some(ref mut dev) = sound_device
                        && let Some(update) = dev.poll_captured_app(&audio_sender)
                    {
                        apply_device_update(&mut app, update);
                        refresh_apps(&mut app, dev.list_apps());
                    }
                    if let Some(ref mut dev) = sound_device
                        && app.overlay == Overlay::Devices
                        && app.devices.tab == DeviceTab::Apps
                        && let Some(update) = dev.poll_apps()
                    {
                        refresh_apps(&mut app, update);
                    }
                    if let Some(ref mut dev) = sound_device
                        && let Some(update) =
                            dev.poll_device_changes(app.devices.selected, &audio_sender)
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
    // Best effort: failing to save the options must not turn a clean exit into an error.
    let _ = app.config().save();
    Ok(())
}
