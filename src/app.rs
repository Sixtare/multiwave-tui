use crate::{
    color_picker::ColorPicker,
    palette::{BarColor, Palette},
};

/// Fewest bars the visualizer shows, even on a terminal too narrow to fit them.
pub const MIN_BARS: usize = 32;
/// Most bars the visualizer shows, however wide the terminal is.
pub const MAX_BARS: usize = 256;
/// Bars added or removed by each key press.
pub const BAR_STEP: usize = 8;
/// Terminal columns used by one bar: its glyph plus a spacer.
pub const COLUMNS_PER_BAR: usize = 2;
/// Lowest and highest sensitivity (gain) multiplier, and how much each key press changes it.
pub const MIN_GAIN: f32 = 0.1;
pub const MAX_GAIN: f32 = 5.0;
const GAIN_STEP: f32 = 0.10;
/// Bar response with smoothing off: fast on both ways, applied on every audio frame and tick.
const PLAIN_ATTACK_FACTOR: f32 = 0.65;
const PLAIN_DECAY_FACTOR: f32 = 0.20;
/// How long a bar count, sensitivity, smoothing or color notice stays on screen, in ticks (~1.5 s at
/// 60 FPS).
const NOTICE_TICKS: u64 = 90;

/// Number of rows in the main menu: output device, bar color, sensitivity, bars and smoothing.
pub const MENU_ROWS: usize = 5;

#[derive(Debug)]
pub struct App {
    /// Should the application exit?
    pub should_quit: bool,
    /// Tick counter.
    pub tick_counter: u64,
    /// Terminal width in columns, which limits how many bars fit.
    terminal_width: u16,
    /// Bar count the user asked for. It is kept when the terminal gets too narrow, so the bars
    /// grow back once there is room again.
    chosen_bar_count: usize,
    /// Bar count actually shown: the chosen count limited by what fits the terminal.
    pub bar_count: usize,
    /// Tick until which the bar count notice is shown.
    bar_notice_until: u64,
    /// Tick until which the sensitivity notice is shown.
    gain_notice_until: u64,
    /// Tick until which the smoothing notice is shown.
    smoothing_notice_until: u64,
    /// Tick until which the color notice is shown.
    color_notice_until: u64,
    /// Vector containing normalized and smoothed audio bar magnitudes (0.0 to 1.0).
    pub audio_bars: Vec<f32>,
    /// Vector containing the target values most recently received from the FFT.
    pub target_bars: Vec<f32>,
    /// Manual sensitivity multiplier, applied by the audio processor on top of its automatic
    /// sensitivity.
    pub gain: f32,
    /// Whether smoothing is on: slower, per-tick bar motion plus the audio processor's treble
    /// power averaging. Off by default.
    pub smoothing: bool,
    /// Attack factor with smoothing on: fast and damped rising response.
    pub attack_factor: f32,
    /// Decay factor with smoothing on: gradual and fluid falling response.
    pub decay_factor: f32,
    /// Whether to display the main menu that gathers every option.
    pub show_menu: bool,
    /// Highlighted row of the main menu (see [`MENU_ROWS`]).
    pub menu_row: usize,
    /// Whether the open popup was opened from the main menu, which then comes back when it closes.
    return_to_menu: bool,
    /// Whether to display the output device selection popup.
    pub show_popup: bool,
    /// List of available audio device names for display in the popup menu.
    pub devices: Vec<String>,
    /// Index of the currently highlighted device in the popup menu.
    pub selected_device_idx: usize,
    /// Index of the currently active audio device being captured.
    pub active_device_idx: usize,
    /// First device row visible in the popup when the list is scrollable.
    pub device_scroll_offset: usize,
    /// Most recent audio device or stream error, if any.
    pub audio_error: Option<String>,
    /// How the bars are colored. While the color popup is open it holds the live preview.
    pub palette: Palette,
    /// Whether to display the bar color popup.
    pub show_color_popup: bool,
    /// Highlighted row of the color popup (0 is the mode row).
    pub color_row: usize,
    /// Palette from before the color popup opened, to tell whether it changed.
    palette_at_open: Palette,
    /// The color picker, while it is open for the highlighted row.
    pub color_picker: Option<ColorPicker>,
    /// Color of the edited row from before the color picker opened, restored when it is cancelled.
    color_before_picker: BarColor,
}

impl Default for App {
    fn default() -> Self {
        Self {
            should_quit: false,
            tick_counter: 0,
            terminal_width: 0,
            chosen_bar_count: MIN_BARS,
            bar_count: MIN_BARS,
            bar_notice_until: 0,
            gain_notice_until: 0,
            smoothing_notice_until: 0,
            color_notice_until: 0,
            audio_bars: vec![0.0; MIN_BARS],
            target_bars: vec![0.0; MIN_BARS],
            gain: 1.0,
            smoothing: false,
            attack_factor: 0.40,
            decay_factor: 0.12,
            show_menu: false,
            menu_row: 0,
            return_to_menu: false,
            show_popup: false,
            devices: Vec::new(),
            selected_device_idx: 0,
            active_device_idx: 0,
            device_scroll_offset: 0,
            audio_error: None,
            palette: Palette::default(),
            show_color_popup: false,
            color_row: 0,
            palette_at_open: Palette::default(),
            color_picker: None,
            color_before_picker: BarColor::CYAN,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs a new instance of [`App`] with device names and an active index.
    pub fn with_devices(devices: Vec<String>, active_device_idx: usize) -> Self {
        Self {
            devices,
            selected_device_idx: active_device_idx,
            active_device_idx,
            ..Self::default()
        }
    }

    /// Executes an animation cycle on each tick (~60 FPS).
    pub fn tick(&mut self) {
        self.tick_counter += 1;
        // Gradually decay targets for natural sound fade-out
        for target in &mut self.target_bars {
            *target = (*target * 0.94).max(0.0);
        }
        self.apply_smoothing();
    }

    /// Sets should_quit to true to quit the application.
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Records an audio device or stream error for display in the UI.
    pub fn set_audio_error(&mut self, message: impl Into<String>) {
        self.audio_error = Some(message.into());
    }

    /// Clears the audio error after a valid output starts capturing.
    pub fn clear_audio_error(&mut self) {
        self.audio_error = None;
    }

    /// Most bars that fit the terminal width, never below [`MIN_BARS`] or above [`MAX_BARS`].
    pub fn max_bar_count(&self) -> usize {
        (self.terminal_width as usize / COLUMNS_PER_BAR).clamp(MIN_BARS, MAX_BARS)
    }

    /// Records the terminal width and shrinks or regrows the shown bars to fit it.
    pub fn set_terminal_width(&mut self, width: u16) {
        self.terminal_width = width;
        self.sync_bar_count();
    }

    /// Shows [`BAR_STEP`] more bars, up to what fits the terminal.
    pub fn increase_bar_count(&mut self) {
        let target = (self.bar_count + BAR_STEP).min(self.max_bar_count());
        self.set_chosen_bar_count(target);
    }

    /// Shows [`BAR_STEP`] fewer bars, down to [`MIN_BARS`].
    pub fn decrease_bar_count(&mut self) {
        let target = self.bar_count.saturating_sub(BAR_STEP).max(MIN_BARS);
        self.set_chosen_bar_count(target);
    }

    /// The bar count to announce on screen right after the user changed it.
    pub fn bar_notice(&self) -> Option<usize> {
        (self.tick_counter < self.bar_notice_until).then_some(self.bar_count)
    }

    fn set_chosen_bar_count(&mut self, target: usize) {
        // Pressing against a limit keeps the previous choice, so a count that is currently
        // capped by a narrow terminal is not forgotten.
        if target != self.bar_count {
            self.chosen_bar_count = target;
            self.sync_bar_count();
        }
        self.bar_notice_until = self.tick_counter + NOTICE_TICKS;
    }

    /// Recomputes the shown bar count and stretches the current bars to the new length, so the
    /// display does not blank out while the audio side catches up.
    fn sync_bar_count(&mut self) {
        let count = self.chosen_bar_count.clamp(MIN_BARS, self.max_bar_count());
        if count != self.bar_count {
            self.bar_count = count;
            self.audio_bars = resample(&self.audio_bars, count);
            self.target_bars = resample(&self.target_bars, count);
        }
    }

    /// Moves the popup selection cursor down.
    pub fn select_next_device(&mut self) {
        if !self.devices.is_empty() {
            self.selected_device_idx = (self.selected_device_idx + 1) % self.devices.len();
        }
    }

    /// Moves the popup selection cursor up.
    pub fn select_prev_device(&mut self) {
        if !self.devices.is_empty() {
            if self.selected_device_idx == 0 {
                self.selected_device_idx = self.devices.len() - 1;
            } else {
                self.selected_device_idx -= 1;
            }
        }
    }

    /// Scrolls the device popup so the selected row stays within `visible_rows`.
    pub fn ensure_device_visible(&mut self, visible_rows: usize) {
        if visible_rows == 0 || self.devices.is_empty() {
            return;
        }

        let selected = self.selected_device_idx.min(self.devices.len() - 1);
        let max_offset = self.devices.len().saturating_sub(visible_rows);

        if selected < self.device_scroll_offset {
            self.device_scroll_offset = selected;
        } else if selected >= self.device_scroll_offset.saturating_add(visible_rows) {
            self.device_scroll_offset = selected + 1 - visible_rows;
        }

        self.device_scroll_offset = self.device_scroll_offset.min(max_offset);
    }

    /// Updates audio bars received from the FFT and applies smoothing.
    pub fn update_audio_bars(&mut self, bars: Vec<f32>) {
        // A frame computed for a previous bar count can still be in flight after a change.
        if bars.len() != self.bar_count {
            return;
        }

        self.target_bars = bars;
        // With smoothing on the bars only move on ticks, so the motion stays continuous.
        if !self.smoothing {
            self.apply_smoothing();
        }
    }

    /// Moves every bar towards its target: quickly up (attack), more slowly down (decay).
    ///
    /// With smoothing on it runs once per tick only, so the motion is continuous and does not
    /// depend on the audio frame rate. Falling slower than rising is what keeps a bar that dips
    /// and rises again within a few frames from flickering, without holding it still. With
    /// smoothing off it also runs on every audio frame, with the faster plain factors.
    pub fn apply_smoothing(&mut self) {
        let (attack, decay) = if self.smoothing {
            (self.attack_factor, self.decay_factor)
        } else {
            (PLAIN_ATTACK_FACTOR, PLAIN_DECAY_FACTOR)
        };
        for (current, &target) in self.audio_bars.iter_mut().zip(&self.target_bars) {
            let factor = if target > *current { attack } else { decay };
            *current += (target - *current) * factor;

            if *current < 0.005 {
                *current = 0.0;
            } else if *current > 1.0 {
                *current = 1.0;
            }
        }
    }

    /// Increases visualization sensitivity/gain.
    pub fn increase_gain(&mut self) {
        self.gain = round_gain((self.gain + GAIN_STEP).min(MAX_GAIN));
        self.gain_notice_until = self.tick_counter + NOTICE_TICKS;
    }

    /// Decreases visualization sensitivity/gain.
    pub fn decrease_gain(&mut self) {
        self.gain = round_gain((self.gain - GAIN_STEP).max(MIN_GAIN));
        self.gain_notice_until = self.tick_counter + NOTICE_TICKS;
    }

    /// Turns smoothing on or off.
    pub fn toggle_smoothing(&mut self) {
        self.smoothing = !self.smoothing;
        self.smoothing_notice_until = self.tick_counter + NOTICE_TICKS;
    }

    /// The smoothing state to announce on screen right after the user toggled it.
    pub fn smoothing_notice(&self) -> Option<bool> {
        (self.tick_counter < self.smoothing_notice_until).then_some(self.smoothing)
    }

    /// Opens the main menu with the cursor on its first row.
    pub fn open_menu(&mut self) {
        self.show_menu = true;
        self.menu_row = 0;
    }

    /// Closes the main menu.
    pub fn close_menu(&mut self) {
        self.show_menu = false;
    }

    /// Moves the main menu cursor down, wrapping around.
    pub fn select_next_menu_row(&mut self) {
        self.menu_row = (self.menu_row + 1) % MENU_ROWS;
    }

    /// Moves the main menu cursor up, wrapping around.
    pub fn select_prev_menu_row(&mut self) {
        self.menu_row = (self.menu_row + MENU_ROWS - 1) % MENU_ROWS;
    }

    /// Opens the output device popup, remembering whether the main menu was open under it.
    pub fn open_device_popup(&mut self) {
        self.return_to_menu = self.show_menu;
        self.show_menu = false;
        self.show_popup = true;
    }

    /// Closes the output device popup, going back to the main menu if it was opened from there.
    pub fn close_device_popup(&mut self) {
        self.show_popup = false;
        self.restore_menu();
    }

    fn restore_menu(&mut self) {
        if self.return_to_menu {
            self.show_menu = true;
            self.return_to_menu = false;
        }
    }

    /// Opens the color popup.
    pub fn open_color_popup(&mut self) {
        self.return_to_menu = self.show_menu;
        self.show_menu = false;
        self.palette_at_open = self.palette;
        self.color_row = 0;
        self.color_picker = None;
        self.show_color_popup = true;
    }

    /// Closes the color popup, keeping every color chosen in it.
    pub fn close_color_popup(&mut self) {
        self.show_color_popup = false;
        self.restore_menu();
        self.color_picker = None;
        if self.palette != self.palette_at_open {
            self.color_notice_until = self.tick_counter + NOTICE_TICKS;
        }
    }

    /// Moves the color popup cursor down, skipping grayed-out rows.
    pub fn select_next_color_row(&mut self) {
        self.color_row = self.palette.next_row(self.color_row, true);
    }

    /// Moves the color popup cursor up, skipping grayed-out rows.
    pub fn select_prev_color_row(&mut self) {
        self.color_row = self.palette.next_row(self.color_row, false);
    }

    /// Switches the color mode to the next one, or the previous one, when the mode row is
    /// highlighted.
    pub fn change_color_mode(&mut self, forward: bool) {
        match self.color_row {
            0 => self.palette.change_mode(forward),
            Palette::MIRROR_ROW => self.palette.toggle_mirror_dimmed(),
            _ => {}
        }
    }

    /// Acts on the highlighted row: flips the mirror selector, or opens the color picker for a
    /// color row. The mode row is changed with the arrows instead.
    pub fn activate_color_row(&mut self) {
        match self.color_row {
            0 => {}
            Palette::MIRROR_ROW => self.palette.toggle_mirror_dimmed(),
            _ => {
                if let Some(color) = self.palette.row_color(self.color_row) {
                    self.color_before_picker = color;
                    self.color_picker = Some(ColorPicker::new(color));
                }
            }
        }
    }

    /// Lets `edit` change the open color picker, then previews its color on the bars.
    pub fn edit_color_picker(&mut self, edit: impl FnOnce(&mut ColorPicker)) {
        if let Some(picker) = &mut self.color_picker {
            edit(picker);
            self.palette.set_row_color(self.color_row, picker.color());
        }
    }

    /// Closes the color picker, keeping the previewed color when `confirm` and restoring otherwise.
    pub fn close_color_picker(&mut self, confirm: bool) {
        if self.color_picker.take().is_some() && !confirm {
            self.palette
                .set_row_color(self.color_row, self.color_before_picker);
        }
    }

    /// The color choice to announce on screen right after the user confirmed it.
    pub fn color_notice(&self) -> Option<String> {
        (self.tick_counter < self.color_notice_until).then(|| self.palette.describe())
    }

    /// The sensitivity to announce on screen right after the user changed it.
    pub fn gain_notice(&self) -> Option<f32> {
        (self.tick_counter < self.gain_notice_until).then_some(self.gain)
    }
}

/// Rounds the gain to two decimals, so repeated 0.10 steps don't pick up float noise
/// (1.2000000000000002 and the like) and the limits are reached exactly.
fn round_gain(gain: f32) -> f32 {
    (gain * 100.0).round() / 100.0
}

/// Stretches `bars` to `len` entries by picking the nearest source bar for each new one.
fn resample(bars: &[f32], len: usize) -> Vec<f32> {
    if bars.is_empty() {
        return vec![0.0; len];
    }
    (0..len).map(|i| bars[i * bars.len() / len]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::ColorMode;

    fn app_with_width(width: u16) -> App {
        let mut app = App::default();
        app.set_terminal_width(width);
        app
    }

    #[test]
    fn test_color_picker_previews_and_cancel_restores_the_color() {
        let mut app = App::new();
        app.open_color_popup();
        app.select_next_color_row();
        app.activate_color_row();
        assert_eq!(
            app.color_picker.as_ref().map(|p| p.color()),
            Some(BarColor::CYAN)
        );
        app.edit_color_picker(|picker| picker.nudge(-10, 5, false));
        assert_ne!(app.palette.bands[0], BarColor::CYAN);
        assert_eq!(
            app.palette.bands[0],
            app.color_picker.as_ref().unwrap().color()
        );
        app.close_color_picker(false);
        assert_eq!(app.palette, Palette::default());
        assert_eq!(app.color_picker, None);
    }

    #[test]
    fn test_color_picker_confirm_keeps_the_color_and_closing_shows_a_notice() {
        let mut app = App::new();
        app.open_color_popup();
        app.select_next_color_row();
        app.activate_color_row();
        app.edit_color_picker(|picker| {
            picker.next_focus();
            picker.nudge(0, 10, false);
        });
        let picked = app.color_picker.as_ref().unwrap().color();
        assert_ne!(picked, BarColor::CYAN);
        app.close_color_picker(true);
        assert_eq!(app.palette.bands[0], picked);
        assert_eq!(app.color_notice(), None);

        app.close_color_popup();
        assert_eq!(app.color_notice(), Some(picked.hex()));
        for _ in 0..90 {
            app.tick();
        }
        assert_eq!(app.color_notice(), None);
    }

    #[test]
    fn test_closing_the_color_popup_unchanged_shows_no_notice() {
        let mut app = App::new();
        app.open_color_popup();
        app.close_color_popup();
        assert_eq!(app.color_notice(), None);
    }

    #[test]
    fn test_mode_row_cycles_and_cursor_follows_the_selectable_rows() {
        let mut app = App::new();
        assert_eq!(app.palette.mode, ColorMode::Single);
        app.open_color_popup();
        app.select_prev_color_row();
        assert_eq!(app.color_row, 4);
        app.select_next_color_row();
        assert_eq!(app.color_row, 0);
        // Single color skips the mids and treble rows.
        app.select_next_color_row();
        app.select_next_color_row();
        assert_eq!(app.color_row, 4);
        app.select_next_color_row();

        // Enter does nothing on the mode row; the arrows change the mode.
        app.activate_color_row();
        assert_eq!(app.palette.mode, ColorMode::Single);
        app.change_color_mode(true);
        assert_eq!(app.palette.mode, ColorMode::Gradient);
        app.change_color_mode(false);
        assert_eq!(app.palette.mode, ColorMode::Single);
        app.change_color_mode(true);
        assert_eq!(app.color_picker, None);
        app.select_prev_color_row();
        assert_eq!(app.color_row, 4);
        app.select_prev_color_row();
        assert_eq!(app.color_row, 3);
    }

    #[test]
    fn test_changing_the_mode_only_works_on_the_mode_row() {
        let mut app = App::new();
        app.open_color_popup();
        app.select_next_color_row();
        app.change_color_mode(true);
        assert_eq!(app.palette.mode, ColorMode::Single);
    }

    #[test]
    fn test_mirror_row_toggles_the_checkbox_without_opening_the_list() {
        let mut app = App::new();
        app.open_color_popup();
        app.select_prev_color_row();
        app.activate_color_row();
        assert!(!app.palette.mirror_dimmed);
        assert_eq!(app.color_picker, None);
        app.close_color_popup();
        assert!(app.color_notice().is_some());
    }

    #[test]
    fn test_bar_count_starts_at_minimum() {
        let app = app_with_width(200);
        assert_eq!(app.bar_count, 32);
        assert_eq!(app.audio_bars.len(), 32);
    }

    #[test]
    fn test_bar_count_steps_by_eight_up_to_the_cap() {
        let mut app = app_with_width(1000);
        for expected in [40, 48, 56] {
            app.increase_bar_count();
            assert_eq!(app.bar_count, expected);
            assert_eq!(app.audio_bars.len(), expected);
            assert_eq!(app.target_bars.len(), expected);
        }

        for _ in 0..100 {
            app.increase_bar_count();
        }
        assert_eq!(app.bar_count, MAX_BARS);
    }

    #[test]
    fn test_bar_count_last_step_stops_at_what_fits() {
        // 200 columns fit 100 bars: 32, 40, ..., 96, then 100 rather than 104.
        let mut app = app_with_width(200);
        for _ in 0..20 {
            app.increase_bar_count();
        }
        assert_eq!(app.bar_count, 100);
        assert_eq!(app.max_bar_count(), 100);

        app.decrease_bar_count();
        assert_eq!(app.bar_count, 92);
    }

    #[test]
    fn test_bar_count_never_goes_below_minimum() {
        let mut app = app_with_width(200);
        app.decrease_bar_count();
        assert_eq!(app.bar_count, MIN_BARS);
    }

    #[test]
    fn test_bar_count_is_minimum_on_a_too_narrow_terminal() {
        let mut app = app_with_width(40);
        app.increase_bar_count();
        assert_eq!(app.bar_count, MIN_BARS);
        assert_eq!(app.max_bar_count(), MIN_BARS);
    }

    #[test]
    fn test_bar_count_shrinks_with_terminal_and_grows_back() {
        let mut app = app_with_width(400);
        for _ in 0..8 {
            app.increase_bar_count();
        }
        assert_eq!(app.bar_count, 96);

        app.set_terminal_width(120);
        assert_eq!(app.bar_count, 60);
        assert_eq!(app.audio_bars.len(), 60);

        app.set_terminal_width(400);
        assert_eq!(app.bar_count, 96);
        assert_eq!(app.audio_bars.len(), 96);
    }

    #[test]
    fn test_pressing_against_the_cap_keeps_the_chosen_count() {
        let mut app = app_with_width(400);
        for _ in 0..8 {
            app.increase_bar_count();
        }
        app.set_terminal_width(120);

        // 60 is the cap here; pressing → must not forget the 96 that was chosen.
        app.increase_bar_count();
        app.set_terminal_width(400);

        assert_eq!(app.bar_count, 96);
    }

    #[test]
    fn test_decreasing_from_a_capped_count_starts_from_what_is_shown() {
        let mut app = app_with_width(400);
        for _ in 0..8 {
            app.increase_bar_count();
        }
        app.set_terminal_width(120);

        app.decrease_bar_count();

        assert_eq!(app.bar_count, 52);
        app.set_terminal_width(400);
        assert_eq!(app.bar_count, 52);
    }

    #[test]
    fn test_changing_bar_count_stretches_current_bars() {
        let mut app = app_with_width(200);
        app.audio_bars = vec![0.5; 32];
        app.target_bars = vec![0.5; 32];

        app.increase_bar_count();

        assert_eq!(app.audio_bars, vec![0.5; 40]);
        assert_eq!(app.target_bars, vec![0.5; 40]);
    }

    #[test]
    fn test_stale_audio_frames_are_ignored_after_a_change() {
        let mut app = app_with_width(200);
        app.increase_bar_count();

        app.update_audio_bars(vec![1.0; 32]);

        assert_eq!(app.target_bars, vec![0.0; 40]);
        app.update_audio_bars(vec![1.0; 40]);
        assert_eq!(app.target_bars, vec![1.0; 40]);
    }

    #[test]
    fn test_bar_notice_shows_after_a_change_then_expires() {
        let mut app = app_with_width(200);
        assert_eq!(app.bar_notice(), None);

        app.increase_bar_count();
        assert_eq!(app.bar_notice(), Some(40));

        for _ in 0..NOTICE_TICKS {
            app.tick();
        }
        assert_eq!(app.bar_notice(), None);
    }

    #[test]
    fn test_bar_notice_also_shows_when_already_at_a_limit() {
        let mut app = app_with_width(200);
        app.decrease_bar_count();
        assert_eq!(app.bar_notice(), Some(MIN_BARS));
    }

    #[test]
    fn test_app_update_audio_bars_smoothing() {
        let mut app = App::default();
        assert_eq!(app.audio_bars[0], 0.0);

        let new_bars = vec![1.0; 32];
        app.update_audio_bars(new_bars);
        app.tick();
        let risen = app.audio_bars[0];
        assert!(risen > 0.2 && risen < 1.0);

        let zero_bars = vec![0.0; 32];
        app.update_audio_bars(zero_bars);
        app.tick();
        assert!(app.audio_bars[0] > 0.0 && app.audio_bars[0] < risen);
    }

    #[test]
    fn test_smoothing_is_off_by_default_and_bars_react_to_each_audio_frame() {
        let mut app = App::default();
        assert!(!app.smoothing);

        app.update_audio_bars(vec![1.0; 32]);

        // The plain response moves the bars right away, without waiting for a tick.
        assert!(app.audio_bars[0] > 0.6);
    }

    #[test]
    fn test_smoothing_notice_shows_after_a_toggle_then_expires() {
        let mut app = App::default();
        assert_eq!(app.smoothing_notice(), None);

        app.toggle_smoothing();
        assert_eq!(app.smoothing_notice(), Some(true));

        for _ in 0..NOTICE_TICKS {
            app.tick();
        }
        assert_eq!(app.smoothing_notice(), None);

        app.toggle_smoothing();
        assert_eq!(app.smoothing_notice(), Some(false));
    }

    #[test]
    fn test_bars_move_a_little_on_every_tick() {
        let mut app = App {
            smoothing: true,
            ..App::default()
        };
        app.update_audio_bars(vec![1.0; 32]);

        let mut previous = app.audio_bars[0];
        for _ in 0..5 {
            app.tick();
            assert!(app.audio_bars[0] > previous);
            previous = app.audio_bars[0];
        }
    }

    #[test]
    fn test_a_bar_falls_slower_than_it_rises() {
        let mut up = App {
            smoothing: true,
            ..App::default()
        };
        up.update_audio_bars(vec![1.0; 32]);
        up.tick();

        let mut down = App {
            smoothing: true,
            audio_bars: vec![1.0; 32],
            ..App::default()
        };
        down.update_audio_bars(vec![0.0; 32]);
        down.tick();

        assert!(1.0 - down.audio_bars[0] < up.audio_bars[0]);
    }

    #[test]
    fn test_a_brief_dip_barely_moves_the_bar() {
        let mut steady = App {
            smoothing: true,
            audio_bars: vec![0.8; 32],
            ..App::default()
        };
        let mut dipping = App {
            smoothing: true,
            audio_bars: vec![0.8; 32],
            ..App::default()
        };

        // A 0.8 level where one frame in three drops to 0.3.
        for tick in 0..30 {
            steady.update_audio_bars(vec![0.8; 32]);
            dipping.update_audio_bars(vec![if tick % 3 == 1 { 0.3 } else { 0.8 }; 32]);
            steady.tick();
            dipping.tick();
        }

        assert!((steady.audio_bars[0] - dipping.audio_bars[0]).abs() < 0.1);
    }

    #[test]
    fn test_app_gain_adjustment() {
        let mut app = App {
            audio_bars: vec![0.4; 32],
            ..App::default()
        };
        app.increase_gain();
        assert_eq!(app.gain, 1.1);
        // The gain reaches the bars through the audio processor, not through the app.
        assert_eq!(app.audio_bars[0], 0.4);
    }

    #[test]
    fn test_gain_goes_up_to_five_and_down_to_a_tenth() {
        let mut app = App::default();
        for _ in 0..200 {
            app.increase_gain();
        }
        assert_eq!(app.gain, MAX_GAIN);
        assert_eq!(MAX_GAIN, 5.0);

        for _ in 0..200 {
            app.decrease_gain();
        }
        assert_eq!(app.gain, MIN_GAIN);
    }

    #[test]
    fn test_gain_moves_in_steps_of_a_tenth_without_drift() {
        let mut app = App::default();
        for expected in [1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9, 2.0] {
            app.increase_gain();
            assert_eq!(app.gain, expected);
        }

        for expected in [1.9, 1.8, 1.7] {
            app.decrease_gain();
            assert_eq!(app.gain, expected);
        }
    }

    #[test]
    fn test_gain_reaches_both_limits_exactly() {
        let mut app = App::default();
        for _ in 0..40 {
            app.increase_gain();
        }
        assert_eq!(app.gain, MAX_GAIN);

        for _ in 0..49 {
            app.decrease_gain();
        }
        assert_eq!(app.gain, MIN_GAIN);
    }

    #[test]
    fn test_gain_notice_shows_after_a_change_then_expires() {
        let mut app = App::default();
        assert_eq!(app.gain_notice(), None);

        app.increase_gain();
        assert_eq!(app.gain_notice(), Some(1.1));

        for _ in 0..NOTICE_TICKS {
            app.tick();
        }
        assert_eq!(app.gain_notice(), None);
    }

    #[test]
    fn test_gain_notice_also_shows_when_already_at_a_limit() {
        let mut app = App {
            gain: MAX_GAIN,
            ..App::default()
        };
        app.increase_gain();
        assert_eq!(app.gain_notice(), Some(MAX_GAIN));
    }

    #[test]
    fn test_gain_and_bar_notices_are_independent() {
        let mut app = App::default();
        app.set_terminal_width(200);

        app.increase_gain();
        assert_eq!(app.bar_notice(), None);

        app.increase_bar_count();
        assert_eq!(app.bar_notice(), Some(40));
        assert_eq!(app.gain_notice(), Some(1.1));
    }

    #[test]
    fn test_audio_error_clears_after_success() {
        let mut app = App::default();
        app.set_audio_error("Unsupported sample format: I32");
        assert_eq!(
            app.audio_error.as_deref(),
            Some("Unsupported sample format: I32")
        );

        app.clear_audio_error();
        assert!(app.audio_error.is_none());
    }

    #[test]
    fn test_device_popup_scroll_keeps_selection_visible() {
        let devices = (0..10).map(|i| format!("Device {i}")).collect();
        let mut app = App::with_devices(devices, 0);

        app.ensure_device_visible(4);
        assert_eq!(app.device_scroll_offset, 0);

        app.selected_device_idx = 5;
        app.ensure_device_visible(4);
        assert_eq!(app.device_scroll_offset, 2);

        app.selected_device_idx = 0;
        app.ensure_device_visible(4);
        assert_eq!(app.device_scroll_offset, 0);

        app.selected_device_idx = 9;
        app.ensure_device_visible(4);
        assert_eq!(app.device_scroll_offset, 6);
    }

    #[test]
    fn test_app_device_navigation() {
        let devices = vec![
            "Default Device".to_string(),
            "Headphones".to_string(),
            "HDMI Audio".to_string(),
        ];
        let mut app = App::with_devices(devices, 0);
        assert_eq!(app.selected_device_idx, 0);

        app.select_next_device();
        assert_eq!(app.selected_device_idx, 1);

        app.select_next_device();
        assert_eq!(app.selected_device_idx, 2);

        // Wrap-around forward
        app.select_next_device();
        assert_eq!(app.selected_device_idx, 0);

        // Wrap-around backward
        app.select_prev_device();
        assert_eq!(app.selected_device_idx, 2);
    }
}
