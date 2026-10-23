use std::{io, panic};

use color_eyre::Result;
use ratatui::crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

pub type CrosstermTerminal =
    ratatui::Terminal<ratatui::backend::CrosstermBackend<io::BufWriter<io::Stderr>>>;

use crate::{app::App, event::EventHandler, ui};

pub struct Tui {
    /// Interface to the Terminal.
    terminal: CrosstermTerminal,
    /// Terminal event handler.
    pub events: EventHandler,
}

impl Tui {
    /// Constructs a new instance of [`Tui`].
    pub fn new(terminal: CrosstermTerminal, events: EventHandler) -> Self {
        Self { terminal, events }
    }

    /// Initializes the terminal interface.
    ///
    /// It enables the raw mode and sets terminal properties.
    pub fn enter(&mut self) -> Result<()> {
        terminal::enable_raw_mode()?;
        execute!(io::stderr(), EnterAlternateScreen, EnableMouseCapture)?;

        // Define a custom panic hook to reset the terminal properties.
        // This way, you won't have your terminal messed up if an unexpected error happens.
        let panic_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic| {
            // A panic hook must not panic again: reset on a best-effort basis and always
            // delegate to the previous hook so the original panic is still reported.
            let _ = Self::reset();
            panic_hook(panic);
        }));

        self.terminal.hide_cursor()?;
        self.terminal.clear()?;
        Ok(())
    }

    /// Resets the terminal interface.
    ///
    /// This function is also used for the panic hook to revert
    /// the terminal properties if unexpected errors occur.
    fn reset() -> Result<()> {
        // On Windows, DisableMouseCapture restores the console input mode captured by
        // EnableMouseCapture, which was taken while raw mode was already active. Restore mouse
        // capture first so the subsequent disable_raw_mode call is the final word.
        let leave_alternate = execute!(io::stderr(), LeaveAlternateScreen);
        let disable_mouse = execute!(io::stderr(), DisableMouseCapture);
        let disable_raw = terminal::disable_raw_mode();

        leave_alternate?;
        disable_mouse?;
        disable_raw?;
        Ok(())
    }

    /// Exits the terminal interface.
    ///
    /// It disables the raw mode and reverts back the terminal properties.
    pub fn exit(&mut self) -> Result<()> {
        // Try both cleanup steps even if one fails, so a partial reset does not leave the
        // cursor hidden.
        let reset_result = Self::reset();
        let cursor_result = self.terminal.show_cursor();
        reset_result?;
        cursor_result?;
        Ok(())
    }

    pub fn draw(&mut self, app: &mut App) -> Result<()> {
        self.terminal.draw(|frame| ui::render(app, frame))?;
        Ok(())
    }
}
