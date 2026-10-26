use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{App, Overlay},
    color_picker::ColorPicker,
    device_menu::DeviceTab,
    event::{BatchEvent, EventBatch},
    menu::MenuRow,
};

/// Actions that require external handling (e.g., audio hardware), performed by `main`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppAction {
    /// Request to change the active captured audio device to the specified index.
    ChangeDevice(usize),
    /// Request to capture only the application at this row of the popup's application list.
    ChangeApp(usize),
    /// Refresh the list of applications playing audio.
    ListApps,
    /// Make the running capture produce this many bars.
    SetBarCount(usize),
    /// Make the running capture apply this manual sensitivity.
    SetGain(f32),
    /// Make the running capture smooth the treble bars with this strength (0.0 is off, 1.0 full).
    SetSmoothing(f32),
    /// Make the event loop tick (and so draw) this many times per second.
    SetFps(u32),
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
    let smoothing_before = app.smoothing_level;
    let fps_before = app.fps;

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

    if app.smoothing_level != smoothing_before {
        outcome
            .actions
            .push(AppAction::SetSmoothing(app.smoothing_strength()));
    }

    if app.fps != fps_before {
        outcome.actions.push(AppAction::SetFps(app.fps));
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
        KeyCode::Esc => app.color.close_picker(&mut app.palette, false),
        KeyCode::Char('c') | KeyCode::Char('C') => app.color.close_picker(&mut app.palette, false),
        KeyCode::Enter => app.color.close_picker(&mut app.palette, true),
        KeyCode::Tab => app
            .color
            .edit_picker(&mut app.palette, ColorPicker::next_focus),
        KeyCode::Up => app
            .color
            .edit_picker(&mut app.palette, |p| p.nudge(0, -1, fine)),
        KeyCode::Down => app
            .color
            .edit_picker(&mut app.palette, |p| p.nudge(0, 1, fine)),
        KeyCode::Left => app
            .color
            .edit_picker(&mut app.palette, |p| p.nudge(-1, 0, fine)),
        KeyCode::Right => app
            .color
            .edit_picker(&mut app.palette, |p| p.nudge(1, 0, fine)),
        _ => {}
    }
}

/// Handles a key while the main menu is open.
fn update_menu(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Char('c') | KeyCode::Char('C') if key_event.modifiers == KeyModifiers::CONTROL => {
            app.quit()
        }
        KeyCode::Esc => app.close_menu(),
        KeyCode::Up => app.select_prev_menu_row(),
        KeyCode::Down => app.select_next_menu_row(),
        KeyCode::Char('o') | KeyCode::Char('O') => app.open_device_popup(),
        KeyCode::Char('c') | KeyCode::Char('C') => app.open_color_popup(),
        KeyCode::Char('s') | KeyCode::Char('S') => app.toggle_smoothing(),
        KeyCode::Char('g') | KeyCode::Char('G') => app.toggle_bar_gap(),
        KeyCode::Char('m') | KeyCode::Char('M') => app.toggle_mirror(),
        KeyCode::Char('f') | KeyCode::Char('F') => app.change_fps(true),
        KeyCode::Char('r') | KeyCode::Char('R') => app.toggle_render_mode(),
        KeyCode::Enter => match app.menu_row {
            MenuRow::Device => app.open_device_popup(),
            MenuRow::Color => app.open_color_popup(),
            MenuRow::Smoothing => app.toggle_smoothing(),
            MenuRow::BarGap => app.toggle_bar_gap(),
            MenuRow::Mirror => app.toggle_mirror(),
            MenuRow::Fps => app.change_fps(true),
            MenuRow::Render => app.toggle_render_mode(),
            MenuRow::Sensitivity | MenuRow::Bars => {}
        },
        KeyCode::Left | KeyCode::Right => {
            let forward = key_event.code == KeyCode::Right;
            match (app.menu_row, forward) {
                (MenuRow::Sensitivity, true) => app.increase_gain(),
                (MenuRow::Sensitivity, false) => app.decrease_gain(),
                (MenuRow::Bars, true) => app.increase_bar_count(),
                (MenuRow::Bars, false) => app.decrease_bar_count(),
                (MenuRow::Smoothing, forward) => app.step_smoothing(forward),
                (MenuRow::BarGap, _) => app.toggle_bar_gap(),
                (MenuRow::Mirror, _) => app.toggle_mirror(),
                (MenuRow::Fps, forward) => app.change_fps(forward),
                (MenuRow::Render, _) => app.toggle_render_mode(),
                (MenuRow::Device | MenuRow::Color, _) => {}
            }
        }
        _ => {}
    }
}

pub fn update(app: &mut App, key_event: KeyEvent) -> Option<AppAction> {
    if app.overlay == Overlay::Menu {
        update_menu(app, key_event);
        return None;
    }

    // The color popup previews on the bars, so it only touches `App` and needs no action.
    if app.overlay == Overlay::Colors {
        match key_event.code {
            KeyCode::Char('c') | KeyCode::Char('C')
                if key_event.modifiers == KeyModifiers::CONTROL =>
            {
                app.quit();
            }
            // While the color picker is open it takes the keys, and Esc only closes the picker.
            _ if app.color.picker.is_some() => update_color_picker(app, key_event),
            KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => app.close_color_popup(),
            KeyCode::Enter => app.color.activate(&mut app.palette),
            KeyCode::Left => app.color.change_option(&mut app.palette, false),
            KeyCode::Right => app.color.change_option(&mut app.palette, true),
            KeyCode::Up => app.color.select_prev(&app.palette),
            KeyCode::Down => app.color.select_next(&app.palette),
            _ => {}
        }
        return None;
    }

    // If the device selection popup is open, handle navigation and confirmation
    if app.overlay == Overlay::Devices {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('o') | KeyCode::Char('O') => {
                app.close_device_popup();
            }
            KeyCode::Up => {
                app.select_prev_device();
            }
            KeyCode::Down => {
                app.select_next_device();
            }
            // Like the color popup's mode row, the mode is changed with the arrows.
            KeyCode::Left | KeyCode::Right if app.devices.mode_focused => {
                app.toggle_device_tab();
                if app.devices.tab == DeviceTab::Apps {
                    return Some(AppAction::ListApps);
                }
            }
            KeyCode::Enter if app.devices.mode_focused => {}
            KeyCode::Enter => {
                // Active index is updated only after the stream starts successfully.
                return match app.devices.tab {
                    DeviceTab::Outputs => {
                        app.close_device_popup();
                        Some(AppAction::ChangeDevice(app.devices.selected))
                    }
                    // With nothing playing there is nothing to pick; the popup stays open.
                    DeviceTab::Apps if app.devices.apps.names.is_empty() => None,
                    DeviceTab::Apps => {
                        app.close_device_popup();
                        Some(AppAction::ChangeApp(app.devices.apps.selected))
                    }
                };
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
        KeyCode::Esc => app.open_menu(),
        KeyCode::Char('o') | KeyCode::Char('O') => app.open_device_popup(),
        KeyCode::Char('s') => app.toggle_smoothing(),
        KeyCode::Char('g') | KeyCode::Char('G') => app.toggle_bar_gap(),
        KeyCode::Char('m') | KeyCode::Char('M') => app.toggle_mirror(),
        KeyCode::Char('f') | KeyCode::Char('F') => app.change_fps(true),
        KeyCode::Char('r') | KeyCode::Char('R') => app.toggle_render_mode(),
        KeyCode::Char('c') | KeyCode::Char('C') => app.open_color_popup(),
        KeyCode::Right => app.increase_bar_count(),
        KeyCode::Left => app.decrease_bar_count(),
        KeyCode::Up => app.increase_gain(),
        KeyCode::Down => app.decrease_gain(),
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
        render_mode::RenderMode,
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
        assert_eq!(app.motion.targets(), expected.motion.targets());
        assert_eq!(app.motion.bars(), expected.motion.bars());
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
        assert!(app.overlay == Overlay::Colors);

        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('q'));
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.smoothing_level, 0);
        assert!(!app.should_quit);
        assert_eq!(app.bar_count, 32);

        // Enter does nothing on the mode row; Right goes from single color through the
        // gradients to the rainbow, and Left goes back.
        assert_eq!(app.palette.mode, ColorMode::Single);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.palette.mode, ColorMode::Single);
        for expected in [
            ColorMode::Gradient,
            ColorMode::Vertical,
            ColorMode::Rainbow,
            ColorMode::Single,
        ] {
            press(&mut app, KeyCode::Right);
            assert_eq!(app.palette.mode, expected);
        }
        press(&mut app, KeyCode::Left);
        assert_eq!(app.palette.mode, ColorMode::Rainbow);
        assert_eq!(app.color.picker, None);
        assert_eq!(app.bar_count, 32);
    }

    fn open_picker_on_bass(app: &mut App) {
        press(app, KeyCode::Char('c'));
        press(app, KeyCode::Down);
        press(app, KeyCode::Enter);
        assert!(app.color.picker.is_some());
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
        assert!(app.overlay == Overlay::Colors);
        assert_eq!(app.smoothing_level, 0);

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.color.picker, None);
        assert!(app.overlay == Overlay::Colors);
        let kept = app.palette.bands[0];

        press(&mut app, KeyCode::Esc);
        assert!(app.overlay != Overlay::Colors);
        assert_eq!(app.palette.bands[0], kept);
    }

    #[test]
    fn test_tab_moves_between_square_and_hue_strip() {
        let mut app = App::new();
        open_picker_on_bass(&mut app);

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.color.picker.as_ref().unwrap().focus, PickerFocus::Hue);
        press(&mut app, KeyCode::Down);
        assert_ne!(app.palette.bands[0], BarColor::CYAN);

        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.color.picker.as_ref().unwrap().focus,
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
            assert!(app.overlay == Overlay::Colors);
            assert_eq!(app.color.picker, None);
            assert_eq!(app.palette, crate::palette::Palette::default());

            press(&mut app, closing);
            assert!(app.overlay != Overlay::Colors);
            assert!(app.overlay != Overlay::Devices);
        }
    }

    #[test]
    fn test_c_is_swallowed_by_the_device_popup_and_ctrl_c_quits_from_the_color_popup() {
        let mut app = App::new();
        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Char('c'));
        assert!(app.overlay != Overlay::Colors);

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
                key(KeyCode::Char('o')),
                key(KeyCode::Down),
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
        assert_eq!(app.smoothing_level, 0);

        let outcome = apply_batch(&mut app, batch_of(vec![key(KeyCode::Char('s'))]));
        assert_eq!(app.smoothing_level, crate::app::MAX_SMOOTHING_LEVEL);
        assert_eq!(outcome.actions, vec![AppAction::SetSmoothing(1.0)]);

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![key(KeyCode::Char('s')), key(KeyCode::Char('s'))]),
        );
        assert_eq!(app.smoothing_level, crate::app::MAX_SMOOTHING_LEVEL);
        assert!(outcome.actions.is_empty());

        let outcome = apply_batch(&mut app, batch_of(vec![key(KeyCode::Char('s'))]));
        assert_eq!(app.smoothing_level, 0);
        assert_eq!(outcome.actions, vec![AppAction::SetSmoothing(0.0)]);
    }

    #[test]
    fn test_esc_opens_and_closes_the_popup_and_q_quits() {
        let mut app = App::with_devices(vec!["A".into()], 0);

        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE),
        );
        assert!(app.overlay == Overlay::Devices && !app.should_quit);

        // `s` is swallowed while the popup is open.
        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        );
        assert!(app.overlay == Overlay::Devices && app.smoothing_level == 0);

        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.overlay != Overlay::Devices && !app.should_quit);

        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        );
        assert!(app.should_quit);
    }

    #[test]
    fn test_device_popup_opens_on_the_mode_row() {
        let mut app = App::with_devices(vec!["A".into(), "B".into(), "C".into()], 1);
        press(&mut app, KeyCode::Char('o'));
        assert!(app.devices.mode_focused);

        // Down reaches the active device, Up from the mode row the last one.
        press(&mut app, KeyCode::Down);
        assert_eq!((app.devices.mode_focused, app.devices.selected), (false, 1));
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        assert!(app.devices.mode_focused);
        press(&mut app, KeyCode::Up);
        assert_eq!((app.devices.mode_focused, app.devices.selected), (false, 2));
    }

    #[test]
    fn test_apps_tab_lists_applications_and_picks_one() {
        let mut app = App::with_devices(vec!["A".into()], 0);
        press(&mut app, KeyCode::Char('o'));
        assert_eq!(app.devices.tab, DeviceTab::Outputs);

        // The popup opens on the mode row, where the arrows change the mode.
        assert!(app.devices.mode_focused);
        let action = update(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!((action, app.devices.tab), (None, DeviceTab::Outputs));
        let action = update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(action, Some(AppAction::ListApps));
        assert_eq!(app.devices.tab, DeviceTab::Apps);

        // Nothing is playing: Enter keeps the popup open.
        let action = update(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!((action, app.overlay), (None, Overlay::Devices));

        app.devices
            .replace_apps(vec!["Spotify".into(), "Firefox".into()], None);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        let action = update(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(action, Some(AppAction::ChangeApp(1)));
        assert_eq!(app.overlay, Overlay::None);

        // The popup always opens on the outputs again.
        press(&mut app, KeyCode::Char('o'));
        assert_eq!(app.devices.tab, DeviceTab::Outputs);
    }

    #[test]
    fn test_esc_opens_the_menu_and_closes_it() {
        let mut app = App::with_devices(vec!["A".into()], 0);
        press(&mut app, KeyCode::Esc);
        assert!(app.overlay == Overlay::Menu && app.overlay != Overlay::Devices);

        // Keys of the closed screen are swallowed, `q` does not quit.
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.bar_count, 32);
        assert!(!app.should_quit);

        press(&mut app, KeyCode::Esc);
        assert!(app.overlay != Overlay::Menu);
    }

    #[test]
    fn test_menu_opens_submenus_and_comes_back() {
        let mut app = App::with_devices(vec!["A".into()], 0);
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Enter);
        assert!(app.overlay == Overlay::Devices && app.overlay != Overlay::Menu);
        press(&mut app, KeyCode::Esc);
        assert!(app.overlay != Overlay::Devices && app.overlay == Overlay::Menu);

        press(&mut app, KeyCode::Char('c'));
        assert!(app.overlay == Overlay::Colors && app.overlay != Overlay::Menu);
        press(&mut app, KeyCode::Esc);
        assert!(app.overlay != Overlay::Colors && app.overlay == Overlay::Menu);

        // Opened by the direct shortcut, a popup closes everything.
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Char('o'));
        assert!(app.overlay != Overlay::Devices && app.overlay != Overlay::Menu);
    }

    #[test]
    fn test_menu_edits_sensitivity_bars_and_smoothing() {
        let mut app = App::new();
        app.set_terminal_width(200);
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Right);
        assert_eq!(app.gain, 1.1);
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.gain, 0.9);

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Right);
        assert_eq!(app.bar_count, 40);

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.smoothing_level, crate::app::MAX_SMOOTHING_LEVEL);
        press(&mut app, KeyCode::Char('s'));
        assert_eq!(app.smoothing_level, 0);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Right);
        assert_eq!(app.smoothing_level, 2);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.smoothing_level, 1);
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.smoothing_level, 0);

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(!app.bar_gap);
        press(&mut app, KeyCode::Char('g'));
        assert!(app.bar_gap);

        press(&mut app, KeyCode::Down);
        assert_eq!(app.menu_row, MenuRow::Mirror);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.fps, 60);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.fps, 90);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.fps, 60);
        press(&mut app, KeyCode::Char('f'));
        assert_eq!(app.fps, 90);

        press(&mut app, KeyCode::Down);
        assert_eq!(app.menu_row, MenuRow::Render);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.menu_row, MenuRow::Device);
    }

    #[test]
    fn test_f_cycles_through_the_frame_rates_and_reports_each_change() {
        let mut app = App::new();
        let mut seen = Vec::new();
        for _ in 0..5 {
            let batch = batch_of(vec![key(KeyCode::Char('f'))]);
            let outcome = apply_batch(&mut app, batch);
            assert!(matches!(outcome.actions[0], AppAction::SetFps(_)));
            seen.push(app.fps);
        }
        assert_eq!(seen, [90, 120, 30, 60, 90]);
        assert_eq!(app.fps_notice(), Some(90));
    }

    #[test]
    fn test_m_toggles_the_mirror_from_the_visualizer_and_the_menu() {
        let mut app = App::new();
        assert!(app.mirror);
        press(&mut app, KeyCode::Char('m'));
        assert!(!app.mirror);
        assert_eq!(app.mirror_notice(), Some(false));

        press(&mut app, KeyCode::Esc);
        app.menu_row = MenuRow::Mirror;
        press(&mut app, KeyCode::Enter);
        assert!(app.mirror);
        press(&mut app, KeyCode::Right);
        assert!(!app.mirror);
        press(&mut app, KeyCode::Char('M'));
        assert!(app.mirror);
    }

    #[test]
    fn test_r_switches_the_render_mode_from_the_visualizer_and_the_menu() {
        let mut app = App::new();
        assert_eq!(app.render_mode, RenderMode::Blocks);
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(app.render_mode, RenderMode::Canvas);
        assert_eq!(app.render_notice(), Some(RenderMode::Canvas));

        press(&mut app, KeyCode::Esc);
        app.menu_row = MenuRow::Render;
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.render_mode, RenderMode::Blocks);
        press(&mut app, KeyCode::Right);
        assert_eq!(app.render_mode, RenderMode::Canvas);
        press(&mut app, KeyCode::Char('R'));
        assert_eq!(app.render_mode, RenderMode::Blocks);
    }

    #[test]
    fn test_g_toggles_the_bar_gap_and_more_bars_fit_without_it() {
        let mut app = App::new();
        app.set_terminal_width(100);
        assert!(app.bar_gap);
        assert_eq!(app.max_bar_count(), 50);
        press(&mut app, KeyCode::Char('g'));
        assert!(!app.bar_gap);
        assert_eq!(app.max_bar_count(), 100);
        assert_eq!(app.bar_gap_notice(), Some(false));
        press(&mut app, KeyCode::Char('g'));
        assert!(app.bar_gap);
    }

    #[test]
    fn test_batch_stops_at_quit() {
        let mut app = App::with_devices(vec!["A".into(), "B".into()], 0);
        app.overlay = Overlay::Devices;
        app.devices.selected = 1;

        let outcome = apply_batch(
            &mut app,
            batch_of(vec![
                Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
                key(KeyCode::Enter),
                Event::Tick,
            ]),
        );

        assert!(app.should_quit);
        assert!(
            app.overlay == Overlay::Devices,
            "keys after the quit must not run"
        );
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
                key(KeyCode::Right),
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
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.bar_count, 32);
        assert_eq!(app.gain, gain);
    }

    #[test]
    fn test_popup_open_does_not_change_bar_count() {
        let mut app = App::with_devices(vec!["A".into(), "B".into()], 0);
        app.set_terminal_width(400);
        app.overlay = Overlay::Devices;

        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

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
                key(KeyCode::Up),
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
