use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Rect, Size},
    style::{Color, Style},
    text::Line,
    widgets::{
        Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Widget,
    },
};

use crate::app::{App, COLUMNS_PER_BAR};

/// Everything [`render`] reads from the app, plus the terminal size.
///
/// The main loop compares it with the state of the last drawn frame and skips frames that would
/// come out identical, e.g. during silence once the bars have settled at zero.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameState {
    size: Size,
    /// Displayed bars; left empty while the popup hides the visualizer.
    bars: Vec<f32>,
    show_popup: bool,
    devices: Vec<String>,
    selected_device_idx: usize,
    active_device_idx: usize,
    device_scroll_offset: usize,
    audio_error: Option<String>,
    /// Bar count announcement, while it is on screen.
    bar_notice: Option<usize>,
    /// Sensitivity announcement, while it is on screen.
    gain_notice: Option<f32>,
}

impl FrameState {
    /// Captures what the next frame would show for `app` on a terminal of `size`.
    pub fn capture(app: &App, size: Size) -> Self {
        Self {
            size,
            bars: if app.show_popup {
                Vec::new()
            } else {
                app.audio_bars.clone()
            },
            show_popup: app.show_popup,
            devices: app.devices.clone(),
            selected_device_idx: app.selected_device_idx,
            active_device_idx: app.active_device_idx,
            device_scroll_offset: app.device_scroll_offset,
            audio_error: app.audio_error.clone(),
            bar_notice: app.bar_notice().filter(|_| !app.show_popup),
            gain_notice: app.gain_notice().filter(|_| !app.show_popup),
        }
    }
}

pub fn render(app: &mut App, frame: &mut Frame) {
    match app.show_popup {
        false => draw_soundwave(frame, frame.area(), &app.audio_bars),
        // Uses the full screen, centering the visualizer vertically and horizontally
        true => render_popup(frame, app),
    }

    // Notices stack down the top-right corner, so changing both settings at once shows both.
    let mut notice_row = 0;
    if let Some(count) = app.bar_notice().filter(|_| !app.show_popup) {
        render_notice(frame, notice_row, &format!(" {count} bars "));
        notice_row += 1;
    }
    if let Some(gain) = app.gain_notice().filter(|_| !app.show_popup) {
        render_notice(frame, notice_row, &format!(" sensitivity {gain:.2}x "));
    }

    if let Some(error) = &app.audio_error {
        render_audio_error(frame, error);
    }
}

/// Draws a short notice in cyan, right-aligned on `row` (counted from the top of the frame),
/// e.g. ` 64 bars ` or ` sensitivity 2.50x `.
fn render_notice(frame: &mut Frame, row: u16, text: &str) {
    let area = frame.area();
    let width = (text.chars().count() as u16).min(area.width);
    if width == 0 || row >= area.height {
        return;
    }

    let notice_area = Rect {
        x: area.x + area.width - width,
        y: area.y + row,
        width,
        height: 1,
    };

    frame.render_widget(Clear, notice_area);
    frame.render_widget(
        Paragraph::new(Line::styled(
            text.to_string(),
            Style::default().fg(Color::Cyan),
        )),
        notice_area,
    );
}

/// Draws the current audio error as a status line at the bottom of the screen.
fn render_audio_error(frame: &mut Frame, message: &str) {
    let area = frame.area();
    if area.height == 0 {
        return;
    }

    let status_area = Rect {
        x: area.x,
        y: area.y + area.height - 1,
        width: area.width,
        height: 1,
    };

    frame.render_widget(Clear, status_area);
    frame.render_widget(
        Paragraph::new(Line::styled(
            format!(" Audio error: {message}"),
            Style::default().fg(Color::Red),
        )),
        status_area,
    );
}

// Characters for the upper bars (base-anchored)
const WAVE_CHARS: &[char] = &[' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// Characters for the lower mirror (top-anchored / upside down)
const INVERTED_WAVE_CHARS: &[char] = &[' ', '▔', '▀', '█'];

// Draws popup menu with available output audio devices
fn render_popup(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    let popup_block = Block::bordered()
        .title(" Select Output Device (↑/↓ to navigate, Enter to select, Esc/s to close) ")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded)
        .style(Style::default());

    let items_count = app.devices.len().max(1);
    let desired_height = (items_count as u16 + 4).clamp(6, 18);
    // Keep a small margin around the popup, and never exceed the terminal height.
    let max_height = area.height.saturating_sub(2).max(3).min(area.height);
    let popup_height = desired_height.min(max_height);
    let centered_area = area.centered(Constraint::Percentage(70), Constraint::Length(popup_height));

    // Clears out any background in the area before rendering the popup
    frame.render_widget(Clear, centered_area);

    let list_height = popup_height.saturating_sub(2) as usize;
    if list_height > 0 {
        app.ensure_device_visible(list_height);
    }
    let needs_scrollbar =
        app.devices.len() > list_height && list_height > 0 && centered_area.width > 2;

    let inner_area = popup_block.inner(centered_area);
    frame.render_widget(popup_block, centered_area);

    let mut lines = Vec::new();
    if app.devices.is_empty() {
        lines.push(Line::styled(
            "   No output devices detected.",
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        for (i, name) in app.devices.iter().enumerate() {
            let is_selected = i == app.selected_device_idx;
            let is_active = i == app.active_device_idx;

            let prefix = if is_selected { " > " } else { "   " };
            let suffix = if is_active { " [Active]" } else { "" };
            let content = format!("{}{}{}", prefix, name, suffix);

            let style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::Cyan).bold()
            } else if is_active {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };

            lines.push(Line::styled(content, style));
        }
    }

    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .scroll((app.device_scroll_offset as u16, 0));

    let mut content_area = inner_area;
    if needs_scrollbar && content_area.width > 0 {
        // Reserve the rightmost inner column for the scrollbar.
        content_area.width -= 1;
    }

    frame.render_widget(paragraph, content_area);

    if needs_scrollbar {
        let scrollbar_area = Rect {
            x: inner_area.x + inner_area.width - 1,
            y: inner_area.y,
            width: 1,
            height: inner_area.height,
        };
        let mut scrollbar_state = ScrollbarState::new(app.devices.len())
            .position(app.selected_device_idx.min(app.devices.len() - 1))
            .viewport_content_length(list_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .track_style(Style::default().fg(Color::DarkGray))
            .thumb_symbol("█")
            .thumb_style(Style::default().fg(Color::Cyan));

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

/// Picks the glyph for one bar cell, given the row's threshold and the glyph ramp to use.
fn bar_glyph(amplitude: f32, threshold: f32, rows: usize, glyphs: &[char]) -> char {
    let row_span = 1.0 / rows as f32;
    if amplitude >= threshold + row_span {
        '█'
    } else if amplitude >= threshold {
        let fractional = (amplitude - threshold) * rows as f32;
        let glyph_index = (fractional * (glyphs.len() - 1) as f32).round() as usize;
        glyphs[glyph_index.clamp(0, glyphs.len() - 1)]
    } else {
        ' '
    }
}

/// Picks one of three colors by which third of the spectrum the bar belongs to.
fn band_color(bar: usize, bar_count: usize, [bass, mids, treble]: [Color; 3]) -> Color {
    if bar < bar_count / 3 {
        bass
    } else if bar < (bar_count * 2) / 3 {
        mids
    } else {
        treble
    }
}

/// Mirrored spectrum: bars grow upward from a center divider and are reflected below it.
///
/// Writes cells straight into the buffer. Building a `Paragraph` of styled spans meant one
/// `String` allocation per cell plus unicode-width measurement for centering, every frame,
/// which dominated the app's CPU use.
struct Soundwave<'a> {
    bars: &'a [f32],
}

impl Widget for Soundwave<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || self.bars.is_empty() {
            return;
        }

        // Reserves 1 line for the center horizon/divider
        let usable_lines = (area.height - 1) as usize;

        // Complete symmetrical division: no artificial clamp limits.
        // The divider line is fixed at the exact center of the screen and bars
        // utilize 100% of the available vertical height (reaching the top and bottom).
        let upper_rows = usable_lines / 2;
        let mirror_rows = usable_lines / 2;
        let content_lines = (upper_rows + 1 + mirror_rows) as u16;
        // Vertically centers while filling the entire screen; a spare line goes above.
        let top = area.y + (area.height - content_lines).div_ceil(2);
        let divider_y = top + upper_rows as u16;

        // Each bar is a glyph followed by a spacer. The row is centered horizontally; when the
        // terminal is narrower than the row, the treble side is clipped.
        let bar_count = self.bars.len();
        let row_width = bar_count * COLUMNS_PER_BAR;
        let visible_width = row_width.min(area.width as usize);
        let left = area.x + (area.width - visible_width as u16) / 2;
        let mut set_cell = |column: usize, y: u16, glyph: char, color: Color| {
            if column < visible_width {
                buf[(left + column as u16, y)].set_char(glyph).set_fg(color);
            }
        };

        for (i, &amplitude) in self.bars.iter().enumerate() {
            // Vibrant gradient: bass cyan -> mids green -> treble magenta
            let color = band_color(i, bar_count, [Color::Cyan, Color::Green, Color::Magenta]);
            // Mirror reflection colors: softer/darker tones for depth effect
            let mirror_color = band_color(
                i,
                bar_count,
                [
                    Color::Rgb(0, 120, 160),
                    Color::Rgb(30, 130, 70),
                    Color::Rgb(130, 40, 130),
                ],
            );
            let normalized = amplitude.clamp(0.0, 1.0);
            // Slight reflection attenuation for a more realistic mirror/water effect
            let mirror_amplitude = (amplitude * 0.85).clamp(0.0, 1.0);

            // 1. Upper Visualizer (grows upward from the center line to the top of the screen)
            for row in 0..upper_rows {
                let threshold = row as f32 / upper_rows as f32;
                let glyph = bar_glyph(normalized, threshold, upper_rows, WAVE_CHARS);
                let y = divider_y - 1 - row as u16;
                set_cell(i * 2, y, glyph, color);
                set_cell(i * 2 + 1, y, ' ', color);
            }

            // 2. Subtle center divider line (mirror horizon at screen center)
            set_cell(i * 2, divider_y, '─', Color::DarkGray);
            set_cell(i * 2 + 1, divider_y, '─', Color::DarkGray);

            // 3. Mirror Effect (upside down, grows down towards screen bottom)
            for row in 0..mirror_rows {
                let threshold = row as f32 / mirror_rows as f32;
                let glyph = bar_glyph(
                    mirror_amplitude,
                    threshold,
                    mirror_rows,
                    INVERTED_WAVE_CHARS,
                );
                let y = divider_y + 1 + row as u16;
                set_cell(i * 2, y, glyph, mirror_color);
                set_cell(i * 2 + 1, y, ' ', mirror_color);
            }
        }
    }
}

pub fn draw_soundwave(frame: &mut Frame, area: Rect, bars: &[f32]) {
    frame.render_widget(Soundwave { bars }, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    const SIZE: Size = Size::new(80, 24);

    fn render_bars(bars: &[f32], width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_soundwave(frame, frame.area(), bars))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn test_soundwave_layout() {
        let mut bars = vec![0.0; 32];
        bars[0] = 1.0;
        let buffer = render_bars(&bars, 80, 7);

        // 7 rows: 3 upper, divider, 3 mirror; 64 columns centered in 80.
        assert_eq!(buffer[(8, 0)].symbol(), "█");
        assert_eq!(buffer[(8, 0)].fg, Color::Cyan);
        assert_eq!(buffer[(9, 0)].symbol(), " ");
        assert_eq!(buffer[(10, 0)].symbol(), " ");
        assert_eq!(buffer[(7, 3)].symbol(), " ");
        assert_eq!(buffer[(8, 3)].symbol(), "─");
        assert_eq!(buffer[(71, 3)].symbol(), "─");
        assert_eq!(buffer[(72, 3)].symbol(), " ");
        // The mirror is attenuated to 0.85, so its last row is partially filled.
        assert_eq!(buffer[(8, 4)].symbol(), "█");
        assert_eq!(buffer[(8, 4)].fg, Color::Rgb(0, 120, 160));
        assert_eq!(buffer[(8, 6)].symbol(), "▀");
    }

    #[test]
    fn test_soundwave_clips_treble_on_narrow_terminal() {
        let buffer = render_bars(&[1.0; 32], 40, 7);

        assert_eq!(buffer[(0, 0)].fg, Color::Cyan);
        assert_eq!(buffer[(39, 0)].fg, Color::Green);
    }

    #[test]
    fn test_frame_state_is_stable_for_unchanged_app() {
        let app = App::with_devices(vec!["A".into()], 0);

        assert_eq!(
            FrameState::capture(&app, SIZE),
            FrameState::capture(&app, SIZE)
        );
    }

    #[test]
    fn test_frame_state_changes_with_bars_and_size() {
        let mut app = App::new();
        let before = FrameState::capture(&app, SIZE);

        assert_ne!(before, FrameState::capture(&app, Size::new(100, 24)));
        app.update_audio_bars(vec![0.5; 32]);
        assert_ne!(before, FrameState::capture(&app, SIZE));
    }

    #[test]
    fn test_frame_state_changes_while_the_bar_notice_is_visible() {
        let mut app = App::new();
        app.set_terminal_width(200);
        let before = FrameState::capture(&app, SIZE);

        app.increase_bar_count();
        let during = FrameState::capture(&app, SIZE);
        assert_ne!(before, during);

        for _ in 0..90 {
            app.tick();
        }
        let after = FrameState::capture(&app, SIZE);
        assert_ne!(during, after);
        assert_eq!(after.bar_notice, None);
    }

    #[test]
    fn test_bar_notice_is_drawn_in_the_top_right_corner() {
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal
            .draw(|frame| render_notice(frame, 0, " 64 bars "))
            .unwrap();
        let buffer = terminal.backend().buffer();

        let top_row: String = (0..40).map(|x| buffer[(x, 0)].symbol()).collect();
        assert!(top_row.ends_with(" 64 bars "), "{top_row:?}");
        assert!(top_row.starts_with(' '));
    }

    #[test]
    fn test_bar_notice_is_cyan_text_without_a_background() {
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal
            .draw(|frame| render_notice(frame, 0, " 64 bars "))
            .unwrap();
        let buffer = terminal.backend().buffer();

        for x in 31..40 {
            let cell = &buffer[(x, 0)];
            assert_eq!(cell.fg, Color::Cyan, "column {x}");
            assert_eq!(cell.bg, Color::Reset, "column {x}");
        }
    }

    #[test]
    fn test_sensitivity_notice_is_drawn_like_the_bar_count_one() {
        let mut app = App::new();
        app.increase_gain();
        app.increase_gain();

        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let top_row: String = (0..40).map(|x| buffer[(x, 0)].symbol()).collect();
        assert!(top_row.ends_with(" sensitivity 1.20x "), "{top_row:?}");
        assert_eq!(buffer[(39, 0)].fg, Color::Cyan);
        assert_eq!(buffer[(39, 0)].bg, Color::Reset);
    }

    #[test]
    fn test_both_notices_stack_in_the_top_right_corner() {
        let mut app = App::new();
        app.set_terminal_width(40);
        app.increase_bar_count();
        app.increase_gain();

        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let row = |y: u16| -> String { (0..40).map(|x| buffer[(x, y)].symbol()).collect() };
        assert!(row(0).ends_with(" 32 bars "), "{:?}", row(0));
        assert!(row(1).ends_with(" sensitivity 1.10x "), "{:?}", row(1));
    }

    #[test]
    fn test_frame_state_changes_while_the_sensitivity_notice_is_visible() {
        let mut app = App::new();
        let before = FrameState::capture(&app, SIZE);

        app.increase_gain();
        let during = FrameState::capture(&app, SIZE);
        assert_ne!(before, during);

        for _ in 0..90 {
            app.tick();
        }
        let after = FrameState::capture(&app, SIZE);
        assert_ne!(during, after);
        assert_eq!(after.gain_notice, None);
    }

    #[test]
    fn test_frame_state_ignores_bars_behind_popup() {
        let mut app = App::new();
        app.show_popup = true;
        let before = FrameState::capture(&app, SIZE);

        app.update_audio_bars(vec![0.5; 32]);

        assert_eq!(before, FrameState::capture(&app, SIZE));
    }
}
