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
use update::update;

fn main() -> Result<()> {
    // Inicializa o estado da aplicação
    let mut app = App::new();

    // Inicializa o manipulador de eventos da TUI (~60 FPS com 16ms)
    let events = EventHandler::new(16);
    let audio_sender = events.sender();

    // Inicializa a captura de áudio no sound_device
    let mut sound_device = SoundDevice::new().ok();
    if let Some(ref mut dev) = sound_device {
        // Dispara a captura de áudio na thread de alta performance do cpal (32 barras de FFT, ganho 2.5)
        if let Err(e) = dev.start_capture(audio_sender, 32, 2.5) {
            eprintln!("Falha ao iniciar stream de áudio cpal: {}", e);
        }
    }

    // Inicializa a interface de terminal
    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    let mut tui = Tui::new(terminal, events);
    tui.enter()?;

    // Loop principal da aplicação
    while !app.should_quit {
        // Renderiza a interface do usuário
        tui.draw(&mut app)?;

        // Aguarda e processa o próximo evento
        match tui.events.next()? {
            Event::Tick => {
                app.increment_tick_counter();
            }
            Event::Key(key_event) => {
                update(&mut app, key_event);
            }
            Event::AudioBars(bars) => {
                // Atualiza o estado da aplicação com as barras calculadas pelo realfft
                app.update_audio_bars(bars);
            }
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
        }
    }

    // Restaura o terminal ao sair
    tui.exit()?;
    Ok(())
}