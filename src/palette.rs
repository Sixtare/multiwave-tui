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

/// How the bars are colored: one color per third of the spectrum, or the bass color throughout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Whether the mids and treble follow the bass color instead of using their own.
    pub single_mode: bool,
    /// Colors of the bass, mids and treble thirds. The mids and treble colors are kept while
    /// single-color mode hides them.
    pub bands: [BarColor; 3],
    /// Whether the mirror reflection is a darker tone of the bar colors. Off, it uses the same
    /// colors as the bars.
    pub mirror_dimmed: bool,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            single_mode: false,
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
        row < Self::ROWS && !(self.single_mode && (2..=3).contains(&row))
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

    /// The color a band is drawn with.
    fn band_color(&self, band: usize) -> BarColor {
        if self.single_mode {
            self.bands[0]
        } else {
            self.bands[band]
        }
    }

    /// Flips between gradient and single-color mode.
    pub fn toggle_mode(&mut self) {
        self.single_mode = !self.single_mode;
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

    /// The colors of the bass, mids and treble thirds.
    pub fn colors(&self) -> [Color; 3] {
        [0, 1, 2].map(|band| self.band_color(band).color())
    }

    /// The mirror tones (darker unless the mirror is not dimmed) of the bass, mids and treble thirds.
    pub fn mirror_colors(&self) -> [Color; 3] {
        if !self.mirror_dimmed {
            return self.colors();
        }
        [0, 1, 2].map(|band| self.band_color(band).mirror_color())
    }

    /// Short description for the on-screen notice.
    pub fn describe(&self) -> String {
        if self.single_mode {
            self.bands[0].hex()
        } else {
            "Gradient".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_keeps_cyan_green_magenta() {
        assert_eq!(
            Palette::default().colors(),
            [
                Color::Rgb(0, 255, 255),
                Color::Rgb(0, 205, 0),
                Color::Rgb(255, 0, 255)
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
        let mut palette = Palette::default();
        palette.set_row_color(2, BarColor::rgb(1, 2, 3));
        palette.set_row_color(3, BarColor::rgb(4, 5, 6));
        palette.set_row_color(0, BarColor::rgb(7, 8, 9));
        assert_eq!(palette.bands[1], BarColor::rgb(1, 2, 3));
        assert_eq!(palette.bands[2], BarColor::rgb(4, 5, 6));
        assert_eq!(palette.bands[0], BarColor::CYAN);
    }

    #[test]
    fn test_single_mode_makes_mids_and_treble_follow_the_bass() {
        let mut palette = Palette::default();
        palette.toggle_mode();
        assert!(palette.single_mode);
        assert!(!palette.row_selectable(2));
        assert!(!palette.row_selectable(3));
        assert_eq!(palette.row_color(3), Some(BarColor::CYAN));

        let custom = BarColor::rgb(10, 20, 30);
        palette.set_row_color(1, custom);
        // Mids and treble cannot be edited while they follow the bass.
        palette.set_row_color(2, BarColor::rgb(255, 0, 0));
        assert_eq!(palette.colors(), [custom.color(); 3]);
        assert_eq!(palette.mirror_colors(), [custom.mirror_color(); 3]);
        assert_eq!(palette.describe(), "#0A141E");

        // Going back to the gradient brings back the mids and treble colors.
        palette.toggle_mode();
        assert!(palette.row_selectable(2));
        assert_eq!(palette.bands, [custom, BarColor::GREEN, BarColor::MAGENTA]);
    }

    #[test]
    fn test_cursor_skips_grayed_out_rows() {
        let mut palette = Palette::default();
        assert_eq!(palette.next_row(3, true), 4);
        assert_eq!(palette.next_row(4, true), 0);
        palette.toggle_mode();
        assert_eq!(palette.next_row(1, true), 4);
        assert_eq!(palette.next_row(4, false), 1);
        assert_eq!(palette.next_row(0, false), 4);
    }

    #[test]
    fn test_mirror_without_dimming_uses_the_bar_colors() {
        let mut palette = Palette::default();
        assert_ne!(palette.mirror_colors(), palette.colors());
        palette.toggle_mirror_dimmed();
        assert_eq!(palette.mirror_colors(), palette.colors());
        palette.toggle_mode();
        assert_eq!(palette.mirror_colors(), [BarColor::CYAN.color(); 3]);
        // The checkbox row never edits a band.
        palette.set_row_color(Palette::MIRROR_ROW, BarColor::rgb(255, 0, 0));
        assert_eq!(palette.bands[0], BarColor::CYAN);
    }

    #[test]
    fn test_row_color_is_none_for_the_mode_row() {
        let palette = Palette::default();
        assert_eq!(palette.row_color(0), None);
        assert_eq!(palette.row_color(3), Some(BarColor::MAGENTA));
    }
}
