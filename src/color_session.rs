//! The state of one visit to the bar color popup: the highlighted row and the color picker.
//!
//! The session edits the app's [`Palette`] live as a preview, so every operation takes the palette
//! it acts on. It remembers what the palette looked like when it opened, to tell whether the visit
//! changed anything, and the color from before a picker opened, to restore it on cancel.

use crate::{
    color_picker::ColorPicker,
    palette::{BarColor, Palette},
};

#[derive(Debug)]
pub struct ColorSession {
    /// Highlighted row of the popup (0 is the mode row).
    pub row: usize,
    /// The color picker, while it is open for the highlighted row.
    pub picker: Option<ColorPicker>,
    /// Palette from before the popup opened, to tell whether it changed.
    palette_at_open: Palette,
    /// Color of the edited row from before the picker opened, restored when it is cancelled.
    color_before_picker: BarColor,
}

impl Default for ColorSession {
    fn default() -> Self {
        Self::open(&Palette::default())
    }
}

impl ColorSession {
    /// The row of the render mode, after the palette's own rows. The rows from here on are
    /// toggled by the app, since they are not part of the palette.
    pub const RENDER_ROW: usize = Palette::ROWS;
    /// The row of the gap between bars.
    pub const GAP_ROW: usize = Self::RENDER_ROW + 1;
    /// The row of the mirror, the last one.
    pub const MIRROR_ROW: usize = Self::RENDER_ROW + 2;

    /// Starts a session on the mode row for `palette`.
    pub fn open(palette: &Palette) -> Self {
        Self {
            row: 0,
            picker: None,
            palette_at_open: *palette,
            color_before_picker: BarColor::CYAN,
        }
    }

    /// Ends the session, keeping every color chosen. Returns whether `palette` differs from how
    /// it was when the session opened.
    pub fn close(&mut self, palette: &Palette) -> bool {
        self.picker = None;
        *palette != self.palette_at_open
    }

    /// Moves the cursor down, skipping grayed-out rows. The rows after the palette's (render, gap,
    /// mirror) are always selectable; the cursor wraps from the last one to the first.
    pub fn select_next(&mut self, palette: &Palette) {
        self.row = match self.row {
            Self::MIRROR_ROW => 0,
            row if row >= Self::RENDER_ROW => row + 1,
            row => match palette.next_row(row, true) {
                wrapped if wrapped <= row => Self::RENDER_ROW,
                next => next,
            },
        };
    }

    /// Moves the cursor up, skipping grayed-out rows.
    pub fn select_prev(&mut self, palette: &Palette) {
        self.row = match self.row {
            Self::RENDER_ROW => Palette::FADE_ROW,
            row if row > Self::RENDER_ROW => row - 1,
            row => match palette.next_row(row, false) {
                wrapped if wrapped >= row => Self::MIRROR_ROW,
                prev => prev,
            },
        };
    }

    /// Steps the value of the highlighted row with the arrows: the color mode to the next or
    /// previous one on the mode row, the fade level on the fade row. Other rows ignore it.
    pub fn change_option(&mut self, palette: &mut Palette, forward: bool) {
        match self.row {
            0 => palette.change_mode(forward),
            Palette::FADE_ROW => palette.step_fade_level(forward),
            _ => {}
        }
    }

    /// Acts on the highlighted row: opens the color picker for a color row. The mode and fade
    /// rows are changed with the arrows instead.
    pub fn activate(&mut self, palette: &mut Palette) {
        match self.row {
            0 | Palette::FADE_ROW | Self::RENDER_ROW..=Self::MIRROR_ROW => {}
            _ => {
                if let Some(color) = palette.row_color(self.row) {
                    self.color_before_picker = color;
                    self.picker = Some(ColorPicker::new(color));
                }
            }
        }
    }

    /// Lets `edit` change the open picker, then previews its color on `palette`.
    pub fn edit_picker(&mut self, palette: &mut Palette, edit: impl FnOnce(&mut ColorPicker)) {
        if let Some(picker) = &mut self.picker {
            edit(picker);
            palette.set_row_color(self.row, picker.color());
        }
    }

    /// Closes the picker, keeping the previewed color when `confirm` and restoring otherwise.
    pub fn close_picker(&mut self, palette: &mut Palette, confirm: bool) {
        if self.picker.take().is_some() && !confirm {
            palette.set_row_color(self.row, self.color_before_picker);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cancelling_the_picker_restores_the_color() {
        let mut palette = Palette::default();
        let mut session = ColorSession::open(&palette);
        session.select_next(&palette);
        session.activate(&mut palette);
        let original = session.picker.as_ref().unwrap().color();

        session.edit_picker(&mut palette, |picker| picker.nudge(-10, 5, false));
        assert_ne!(palette.row_color(session.row), Some(original));

        session.close_picker(&mut palette, false);
        assert_eq!(palette.row_color(session.row), Some(original));
        assert!(session.picker.is_none());
    }

    #[test]
    fn test_close_tells_whether_the_palette_changed() {
        let mut palette = Palette::default();
        let mut session = ColorSession::open(&palette);
        assert!(!session.close(&palette));

        let mut session = ColorSession::open(&palette);
        session.change_option(&mut palette, true);
        assert!(session.close(&palette));
    }
}
