use ratatui::style::Color;

/// Fraction of its brightness that a color keeps in the mirror reflection.
const MIRROR_BRIGHTNESS: f32 = 0.5;

/// A color the user can pick for the bars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarColor {
    Cyan,
    Green,
    Lime,
    Yellow,
    Orange,
    Red,
    Pink,
    Magenta,
    Purple,
    Blue,
    White,
    Gray,
}

impl BarColor {
    /// Every pickable color, in the order Left/Right cycles through them.
    pub const ALL: [BarColor; 12] = [
        BarColor::Cyan,
        BarColor::Green,
        BarColor::Lime,
        BarColor::Yellow,
        BarColor::Orange,
        BarColor::Red,
        BarColor::Pink,
        BarColor::Magenta,
        BarColor::Purple,
        BarColor::Blue,
        BarColor::White,
        BarColor::Gray,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BarColor::Cyan => "Cyan",
            BarColor::Green => "Green",
            BarColor::Lime => "Lime",
            BarColor::Yellow => "Yellow",
            BarColor::Orange => "Orange",
            BarColor::Red => "Red",
            BarColor::Pink => "Pink",
            BarColor::Magenta => "Magenta",
            BarColor::Purple => "Purple",
            BarColor::Blue => "Blue",
            BarColor::White => "White",
            BarColor::Gray => "Gray",
        }
    }

    /// The color as drawn on the bars. The classic ANSI ones follow the terminal's theme.
    pub fn color(self) -> Color {
        match self {
            BarColor::Cyan => Color::Cyan,
            BarColor::Green => Color::Green,
            BarColor::Yellow => Color::Yellow,
            BarColor::Red => Color::Red,
            BarColor::Magenta => Color::Magenta,
            BarColor::Blue => Color::Blue,
            BarColor::White => Color::White,
            other => {
                let (r, g, b) = other.rgb();
                Color::Rgb(r, g, b)
            }
        }
    }

    /// Fixed RGB value, used to derive the darker mirror tone.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            BarColor::Cyan => (0, 255, 255),
            BarColor::Green => (0, 205, 0),
            BarColor::Lime => (160, 255, 0),
            BarColor::Yellow => (255, 255, 0),
            BarColor::Orange => (255, 150, 0),
            BarColor::Red => (255, 0, 0),
            BarColor::Pink => (255, 105, 180),
            BarColor::Magenta => (255, 0, 255),
            BarColor::Purple => (150, 60, 240),
            BarColor::Blue => (60, 100, 255),
            BarColor::White => (255, 255, 255),
            BarColor::Gray => (150, 150, 150),
        }
    }

    /// The darker tone of this color used for the mirror reflection.
    pub fn mirror_color(self) -> Color {
        let dim = |channel: u8| (channel as f32 * MIRROR_BRIGHTNESS).round() as u8;
        let (r, g, b) = self.rgb();
        Color::Rgb(dim(r), dim(g), dim(b))
    }

    fn step(self, forward: bool) -> BarColor {
        let len = Self::ALL.len();
        let index = Self::ALL.iter().position(|&c| c == self).unwrap_or(0);
        Self::ALL[if forward {
            (index + 1) % len
        } else {
            (index + len - 1) % len
        }]
    }
}

/// How the bars are colored: one color per third of the spectrum, or a single color throughout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Whether every bar uses [`Palette::single`] instead of the three band colors.
    pub single_mode: bool,
    /// Colors of the bass, mids and treble thirds.
    pub bands: [BarColor; 3],
    /// Color of every bar in single-color mode.
    pub single: BarColor,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            single_mode: false,
            bands: [BarColor::Cyan, BarColor::Green, BarColor::Magenta],
            single: BarColor::Cyan,
        }
    }
}

impl Palette {
    /// Number of rows the picker shows: the mode row plus one per editable color.
    pub fn row_count(&self) -> usize {
        if self.single_mode { 2 } else { 4 }
    }

    /// The color a picker row edits (row 0 is the mode and has none).
    pub fn row_color(&self, row: usize) -> Option<BarColor> {
        match (row, self.single_mode) {
            (0, _) => None,
            (1, true) => Some(self.single),
            (1..=3, false) => Some(self.bands[row - 1]),
            _ => None,
        }
    }

    /// Row 0 flips the mode; the other rows cycle their color forward or backward.
    ///
    /// Switching to single-color mode starts from the bass color; each mode keeps its own colors.
    pub fn adjust(&mut self, row: usize, forward: bool) {
        match (row, self.single_mode) {
            (0, false) => {
                self.single_mode = true;
                self.single = self.bands[0];
            }
            (0, true) => self.single_mode = false,
            (1, true) => self.single = self.single.step(forward),
            (1..=3, false) => self.bands[row - 1] = self.bands[row - 1].step(forward),
            _ => {}
        }
    }

    /// The colors of the bass, mids and treble thirds.
    pub fn colors(&self) -> [Color; 3] {
        if self.single_mode {
            [self.single.color(); 3]
        } else {
            self.bands.map(BarColor::color)
        }
    }

    /// The darker mirror tones of the bass, mids and treble thirds.
    pub fn mirror_colors(&self) -> [Color; 3] {
        if self.single_mode {
            [self.single.mirror_color(); 3]
        } else {
            self.bands.map(BarColor::mirror_color)
        }
    }

    /// Short description for the on-screen notice.
    pub fn describe(&self) -> String {
        if self.single_mode {
            self.single.name().to_string()
        } else {
            "Gradient".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_keeps_the_original_cyan_green_magenta() {
        assert_eq!(
            Palette::default().colors(),
            [Color::Cyan, Color::Green, Color::Magenta]
        );
    }

    #[test]
    fn test_mirror_is_half_as_bright() {
        assert_eq!(BarColor::Cyan.mirror_color(), Color::Rgb(0, 128, 128));
        assert_eq!(BarColor::White.mirror_color(), Color::Rgb(128, 128, 128));
    }

    #[test]
    fn test_colors_cycle_both_ways() {
        assert_eq!(BarColor::Gray.step(true), BarColor::Cyan);
        assert_eq!(BarColor::Cyan.step(false), BarColor::Gray);
    }

    #[test]
    fn test_adjust_cycles_the_band_colors() {
        let mut palette = Palette::default();
        palette.adjust(2, true);
        assert_eq!(palette.bands[1], BarColor::Lime);
        palette.adjust(3, false);
        assert_eq!(palette.bands[2], BarColor::Pink);
    }

    #[test]
    fn test_single_mode_starts_from_bass_and_keeps_bands() {
        let mut palette = Palette::default();
        palette.adjust(1, true);
        palette.adjust(0, true);
        assert!(palette.single_mode);
        assert_eq!(palette.single, palette.bands[0]);
        assert_eq!(palette.row_count(), 2);

        palette.adjust(1, true);
        assert_eq!(palette.colors(), [BarColor::Lime.color(); 3]);
        assert_eq!(palette.mirror_colors(), [BarColor::Lime.mirror_color(); 3]);

        palette.adjust(0, true);
        assert!(!palette.single_mode);
        assert_eq!(palette.bands[0], BarColor::Green);
        assert_eq!(palette.row_count(), 4);
    }

    #[test]
    fn test_row_color_follows_the_mode() {
        let mut palette = Palette::default();
        assert_eq!(palette.row_color(0), None);
        assert_eq!(palette.row_color(3), Some(BarColor::Magenta));
        palette.single_mode = true;
        assert_eq!(palette.row_color(1), Some(BarColor::Cyan));
        assert_eq!(palette.row_color(2), None);
    }
}
