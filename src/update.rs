use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

pub fn update(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc | KeyCode::Char('q') => app.quit(),
        KeyCode::Char('c') | KeyCode::Char('C') if key_event.modifiers == KeyModifiers::CONTROL => {
            app.quit()
        },
        KeyCode::Char('s') => app.show_popup = !app.show_popup,
        KeyCode::Right | KeyCode::Char('l') => app.increment_counter(),
        KeyCode::Left | KeyCode::Char('j') => app.decrement_counter(),
        KeyCode::Up | KeyCode::Char('i') => app.increase_gain(),
        KeyCode::Down | KeyCode::Char('k') => app.decrease_gain(),
        _ => {}
    };
}