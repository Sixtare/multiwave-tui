use crate::{
    bar_motion::BarMotion,
    color_session::ColorSession,
    config::Config,
    device_menu::{DeviceMenu, DeviceTab},
    menu::MenuRow,
    notices::{NoticeKind, Notices},
    palette::{MAX_FADE_LEVEL, Palette},
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
/// Highest smoothing level, in tenths (1.0); level 0 is smoothing off.
pub const MAX_SMOOTHING_LEVEL: u8 = 10;
/// The frame rates the user can pick, and the one the animation constants were tuned at.
pub const FPS_OPTIONS: [u32; 4] = [30, 60, 90, 120];
pub const DEFAULT_FPS: u32 = 60;

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
    /// Which notices are on screen.
    notices: Notices,
    /// The animated bar heights (0.0 to 1.0) and the targets most recently received from the FFT.
    pub motion: BarMotion,
    /// Manual sensitivity multiplier, applied by the audio processor on top of its automatic
    /// sensitivity.
    pub gain: f32,
    /// Smoothing level in tenths (0 to [`MAX_SMOOTHING_LEVEL`]): slower, per-tick bar motion plus
    /// the audio processor's treble power averaging, both stronger the higher the level. 0 is
    /// off, the default.
    pub smoothing_level: u8,
    /// Last non-zero smoothing level, which the shortcut brings back after switching it off.
    last_smoothing_level: u8,
    /// Whether a blank column separates neighbouring bars. On by default; off packs the bars
    /// against each other.
    pub bar_gap: bool,
    /// Whether the line between the bars and their reflection is drawn. On by default.
    pub divider: bool,
    /// Whether the reflection below the divider line is drawn. On by default.
    pub mirror: bool,
    /// Frames (ticks) per second, one of [`FPS_OPTIONS`].
    pub fps: u32,
    /// Attack factor at full smoothing: fast and damped rising response.
    pub attack_factor: f32,
    /// Decay factor at full smoothing: gradual and fluid falling response.
    pub decay_factor: f32,
    /// The overlay that is open, if any.
    pub overlay: Overlay,
    /// Highlighted row of the main menu.
    pub menu_row: MenuRow,
    /// Tick at which the menu cursor reached its current row, where the device name carousel
    /// restarts.
    menu_row_since: u64,
    /// Same for the highlighted row of the device popup.
    device_row_since: u64,
    /// Whether the open popup was opened from the main menu, which then comes back when it closes.
    return_to_menu: bool,
    /// The devices listed in the popup, with the cursor, the active one and the scroll position.
    pub devices: DeviceMenu,
    /// Most recent audio device or stream error, if any.
    pub audio_error: Option<String>,
    /// How the bars are colored. While the color popup is open it holds the live preview.
    pub palette: Palette,
    /// The open (or last) visit to the bar color popup.
    pub color: ColorSession,
}

impl Default for App {
    fn default() -> Self {
        Self {
            should_quit: false,
            tick_counter: 0,
            terminal_width: 0,
            chosen_bar_count: MIN_BARS,
            bar_count: MIN_BARS,
            notices: Notices::default(),
            motion: BarMotion::new(MIN_BARS),
            gain: 1.0,
            smoothing_level: 0,
            last_smoothing_level: MAX_SMOOTHING_LEVEL,
            bar_gap: true,
            divider: true,
            mirror: true,
            fps: DEFAULT_FPS,
            attack_factor: 0.40,
            decay_factor: 0.12,
            overlay: Overlay::None,
            menu_row: MenuRow::default(),
            menu_row_since: 0,
            device_row_since: 0,
            return_to_menu: false,
            devices: DeviceMenu::default(),
            audio_error: None,
            palette: Palette::default(),
            color: ColorSession::default(),
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
            devices: DeviceMenu::new(devices, active_device_idx),
            ..Self::default()
        }
    }

    /// The options to save between sessions.
    pub fn config(&self) -> Config {
        Config {
            bars: self.chosen_bar_count,
            gain: self.gain,
            smoothing_level: self.smoothing_level,
            last_smoothing_level: self.last_smoothing_level,
            bar_gap: self.bar_gap,
            divider: self.divider,
            mirror: self.mirror,
            fps: self.fps,
            color_mode: self.palette.mode,
            bands: self.palette.bands,
            fade_level: self.palette.fade_level,
        }
    }

    /// Takes the saved options, clamping each to its valid range. Call before the terminal width
    /// is known, since that is what limits the shown bar count.
    pub fn apply_config(&mut self, config: &Config) {
        self.chosen_bar_count = config.bars.clamp(MIN_BARS, MAX_BARS);
        self.gain = round_gain(config.gain.clamp(MIN_GAIN, MAX_GAIN));
        self.smoothing_level = config.smoothing_level.min(MAX_SMOOTHING_LEVEL);
        self.last_smoothing_level = config.last_smoothing_level.clamp(1, MAX_SMOOTHING_LEVEL);
        self.bar_gap = config.bar_gap;
        self.divider = config.divider;
        self.mirror = config.mirror;
        if FPS_OPTIONS.contains(&config.fps) {
            self.fps = config.fps;
        }
        self.palette.mode = config.color_mode;
        self.palette.bands = config.bands;
        self.palette.fade_level = config.fade_level.min(MAX_FADE_LEVEL);
        self.sync_bar_count();
    }

    /// Executes an animation cycle on each tick (`fps` times per second).
    ///
    /// The per-tick factors are tuned at 60 FPS, so at other rates they are rescaled to give the
    /// same response per second.
    pub fn tick(&mut self) {
        self.tick_counter += 1;
        let steps = DEFAULT_FPS as f32 / self.fps as f32;
        // Gradually decay targets for natural sound fade-out
        self.motion.decay_targets(steps);
        self.apply_response(steps);
    }

    fn notify(&mut self, kind: NoticeKind) {
        self.notices.show(kind, self.tick_counter, self.fps);
    }

    fn notice_active(&self, kind: NoticeKind) -> bool {
        self.notices.is_active(kind, self.tick_counter)
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
        self.notice_active(NoticeKind::Bars)
            .then_some(self.bar_count)
    }

    fn set_chosen_bar_count(&mut self, target: usize) {
        // Pressing against a limit keeps the previous choice, so a count that is currently
        // capped by a narrow terminal is not forgotten.
        if target != self.bar_count {
            self.chosen_bar_count = target;
            self.sync_bar_count();
        }
        self.notify(NoticeKind::Bars);
    }

    /// Recomputes the shown bar count and stretches the current bars to the new length, so the
    /// display does not blank out while the audio side catches up.
    fn sync_bar_count(&mut self) {
        let count = self.chosen_bar_count.clamp(MIN_BARS, self.max_bar_count());
        if count != self.bar_count {
            self.bar_count = count;
            self.motion.resize(count);
        }
    }

    /// Updates audio bars received from the FFT and applies smoothing.
    pub fn update_audio_bars(&mut self, bars: Vec<f32>) {
        // A frame computed for a previous bar count can still be in flight after a change.
        if !self.motion.set_targets(bars) {
            return;
        }
        // With smoothing on the bars only move on ticks, so the motion stays continuous.
        if self.smoothing_level == 0 {
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
        let (attack, decay) = if self.smoothing_level > 0 {
            // From no smoothing (the bars jump to their targets) at level 0.0 to the full
            // factors at 1.0.
            let strength = self.smoothing_strength();
            (
                1.0 + (self.attack_factor - 1.0) * strength,
                1.0 + (self.decay_factor - 1.0) * strength,
            )
        } else {
            (PLAIN_ATTACK_FACTOR, PLAIN_DECAY_FACTOR)
        };
        self.motion.step(attack, decay, steps);
    }

    /// Increases visualization sensitivity/gain.
    pub fn increase_gain(&mut self) {
        self.gain = round_gain((self.gain + GAIN_STEP).min(MAX_GAIN));
        self.notify(NoticeKind::Gain);
    }

    /// Decreases visualization sensitivity/gain.
    pub fn decrease_gain(&mut self) {
        self.gain = round_gain((self.gain - GAIN_STEP).max(MIN_GAIN));
        self.notify(NoticeKind::Gain);
    }

    /// The smoothing level as a strength, 0.0 (off) to 1.0.
    pub fn smoothing_strength(&self) -> f32 {
        self.smoothing_level as f32 / MAX_SMOOTHING_LEVEL as f32
    }

    /// The smoothing level as shown: `off`, then `0.1` to `1.0`.
    pub fn smoothing_text(&self) -> String {
        if self.smoothing_level == 0 {
            "off".to_string()
        } else {
            format!("{:.1}", self.smoothing_strength())
        }
    }

    /// Switches smoothing off, or back to the last level it was on when it is off.
    pub fn toggle_smoothing(&mut self) {
        self.smoothing_level = if self.smoothing_level == 0 {
            self.last_smoothing_level
        } else {
            0
        };
        self.notify(NoticeKind::Smoothing);
    }

    /// Raises or lowers the smoothing by one tenth, within its limits.
    pub fn step_smoothing(&mut self, forward: bool) {
        self.smoothing_level = if forward {
            (self.smoothing_level + 1).min(MAX_SMOOTHING_LEVEL)
        } else {
            self.smoothing_level.saturating_sub(1)
        };
        if self.smoothing_level > 0 {
            self.last_smoothing_level = self.smoothing_level;
        }
        self.notify(NoticeKind::Smoothing);
    }

    /// The smoothing level to announce on screen right after the user changed it.
    pub fn smoothing_notice(&self) -> Option<String> {
        self.notice_active(NoticeKind::Smoothing)
            .then(|| self.smoothing_text())
    }

    /// Turns the gap between bars on or off. Bars take fewer columns without it, so more of the
    /// chosen count can fit.
    pub fn toggle_bar_gap(&mut self) {
        self.bar_gap = !self.bar_gap;
        self.notify(NoticeKind::BarGap);
        self.sync_bar_count();
    }

    /// The bar gap state to announce on screen right after the user toggled it.
    pub fn bar_gap_notice(&self) -> Option<bool> {
        self.notice_active(NoticeKind::BarGap)
            .then_some(self.bar_gap)
    }

    /// Turns the line between the bars and their reflection on or off.
    pub fn toggle_divider(&mut self) {
        self.divider = !self.divider;
        self.notify(NoticeKind::Divider);
    }

    /// The divider state to announce on screen right after the user toggled it.
    pub fn divider_notice(&self) -> Option<bool> {
        self.notice_active(NoticeKind::Divider)
            .then_some(self.divider)
    }

    /// Turns the reflection below the divider line on or off.
    pub fn toggle_mirror(&mut self) {
        self.mirror = !self.mirror;
        self.notify(NoticeKind::Mirror);
    }

    /// The mirror state to announce on screen right after the user toggled it.
    pub fn mirror_notice(&self) -> Option<bool> {
        self.notice_active(NoticeKind::Mirror)
            .then_some(self.mirror)
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
        self.notify(NoticeKind::Fps);
    }

    /// The frame rate to announce on screen right after the user changed it.
    pub fn fps_notice(&self) -> Option<u32> {
        self.notice_active(NoticeKind::Fps).then_some(self.fps)
    }

    /// Opens the main menu with the cursor on its first row.
    pub fn open_menu(&mut self) {
        self.overlay = Overlay::Menu;
        self.menu_row = MenuRow::default();
        self.menu_row_since = self.tick_counter;
    }

    /// The active device's name cut to `width` characters for the menu. A longer name scrolls
    /// like a carousel while the device row is selected, after a short pause on its start, and
    /// shows its start otherwise.
    pub fn menu_device_text(&self, width: usize) -> String {
        let elapsed = self.tick_counter.saturating_sub(self.menu_row_since);
        carousel_text(
            self.devices.active_name(),
            width,
            self.menu_row == MenuRow::Device,
            elapsed,
            self.fps,
        )
    }

    /// The highlighted row of the device popup cut to `width` characters. A longer name scrolls
    /// like a carousel, after a short pause on its start.
    pub fn device_row_text(&self, width: usize) -> String {
        let name = self
            .devices
            .shown_names()
            .get(self.devices.shown_selected())
            .map_or("", String::as_str);
        let elapsed = self.tick_counter.saturating_sub(self.device_row_since);
        carousel_text(name, width, true, elapsed, self.fps)
    }

    /// Moves the device popup cursor down, restarting the carousel on the new row.
    pub fn select_next_device(&mut self) {
        self.devices.select_next();
        self.device_row_since = self.tick_counter;
    }

    /// Moves the device popup cursor up, restarting the carousel on the new row.
    pub fn select_prev_device(&mut self) {
        self.devices.select_prev();
        self.device_row_since = self.tick_counter;
    }

    /// Switches the device popup between outputs and applications.
    pub fn toggle_device_tab(&mut self) {
        self.devices.toggle_tab();
        self.device_row_since = self.tick_counter;
    }

    /// Closes the main menu.
    pub fn close_menu(&mut self) {
        self.overlay = Overlay::None;
    }

    /// Moves the main menu cursor down, wrapping around.
    pub fn select_next_menu_row(&mut self) {
        self.menu_row = self.menu_row.next();
        self.menu_row_since = self.tick_counter;
    }

    /// Moves the main menu cursor up, wrapping around.
    pub fn select_prev_menu_row(&mut self) {
        self.menu_row = self.menu_row.previous();
        self.menu_row_since = self.tick_counter;
    }

    /// Opens the output device popup, remembering whether the main menu was open under it.
    pub fn open_device_popup(&mut self) {
        self.return_to_menu = self.overlay == Overlay::Menu;
        self.devices.tab = DeviceTab::Outputs;
        self.devices.mode_focused = false;
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
        self.color = ColorSession::open(&self.palette);
        self.overlay = Overlay::Colors;
    }

    /// Closes the color popup, keeping every color chosen in it.
    pub fn close_color_popup(&mut self) {
        self.overlay = Overlay::None;
        self.restore_menu();
        if self.color.close(&self.palette) {
            self.notify(NoticeKind::Color);
        }
    }

    /// The text of every notice on screen, in the order they stack down the top-right corner.
    pub fn visible_notices(&self) -> Vec<String> {
        let on_off = |on: bool| if on { "on" } else { "off" };
        [
            self.bar_notice().map(|count| format!(" {count} bars ")),
            self.gain_notice()
                .map(|gain| format!(" sensitivity {gain:.2}x ")),
            self.smoothing_notice()
                .map(|level| format!(" smoothing {level} ")),
            self.bar_gap_notice()
                .map(|on| format!(" bar gap {} ", on_off(on))),
            self.divider_notice()
                .map(|on| format!(" divider {} ", on_off(on))),
            self.mirror_notice()
                .map(|on| format!(" mirror {} ", on_off(on))),
            self.fps_notice().map(|fps| format!(" {fps} fps ")),
            self.color_notice().map(|color| format!(" color: {color} ")),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// The color choice to announce on screen right after the user confirmed it.
    pub fn color_notice(&self) -> Option<String> {
        self.notice_active(NoticeKind::Color)
            .then(|| self.palette.describe())
    }

    /// The sensitivity to announce on screen right after the user changed it.
    pub fn gain_notice(&self) -> Option<f32> {
        self.notice_active(NoticeKind::Gain).then_some(self.gain)
    }
}

/// Rounds the gain to two decimals, so repeated 0.10 steps don't pick up float noise
/// (1.2000000000000002 and the like) and the limits are reached exactly.
fn round_gain(gain: f32) -> f32 {
    (gain * 100.0).round() / 100.0
}

/// `text` cut to `width` characters. When it is longer and `scrolling`, it loops through the
/// window like a carousel, after a pause of one second on its start; `elapsed` is the number of
/// ticks since the row was reached.
fn carousel_text(text: &str, width: usize, scrolling: bool, elapsed: u64, fps: u32) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    if !scrolling {
        return chars[..width].iter().collect();
    }
    // The text loops with a gap between its end and its next start.
    let cycle: Vec<char> = chars.iter().copied().chain("   ".chars()).collect();
    let hold = u64::from(fps);
    let ticks_per_step = u64::from((fps / 8).max(1));
    let offset = elapsed.saturating_sub(hold) / ticks_per_step % cycle.len() as u64;
    (0..width)
        .map(|i| cycle[(offset as usize + i) % cycle.len()])
        .collect()
}

/// Stretches `bars` to `len` entries by picking the nearest source bar for each new one.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::ColorMode;
    use crate::{notices::NOTICE_TICKS, palette::BarColor};

    fn app_with_width(width: u16) -> App {
        let mut app = App::default();
        app.set_terminal_width(width);
        app
    }

    #[test]
    fn test_config_round_trips_through_the_app_and_is_clamped() {
        let mut app = app_with_width(400);
        app.gain = 2.0;
        app.bar_gap = false;
        app.fps = 120;
        app.toggle_smoothing();
        app.palette.mode = ColorMode::Rainbow;
        app.set_chosen_bar_count(96);

        let mut restored = App::default();
        restored.apply_config(&app.config());
        restored.set_terminal_width(400);
        assert_eq!(restored.config(), app.config());
        assert_eq!(restored.bar_count, 96);

        let mut config = app.config();
        config.bars = 9999;
        config.gain = 99.0;
        config.fps = 45;
        config.fade_level = 200;
        config.last_smoothing_level = 0;
        restored.apply_config(&config);
        assert_eq!(restored.config().bars, MAX_BARS);
        assert_eq!(restored.gain, MAX_GAIN);
        assert_eq!(restored.fps, 120);
        assert_eq!(restored.palette.fade_level, MAX_FADE_LEVEL);
        assert_eq!(restored.config().last_smoothing_level, 1);
    }

    #[test]
    fn test_color_picker_previews_and_cancel_restores_the_color() {
        let mut app = App::new();
        app.open_color_popup();
        app.color.select_next(&app.palette);
        app.color.activate(&mut app.palette);
        assert_eq!(
            app.color.picker.as_ref().map(|p| p.color()),
            Some(BarColor::CYAN)
        );
        app.color
            .edit_picker(&mut app.palette, |picker| picker.nudge(-10, 5, false));
        assert_ne!(app.palette.bands[0], BarColor::CYAN);
        assert_eq!(
            app.palette.bands[0],
            app.color.picker.as_ref().unwrap().color()
        );
        app.color.close_picker(&mut app.palette, false);
        assert_eq!(app.palette, Palette::default());
        assert_eq!(app.color.picker, None);
    }

    #[test]
    fn test_color_picker_confirm_keeps_the_color_and_closing_shows_a_notice() {
        let mut app = App::new();
        app.open_color_popup();
        app.color.select_next(&app.palette);
        app.color.activate(&mut app.palette);
        app.color.edit_picker(&mut app.palette, |picker| {
            picker.next_focus();
            picker.nudge(0, 10, false);
        });
        let picked = app.color.picker.as_ref().unwrap().color();
        assert_ne!(picked, BarColor::CYAN);
        app.color.close_picker(&mut app.palette, true);
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
        app.color.select_prev(&app.palette);
        assert_eq!(app.color.row, 4);
        app.color.select_next(&app.palette);
        assert_eq!(app.color.row, 0);
        // Single color skips the mids and treble rows.
        app.color.select_next(&app.palette);
        app.color.select_next(&app.palette);
        assert_eq!(app.color.row, 4);
        app.color.select_next(&app.palette);
        assert_eq!(app.color.row, 0);

        // Enter does nothing on the mode row; the arrows change the mode.
        app.color.activate(&mut app.palette);
        assert_eq!(app.palette.mode, ColorMode::Single);
        app.color.change_option(&mut app.palette, true);
        assert_eq!(app.palette.mode, ColorMode::Gradient);
        app.color.change_option(&mut app.palette, false);
        assert_eq!(app.palette.mode, ColorMode::Single);
        app.color.change_option(&mut app.palette, true);
        assert_eq!(app.color.picker, None);
        app.color.select_prev(&app.palette);
        assert_eq!(app.color.row, 4);
        app.color.select_prev(&app.palette);
        assert_eq!(app.color.row, 3);
    }

    #[test]
    fn test_changing_the_mode_only_works_on_the_mode_row() {
        let mut app = App::new();
        app.open_color_popup();
        app.color.select_next(&app.palette);
        app.color.change_option(&mut app.palette, true);
        assert_eq!(app.palette.mode, ColorMode::Single);
    }

    #[test]
    fn test_fade_row_steps_the_level_with_the_arrows() {
        let mut app = App::new();
        app.open_color_popup();
        app.color.select_prev(&app.palette);
        assert_eq!(app.color.row, Palette::FADE_ROW);
        assert_eq!(app.palette.fade_level, 5);
        // Enter does nothing; the arrows step the level by a tenth.
        app.color.activate(&mut app.palette);
        assert_eq!(app.palette.fade_level, 5);
        app.color.change_option(&mut app.palette, true);
        assert_eq!(app.palette.fade_level, 6);
        for _ in 0..10 {
            app.color.change_option(&mut app.palette, false);
        }
        assert_eq!(app.palette.fade_level, 0);
        assert_eq!(app.palette.fade_level_text(), "Off");
        assert_eq!(app.color.picker, None);
        // The row stays selectable while the fade is off.
        app.color.select_next(&app.palette);
        app.color.select_prev(&app.palette);
        assert_eq!(app.color.row, Palette::FADE_ROW);
    }

    #[test]
    fn test_bar_count_starts_at_minimum() {
        let app = app_with_width(200);
        assert_eq!(app.bar_count, 32);
        assert_eq!(app.motion.bars().len(), 32);
    }

    #[test]
    fn test_bar_count_steps_by_eight_up_to_the_cap() {
        let mut app = app_with_width(1000);
        for expected in [40, 48, 56] {
            app.increase_bar_count();
            assert_eq!(app.bar_count, expected);
            assert_eq!(app.motion.bars().len(), expected);
            assert_eq!(app.motion.targets().len(), expected);
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
        assert_eq!(app.motion.bars().len(), 60);

        app.set_terminal_width(400);
        assert_eq!(app.bar_count, 96);
        assert_eq!(app.motion.bars().len(), 96);
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
        app.motion = BarMotion::from_parts(vec![0.5; 32], vec![0.5; 32]);

        app.increase_bar_count();

        assert_eq!(app.motion.bars(), vec![0.5; 40]);
        assert_eq!(app.motion.targets(), vec![0.5; 40]);
    }

    #[test]
    fn test_stale_audio_frames_are_ignored_after_a_change() {
        let mut app = app_with_width(200);
        app.increase_bar_count();

        app.update_audio_bars(vec![1.0; 32]);

        assert_eq!(app.motion.targets(), vec![0.0; 40]);
        app.update_audio_bars(vec![1.0; 40]);
        assert_eq!(app.motion.targets(), vec![1.0; 40]);
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
        assert_eq!(app.motion.bars()[0], 0.0);

        let new_bars = vec![1.0; 32];
        app.update_audio_bars(new_bars);
        app.tick();
        let risen = app.motion.bars()[0];
        assert!(risen > 0.2 && risen < 1.0);

        let zero_bars = vec![0.0; 32];
        app.update_audio_bars(zero_bars);
        app.tick();
        assert!(app.motion.bars()[0] > 0.0 && app.motion.bars()[0] < risen);
    }

    #[test]
    fn test_smoothing_is_off_by_default_and_bars_react_to_each_audio_frame() {
        let mut app = App::default();
        assert_eq!(app.smoothing_level, 0);

        app.update_audio_bars(vec![1.0; 32]);

        // The plain response moves the bars right away, without waiting for a tick.
        assert!(app.motion.bars()[0] > 0.6);
    }

    #[test]
    fn test_smoothing_notice_shows_after_a_toggle_then_expires() {
        let mut app = App::default();
        assert_eq!(app.smoothing_notice(), None);

        app.toggle_smoothing();
        assert_eq!(app.smoothing_notice().as_deref(), Some("1.0"));

        for _ in 0..NOTICE_TICKS {
            app.tick();
        }
        assert_eq!(app.smoothing_notice(), None);

        app.toggle_smoothing();
        assert_eq!(app.smoothing_notice().as_deref(), Some("off"));
    }

    #[test]
    fn test_smoothing_steps_by_a_tenth_between_off_and_one() {
        let mut app = App::default();
        assert_eq!(app.smoothing_text(), "off");
        app.step_smoothing(false);
        assert_eq!(app.smoothing_level, 0);
        app.step_smoothing(true);
        assert_eq!(app.smoothing_text(), "0.1");
        for _ in 0..20 {
            app.step_smoothing(true);
        }
        assert_eq!(app.smoothing_text(), "1.0");
        assert_eq!(app.smoothing_notice().as_deref(), Some("1.0"));
        for _ in 0..10 {
            app.step_smoothing(false);
        }
        assert_eq!(app.smoothing_text(), "off");
    }

    #[test]
    fn test_a_higher_smoothing_level_moves_the_bars_less_per_tick() {
        let rise = |level: u8| {
            let mut app = App {
                smoothing_level: level,
                ..App::default()
            };
            app.update_audio_bars(vec![1.0; 32]);
            app.tick();
            app.motion.bars()[0]
        };
        assert!(rise(2) > rise(5));
        assert!(rise(5) > rise(10));
    }

    #[test]
    fn test_bars_move_a_little_on_every_tick() {
        let mut app = App {
            smoothing_level: MAX_SMOOTHING_LEVEL,
            ..App::default()
        };
        app.update_audio_bars(vec![1.0; 32]);

        let mut previous = app.motion.bars()[0];
        for _ in 0..5 {
            app.tick();
            assert!(app.motion.bars()[0] > previous);
            previous = app.motion.bars()[0];
        }
    }

    #[test]
    fn test_a_bar_falls_slower_than_it_rises() {
        let mut up = App {
            smoothing_level: MAX_SMOOTHING_LEVEL,
            ..App::default()
        };
        up.update_audio_bars(vec![1.0; 32]);
        up.tick();

        let mut down = App {
            smoothing_level: MAX_SMOOTHING_LEVEL,
            motion: BarMotion::from_parts(vec![1.0; 32], vec![0.0; 32]),
            ..App::default()
        };
        down.update_audio_bars(vec![0.0; 32]);
        down.tick();

        assert!(1.0 - down.motion.bars()[0] < up.motion.bars()[0]);
    }

    #[test]
    fn test_a_brief_dip_barely_moves_the_bar() {
        let mut steady = App {
            smoothing_level: MAX_SMOOTHING_LEVEL,
            motion: BarMotion::from_parts(vec![0.8; 32], vec![0.0; 32]),
            ..App::default()
        };
        let mut dipping = App {
            smoothing_level: MAX_SMOOTHING_LEVEL,
            motion: BarMotion::from_parts(vec![0.8; 32], vec![0.0; 32]),
            ..App::default()
        };

        // A 0.8 level where one frame in three drops to 0.3.
        for tick in 0..30 {
            steady.update_audio_bars(vec![0.8; 32]);
            dipping.update_audio_bars(vec![if tick % 3 == 1 { 0.3 } else { 0.8 }; 32]);
            steady.tick();
            dipping.tick();
        }

        assert!((steady.motion.bars()[0] - dipping.motion.bars()[0]).abs() < 0.1);
    }

    #[test]
    fn test_app_gain_adjustment() {
        let mut app = App {
            motion: BarMotion::from_parts(vec![0.4; 32], vec![0.0; 32]),
            ..App::default()
        };
        app.increase_gain();
        assert_eq!(app.gain, 1.1);
        // The gain reaches the bars through the audio processor, not through the app.
        assert_eq!(app.motion.bars()[0], 0.4);
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

        app.devices.ensure_visible(4);
        assert_eq!(app.devices.scroll_offset(), 0);

        app.devices.selected = 5;
        app.devices.ensure_visible(4);
        assert_eq!(app.devices.scroll_offset(), 2);

        app.devices.selected = 0;
        app.devices.ensure_visible(4);
        assert_eq!(app.devices.scroll_offset(), 0);

        app.devices.selected = 9;
        app.devices.ensure_visible(4);
        assert_eq!(app.devices.scroll_offset(), 6);
    }

    #[test]
    fn test_app_device_navigation() {
        let devices = vec![
            "Default Device".to_string(),
            "Headphones".to_string(),
            "HDMI Audio".to_string(),
        ];
        let mut app = App::with_devices(devices, 0);
        assert_eq!(app.devices.selected, 0);

        app.devices.select_next();
        assert_eq!(app.devices.selected, 1);

        app.devices.select_next();
        assert_eq!(app.devices.selected, 2);

        // Wrapping forward goes through the mode row
        app.devices.select_next();
        assert!(app.devices.mode_focused);
        app.devices.select_next();
        assert_eq!((app.devices.mode_focused, app.devices.selected), (false, 0));

        // and so does wrapping backward
        app.devices.select_prev();
        assert!(app.devices.mode_focused);
        app.devices.select_prev();
        assert_eq!((app.devices.mode_focused, app.devices.selected), (false, 2));
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
            app.motion = BarMotion::from_parts(vec![1.0; 32], vec![0.0; 32]);
            for _ in 0..fps {
                app.tick();
            }
            app.motion.bars()[0]
        };
        let at_60 = fall_after_one_second(60);
        for fps in [30, 90, 120] {
            let at = fall_after_one_second(fps);
            assert!((at - at_60).abs() < 0.01, "{fps} fps: {at} vs {at_60}");
        }
    }

    #[test]
    fn test_visible_notices_stack_in_a_fixed_order() {
        let mut app = app_with_width(200);
        assert!(app.visible_notices().is_empty());

        app.increase_gain();
        app.toggle_bar_gap();
        app.increase_bar_count();

        assert_eq!(
            app.visible_notices(),
            [" 40 bars ", " sensitivity 1.10x ", " bar gap off "]
        );
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

    #[test]
    fn test_toggle_smoothing_brings_back_the_last_level() {
        let mut app = App::default();
        for _ in 0..4 {
            app.step_smoothing(true);
        }
        assert_eq!(app.smoothing_level, 4);
        app.toggle_smoothing();
        assert_eq!(app.smoothing_level, 0);
        app.toggle_smoothing();
        assert_eq!(app.smoothing_level, 4);
    }

    #[test]
    fn test_menu_device_text_scrolls_only_while_the_row_is_selected() {
        let mut app = App {
            devices: DeviceMenu::new(vec!["Short".into(), "A very long device name".into()], 1),
            ..App::default()
        };
        assert_eq!(app.menu_device_text(23), "A very long device name");
        assert_eq!(app.menu_device_text(10), "A very lon");
        app.tick_counter = 1000;
        assert_ne!(app.menu_device_text(10), "A very lon");
        app.menu_row = MenuRow::Color;
        assert_eq!(app.menu_device_text(10), "A very lon");
        app.devices.active = 0;
        assert_eq!(app.menu_device_text(10), "Short");
    }
}
