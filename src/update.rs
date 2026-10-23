use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::App,
    event::{BatchEvent, EventBatch},
};

/// Actions that require external handling (e.g., audio hardware), performed by `main`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppAction {
    /// Request to change the active captured audio device to the specified index.
    ChangeDevice(usize),
    /// Make the running capture produce this many bars.
    SetBarCount(usize),
    /// Make the running capture apply this manual sensitivity.
    SetGain(f32),
    /// Make the running capture smooth (or not) the treble bars.
    SetSmoothing(bool),
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
    let bar_count_before = app.bar_count;
    let gain_before = app.gain;
    let smoothing_before = app.smoothing;

    // Applied first so keys and audio frames in the same batch see the new terminal width.
    if let Some(width) = batch.latest_width {
        app.set_terminal_width(width);
    }

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

    // Keys and resizes can both change the bar count; the capture only needs the final one.
    if app.bar_count != bar_count_before {
        outcome.actions.push(AppAction::SetBarCount(app.bar_count));
    }

    if app.gain != gain_before {
        outcome.actions.push(AppAction::SetGain(app.gain));
    }

    if app.smoothing != smoothing_before {
        outcome.actions.push(AppAction::SetSmoothing(app.smoothing));
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
            KeyCode::Esc => {
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
        KeyCode::Char('q') => app.quit(),
        KeyCode::Char('c') | KeyCode::Char('C') if key_event.modifiers == KeyModifiers::CONTROL => {
            app.quit()
        }
        KeyCode::Esc => {
            app.show_popup = true;
        }
        KeyCode::Char('s') => app.toggle_smoothing(),
        KeyCode::Right | KeyCode::Char('l') => app.increase_bar_count(),
        KeyCode::Left | KeyCode::Char('j') => app.decrease_bar_count(),
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
        app.set_terminal_width(200);
        let mut events: Vec<Event> = (1..=50)
            .map(|level| Event::AudioBars(vec![level as f32 / 50.0; 32]))
            .collect();
        events.push(key(KeyCode::Right));
        events.push(Event::Tick);
        events.push(key(KeyCode::Right));
        events.push(Event::Tick);

        let outcome = apply_batch(&mut app, batch_of(events));

        // Only the last bars frame matters: same state as applying just that frame, then the
        // two key presses and the two ticks one by one.
        let mut expected = App::new();
        expected.set_terminal_width(200);
        expected.update_audio_bars(vec![1.0; 32]);
        expected.increase_bar_count();
        expected.increase_bar_count();
        expected.tick();
        expected.tick();
        assert_eq!(app.bar_count, 48);
        assert_eq!(app.target_bars, expected.target_bars);
        assert_eq!(app.audio_bars, expected.audio_bars);
        assert_eq!(app.tick_counter, 2);
        assert!(outcome.draw);
        assert_eq!(
            outcome.actions,
            vec![AppAction::SetBarCount(48), AppAction::PollDevices]
        );
    }

    #[test]
    fn test_batch_without_tick_does_not_draw() {
        let mut app = App::new();
        app.set_terminal_width(200);

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![Event::AudioBars(vec![0.5; 40]), key(KeyCode::Right)]),
        );

        assert_eq!(app.bar_count, 40);
        assert_eq!(
            outcome,
            BatchOutcome {
                actions: vec![AppAction::SetBarCount(40)],
                draw: false,
            }
        );
    }

    #[test]
    fn test_batch_keeps_hardware_actions_in_order() {
        let mut app = App::with_devices(vec!["A".into(), "B".into()], 0);

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![
                Event::AudioDeviceLost,
                key(KeyCode::Esc),
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
    fn test_s_toggles_smoothing_and_reports_it_once() {
        let mut app = App::default();
        assert!(!app.smoothing);

        let outcome = apply_batch(&mut app, batch_of(vec![key(KeyCode::Char('s'))]));
        assert!(app.smoothing);
        assert_eq!(outcome.actions, vec![AppAction::SetSmoothing(true)]);

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![key(KeyCode::Char('s')), key(KeyCode::Char('s'))]),
        );
        assert!(app.smoothing);
        assert!(outcome.actions.is_empty());

        let outcome = apply_batch(&mut app, batch_of(vec![key(KeyCode::Char('s'))]));
        assert!(!app.smoothing);
        assert_eq!(outcome.actions, vec![AppAction::SetSmoothing(false)]);
    }

    #[test]
    fn test_esc_opens_and_closes_the_popup_and_q_quits() {
        let mut app = App::with_devices(vec!["A".into()], 0);

        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.show_popup && !app.should_quit);

        // `s` is swallowed while the popup is open.
        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        assert!(app.show_popup && !app.smoothing);

        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.show_popup && !app.should_quit);

        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        );
        assert!(app.should_quit);
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

    #[test]
    fn test_resize_shrinks_bars_and_asks_the_capture_to_follow() {
        let mut app = App::new();
        app.set_terminal_width(400);
        for _ in 0..8 {
            app.increase_bar_count();
        }

        let outcome = apply_batch(&mut app, batch_of(vec![Event::Resize(120, 40)]));

        assert_eq!(app.bar_count, 60);
        assert_eq!(outcome.actions, vec![AppAction::SetBarCount(60)]);

        let outcome = apply_batch(&mut app, batch_of(vec![Event::Resize(400, 40)]));

        assert_eq!(app.bar_count, 96);
        assert_eq!(outcome.actions, vec![AppAction::SetBarCount(96)]);
    }

    #[test]
    fn test_several_bar_keys_in_one_batch_send_only_the_final_count() {
        let mut app = App::new();
        app.set_terminal_width(400);

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![
                key(KeyCode::Right),
                key(KeyCode::Right),
                key(KeyCode::Char('l')),
                key(KeyCode::Left),
            ]),
        );

        assert_eq!(app.bar_count, 48);
        assert_eq!(outcome.actions, vec![AppAction::SetBarCount(48)]);
    }

    #[test]
    fn test_unchanged_bar_count_sends_no_action() {
        let mut app = App::new();
        app.set_terminal_width(400);

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![key(KeyCode::Left), Event::Resize(500, 40)]),
        );

        assert!(outcome.actions.is_empty());
    }

    #[test]
    fn test_resize_applies_before_keys_in_the_same_batch() {
        let mut app = App::new();

        apply_batch(
            &mut app,
            batch_of(vec![key(KeyCode::Right), Event::Resize(200, 40)]),
        );

        assert_eq!(app.bar_count, 40);
    }

    #[test]
    fn test_horizontal_arrows_and_j_l_change_bar_count_not_gain() {
        let mut app = App::new();
        app.set_terminal_width(400);
        let gain = app.gain;

        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.bar_count, 40);
        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(app.bar_count, 32);
        assert_eq!(app.gain, gain);
    }

    #[test]
    fn test_popup_open_does_not_change_bar_count() {
        let mut app = App::with_devices(vec!["A".into(), "B".into()], 0);
        app.set_terminal_width(400);
        app.show_popup = true;

        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        );

        assert_eq!(app.bar_count, 32);
    }

    #[test]
    fn test_gain_keys_in_a_batch_send_only_the_final_gain() {
        let mut app = App::new();

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![
                key(KeyCode::Up),
                key(KeyCode::Up),
                key(KeyCode::Char('i')),
                key(KeyCode::Down),
            ]),
        );

        assert_eq!(app.gain, 1.2);
        assert_eq!(outcome.actions, vec![AppAction::SetGain(1.2)]);
    }

    #[test]
    fn test_gain_at_its_limit_sends_no_action() {
        let mut app = App::new();
        app.gain = crate::app::MAX_GAIN;

        let outcome = apply_batch(&mut app, batch_of(vec![key(KeyCode::Up)]));

        assert!(outcome.actions.is_empty());
    }
}
