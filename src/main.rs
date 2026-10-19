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
use ratatui::{backend::CrosstermBackend, Terminal};
use sound_device::SoundDevice;
use tui::Tui;
use update::{update, AppAction};

fn main() -> Result<()> {
    // Initialize the TUI event handler (~60 FPS at 16ms)
    let events = EventHandler::new(16);
    let audio_sender = events.sender();

    // Initialize sound device capture
    let mut sound_device = SoundDevice::new().ok();
    let (device_names, active_idx) = match sound_device.as_mut() {
        Some(dev) => {
            if let Err(e) = dev.start_capture(audio_sender.clone(), 32, 2.5) {
                eprintln!("Failed to start CPAL audio stream: {}", e);
            }
            (dev.device_names(), dev.active_idx)
        }
        None => (Vec::new(), 0),
    };

    // Initialize application state with device names (App does NOT hold SoundDevice)
    let mut app = App::with_devices(device_names, active_idx);

    // Initialize terminal user interface
    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    let mut tui = Tui::new(terminal, events);
    tui.enter()?;

    // Main application loop
    while !app.should_quit {
        // Render the user interface
        tui.draw(&mut app)?;

        // Wait for and process the next event
        match tui.events.next()? {
            Event::Tick => {
                app.tick();
            }
            Event::Key(key_event) => {
                if let Some(action) = update(&mut app, key_event) {
                    match action {
                        AppAction::ChangeDevice(new_idx) => {
                            if let Some(ref mut dev) = sound_device {
                                if let Err(e) = dev.switch_device(
                                    new_idx,
                                    audio_sender.clone(),
                                    32,
                                    2.5,
                                ) {
                                    eprintln!("Failed to switch audio device: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            Event::AudioBars(bars) => {
                // Update application state with bars calculated by realfft
                app.update_audio_bars(bars);
            }
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
        }
    }

    // Restore the terminal upon exit
    tui.exit()?;
    Ok(())
}