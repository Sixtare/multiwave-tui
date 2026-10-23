use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Rect, Size},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Widget,
    },
};

use crate::{
    app::{App, COLUMNS_PER_BAR},
    palette::{BarColor, Palette},
};

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
    /// Smoothing announcement, while it is on screen.
    smoothing_notice: Option<bool>,
    palette: Palette,
    show_color_popup: bool,
    color_row: usize,
    color_picker: Option<usize>,
    /// Color announcement, while it is on screen.
    color_notice: Option<String>,
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
            smoothing_notice: app.smoothing_notice().filter(|_| !app.show_popup),
            palette: app.palette,
            show_color_popup: app.show_color_popup,
            color_row: app.color_row,
            color_picker: app.color_picker,
            color_notice: app.color_notice().filter(|_| !app.show_popup),
        }
    }
}

pub fn render(app: &mut App, frame: &mut Frame) {
    match app.show_popup {
        false => {
            draw_soundwave(frame, frame.area(), &app.audio_bars, &app.palette);
            if app.show_color_popup {
                render_color_popup(frame, app);
            }
        }
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
        notice_row += 1;
    }
    if let Some(on) = app.smoothing_notice().filter(|_| !app.show_popup) {
        let state = if on { "on" } else { "off" };
        render_notice(frame, notice_row, &format!(" smoothing {state} "));
        notice_row += 1;
    }
    if let Some(color) = app.color_notice().filter(|_| !app.show_popup) {
        render_notice(frame, notice_row, &format!(" color: {color} "));
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

/// Draws the bar color popup over the visualizer, which keeps moving as a live preview.
///
/// While a row's list of colors is open it is drawn in a second box to the right.
fn render_color_popup(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let palette = &app.palette;
    // Same size in both modes, so the box does not jump when the mode changes.
    let height = (Palette::MAX_ROWS as u16 + 2).min(area.height);
    let popup_area = area.centered(
        Constraint::Length(34.min(area.width)),
        Constraint::Length(height),
    );
    if popup_area.width < 3 || popup_area.height < 3 {
        return;
    }

    let block = Block::bordered()
        .title(" Bar Colors ")
        .title_alignment(Alignment::Center)
        .title_bottom(Line::from(" Enter select  Esc close ").centered())
        .border_type(BorderType::Rounded);
    let inner = block.inner(popup_area);
    frame.render_widget(Clear, popup_area);
    frame.render_widget(block, popup_area);

    let labels: &[&str] = if palette.single_mode {
        &["Mode", "Color"]
    } else {
        &["Mode", "Bass", "Mids", "Treble"]
    };
    let lines: Vec<Line> = labels
        .iter()
        .enumerate()
        .map(|(row, label)| {
            let selected = row == app.color_row;
            let text_style = if selected {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::White)
            };
            let value = match palette.row_color(row) {
                Some(color) => color_value(color),
                None => vec![Span::styled(
                    if palette.single_mode {
                        "Single color"
                    } else {
                        "Gradient"
                    },
                    text_style,
                )],
            };
            let mut spans = vec![Span::styled(
                format!("{} {:<7}", if selected { ">" } else { " " }, label),
                text_style,
            )];
            spans.extend(value);
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);

    if let Some(cursor) = app.color_picker {
        render_color_list(frame, popup_area, cursor);
    }
}

/// Draws the list of pickable colors to the right of `popup_area`, or over its right edge when
/// the terminal has no room beside it.
fn render_color_list(frame: &mut Frame, popup_area: Rect, cursor: usize) {
    let area = frame.area();
    let width = 20.min(area.width);
    let colors = BarColor::ALL;
    let height = (colors.len() as u16 + 2).min(area.height);
    // Centered on the same horizontal axis as the popup, but kept inside the terminal.
    let centered_y = (popup_area.y + popup_area.height / 2).saturating_sub(height / 2);
    let y = centered_y.min(area.y + area.height - height);
    let x = if popup_area.right() + width <= area.right() {
        popup_area.right()
    } else {
        area.right() - width
    };
    let list_area = Rect {
        x,
        y,
        width,
        height,
    };
    if list_area.width < 3 || list_area.height < 3 {
        return;
    }

    let block = Block::bordered()
        .title(" Color ")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded);
    let inner = block.inner(list_area);
    frame.render_widget(Clear, list_area);
    frame.render_widget(block, list_area);

    // Scroll so the cursor stays visible on a short terminal.
    let visible = inner.height as usize;
    let offset = (cursor + 1).saturating_sub(visible);
    let lines: Vec<Line> = colors
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
        .map(|(i, &color)| {
            let style = if i == cursor {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::White)
            };
            let mut spans = vec![Span::styled(if i == cursor { "> " } else { "  " }, style)];
            spans.extend(color_value(color));
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// A color swatch followed by the color's name.
fn color_value(color: BarColor) -> Vec<Span<'static>> {
    vec![
        Span::styled("████", Style::default().fg(color.color())),
        Span::raw(format!(" {}", color.name())),
    ]
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
    palette: &'a Palette,
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

        let band_colors = self.palette.colors();
        let mirror_band_colors = self.palette.mirror_colors();
        for (i, &amplitude) in self.bars.iter().enumerate() {
            let color = band_color(i, bar_count, band_colors);
            // Mirror reflection colors: softer/darker tones for depth effect
            let mirror_color = band_color(i, bar_count, mirror_band_colors);
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

pub fn draw_soundwave(frame: &mut Frame, area: Rect, bars: &[f32], palette: &Palette) {
    frame.render_widget(Soundwave { bars, palette }, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    const SIZE: Size = Size::new(80, 24);

    fn render_bars(bars: &[f32], width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_soundwave(frame, frame.area(), bars, &Palette::default()))
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
        assert_eq!(buffer[(8, 4)].fg, Color::Rgb(0, 128, 128));
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
        app.tick();
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
    fn test_frame_state_changes_while_the_smoothing_notice_is_visible() {
        let mut app = App::new();
        let before = FrameState::capture(&app, SIZE);

        app.toggle_smoothing();
        let during = FrameState::capture(&app, SIZE);
        assert_ne!(before, during);
        assert_eq!(during.smoothing_notice, Some(true));

        for _ in 0..90 {
            app.tick();
        }
        assert_eq!(FrameState::capture(&app, SIZE).smoothing_notice, None);
    }

    #[test]
    fn test_color_popup_previews_on_the_bars_and_is_drawn_over_them() {
        let mut app = App::new();
        app.open_color_popup();
        app.activate_color_row();
        app.update_audio_bars(vec![1.0; 32]);
        app.tick();

        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let text: String = (0..12)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .map(|(x, y)| buffer[(x, y)].symbol().to_string())
            .collect();
        assert!(text.contains("Bar Colors"));
        assert!(text.contains("Single color"));
        // Bars outside the popup already use the previewed single color.
        assert_eq!(buffer[(8, 1)].fg, Color::Cyan);
    }

    #[test]
    fn test_color_list_opens_to_the_right_of_the_popup() {
        let mut app = App::new();
        app.open_color_popup();
        app.select_next_color_row();
        app.activate_color_row();

        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let row = |y: u16| -> String { (0..80).map(|x| buffer[(x, y)].symbol()).collect() };
        let title_row = (0..20).find(|&y| row(y).contains("Bar Colors")).unwrap();
        let popup_col = row(title_row).find("Bar Colors").unwrap();
        let list_title_col = (0..20)
            .filter_map(|y| row(y).find(" Color "))
            .next()
            .expect("color list title");
        assert!(list_title_col > popup_col + 10);

        // Both boxes share the same center row: compare their top and bottom corners.
        let corner_rows = |x: u16| -> (u16, u16) {
            let top = (0..20).find(|&y| buffer[(x, y)].symbol() == "╭").unwrap();
            let bottom = (0..20).find(|&y| buffer[(x, y)].symbol() == "╰").unwrap();
            (top, bottom)
        };
        let (popup_top, popup_bottom) = corner_rows(23);
        let (list_top, list_bottom) = corner_rows(57);
        assert_eq!(popup_top + popup_bottom, list_top + list_bottom);
        let all: String = (0..20).map(row).collect();
        assert!(all.contains("Gray"));
    }

    #[test]
    fn test_color_popup_keeps_its_size_in_single_color_mode() {
        let height_of = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
            terminal.draw(|frame| render(app, frame)).unwrap();
            let buffer = terminal.backend().buffer();
            let top = (0..20).find(|&y| buffer[(23, y)].symbol() == "╭").unwrap();
            let bottom = (0..20).find(|&y| buffer[(23, y)].symbol() == "╰").unwrap();
            bottom - top
        };
        let mut app = App::new();
        app.open_color_popup();
        let gradient = height_of(&mut app);
        app.activate_color_row();
        assert!(app.palette.single_mode);
        assert_eq!(height_of(&mut app), gradient);
    }

    #[test]
    fn test_frame_state_changes_with_the_palette_and_color_popup() {
        let mut app = App::new();
        let before = FrameState::capture(&app, SIZE);

        app.open_color_popup();
        let open = FrameState::capture(&app, SIZE);
        assert_ne!(before, open);
        app.select_next_color_row();
        let moved = FrameState::capture(&app, SIZE);
        assert_ne!(open, moved);
        app.activate_color_row();
        let picking = FrameState::capture(&app, SIZE);
        assert_ne!(moved, picking);
        app.move_color_picker(true);
        assert_ne!(picking, FrameState::capture(&app, SIZE));
    }

    #[test]
    fn test_color_notice_is_drawn_after_confirming() {
        let mut app = App::new();
        app.open_color_popup();
        app.activate_color_row();
        app.close_color_popup();

        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let top_row: String = (0..40).map(|x| buffer[(x, 0)].symbol()).collect();
        assert!(top_row.ends_with(" color: Cyan "), "{top_row:?}");
        assert_eq!(
            FrameState::capture(&app, SIZE).color_notice.as_deref(),
            Some("Cyan")
        );
    }

    #[test]
    fn test_frame_state_ignores_bars_behind_popup() {
        let mut app = App::new();
        app.show_popup = true;
        let before = FrameState::capture(&app, SIZE);

        app.update_audio_bars(vec![0.5; 32]);

        app.tick();

        assert_eq!(before, FrameState::capture(&app, SIZE));
    }
}
