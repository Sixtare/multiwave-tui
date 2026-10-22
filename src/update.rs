use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

/// Actions triggered by keyboard events that require external handling (e.g., audio hardware).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    /// Request to change the active captured audio device to the specified index.
    ChangeDevice(usize),
}

pub fn update(app: &mut App, key_event: KeyEvent) -> Option<AppAction> {
    // If the device selection popup is open, handle navigation and confirmation
    if app.show_popup {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('s') => {
                app.show_popup = false;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.select_prev_device();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.select_next_device();
            }
            KeyCode::Enter => {
                // Active index is updated only after the stream starts successfully.
                app.show_popup = false;
                return Some(AppAction::ChangeDevice(app.selected_device_idx));
            }
            KeyCode::Char('c') | KeyCode::Char('C')
                if key_event.modifiers == KeyModifiers::CONTROL =>
            {
                app.quit();
            }
            _ => {}
        }
        return None;
    }

    // Default keyboard navigation when popup is closed
    match key_event.code {
        KeyCode::Esc | KeyCode::Char('q') => app.quit(),
        KeyCode::Char('c') | KeyCode::Char('C') if key_event.modifiers == KeyModifiers::CONTROL => {
            app.quit()
        }
        KeyCode::Char('s') => {
            app.show_popup = true;
        }
        KeyCode::Right | KeyCode::Char('l') => app.increment_counter(),
        KeyCode::Left | KeyCode::Char('j') => app.decrement_counter(),
        KeyCode::Up | KeyCode::Char('i') => app.increase_gain(),
        KeyCode::Down | KeyCode::Char('k') => app.decrease_gain(),
        _ => {}
    }
    None
}
