use ratatui::style::Color;

/// Hue, in degrees, of the last bar in rainbow mode: red at the first bar through violet. It stops
/// short of 360 so the end does not wrap back to red.
const RAINBOW_END_HUE: f32 = 300.0;
/// Saturation of the rainbow colors, a bit under full so they are not so harsh.
const RAINBOW_SATURATION: f32 = 0.70;

/// Fraction of its brightness that a color keeps in the mirror reflection.
const MIRROR_BRIGHTNESS: f32 = 0.35;

/// How much darker than its full color a bar's base gets at the highest fade level: the base
/// keeps `1 - FADE_RANGE` of its brightness. Lower levels scale it down proportionally.
const FADE_RANGE: f32 = 0.8;
/// Lowest and highest fade level, in tenths.
pub const MIN_FADE_LEVEL: u8 = 1;
pub const MAX_FADE_LEVEL: u8 = 10;

/// A color the user can pick for the bars: any 24-bit RGB value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl BarColor {
    pub const CYAN: BarColor = BarColor::rgb(0, 255, 255);
    pub const BLUE: BarColor = BarColor::rgb(80, 120, 255);
    pub const PINK: BarColor = BarColor::rgb(255, 50, 200);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// The color as drawn on the bars.
    pub fn color(self) -> Color {
        Color::Rgb(self.r, self.g, self.b)
    }

    /// The darker tone of this color used for the mirror reflection.
    pub fn mirror_color(self) -> Color {
        let dim = |channel: u8| (channel as f32 * MIRROR_BRIGHTNESS).round() as u8;
        Color::Rgb(dim(self.r), dim(self.g), dim(self.b))
    }

    /// The color `t` (0 to 1) of the way from `self` to `other`, blended in RGB.
    pub fn lerp(self, other: BarColor, t: f32) -> BarColor {
        let t = t.clamp(0.0, 1.0);
        let channel =
            |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * t).round() as u8;
        BarColor::rgb(
            channel(self.r, other.r),
            channel(self.g, other.g),
            channel(self.b, other.b),
        )
    }

    /// `#RRGGBB`.
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Parses `RRGGBB` or `#RRGGBB`.
    pub fn parse_hex(text: &str) -> Option<BarColor> {
        let digits = text.strip_prefix('#').unwrap_or(text);
        if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
        Some(BarColor::rgb(channel(0)?, channel(2)?, channel(4)?))
    }

    /// Hue in degrees (0 to 360), saturation and value (0 to 1) to RGB.
    pub fn from_hsv(hue: f32, saturation: f32, value: f32) -> BarColor {
        let chroma = value * saturation;
        let sector = (hue % 360.0) / 60.0;
        let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
        let (r, g, b) = match sector as u32 {
            0 => (chroma, second, 0.0),
            1 => (second, chroma, 0.0),
            2 => (0.0, chroma, second),
            3 => (0.0, second, chroma),
            4 => (second, 0.0, chroma),
            _ => (chroma, 0.0, second),
        };
        let offset = value - chroma;
        let channel = |v: f32| ((v + offset) * 255.0).round().clamp(0.0, 255.0) as u8;
        BarColor::rgb(channel(r), channel(g), channel(b))
    }

    /// RGB to hue in degrees (0 to 360), saturation and value (0 to 1). The hue of a gray is 0.
    pub fn to_hsv(self) -> (f32, f32, f32) {
        let (r, g, b) = (
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
        );
        let max = r.max(g).max(b);
        let delta = max - r.min(g).min(b);
        let hue = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };
        let saturation = if max == 0.0 { 0.0 } else { delta / max };
        (hue, saturation, max)
    }
}

/// How the bars take their colors from the three picked colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    /// A smooth blend: bass at the left end, mids in the middle and treble at the right end.
    Gradient,
    /// The colors of the rainbow along the bars, from red at the first bar to violet at the last.
    /// It does not use the picked colors.
    Rainbow,
    /// A smooth blend up every bar: bass at the base, mids halfway and treble at the top.
    Vertical,
    /// The bass color on every bar.
    Single,
}

impl ColorMode {
    /// The mode after this one in the picker's order.
    pub fn next(self) -> ColorMode {
        match self {
            ColorMode::Single => ColorMode::Gradient,
            ColorMode::Gradient => ColorMode::Vertical,
            ColorMode::Vertical => ColorMode::Rainbow,
            ColorMode::Rainbow => ColorMode::Single,
        }
    }

    /// The mode before this one in the picker's order.
    pub fn previous(self) -> ColorMode {
        match self {
            ColorMode::Single => ColorMode::Rainbow,
            ColorMode::Gradient => ColorMode::Single,
            ColorMode::Vertical => ColorMode::Gradient,
            ColorMode::Rainbow => ColorMode::Vertical,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ColorMode::Gradient => "Gradient",
            ColorMode::Rainbow => "Rainbow",
            ColorMode::Vertical => "Vertical gradient",
            ColorMode::Single => "Single color",
        }
    }
}

/// How the bars are colored: by a smooth gradient, by the rainbow or with the bass color throughout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub mode: ColorMode,
    /// Colors of the bass, mids and treble. The mids and treble colors are kept while
    /// single-color mode hides them.
    pub bands: [BarColor; 3],
    /// Whether the bars fade from dark at the base to their full color at the top. Off, every
    /// cell has its full color.
    pub fade: bool,
    /// Intensity of the fade in tenths (`MIN_FADE_LEVEL` to `MAX_FADE_LEVEL`): the higher, the
    /// darker the base. Kept while the fade is off.
    pub fade_level: u8,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            mode: ColorMode::Single,
            bands: [BarColor::CYAN, BarColor::BLUE, BarColor::PINK],
            fade: true,
            fade_level: 5,
        }
    }
}

impl Palette {
    /// Rows the picker shows: the mode row, bass, mids, treble, the fade switch and level.
    pub const ROWS: usize = 6;
    /// The row of the fade checkbox.
    pub const FADE_ROW: usize = 4;
    /// The row of the fade intensity, grayed out while the fade is off.
    pub const FADE_LEVEL_ROW: usize = 5;

    /// Whether the cursor can rest on `row`. In single-color mode the mids and treble are grayed
    /// out, and in rainbow mode all three colors are, since none of them is used.
    pub fn row_selectable(&self, row: usize) -> bool {
        if row >= Self::ROWS {
            return false;
        }
        if row == Self::FADE_LEVEL_ROW && !self.fade {
            return false;
        }
        match self.mode {
            ColorMode::Single => !(2..=3).contains(&row),
            ColorMode::Rainbow => !(1..=3).contains(&row),
            _ => true,
        }
    }

    /// The next selectable row after `row`, wrapping around.
    pub fn next_row(&self, row: usize, forward: bool) -> usize {
        let step = |row: usize| {
            if forward {
                (row + 1) % Self::ROWS
            } else {
                (row + Self::ROWS - 1) % Self::ROWS
            }
        };
        let mut next = step(row);
        while !self.row_selectable(next) {
            next = step(next);
        }
        next
    }

    /// The color a picker row shows (the mode and checkbox rows have none). In single-color mode the mids and
    /// treble rows show the bass color they follow.
    pub fn row_color(&self, row: usize) -> Option<BarColor> {
        match row {
            1..=3 => Some(self.band_color(row - 1)),
            _ => None,
        }
    }

    /// The color a band is shown with in the picker.
    fn band_color(&self, band: usize) -> BarColor {
        if self.mode == ColorMode::Single {
            self.bands[0]
        } else {
            self.bands[band]
        }
    }

    /// Switches to the next color mode, or the previous one.
    pub fn change_mode(&mut self, forward: bool) {
        self.mode = if forward {
            self.mode.next()
        } else {
            self.mode.previous()
        };
    }

    /// Turns the fade at the base of the bars on or off.
    pub fn toggle_fade(&mut self) {
        self.fade = !self.fade;
    }

    /// Raises or lowers the fade intensity by one tenth, within its limits.
    pub fn step_fade_level(&mut self, forward: bool) {
        self.fade_level = if forward {
            (self.fade_level + 1).min(MAX_FADE_LEVEL)
        } else {
            self.fade_level.saturating_sub(1).max(MIN_FADE_LEVEL)
        };
    }

    /// The fade intensity as shown, `0.1` to `1.0`.
    pub fn fade_level_text(&self) -> String {
        format!("{:.1}", self.fade_level as f32 / 10.0)
    }

    /// Brightness of a bar's base cell, from `0.2` at the highest level to nearly `1.0` at the
    /// lowest; `1.0` while the fade is off.
    pub fn fade_floor(&self) -> f32 {
        if self.fade {
            1.0 - self.fade_level as f32 / MAX_FADE_LEVEL as f32 * FADE_RANGE
        } else {
            1.0
        }
    }

    /// Sets the color a picker row edits. Only the bass can be edited in single-color mode.
    pub fn set_row_color(&mut self, row: usize, color: BarColor) {
        if (1..=3).contains(&row) && self.row_selectable(row) {
            self.bands[row - 1] = color;
        }
    }

    /// The color of bar `bar` out of `bar_count`. In vertical mode that is the color of its base.
    pub fn bar_color(&self, bar: usize, bar_count: usize) -> BarColor {
        match self.mode {
            ColorMode::Single | ColorMode::Vertical => self.bands[0],
            ColorMode::Gradient => self.blend(Self::position(bar, bar_count)),
            ColorMode::Rainbow => BarColor::from_hsv(
                Self::position(bar, bar_count) * RAINBOW_END_HUE,
                RAINBOW_SATURATION,
                1.0,
            ),
        }
    }

    /// Where bar `bar` sits along the row: 0 at the first bar, 1 at the last.
    fn position(bar: usize, bar_count: usize) -> f32 {
        if bar_count > 1 {
            bar as f32 / (bar_count - 1) as f32
        } else {
            0.0
        }
    }

    /// The three picked colors blended: the first at `t` 0, the second at 0.5, the third at 1.
    fn blend(&self, t: f32) -> BarColor {
        let [first, second, third] = self.bands;
        if t <= 0.5 {
            first.lerp(second, t * 2.0)
        } else {
            second.lerp(third, (t - 0.5) * 2.0)
        }
    }

    /// Whether the colors change along the height of the bars instead of across them.
    pub fn is_vertical(&self) -> bool {
        self.mode == ColorMode::Vertical
    }

    /// The color at height `t` of a bar in vertical mode: 0 at its base, 1 at the far end.
    pub fn height_color(&self, t: f32) -> BarColor {
        self.blend(t)
    }

    /// The reflection's color at height `t` of a bar in vertical mode.
    pub fn mirror_height_color(&self, t: f32) -> Color {
        self.height_color(t).mirror_color()
    }

    /// The label of a picker row. In vertical mode the three colors are the bar's base, middle
    /// and top.
    pub fn row_label(&self, row: usize) -> &'static str {
        match (row, self.mode) {
            (0, _) => "Mode",
            (1, ColorMode::Vertical) => "Base",
            (2, ColorMode::Vertical) => "Middle",
            (3, ColorMode::Vertical) => "Top",
            (1, _) => "Bass",
            (2, _) => "Mids",
            (3, _) => "Treble",
            (Self::FADE_ROW, _) => "Fade",
            _ => "Intensity",
        }
    }

    /// The color of bar `bar`'s reflection: a darker tone of its color.
    pub fn mirror_bar_color(&self, bar: usize, bar_count: usize) -> Color {
        self.bar_color(bar, bar_count).mirror_color()
    }

    /// Short description for the on-screen notice.
    pub fn describe(&self) -> String {
        match self.mode {
            ColorMode::Single => self.bands[0].hex(),
            mode => mode.label().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_single_color_cyan() {
        let palette = Palette::default();
        assert_eq!(palette.mode, ColorMode::Single);
        assert_eq!(
            palette.bands,
            [BarColor::CYAN, BarColor::BLUE, BarColor::PINK]
        );
        for bar in [0, 15, 31] {
            assert_eq!(palette.bar_color(bar, 32), BarColor::CYAN);
        }
    }

    #[test]
    fn test_mirror_is_dimmer() {
        assert_eq!(BarColor::CYAN.mirror_color(), Color::Rgb(0, 89, 89));
        assert_eq!(
            BarColor::rgb(255, 255, 255).mirror_color(),
            Color::Rgb(89, 89, 89)
        );
    }

    #[test]
    fn test_hex_round_trip_and_parsing() {
        assert_eq!(BarColor::rgb(255, 0, 16).hex(), "#FF0010");
        assert_eq!(
            BarColor::parse_hex("#ff0010"),
            Some(BarColor::rgb(255, 0, 16))
        );
        assert_eq!(
            BarColor::parse_hex("00FF00"),
            Some(BarColor::rgb(0, 255, 0))
        );
        assert_eq!(BarColor::parse_hex("#12345"), None);
        assert_eq!(BarColor::parse_hex("GG0000"), None);
    }

    #[test]
    fn test_hsv_conversion_round_trips() {
        for color in [
            BarColor::CYAN,
            BarColor::BLUE,
            BarColor::rgb(255, 0, 0),
            BarColor::rgb(12, 200, 99),
            BarColor::rgb(128, 128, 128),
            BarColor::rgb(0, 0, 0),
        ] {
            let (h, s, v) = color.to_hsv();
            assert_eq!(BarColor::from_hsv(h, s, v), color, "{color:?}");
        }
        assert_eq!(BarColor::from_hsv(0.0, 1.0, 1.0), BarColor::rgb(255, 0, 0));
        assert_eq!(
            BarColor::from_hsv(120.0, 1.0, 1.0),
            BarColor::rgb(0, 255, 0)
        );
        assert_eq!(
            BarColor::from_hsv(360.0, 1.0, 1.0),
            BarColor::rgb(255, 0, 0)
        );
    }

    #[test]
    fn test_set_row_color_edits_the_band() {
        let mut palette = Palette {
            mode: ColorMode::Gradient,
            ..Palette::default()
        };
        palette.set_row_color(2, BarColor::rgb(1, 2, 3));
        palette.set_row_color(3, BarColor::rgb(4, 5, 6));
        palette.set_row_color(0, BarColor::rgb(7, 8, 9));
        assert_eq!(palette.bands[1], BarColor::rgb(1, 2, 3));
        assert_eq!(palette.bands[2], BarColor::rgb(4, 5, 6));
        assert_eq!(palette.bands[0], BarColor::CYAN);
    }

    #[test]
    fn test_mode_cycles_single_gradient_vertical_rainbow() {
        let mut palette = Palette::default();
        assert_eq!(palette.mode, ColorMode::Single);
        for expected in [
            ColorMode::Gradient,
            ColorMode::Vertical,
            ColorMode::Rainbow,
            ColorMode::Single,
        ] {
            palette.change_mode(true);
            assert_eq!(palette.mode, expected);
        }
        // Backwards wraps around the other way.
        for expected in [
            ColorMode::Rainbow,
            ColorMode::Vertical,
            ColorMode::Gradient,
            ColorMode::Single,
        ] {
            palette.change_mode(false);
            assert_eq!(palette.mode, expected);
        }
    }

    #[test]
    fn test_single_mode_makes_mids_and_treble_follow_the_bass() {
        let mut palette = Palette {
            mode: ColorMode::Single,
            ..Palette::default()
        };
        assert!(!palette.row_selectable(2));
        assert!(!palette.row_selectable(3));
        assert_eq!(palette.row_color(3), Some(BarColor::CYAN));

        let custom = BarColor::rgb(10, 20, 30);
        palette.set_row_color(1, custom);
        // Mids and treble cannot be edited while they follow the bass.
        palette.set_row_color(2, BarColor::rgb(255, 0, 0));
        for bar in [0, 15, 31] {
            assert_eq!(palette.bar_color(bar, 32), custom);
            assert_eq!(palette.mirror_bar_color(bar, 32), custom.mirror_color());
        }
        assert_eq!(palette.describe(), "#0A141E");

        // The mids and treble colors come back with the other modes.
        palette.mode = ColorMode::Gradient;
        assert!(palette.row_selectable(2));
        assert_eq!(palette.bands, [custom, BarColor::BLUE, BarColor::PINK]);
    }

    #[test]
    fn test_gradient_blends_from_bass_through_mids_to_treble() {
        let palette = Palette {
            mode: ColorMode::Gradient,
            bands: [
                BarColor::rgb(0, 0, 0),
                BarColor::rgb(200, 100, 0),
                BarColor::rgb(200, 100, 200),
            ],
            ..Palette::default()
        };

        assert_eq!(palette.bar_color(0, 33), palette.bands[0]);
        assert_eq!(palette.bar_color(16, 33), palette.bands[1]);
        assert_eq!(palette.bar_color(32, 33), palette.bands[2]);
        // A quarter and three quarters of the way: halfway between the neighbouring colors.
        assert_eq!(palette.bar_color(8, 33), BarColor::rgb(100, 50, 0));
        assert_eq!(palette.bar_color(24, 33), BarColor::rgb(200, 100, 100));
        assert_eq!(palette.describe(), "Gradient");
    }

    #[test]
    fn test_gradient_changes_smoothly_bar_to_bar() {
        let palette = Palette {
            mode: ColorMode::Gradient,
            ..Palette::default()
        };
        let colors: Vec<BarColor> = (0..64).map(|bar| palette.bar_color(bar, 64)).collect();

        // No jump between neighbours, unlike flat colors, and every bar differs from the next.
        for pair in colors.windows(2) {
            assert_ne!(pair[0], pair[1]);
            let jump = |a: u8, b: u8| a.abs_diff(b);
            assert!(jump(pair[0].r, pair[1].r) < 20, "{pair:?}");
            assert!(jump(pair[0].g, pair[1].g) < 20, "{pair:?}");
            assert!(jump(pair[0].b, pair[1].b) < 20, "{pair:?}");
        }
    }

    #[test]
    fn test_vertical_blends_from_base_through_middle_to_top() {
        let palette = Palette {
            mode: ColorMode::Vertical,
            bands: [
                BarColor::rgb(0, 0, 0),
                BarColor::rgb(200, 100, 0),
                BarColor::rgb(200, 100, 200),
            ],
            ..Palette::default()
        };

        assert!(palette.is_vertical());
        assert_eq!(palette.height_color(0.0), palette.bands[0]);
        assert_eq!(palette.height_color(0.5), palette.bands[1]);
        assert_eq!(palette.height_color(1.0), palette.bands[2]);
        assert_eq!(palette.height_color(0.25), BarColor::rgb(100, 50, 0));
        assert_eq!(
            palette.mirror_height_color(1.0),
            palette.bands[2].mirror_color()
        );
        // Every bar starts from the same base color; all three colors stay editable.
        assert_eq!(palette.bar_color(20, 32), palette.bands[0]);
        assert!(palette.row_selectable(2) && palette.row_selectable(3));
        assert_eq!(palette.describe(), "Vertical gradient");
    }

    #[test]
    fn test_row_labels_name_the_base_middle_and_top_in_vertical_mode() {
        let mut palette = Palette::default();
        let labels = |palette: &Palette| {
            (0..Palette::ROWS)
                .map(|row| palette.row_label(row))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(&palette),
            ["Mode", "Bass", "Mids", "Treble", "Fade", "Intensity"]
        );
        palette.mode = ColorMode::Vertical;
        assert_eq!(
            labels(&palette),
            ["Mode", "Base", "Middle", "Top", "Fade", "Intensity"]
        );
    }

    #[test]
    fn test_gradient_with_one_bar_uses_the_bass_color() {
        let palette = Palette {
            mode: ColorMode::Gradient,
            ..Palette::default()
        };
        assert_eq!(palette.bar_color(0, 1), BarColor::CYAN);
    }

    #[test]
    fn test_lerp_blends_and_clamps() {
        let black = BarColor::rgb(0, 0, 0);
        let white = BarColor::rgb(255, 255, 255);
        assert_eq!(black.lerp(white, 0.0), black);
        assert_eq!(black.lerp(white, 1.0), white);
        assert_eq!(black.lerp(white, 0.5), BarColor::rgb(128, 128, 128));
        assert_eq!(black.lerp(white, 2.0), white);
    }

    #[test]
    fn test_fade_level_steps_within_its_limits_and_is_grayed_out_while_off() {
        let mut palette = Palette::default();
        assert_eq!(palette.fade_level_text(), "0.5");
        assert!((palette.fade_floor() - 0.6).abs() < 1e-6);
        for _ in 0..20 {
            palette.step_fade_level(true);
        }
        assert_eq!(palette.fade_level_text(), "1.0");
        assert!((palette.fade_floor() - 0.2).abs() < 1e-6);
        for _ in 0..20 {
            palette.step_fade_level(false);
        }
        assert_eq!(palette.fade_level_text(), "0.1");

        assert!(palette.row_selectable(Palette::FADE_LEVEL_ROW));
        palette.toggle_fade();
        assert!(!palette.row_selectable(Palette::FADE_LEVEL_ROW));
        assert_eq!(palette.fade_floor(), 1.0);
        assert_eq!(palette.next_row(Palette::FADE_ROW, true), 0);
    }

    #[test]
    fn test_cursor_skips_grayed_out_rows() {
        let mut palette = Palette::default();
        assert_eq!(palette.next_row(3, true), 4);
        assert_eq!(palette.next_row(4, true), 5);
        assert_eq!(palette.next_row(5, true), 0);
        palette.mode = ColorMode::Single;
        assert_eq!(palette.next_row(1, true), 4);
        assert_eq!(palette.next_row(4, false), 1);
        assert_eq!(palette.next_row(0, false), 5);
    }

    #[test]
    fn test_rainbow_runs_from_red_to_violet_and_ignores_the_picked_colors() {
        let palette = Palette {
            mode: ColorMode::Rainbow,
            ..Palette::default()
        };
        assert_eq!(palette.bar_color(0, 32), BarColor::rgb(255, 77, 77));
        assert_eq!(
            palette.bar_color(31, 32),
            BarColor::from_hsv(300.0, 0.70, 1.0)
        );
        // The hue only grows along the bars, and every bar has the same saturation, 70%.
        let hues: Vec<f32> = (0..32)
            .map(|i| palette.bar_color(i, 32).to_hsv().0)
            .collect();
        assert!(hues.windows(2).all(|pair| pair[0] < pair[1]));
        assert!((0..32).all(|i| (palette.bar_color(i, 32).to_hsv().1 - 0.70).abs() < 0.01));
        // Mids and treble rows, and the bass one, are grayed out; the mode and fade stay.
        assert!(palette.row_selectable(0) && palette.row_selectable(Palette::FADE_ROW));
        assert!((1..=3).all(|row| !palette.row_selectable(row)));
        assert_eq!(palette.next_row(0, true), Palette::FADE_ROW);
        assert_eq!(palette.describe(), "Rainbow");
    }

    #[test]
    fn test_row_color_is_none_for_the_mode_row() {
        let mut palette = Palette::default();
        assert_eq!(palette.row_color(0), None);
        // By default the mids and treble follow the bass.
        assert_eq!(palette.row_color(3), Some(BarColor::CYAN));
        palette.mode = ColorMode::Gradient;
        assert_eq!(palette.row_color(3), Some(BarColor::PINK));
    }
}
