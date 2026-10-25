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
/// Terminal columns used by one bar with the gap on: its glyph plus a spacer.
pub const COLUMNS_PER_BAR: usize = 2;
/// Terminal columns used by one bar with the gap off: just its glyph, so neighbours touch.
pub const COLUMNS_PER_BAR_TIGHT: usize = 1;
/// Lowest and highest sensitivity (gain) multiplier, and how much each key press changes it.
pub const MIN_GAIN: f32 = 0.1;
pub const MAX_GAIN: f32 = 5.0;
const GAIN_STEP: f32 = 0.10;
/// Bar response with smoothing off: fast on both ways, applied on every audio frame and tick.
const PLAIN_ATTACK_FACTOR: f32 = 0.65;
const PLAIN_DECAY_FACTOR: f32 = 0.20;
/// How long a bar count, sensitivity, smoothing or color notice stays on screen, in ticks (~1.5 s at
/// 60 FPS; other frame rates scale it, see [`App::notice_ticks`]).
const NOTICE_TICKS: u64 = 90;
/// The frame rates the user can pick, and the one the animation constants were tuned at.
pub const FPS_OPTIONS: [u32; 4] = [30, 60, 90, 120];
pub const DEFAULT_FPS: u32 = 60;

/// Number of rows in the main menu: output device, bar color, sensitivity, bars, smoothing, bar
/// gap and frame rate.
pub const MENU_ROWS: usize = 7;

/// The one overlay drawn over the visualizer, which also takes the keys. At most one is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overlay {
    #[default]
    None,
    /// The main menu that gathers every option.
    Menu,
    /// The output device selection popup.
    Devices,
    /// The bar color popup (and its color picker, see [`App::color_picker`]).
    Colors,
}

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
    /// Tick until which the bar gap notice is shown.
    gap_notice_until: u64,
    /// Tick until which the frame rate notice is shown.
    fps_notice_until: u64,
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
    /// Whether a blank column separates neighbouring bars. On by default; off packs the bars
    /// against each other.
    pub bar_gap: bool,
    /// Frames (ticks) per second, one of [`FPS_OPTIONS`].
    pub fps: u32,
    /// Attack factor with smoothing on: fast and damped rising response.
    pub attack_factor: f32,
    /// Decay factor with smoothing on: gradual and fluid falling response.
    pub decay_factor: f32,
    /// The overlay that is open, if any.
    pub overlay: Overlay,
    /// Highlighted row of the main menu (see [`MENU_ROWS`]).
    pub menu_row: usize,
    /// Whether the open popup was opened from the main menu, which then comes back when it closes.
    return_to_menu: bool,
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
            gap_notice_until: 0,
            fps_notice_until: 0,
            color_notice_until: 0,
            audio_bars: vec![0.0; MIN_BARS],
            target_bars: vec![0.0; MIN_BARS],
            gain: 1.0,
            smoothing: false,
            bar_gap: true,
            fps: DEFAULT_FPS,
            attack_factor: 0.40,
            decay_factor: 0.12,
            overlay: Overlay::None,
            menu_row: 0,
            return_to_menu: false,
            devices: Vec::new(),
            selected_device_idx: 0,
            active_device_idx: 0,
            device_scroll_offset: 0,
            audio_error: None,
            palette: Palette::default(),
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

    /// Executes an animation cycle on each tick (`fps` times per second).
    ///
    /// The per-tick factors are tuned at 60 FPS, so at other rates they are rescaled to give the
    /// same response per second.
    pub fn tick(&mut self) {
        self.tick_counter += 1;
        let steps = DEFAULT_FPS as f32 / self.fps as f32;
        // Gradually decay targets for natural sound fade-out
        let target_decay = 0.94f32.powf(steps);
        for target in &mut self.target_bars {
            *target = (*target * target_decay).max(0.0);
        }
        self.apply_response(steps);
    }

    /// How many ticks a notice stays on screen, so it lasts the same time at every frame rate.
    fn notice_ticks(&self) -> u64 {
        NOTICE_TICKS * self.fps as u64 / DEFAULT_FPS as u64
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

    /// Terminal columns one bar takes, which depends on whether the gap between bars is on.
    pub fn columns_per_bar(&self) -> usize {
        if self.bar_gap {
            COLUMNS_PER_BAR
        } else {
            COLUMNS_PER_BAR_TIGHT
        }
    }

    /// Most bars that fit the terminal width, never below [`MIN_BARS`] or above [`MAX_BARS`].
    pub fn max_bar_count(&self) -> usize {
        (self.terminal_width as usize / self.columns_per_bar()).clamp(MIN_BARS, MAX_BARS)
    }

    /// Records the terminal width and shrinks or regrows the shown bars to fit it.
    pub fn set_terminal_width(&mut self, width: u16) {
        self.terminal_width = width;
        self.sync_bar_count();
    }

    /// Shows more bars: the next multiple of [`BAR_STEP`], or what fits the terminal when that
    /// is less, so the row can fill the width even when the width is not a multiple of the step.
    pub fn increase_bar_count(&mut self) {
        let next_multiple = (self.bar_count / BAR_STEP + 1) * BAR_STEP;
        let target = next_multiple.min(self.max_bar_count());
        self.set_chosen_bar_count(target);
    }

    /// Shows fewer bars: the previous multiple of [`BAR_STEP`], down to [`MIN_BARS`]. From a count
    /// that is not a multiple (the terminal's full width) it lands on the multiple below it.
    pub fn decrease_bar_count(&mut self) {
        let previous_multiple = (self.bar_count - 1) / BAR_STEP * BAR_STEP;
        let target = previous_multiple.max(MIN_BARS);
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
        self.bar_notice_until = self.tick_counter + self.notice_ticks();
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
        self.apply_response(1.0);
    }

    /// Moves the bars towards their targets as `steps` 60 FPS ticks would (1.0 for one).
    fn apply_response(&mut self, steps: f32) {
        let (attack, decay) = if self.smoothing {
            (self.attack_factor, self.decay_factor)
        } else {
            (PLAIN_ATTACK_FACTOR, PLAIN_DECAY_FACTOR)
        };
        let (attack, decay) = if steps == 1.0 {
            (attack, decay)
        } else {
            (
                1.0 - (1.0 - attack).powf(steps),
                1.0 - (1.0 - decay).powf(steps),
            )
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
        self.gain_notice_until = self.tick_counter + self.notice_ticks();
    }

    /// Decreases visualization sensitivity/gain.
    pub fn decrease_gain(&mut self) {
        self.gain = round_gain((self.gain - GAIN_STEP).max(MIN_GAIN));
        self.gain_notice_until = self.tick_counter + self.notice_ticks();
    }

    /// Turns smoothing on or off.
    pub fn toggle_smoothing(&mut self) {
        self.smoothing = !self.smoothing;
        self.smoothing_notice_until = self.tick_counter + self.notice_ticks();
    }

    /// The smoothing state to announce on screen right after the user toggled it.
    pub fn smoothing_notice(&self) -> Option<bool> {
        (self.tick_counter < self.smoothing_notice_until).then_some(self.smoothing)
    }

    /// Turns the gap between bars on or off. Bars take fewer columns without it, so more of the
    /// chosen count can fit.
    pub fn toggle_bar_gap(&mut self) {
        self.bar_gap = !self.bar_gap;
        self.gap_notice_until = self.tick_counter + self.notice_ticks();
        self.sync_bar_count();
    }

    /// The bar gap state to announce on screen right after the user toggled it.
    pub fn bar_gap_notice(&self) -> Option<bool> {
        (self.tick_counter < self.gap_notice_until).then_some(self.bar_gap)
    }

    /// Picks the next (or previous) frame rate of [`FPS_OPTIONS`], wrapping around.
    pub fn change_fps(&mut self, forward: bool) {
        let len = FPS_OPTIONS.len();
        let current = FPS_OPTIONS
            .iter()
            .position(|&fps| fps == self.fps)
            .unwrap_or(1);
        let next = if forward {
            current + 1
        } else {
            current + len - 1
        } % len;
        self.fps = FPS_OPTIONS[next];
        self.fps_notice_until = self.tick_counter + self.notice_ticks();
    }

    /// The frame rate to announce on screen right after the user changed it.
    pub fn fps_notice(&self) -> Option<u32> {
        (self.tick_counter < self.fps_notice_until).then_some(self.fps)
    }

    /// Opens the main menu with the cursor on its first row.
    pub fn open_menu(&mut self) {
        self.overlay = Overlay::Menu;
        self.menu_row = 0;
    }

    /// Closes the main menu.
    pub fn close_menu(&mut self) {
        self.overlay = Overlay::None;
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
        self.return_to_menu = self.overlay == Overlay::Menu;
        self.overlay = Overlay::Devices;
    }

    /// Closes the output device popup, going back to the main menu if it was opened from there.
    pub fn close_device_popup(&mut self) {
        self.overlay = Overlay::None;
        self.restore_menu();
    }

    fn restore_menu(&mut self) {
        if self.return_to_menu {
            self.overlay = Overlay::Menu;
            self.return_to_menu = false;
        }
    }

    /// Opens the color popup.
    pub fn open_color_popup(&mut self) {
        self.return_to_menu = self.overlay == Overlay::Menu;
        self.palette_at_open = self.palette;
        self.color_row = 0;
        self.color_picker = None;
        self.overlay = Overlay::Colors;
    }

    /// Closes the color popup, keeping every color chosen in it.
    pub fn close_color_popup(&mut self) {
        self.overlay = Overlay::None;
        self.restore_menu();
        self.color_picker = None;
        if self.palette != self.palette_at_open {
            self.color_notice_until = self.tick_counter + self.notice_ticks();
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

    /// Steps the value of the highlighted row with the arrows: the color mode to the next or
    /// previous one on the mode row, fade on or off on the fade row, its intensity on the level row. Other rows ignore it.
    pub fn change_color_option(&mut self, forward: bool) {
        match self.color_row {
            0 => self.palette.change_mode(forward),
            Palette::FADE_ROW => self.palette.toggle_fade(),
            Palette::FADE_LEVEL_ROW => self.palette.step_fade_level(forward),
            _ => {}
        }
    }

    /// Acts on the highlighted row: flips the mirror checkbox, or opens the color picker for a color
    /// row. The mode, fade and level rows are changed with the arrows instead.
    pub fn activate_color_row(&mut self) {
        match self.color_row {
            0 | Palette::FADE_ROW | Palette::FADE_LEVEL_ROW => {}
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
        assert_eq!(app.color_row, 6);
        app.select_next_color_row();
        assert_eq!(app.color_row, 0);
        // Single color skips the mids and treble rows.
        app.select_next_color_row();
        app.select_next_color_row();
        assert_eq!(app.color_row, 4);
        app.select_next_color_row();
        assert_eq!(app.color_row, 5);
        app.select_next_color_row();
        assert_eq!(app.color_row, 6);
        app.select_next_color_row();

        // Enter does nothing on the mode row; the arrows change the mode.
        app.activate_color_row();
        assert_eq!(app.palette.mode, ColorMode::Single);
        app.change_color_option(true);
        assert_eq!(app.palette.mode, ColorMode::Gradient);
        app.change_color_option(false);
        assert_eq!(app.palette.mode, ColorMode::Single);
        app.change_color_option(true);
        assert_eq!(app.color_picker, None);
        app.select_prev_color_row();
        assert_eq!(app.color_row, 6);
        app.select_prev_color_row();
        assert_eq!(app.color_row, 5);
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
        app.change_color_option(true);
        assert_eq!(app.palette.mode, ColorMode::Single);
    }

    #[test]
    fn test_fade_row_is_switched_with_the_arrows() {
        let mut app = App::new();
        app.open_color_popup();
        app.select_prev_color_row();
        app.select_prev_color_row();
        app.select_prev_color_row();
        assert_eq!(app.color_row, Palette::FADE_ROW);
        assert!(app.palette.fade);
        // Enter does nothing; the arrows switch it between on and off.
        app.activate_color_row();
        assert!(app.palette.fade);
        app.change_color_option(false);
        assert!(!app.palette.fade);
        app.change_color_option(true);
        assert!(app.palette.fade);
        assert_eq!(app.color_picker, None);

        // The level row steps the intensity by a tenth, and is skipped while the fade is off.
        app.select_next_color_row();
        assert_eq!(app.color_row, Palette::FADE_LEVEL_ROW);
        app.change_color_option(true);
        assert_eq!(app.palette.fade_level, 6);
        app.change_color_option(false);
        app.change_color_option(false);
        assert_eq!(app.palette.fade_level, 4);
        app.select_prev_color_row();
        app.change_color_option(false);
        assert!(!app.palette.fade);
        app.select_next_color_row();
        assert_eq!(app.color_row, Palette::MIRROR_ROW);
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

        // From the full width it goes back to the multiple of 8 below, not 100 - 8.
        app.decrease_bar_count();
        assert_eq!(app.bar_count, 96);
        app.increase_bar_count();
        assert_eq!(app.bar_count, 100);
    }

    #[test]
    fn test_increasing_from_a_non_multiple_goes_to_the_next_multiple() {
        // 36 bars fit now; after the terminal widens the next step is 40, not 44.
        let mut app = app_with_width(72);
        app.increase_bar_count();
        assert_eq!(app.bar_count, 36);
        app.set_terminal_width(400);
        app.increase_bar_count();
        assert_eq!(app.bar_count, 40);
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

        assert_eq!(app.bar_count, 56);
        app.set_terminal_width(400);
        assert_eq!(app.bar_count, 56);
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

    #[test]
    fn test_fps_steps_through_the_options_and_wraps() {
        let mut app = App::new();
        assert_eq!(app.fps, 60);
        let mut seen = Vec::new();
        for _ in 0..4 {
            app.change_fps(true);
            seen.push(app.fps);
        }
        assert_eq!(seen, [90, 120, 30, 60]);
        app.change_fps(false);
        assert_eq!(app.fps, 30);
    }

    #[test]
    fn test_bars_fall_equally_fast_per_second_at_every_fps() {
        let fall_after_one_second = |fps: u32| {
            let mut app = App::new();
            app.fps = fps;
            app.audio_bars = vec![1.0; 32];
            for _ in 0..fps {
                app.tick();
            }
            app.audio_bars[0]
        };
        let at_60 = fall_after_one_second(60);
        for fps in [30, 90, 120] {
            let at = fall_after_one_second(fps);
            assert!((at - at_60).abs() < 0.01, "{fps} fps: {at} vs {at_60}");
        }
    }

    #[test]
    fn test_notices_last_the_same_time_at_every_fps() {
        for fps in FPS_OPTIONS {
            let mut app = App::new();
            app.fps = fps;
            app.toggle_bar_gap();
            for _ in 0..(fps * 3 / 4) {
                app.tick();
            }
            assert!(app.bar_gap_notice().is_some(), "{fps} fps: gone too early");
            for _ in 0..(fps * 3 / 4 + 1) {
                app.tick();
            }
            assert!(app.bar_gap_notice().is_none(), "{fps} fps: still shown");
        }
    }
}
