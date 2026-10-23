use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::App,
    event::{BatchEvent, EventBatch},
};

/// Actions that require external handling (e.g., audio hardware), performed by `main`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    /// Request to change the active captured audio device to the specified index.
    ChangeDevice(usize),
    /// Rebuild capture after the audio callback reported a lost or invalidated stream.
    RecoverLostDevice,
    /// Check whether the default output or device list changed (rate-limited by `SoundDevice`).
    PollDevices,
}

/// Result of applying one [`EventBatch`] to the [`App`].
#[derive(Debug, Default, PartialEq)]
pub struct BatchOutcome {
    /// Hardware actions for `main` to perform, in the order they were requested.
    pub actions: Vec<AppAction>,
    /// Whether the frame should be redrawn after the actions run.
    pub draw: bool,
}

/// Applies every event in a batch to the app and collects the hardware actions it requests.
///
/// Once an event quits the app, the rest of the batch is discarded so nothing queued behind the
/// quit key (e.g. an Enter that would switch devices) still runs.
pub fn apply_batch(app: &mut App, batch: EventBatch) -> BatchOutcome {
    let mut outcome = BatchOutcome::default();
    let has_tick = batch.has_tick();

    if let Some(bars) = batch.latest_bars {
        // Update application state with bars calculated by realfft
        app.update_audio_bars(bars);
    }

    for event in batch.events {
        match event {
            BatchEvent::Key(key_event) => {
                if let Some(action) = update(app, key_event) {
                    outcome.actions.push(action);
                }
            }
            BatchEvent::AudioError(message) => app.set_audio_error(message),
            BatchEvent::AudioDeviceLost => outcome.actions.push(AppAction::RecoverLostDevice),
        }

        if app.should_quit {
            return BatchOutcome::default();
        }
    }

    for _ in 0..batch.ticks {
        app.tick();
    }
    if has_tick {
        outcome.actions.push(AppAction::PollDevices);
        outcome.draw = true;
    }

    outcome
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn batch_of(events: Vec<Event>) -> EventBatch {
        let mut batch = EventBatch::default();
        for event in events {
            batch.push(event);
        }
        batch
    }

    #[test]
    fn test_batch_applies_keys_and_latest_bars_with_one_draw() {
        let mut app = App::new();
        let mut events: Vec<Event> = (1..=50)
            .map(|level| Event::AudioBars(vec![level as f32 / 50.0; 32]))
            .collect();
        events.push(key(KeyCode::Right));
        events.push(Event::Tick);
        events.push(key(KeyCode::Right));
        events.push(Event::Tick);

        let outcome = apply_batch(&mut app, batch_of(events));

        // Only the last bars frame matters: same state as a batch holding just that frame.
        let mut expected = App::new();
        apply_batch(
            &mut expected,
            batch_of(vec![
                Event::AudioBars(vec![1.0; 32]),
                Event::Tick,
                Event::Tick,
            ]),
        );
        assert_eq!(app.counter, 2);
        assert_eq!(app.target_bars, expected.target_bars);
        assert_eq!(app.audio_bars, expected.audio_bars);
        assert_eq!(app.tick_counter, 2);
        assert!(outcome.draw);
        assert_eq!(outcome.actions, vec![AppAction::PollDevices]);
    }

    #[test]
    fn test_batch_without_tick_does_not_draw() {
        let mut app = App::new();

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![Event::AudioBars(vec![0.5; 32]), key(KeyCode::Right)]),
        );

        assert_eq!(app.counter, 1);
        assert_eq!(outcome, BatchOutcome::default());
    }

    #[test]
    fn test_batch_keeps_hardware_actions_in_order() {
        let mut app = App::with_devices(vec!["A".into(), "B".into()], 0);

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![
                Event::AudioDeviceLost,
                key(KeyCode::Char('s')),
                key(KeyCode::Down),
                key(KeyCode::Enter),
            ]),
        );

        assert_eq!(
            outcome.actions,
            vec![AppAction::RecoverLostDevice, AppAction::ChangeDevice(1)]
        );
    }

    #[test]
    fn test_batch_stops_at_quit() {
        let mut app = App::with_devices(vec!["A".into(), "B".into()], 0);
        app.show_popup = true;
        app.selected_device_idx = 1;

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![
                Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
                key(KeyCode::Enter),
                Event::Tick,
            ]),
        );

        assert!(app.should_quit);
        assert!(app.show_popup, "keys after the quit must not run");
        assert_eq!(app.tick_counter, 0);
        assert_eq!(outcome, BatchOutcome::default());
    }
}
