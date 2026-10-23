use crate::palette::BarColor;

/// Columns and rows of the saturation/value square.
pub const SQUARE_COLS: usize = 20;
pub const SQUARE_ROWS: usize = 10;
/// Rows of the hue strip, which is as tall as the square.
pub const HUE_ROWS: usize = SQUARE_ROWS;

/// Step of one key press on saturation, value and hue; Shift multiplies it by [`COARSE`].
const FINE_UNIT_STEP: f32 = 0.02;
const FINE_HUE_STEP: f32 = 3.0;
const COARSE: f32 = 5.0;

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
    /// coarse one.
    pub fn nudge(&mut self, dx: i32, dy: i32, coarse: bool) {
        let scale = if coarse { COARSE } else { 1.0 };
        match self.focus {
            PickerFocus::Square => {
                self.saturation =
                    (self.saturation + dx as f32 * FINE_UNIT_STEP * scale).clamp(0.0, 1.0);
                self.value = (self.value - dy as f32 * FINE_UNIT_STEP * scale).clamp(0.0, 1.0);
            }
            PickerFocus::Hue => {
                self.hue = (self.hue + dy as f32 * FINE_HUE_STEP * scale).clamp(0.0, 360.0);
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
    fn test_square_moves_saturation_and_value() {
        let mut picker = ColorPicker::new(BarColor::rgb(255, 0, 0));
        picker.nudge(-1, 1, false);
        assert!(picker.saturation < 1.0 && picker.value < 1.0);
        assert_ne!(picker.color(), BarColor::rgb(255, 0, 0));

        picker.nudge(100, -100, true);
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
        picker.nudge(0, 1, true);
        assert_eq!(picker.hue, 15.0);
        assert_eq!((picker.saturation, picker.value), (1.0, 1.0));
        picker.nudge(0, -100, false);
        assert_eq!(picker.hue, 0.0);
        picker.nudge(0, 1000, false);
        assert_eq!(picker.hue, 360.0);
    }
}
