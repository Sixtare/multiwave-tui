use ratatui::style::Color;

/// Fraction of its brightness that a color keeps in the mirror reflection.
const MIRROR_BRIGHTNESS: f32 = 0.5;

/// A color the user can pick for the bars: any 24-bit RGB value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl BarColor {
    pub const CYAN: BarColor = BarColor::rgb(0, 255, 255);
    pub const GREEN: BarColor = BarColor::rgb(0, 205, 0);
    pub const MAGENTA: BarColor = BarColor::rgb(255, 0, 255);

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
    /// One flat color per third of the spectrum.
    Bands,
    /// A smooth blend: bass at the left end, mids in the middle and treble at the right end.
    Gradient,
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
            ColorMode::Vertical => ColorMode::Bands,
            ColorMode::Bands => ColorMode::Single,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ColorMode::Bands => "Bands",
            ColorMode::Gradient => "Gradient",
            ColorMode::Vertical => "Vertical gradient",
            ColorMode::Single => "Single color",
        }
    }
}

/// How the bars are colored: by bands, by a smooth gradient or with the bass color throughout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub mode: ColorMode,
    /// Colors of the bass, mids and treble. The mids and treble colors are kept while
    /// single-color mode hides them.
    pub bands: [BarColor; 3],
    /// Whether the mirror reflection is a darker tone of the bar colors. Off, it uses the same
    /// colors as the bars.
    pub mirror_dimmed: bool,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            mode: ColorMode::Single,
            bands: [BarColor::CYAN, BarColor::GREEN, BarColor::MAGENTA],
            mirror_dimmed: true,
        }
    }
}

impl Palette {
    /// Rows the picker shows: the mode row, bass, mids, treble and the mirror checkbox.
    pub const ROWS: usize = 5;
    /// The row of the mirror checkbox.
    pub const MIRROR_ROW: usize = 4;

    /// Whether the cursor can rest on `row`. In single-color mode the mids and treble are grayed
    /// out.
    pub fn row_selectable(&self, row: usize) -> bool {
        row < Self::ROWS && !(self.mode == ColorMode::Single && (2..=3).contains(&row))
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

    /// The color a picker row shows (rows 0 and 4 have none). In single-color mode the mids and
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

    /// Switches to the next color mode: bands, gradient, single color.
    pub fn next_mode(&mut self) {
        self.mode = self.mode.next();
    }

    /// Turns the darker mirror tone on or off.
    pub fn toggle_mirror_dimmed(&mut self) {
        self.mirror_dimmed = !self.mirror_dimmed;
    }

    /// Sets the color a picker row edits. Only the bass can be edited in single-color mode.
    pub fn set_row_color(&mut self, row: usize, color: BarColor) {
        if (1..=3).contains(&row) && self.row_selectable(row) {
            self.bands[row - 1] = color;
        }
    }

    /// The color of bar `bar` out of `bar_count`. In vertical mode that is the color of its base.
    pub fn bar_color(&self, bar: usize, bar_count: usize) -> BarColor {
        let [bass, mids, treble] = self.bands;
        match self.mode {
            ColorMode::Single | ColorMode::Vertical => bass,
            ColorMode::Bands => {
                if bar < bar_count / 3 {
                    bass
                } else if bar < (bar_count * 2) / 3 {
                    mids
                } else {
                    treble
                }
            }
            ColorMode::Gradient => {
                // Bass at the first bar, mids at the middle one and treble at the last.
                let t = if bar_count > 1 {
                    bar as f32 / (bar_count - 1) as f32
                } else {
                    0.0
                };
                self.blend(t)
            }
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
        self.reflection(self.height_color(t))
    }

    /// A reflection's tone for `color`: darker unless the mirror is not dimmed.
    fn reflection(&self, color: BarColor) -> Color {
        if self.mirror_dimmed {
            color.mirror_color()
        } else {
            color.color()
        }
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
            _ => "Mirror",
        }
    }

    /// The color of bar `bar`'s reflection: a darker tone of its color unless the mirror is not
    /// dimmed.
    pub fn mirror_bar_color(&self, bar: usize, bar_count: usize) -> Color {
        self.reflection(self.bar_color(bar, bar_count))
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
            [BarColor::CYAN, BarColor::GREEN, BarColor::MAGENTA]
        );
        for bar in [0, 15, 31] {
            assert_eq!(palette.bar_color(bar, 32), BarColor::CYAN);
        }
    }

    #[test]
    fn test_bands_use_one_flat_color_per_third() {
        let palette = Palette {
            mode: ColorMode::Bands,
            ..Palette::default()
        };
        let colors: Vec<BarColor> = [0, 9, 10, 20, 21, 31]
            .iter()
            .map(|&bar| palette.bar_color(bar, 32))
            .collect();
        assert_eq!(
            colors,
            [
                BarColor::CYAN,
                BarColor::CYAN,
                BarColor::GREEN,
                BarColor::GREEN,
                BarColor::MAGENTA,
                BarColor::MAGENTA
            ]
        );
    }

    #[test]
    fn test_mirror_is_half_as_bright() {
        assert_eq!(BarColor::CYAN.mirror_color(), Color::Rgb(0, 128, 128));
        assert_eq!(
            BarColor::rgb(255, 255, 255).mirror_color(),
            Color::Rgb(128, 128, 128)
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
            BarColor::GREEN,
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
            mode: ColorMode::Bands,
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
    fn test_mode_cycles_single_gradient_vertical_bands() {
        let mut palette = Palette::default();
        assert_eq!(palette.mode, ColorMode::Single);
        palette.next_mode();
        assert_eq!(palette.mode, ColorMode::Gradient);
        palette.next_mode();
        assert_eq!(palette.mode, ColorMode::Vertical);
        palette.next_mode();
        assert_eq!(palette.mode, ColorMode::Bands);
        palette.next_mode();
        assert_eq!(palette.mode, ColorMode::Single);
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
        palette.mode = ColorMode::Bands;
        assert!(palette.row_selectable(2));
        assert_eq!(palette.bands, [custom, BarColor::GREEN, BarColor::MAGENTA]);
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

        // No jump between neighbours, unlike the flat bands, and every bar differs from the next.
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
            ["Mode", "Bass", "Mids", "Treble", "Mirror"]
        );
        palette.mode = ColorMode::Vertical;
        assert_eq!(
            labels(&palette),
            ["Mode", "Base", "Middle", "Top", "Mirror"]
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
    fn test_cursor_skips_grayed_out_rows() {
        let mut palette = Palette::default();
        assert_eq!(palette.next_row(3, true), 4);
        assert_eq!(palette.next_row(4, true), 0);
        palette.mode = ColorMode::Single;
        assert_eq!(palette.next_row(1, true), 4);
        assert_eq!(palette.next_row(4, false), 1);
        assert_eq!(palette.next_row(0, false), 4);
    }

    #[test]
    fn test_mirror_without_dimming_uses_the_bar_colors() {
        let mut palette = Palette::default();
        for mode in [ColorMode::Bands, ColorMode::Gradient, ColorMode::Single] {
            palette.mode = mode;
            palette.mirror_dimmed = true;
            let bar = palette.bar_color(5, 32);
            assert_eq!(palette.mirror_bar_color(5, 32), bar.mirror_color());
            palette.toggle_mirror_dimmed();
            assert_eq!(palette.mirror_bar_color(5, 32), bar.color());
        }
        // The checkbox row never edits a band.
        palette.set_row_color(Palette::MIRROR_ROW, BarColor::rgb(255, 0, 0));
        assert_eq!(palette.bands[0], BarColor::CYAN);
    }

    #[test]
    fn test_row_color_is_none_for_the_mode_row() {
        let mut palette = Palette::default();
        assert_eq!(palette.row_color(0), None);
        // By default the mids and treble follow the bass.
        assert_eq!(palette.row_color(3), Some(BarColor::CYAN));
        palette.mode = ColorMode::Bands;
        assert_eq!(palette.row_color(3), Some(BarColor::MAGENTA));
    }
}
