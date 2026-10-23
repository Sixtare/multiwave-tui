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
/// Most hex digits the hex field takes.
const HEX_DIGITS: usize = 6;

/// Which part of the color picker the arrow keys and typing act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerFocus {
    /// The saturation (columns) and value (rows) square.
    Square,
    /// The vertical hue strip.
    Hue,
    /// The `#RRGGBB` text field.
    Hex,
}

/// State of the custom color picker: a hue strip, a saturation/value square and a hex field.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorPicker {
    /// Hue in degrees, 0 to 360.
    pub hue: f32,
    /// Saturation, 0 to 1 (left to right in the square).
    pub saturation: f32,
    /// Value (brightness), 0 to 1 (bottom to top in the square).
    pub value: f32,
    pub focus: PickerFocus,
    /// Hex digits typed so far, without the `#`.
    pub hex: String,
    color: BarColor,
    /// Whether the next digit typed starts a new value instead of extending the shown one.
    replace_on_type: bool,
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
            hex: color.hex()[1..].to_string(),
            color,
            replace_on_type: true,
        }
    }

    /// The color currently picked.
    pub fn color(&self) -> BarColor {
        self.color
    }

    /// Moves the focus to the next part (square, hue strip, hex field).
    pub fn next_focus(&mut self) {
        self.focus = match self.focus {
            PickerFocus::Square => PickerFocus::Hue,
            PickerFocus::Hue => PickerFocus::Hex,
            PickerFocus::Hex => PickerFocus::Square,
        };
        // A half-typed hex value is dropped when leaving the field.
        self.hex = self.color.hex()[1..].to_string();
        self.replace_on_type = true;
    }

    /// Moves the square cursor (`dx` right, `dy` down) or the hue (`dy` down), by one step or a
    /// coarse one. Does nothing in the hex field.
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
            PickerFocus::Hex => return,
        }
        self.color = BarColor::from_hsv(self.hue, self.saturation, self.value);
        self.hex = self.color.hex()[1..].to_string();
        self.replace_on_type = true;
    }

    /// Types one hex digit into the hex field; a complete value becomes the picked color.
    pub fn type_char(&mut self, ch: char) {
        if self.focus != PickerFocus::Hex || !ch.is_ascii_hexdigit() {
            return;
        }
        // The first digit typed after focusing the field replaces the whole value.
        if self.replace_on_type {
            self.hex.clear();
            self.replace_on_type = false;
        }
        if self.hex.len() >= HEX_DIGITS {
            return;
        }
        self.hex.push(ch.to_ascii_uppercase());
        if let Some(color) = BarColor::parse_hex(&self.hex) {
            self.set_color(color);
        }
    }

    /// Deletes the last typed hex digit.
    pub fn backspace(&mut self) {
        if self.focus == PickerFocus::Hex {
            self.hex.pop();
            self.replace_on_type = false;
        }
    }

    fn set_color(&mut self, color: BarColor) {
        self.color = color;
        let (hue, saturation, value) = color.to_hsv();
        // A gray has no hue of its own, so keep the one on the strip.
        if saturation > 0.0 && value > 0.0 {
            self.hue = hue;
        }
        self.saturation = saturation;
        self.value = value;
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
        assert_eq!(picker.hex, "00FFFF");
        assert_eq!(picker.focus, PickerFocus::Square);
        assert_eq!(picker.square_cursor(), (SQUARE_COLS - 1, 0));
    }

    #[test]
    fn test_square_moves_saturation_and_value() {
        let mut picker = ColorPicker::new(BarColor::rgb(255, 0, 0));
        picker.nudge(-1, 1, false);
        assert!(picker.saturation < 1.0 && picker.value < 1.0);
        assert_ne!(picker.color(), BarColor::rgb(255, 0, 0));
        assert_eq!(picker.hex, picker.color().hex()[1..]);

        picker.nudge(100, -100, true);
        assert_eq!((picker.saturation, picker.value), (1.0, 1.0));
        assert_eq!(picker.color(), BarColor::rgb(255, 0, 0));
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

    #[test]
    fn test_typing_hex_picks_the_color_once_complete() {
        let mut picker = ColorPicker::new(BarColor::CYAN);
        picker.next_focus();
        picker.next_focus();
        assert_eq!(picker.focus, PickerFocus::Hex);

        for ch in "ff8000".chars().take(5) {
            picker.type_char(ch);
        }
        assert_eq!(picker.hex, "FF800");
        assert_eq!(picker.color(), BarColor::CYAN);

        picker.type_char('z');
        picker.type_char('0');
        assert_eq!(picker.color(), BarColor::rgb(255, 128, 0));
        // Full: more digits are ignored.
        picker.type_char('1');
        assert_eq!(picker.hex, "FF8000");
        assert!(picker.saturation > 0.99);
    }

    #[test]
    fn test_backspace_and_leaving_the_field_drop_a_partial_value() {
        let mut picker = ColorPicker::new(BarColor::CYAN);
        picker.next_focus();
        picker.next_focus();
        picker.type_char('a');
        picker.type_char('b');
        assert_eq!(picker.hex, "AB");
        picker.backspace();
        assert_eq!(picker.hex, "A");

        picker.next_focus();
        assert_eq!(picker.hex, "00FFFF");
        assert_eq!(picker.color(), BarColor::CYAN);
    }

    #[test]
    fn test_typing_a_gray_keeps_the_hue() {
        let mut picker = ColorPicker::new(BarColor::rgb(0, 0, 255));
        picker.next_focus();
        picker.next_focus();
        for ch in "808080".chars() {
            picker.type_char(ch);
        }
        assert_eq!(picker.color(), BarColor::rgb(128, 128, 128));
        assert_eq!(picker.hue, 240.0);
    }

    #[test]
    fn test_digits_are_ignored_outside_the_hex_field() {
        let mut picker = ColorPicker::new(BarColor::CYAN);
        picker.type_char('a');
        picker.backspace();
        assert_eq!(picker.hex, "00FFFF");
    }
}
