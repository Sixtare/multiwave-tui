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
            bands: [BarColor::Cyan, BarColor::Green, BarColor::Magenta],
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
            self.bands[0].name().to_string()
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
    fn test_set_row_color_edits_the_band() {
        let mut palette = Palette::default();
        palette.set_row_color(2, BarColor::Lime);
        palette.set_row_color(3, BarColor::Pink);
        palette.set_row_color(0, BarColor::Red);
        assert_eq!(palette.bands[1], BarColor::Lime);
        assert_eq!(palette.bands[2], BarColor::Pink);
        assert_eq!(palette.bands[0], BarColor::Cyan);
    }

    #[test]
    fn test_single_mode_makes_mids_and_treble_follow_the_bass() {
        let mut palette = Palette::default();
        palette.toggle_mode();
        assert!(palette.single_mode);
        assert!(!palette.row_selectable(2));
        assert!(!palette.row_selectable(3));
        assert_eq!(palette.row_color(3), Some(BarColor::Cyan));

        palette.set_row_color(1, BarColor::Lime);
        // Mids and treble cannot be edited while they follow the bass.
        palette.set_row_color(2, BarColor::Red);
        assert_eq!(palette.colors(), [BarColor::Lime.color(); 3]);
        assert_eq!(palette.mirror_colors(), [BarColor::Lime.mirror_color(); 3]);
        assert_eq!(palette.describe(), "Lime");

        // Going back to the gradient brings back the mids and treble colors.
        palette.toggle_mode();
        assert!(palette.row_selectable(2));
        assert_eq!(
            palette.bands,
            [BarColor::Lime, BarColor::Green, BarColor::Magenta]
        );
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
        assert_eq!(palette.mirror_colors(), [Color::Cyan; 3]);
        // The checkbox row never edits a band.
        palette.set_row_color(Palette::MIRROR_ROW, BarColor::Red);
        assert_eq!(palette.bands[0], BarColor::Cyan);
    }

    #[test]
    fn test_row_color_is_none_for_the_mode_row() {
        let palette = Palette::default();
        assert_eq!(palette.row_color(0), None);
        assert_eq!(palette.row_color(3), Some(BarColor::Magenta));
    }
}
