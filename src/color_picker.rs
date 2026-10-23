use crate::palette::BarColor;

/// Columns and rows of the saturation/value square.
pub const SQUARE_COLS: usize = 20;
pub const SQUARE_ROWS: usize = 10;
/// Rows of the hue strip, which is as tall as the square.
pub const HUE_ROWS: usize = SQUARE_ROWS;

/// Step of one key press. Saturation and value move the square's cursor exactly one cell, so
/// every press visibly moves it. The hue goes in 10 degree steps, small enough to tell
/// neighbouring shades apart (red and pink are several presses apart); the square's colors
/// change with it. Holding Shift divides every step by [`FINE`] for precise picks.
const SATURATION_STEP: f32 = 1.0 / (SQUARE_COLS - 1) as f32;
const VALUE_STEP: f32 = 1.0 / (SQUARE_ROWS - 1) as f32;
const HUE_STEP: f32 = 10.0;
const FINE: f32 = 5.0;

/// Which part of the color picker the arrow keys and typing act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerFocus {
    /// The saturation (columns) and value (rows) square.
    Square,
    /// The vertical hue strip.
    Hue,
}

/// State of the custom color picker: a hue strip and a saturation/value square, with the picked
/// color's `#RRGGBB` shown below.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorPicker {
    /// Hue in degrees, 0 to 360.
    pub hue: f32,
    /// Saturation, 0 to 1 (left to right in the square).
    pub saturation: f32,
    /// Value (brightness), 0 to 1 (bottom to top in the square).
    pub value: f32,
    pub focus: PickerFocus,
    color: BarColor,
}

impl ColorPicker {
    /// Opens the picker on `color`.
    pub fn new(color: BarColor) -> Self {
        let (hue, saturation, value) = color.to_hsv();
        Self {
            hue,
            saturation,
            value,
            focus: PickerFocus::Square,
            color,
        }
    }

    /// The color currently picked.
    pub fn color(&self) -> BarColor {
        self.color
    }

    /// Switches the focus between the square and the hue strip.
    pub fn next_focus(&mut self) {
        self.focus = match self.focus {
            PickerFocus::Square => PickerFocus::Hue,
            PickerFocus::Hue => PickerFocus::Square,
        };
    }

    /// Moves the square cursor (`dx` right, `dy` down) or the hue (`dy` down), by one step or a
    /// fine one.
    pub fn nudge(&mut self, dx: i32, dy: i32, fine: bool) {
        let scale = if fine { 1.0 / FINE } else { 1.0 };
        match self.focus {
            PickerFocus::Square => {
                self.saturation =
                    (self.saturation + dx as f32 * SATURATION_STEP * scale).clamp(0.0, 1.0);
                self.value = (self.value - dy as f32 * VALUE_STEP * scale).clamp(0.0, 1.0);
            }
            PickerFocus::Hue => {
                self.hue = (self.hue + dy as f32 * HUE_STEP * scale).clamp(0.0, 360.0);
            }
        }
        self.color = BarColor::from_hsv(self.hue, self.saturation, self.value);
    }

    /// Cursor position in the square as (column, row).
    pub fn square_cursor(&self) -> (usize, usize) {
        (
            (self.saturation * (SQUARE_COLS - 1) as f32).round() as usize,
            ((1.0 - self.value) * (SQUARE_ROWS - 1) as f32).round() as usize,
        )
    }

    /// Row of the hue marker in the strip.
    pub fn hue_row(&self) -> usize {
        (self.hue / 360.0 * (HUE_ROWS - 1) as f32).round() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opens_on_the_given_color() {
        let picker = ColorPicker::new(BarColor::CYAN);
        assert_eq!(picker.color(), BarColor::CYAN);
        assert_eq!(picker.focus, PickerFocus::Square);
        assert_eq!(picker.square_cursor(), (SQUARE_COLS - 1, 0));
    }

    #[test]
    fn test_every_press_moves_the_square_cursor_one_cell() {
        let mut picker = ColorPicker::new(BarColor::rgb(255, 0, 0));
        let (col, row) = picker.square_cursor();
        for presses in 1..=5 {
            picker.nudge(-1, 1, false);
            assert_eq!(picker.square_cursor(), (col - presses, row + presses));
        }
        for presses in (0..5).rev() {
            picker.nudge(1, -1, false);
            assert_eq!(picker.square_cursor(), (col - presses, row + presses));
        }
    }

    #[test]
    fn test_hue_steps_reach_the_shades_between_red_and_pink() {
        let mut picker = ColorPicker::new(BarColor::rgb(255, 0, 0));
        picker.next_focus();
        // Red is hue 0 and a pink-red is a few steps before 360, so go the other way round.
        picker.nudge(0, 1000, false);
        let mut seen = vec![picker.color()];
        for _ in 0..3 {
            picker.nudge(0, -1, false);
            seen.push(picker.color());
        }
        assert_eq!(picker.hue, 330.0);
        assert!(seen.windows(2).all(|pair| pair[0] != pair[1]));
        assert_eq!(seen[3], BarColor::rgb(255, 0, 128));
    }

    #[test]
    fn test_fine_steps_are_smaller_than_a_cell() {
        let mut picker = ColorPicker::new(BarColor::rgb(255, 0, 0));
        let before = picker.color();
        picker.nudge(-1, 0, true);
        assert_ne!(picker.color(), before);
        assert_eq!(picker.square_cursor().0, SQUARE_COLS - 1);
    }

    #[test]
    fn test_square_moves_saturation_and_value() {
        let mut picker = ColorPicker::new(BarColor::rgb(255, 0, 0));
        picker.nudge(-1, 1, false);
        assert!(picker.saturation < 1.0 && picker.value < 1.0);
        assert_ne!(picker.color(), BarColor::rgb(255, 0, 0));

        picker.nudge(100, -100, false);
        assert_eq!((picker.saturation, picker.value), (1.0, 1.0));
        assert_eq!(picker.color(), BarColor::rgb(255, 0, 0));
    }

    #[test]
    fn test_tab_switches_between_square_and_hue_strip() {
        let mut picker = ColorPicker::new(BarColor::CYAN);
        picker.next_focus();
        assert_eq!(picker.focus, PickerFocus::Hue);
        picker.next_focus();
        assert_eq!(picker.focus, PickerFocus::Square);
    }

    #[test]
    fn test_hue_strip_moves_only_the_hue() {
        let mut picker = ColorPicker::new(BarColor::rgb(255, 0, 0));
        picker.next_focus();
        assert_eq!(picker.focus, PickerFocus::Hue);
        picker.nudge(5, 0, false);
        assert_eq!(picker.hue, 0.0);
        picker.nudge(0, 1, false);
        assert_eq!(picker.hue, 10.0);
        picker.nudge(0, 1, true);
        assert!((picker.hue - 12.0).abs() < 1e-3);
        picker.nudge(0, -1, false);
        picker.nudge(0, -1, true);
        assert_eq!(picker.hue, 0.0);
        assert_eq!((picker.saturation, picker.value), (1.0, 1.0));
        picker.nudge(0, -100, false);
        assert_eq!(picker.hue, 0.0);
        picker.nudge(0, 1000, false);
        assert_eq!(picker.hue, 360.0);
    }
}
