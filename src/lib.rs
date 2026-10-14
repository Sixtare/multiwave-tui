use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    symbols::border,
    text::{Line, Text},
    widgets::{Block, Paragraph, Widget},
    DefaultTerminal, Frame,
};

pub mod app;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;
    use crate::app::App;

    #[test]
    fn render() {
        let app = App::default();
        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 4));

        app.render(buf.area, &mut buf);

        let mut expected = Buffer::with_lines(vec![
            "┏━━━━━━━━━━━━━ Counter App Tutorial ━━━━━━━━━━━━━┓",
            "┃                    Value: 0                    ┃",
            "┃                                                ┃",
            "┗━ Decrement <Left> Increment <Right> Quit <Q> ━━┛",
        ]);
        let title_style = Style::new().bold();
        let counter_style = Style::new().yellow();
        let key_style = Style::new().blue().bold();
        expected.set_style(Rect::new(14, 0, 22, 1), title_style);
        expected.set_style(Rect::new(28, 1, 1, 1), counter_style);
        expected.set_style(Rect::new(13, 3, 6, 1), key_style);
        expected.set_style(Rect::new(30, 3, 7, 1), key_style);
        expected.set_style(Rect::new(43, 3, 4, 1), key_style);

        assert_eq!(buf, expected);
    }
}