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
pub const MAX_GAIN: f32 = 20.0;
const GAIN_STEP: f32 = 0.25;
/// How long a bar count or sensitivity notice stays on screen, in ticks (~1.5 s at 60 FPS).
const NOTICE_TICKS: u64 = 90;

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
    /// Vector containing normalized and smoothed audio bar magnitudes (0.0 to 1.0).
    pub audio_bars: Vec<f32>,
    /// Vector containing the target values most recently received from the FFT.
    pub target_bars: Vec<f32>,
    /// Sensitivity gain multiplier for visualization.
    pub gain: f32,
    /// Attack factor: fast and damped rising response.
    pub attack_factor: f32,
    /// Decay factor: gradual and fluid falling response.
    pub decay_factor: f32,
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
            audio_bars: vec![0.0; MIN_BARS],
            target_bars: vec![0.0; MIN_BARS],
            gain: 1.0,
            attack_factor: 0.65,
            decay_factor: 0.20,
            show_popup: false,
            devices: Vec::new(),
            selected_device_idx: 0,
            active_device_idx: 0,
            device_scroll_offset: 0,
            audio_error: None,
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
        self.apply_smoothing();
    }

    /// Applies smooth interpolation (Attack / Decay) to prevent abrupt visual jumps.
    pub fn apply_smoothing(&mut self) {
        for (current, &target) in self.audio_bars.iter_mut().zip(self.target_bars.iter()) {
            if target > *current {
                // Attack: fast interpolation towards the target
                *current += (target - *current) * self.attack_factor;
            } else {
                // Decay: gradual and elastic falling decay
                *current -= (*current - target) * self.decay_factor;
            }

            if *current < 0.005 {
                *current = 0.0;
            } else if *current > 1.0 {
                *current = 1.0;
            }
        }
    }

    /// Increases visualization sensitivity/gain.
    pub fn increase_gain(&mut self) {
        self.gain = (self.gain + GAIN_STEP).min(MAX_GAIN);
        self.gain_notice_until = self.tick_counter + NOTICE_TICKS;
    }

    /// Decreases visualization sensitivity/gain.
    pub fn decrease_gain(&mut self) {
        self.gain = (self.gain - GAIN_STEP).max(MIN_GAIN);
        self.gain_notice_until = self.tick_counter + NOTICE_TICKS;
    }

    /// The sensitivity to announce on screen right after the user changed it.
    pub fn gain_notice(&self) -> Option<f32> {
        (self.tick_counter < self.gain_notice_until).then_some(self.gain)
    }

    /// Returns audio bars with the current gain adjustment applied.
    pub fn displayed_bars(&self) -> Vec<f32> {
        self.audio_bars
            .iter()
            .map(|&amp| (amp * self.gain).clamp(0.0, 1.0))
            .collect()
    }
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

    fn app_with_width(width: u16) -> App {
        let mut app = App::default();
        app.set_terminal_width(width);
        app
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
        assert!(app.audio_bars[0] > 0.5 && app.audio_bars[0] < 1.0);

        let zero_bars = vec![0.0; 32];
        app.update_audio_bars(zero_bars);
        assert!(app.audio_bars[0] > 0.0);
    }

    #[test]
    fn test_app_gain_adjustment() {
        let mut app = App {
            audio_bars: vec![0.4; 32],
            ..App::default()
        };
        app.increase_gain();
        assert_eq!(app.gain, 1.25);
        assert_eq!(app.displayed_bars()[0], (0.4_f32 * 1.25).clamp(0.0, 1.0));
    }

    #[test]
    fn test_gain_goes_up_to_twenty_and_down_to_a_tenth() {
        let mut app = App::default();
        for _ in 0..200 {
            app.increase_gain();
        }
        assert_eq!(app.gain, MAX_GAIN);
        assert_eq!(MAX_GAIN, 20.0);

        for _ in 0..200 {
            app.decrease_gain();
        }
        assert_eq!(app.gain, MIN_GAIN);
    }

    #[test]
    fn test_gain_notice_shows_after_a_change_then_expires() {
        let mut app = App::default();
        assert_eq!(app.gain_notice(), None);

        app.increase_gain();
        assert_eq!(app.gain_notice(), Some(1.25));

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
        assert_eq!(app.gain_notice(), Some(1.25));
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
