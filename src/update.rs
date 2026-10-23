use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::App,
    color_picker::ColorPicker,
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
    // A key press is drawn right away instead of waiting for the next tick, so the UI answers
    // input at once. Still one draw per batch, and never more than the batch had work for.
    let has_key = batch
        .events
        .iter()
        .any(|event| matches!(event, BatchEvent::Key(_)));
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
    }
    outcome.draw = has_tick || has_key;

    outcome
}

/// Handles a key while the color picker is open.
fn update_color_picker(app: &mut App, key_event: KeyEvent) {
    let fine = key_event.modifiers.contains(KeyModifiers::SHIFT);
    match key_event.code {
        KeyCode::Esc => app.close_color_picker(false),
        KeyCode::Char('c') | KeyCode::Char('C') => app.close_color_picker(false),
        KeyCode::Enter => app.close_color_picker(true),
        KeyCode::Tab => app.edit_color_picker(ColorPicker::next_focus),
        KeyCode::Up | KeyCode::Char('k') => app.edit_color_picker(|p| p.nudge(0, -1, fine)),
        KeyCode::Down | KeyCode::Char('j') => app.edit_color_picker(|p| p.nudge(0, 1, fine)),
        KeyCode::Left => app.edit_color_picker(|p| p.nudge(-1, 0, fine)),
        KeyCode::Right => app.edit_color_picker(|p| p.nudge(1, 0, fine)),
        _ => {}
    }
}

pub fn update(app: &mut App, key_event: KeyEvent) -> Option<AppAction> {
    // The color popup previews on the bars, so it only touches `App` and needs no action.
    if app.show_color_popup {
        match key_event.code {
            KeyCode::Char('c') | KeyCode::Char('C')
                if key_event.modifiers == KeyModifiers::CONTROL =>
            {
                app.quit();
            }
            // While the color picker is open it takes the keys, and Esc only closes the picker.
            _ if app.color_picker.is_some() => update_color_picker(app, key_event),
            KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => app.close_color_popup(),
            KeyCode::Enter => app.activate_color_row(),
            KeyCode::Up | KeyCode::Char('k') => app.select_prev_color_row(),
            KeyCode::Down | KeyCode::Char('j') => app.select_next_color_row(),
            _ => {}
        }
        return None;
    }

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
        KeyCode::Char('c') | KeyCode::Char('C') => app.open_color_popup(),
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
    use crate::{
        color_picker::PickerFocus,
        event::Event,
        palette::{BarColor, ColorMode},
    };

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
    fn test_batch_with_a_key_draws_without_waiting_for_a_tick() {
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
                draw: true,
            }
        );
    }

    #[test]
    fn test_audio_only_batch_without_tick_does_not_draw() {
        let mut app = App::new();
        app.set_terminal_width(200);

        let outcome = apply_batch(&mut app, batch_of(vec![Event::AudioBars(vec![0.5; 32])]));

        assert_eq!(outcome, BatchOutcome::default());
    }

    fn press(app: &mut App, code: KeyCode) {
        update(app, KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn test_c_opens_the_color_popup_and_swallows_other_keys() {
        let mut app = App::new();
        app.set_terminal_width(200);
        press(&mut app, KeyCode::Char('c'));
        assert!(app.show_color_popup);

        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('q'));
        press(&mut app, KeyCode::Right);
        assert!(!app.smoothing);
        assert!(!app.should_quit);
        assert_eq!(app.bar_count, 32);

        // Enter on the mode row goes from single color through gradient to bands.
        assert_eq!(app.palette.mode, ColorMode::Single);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.palette.mode, ColorMode::Gradient);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.palette.mode, ColorMode::Bands);
        assert_eq!(app.color_picker, None);
    }

    fn open_picker_on_bass(app: &mut App) {
        press(app, KeyCode::Char('c'));
        press(app, KeyCode::Down);
        press(app, KeyCode::Enter);
        assert!(app.color_picker.is_some());
    }

    #[test]
    fn test_enter_opens_the_picker_and_arrows_preview_the_color() {
        let mut app = App::new();
        open_picker_on_bass(&mut app);

        press(&mut app, KeyCode::Left);
        let after_left = app.palette.bands[0];
        assert_ne!(after_left, BarColor::CYAN);
        // Shift moves in fine steps, smaller than the plain press.
        let after_plain = app.palette.bands[0];
        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        let fine = app.palette.bands[0];
        assert_ne!(fine, after_plain);
        assert!(fine.r.abs_diff(after_plain.r) < after_plain.r.abs_diff(BarColor::CYAN.r));
        assert!(app.show_color_popup);
        assert!(!app.smoothing);

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.color_picker, None);
        assert!(app.show_color_popup);
        let kept = app.palette.bands[0];

        press(&mut app, KeyCode::Esc);
        assert!(!app.show_color_popup);
        assert_eq!(app.palette.bands[0], kept);
    }

    #[test]
    fn test_tab_moves_between_square_and_hue_strip() {
        let mut app = App::new();
        open_picker_on_bass(&mut app);

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.color_picker.as_ref().unwrap().focus, PickerFocus::Hue);
        press(&mut app, KeyCode::Down);
        assert_ne!(app.palette.bands[0], BarColor::CYAN);

        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.color_picker.as_ref().unwrap().focus,
            PickerFocus::Square
        );
    }

    #[test]
    fn test_esc_and_c_close_only_the_picker_first() {
        for closing in [KeyCode::Esc, KeyCode::Char('c')] {
            let mut app = App::new();
            open_picker_on_bass(&mut app);
            press(&mut app, KeyCode::Down);
            assert_ne!(app.palette.bands[0], BarColor::CYAN);
            press(&mut app, closing);
            assert!(app.show_color_popup);
            assert_eq!(app.color_picker, None);
            assert_eq!(app.palette, crate::palette::Palette::default());

            press(&mut app, closing);
            assert!(!app.show_color_popup);
            assert!(!app.show_popup);
        }
    }

    #[test]
    fn test_c_is_swallowed_by_the_device_popup_and_ctrl_c_quits_from_the_color_popup() {
        let mut app = App::new();
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('c'));
        assert!(!app.show_color_popup);

        let mut app = App::new();
        press(&mut app, KeyCode::Char('c'));
        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert!(app.should_quit);
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
