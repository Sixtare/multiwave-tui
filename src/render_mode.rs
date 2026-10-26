//! How the bars are drawn: Unicode block glyphs or braille dots.

use crate::bar_layout::CellHalf;

/// The glyph set the visualizer draws the bars with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderMode {
    /// Base-anchored block glyphs (` ▂▃▄▅▆▇█`): 8 steps per cell row.
    #[default]
    Blocks,
    /// Braille dots, like a canvas: 4 steps per cell row on a 2x4 dot grid per cell.
    Canvas,
}

impl RenderMode {
    /// The other mode.
    pub fn toggled(self) -> RenderMode {
        match self {
            RenderMode::Blocks => RenderMode::Canvas,
            RenderMode::Canvas => RenderMode::Blocks,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RenderMode::Blocks => "blocks",
            RenderMode::Canvas => "canvas",
        }
    }
}

/// Braille dot bits of one dot column (left, then right), listed from the bottom dot up.
const BOTTOM_UP: [u8; 8] = [0x40, 0x04, 0x02, 0x01, 0x80, 0x20, 0x10, 0x08];

/// The braille glyph of one cell with `dots` (0 to 4) filled rows, in the dot columns `half`
/// covers; a plain space when empty, like the block glyphs.
/// `from_bottom` fills from the bottom of the cell (bars growing up); otherwise from the top
/// (the reflection growing down).
pub fn braille_glyph(dots: usize, from_bottom: bool, half: CellHalf) -> char {
    let dots = dots.min(4);
    if dots == 0 {
        return ' ';
    }
    let mut mask = 0u8;
    for level in 0..dots {
        // Dot rows are stored bottom-up; the top-down fill takes them in the opposite order.
        let row = if from_bottom { level } else { 3 - level };
        mask |= match half {
            CellHalf::Full => BOTTOM_UP[row] | BOTTOM_UP[row + 4],
            CellHalf::Left => BOTTOM_UP[row],
            CellHalf::Right => BOTTOM_UP[row + 4],
        };
    }
    char::from_u32(0x2800 + mask as u32).expect("braille block is valid")
}

/// The dot bits of a braille glyph; 0 for a space or any other character.
pub fn braille_mask(glyph: char) -> u8 {
    match glyph as u32 {
        code @ 0x2800..=0x28FF => (code - 0x2800) as u8,
        _ => 0,
    }
}

/// Filled dots (0 to 4) of the cell at `row` of a column `rows` cells tall, for a bar of
/// `amplitude` (0.0 to 1.0).
pub fn cell_dots(amplitude: f32, row: usize, rows: usize) -> usize {
    let height = (amplitude.clamp(0.0, 1.0) * (rows * 4) as f32).round() as usize;
    height.saturating_sub(row * 4).min(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_braille_fills_from_the_requested_side() {
        assert_eq!(braille_glyph(0, true, CellHalf::Full), ' ');
        assert_eq!(braille_glyph(1, true, CellHalf::Full), '⣀');
        assert_eq!(braille_glyph(2, true, CellHalf::Full), '⣤');
        assert_eq!(braille_glyph(3, true, CellHalf::Full), '⣶');
        assert_eq!(braille_glyph(4, true, CellHalf::Full), '⣿');
        assert_eq!(braille_glyph(1, false, CellHalf::Full), '⠉');
        assert_eq!(braille_glyph(2, false, CellHalf::Full), '⠛');
        assert_eq!(braille_glyph(3, false, CellHalf::Full), '⠿');
        assert_eq!(braille_glyph(4, false, CellHalf::Full), '⣿');
    }

    #[test]
    fn test_braille_halves_fill_one_dot_column_each() {
        assert_eq!(braille_glyph(4, true, CellHalf::Left), '⡇');
        assert_eq!(braille_glyph(4, true, CellHalf::Right), '⢸');
        let both = braille_mask(braille_glyph(2, true, CellHalf::Left))
            | braille_mask(braille_glyph(3, true, CellHalf::Right));
        assert_eq!(both, 0x40 | 0x04 | 0x80 | 0x20 | 0x10);
        assert_eq!(braille_mask(' '), 0);
    }

    #[test]
    fn test_cell_dots_spread_the_height_over_the_rows() {
        // 2 rows = 8 dots; half the height fills the first row only.
        assert_eq!(cell_dots(0.5, 0, 2), 4);
        assert_eq!(cell_dots(0.5, 1, 2), 0);
        assert_eq!(cell_dots(0.75, 1, 2), 2);
        assert_eq!(cell_dots(0.0, 0, 2), 0);
        assert_eq!(cell_dots(2.0, 1, 2), 4);
    }

    #[test]
    fn test_toggle_switches_between_the_modes() {
        assert_eq!(RenderMode::Blocks.toggled(), RenderMode::Canvas);
        assert_eq!(RenderMode::Canvas.toggled(), RenderMode::Blocks);
    }
}
