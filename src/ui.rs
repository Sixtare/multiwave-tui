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
    app::{App, COLUMNS_PER_BAR, MENU_ROWS},
    audio_processor::loudest_bar_height,
    color_picker::{ColorPicker, HUE_ROWS, PickerFocus, SQUARE_COLS, SQUARE_ROWS},
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
    show_menu: bool,
    menu_row: usize,
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
    /// Manual sensitivity, which sets where the vertical gradient ends.
    gain: f32,
    show_color_popup: bool,
    color_row: usize,
    color_picker: Option<ColorPicker>,
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
            show_menu: app.show_menu,
            menu_row: app.menu_row,
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
            gain: app.gain,
            show_color_popup: app.show_color_popup,
            color_row: app.color_row,
            color_picker: app.color_picker.clone(),
            color_notice: app.color_notice().filter(|_| !app.show_popup),
        }
    }
}

pub fn render(app: &mut App, frame: &mut Frame) {
    match app.show_popup {
        false => {
            draw_soundwave(frame, frame.area(), &app.audio_bars, &app.palette, app.gain);
            if app.show_menu {
                render_menu(frame, app);
            }
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

/// How much the reflection is attenuated, for a more realistic mirror/water effect.
const MIRROR_ATTENUATION: f32 = 0.85;

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

/// Width of the color popup, and of the color picker box beside it: the square, a gap, the hue
/// strip and its marker, plus the borders.
/// Width of the main menu box.
const MENU_WIDTH: u16 = 40;
const POPUP_WIDTH: u16 = 34;
const PICKER_WIDTH: u16 = SQUARE_COLS as u16 + 1 + 2 + 1 + 2;
/// Height of the color picker box: the square, a blank line and the hex line, plus the borders.
const PICKER_HEIGHT: u16 = SQUARE_ROWS as u16 + 2 + 2;

/// Draws the bar color popup over the visualizer, which keeps moving as a live preview.
///
/// While a row's color picker is open it is drawn in a second box to the right.
/// Draws the main menu: every option with its current value and the key that reaches it directly.
fn render_menu(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let width = MENU_WIDTH.min(area.width);
    let height = (MENU_ROWS as u16 + 2).min(area.height);
    let menu_area = area.centered(Constraint::Length(width), Constraint::Length(height));
    if menu_area.width < 3 || menu_area.height < 3 {
        return;
    }

    let block = Block::bordered()
        .title(" Menu ")
        .title_alignment(Alignment::Center)
        .title_bottom(Line::from(" Enter open  Esc close ").centered())
        .border_type(BorderType::Rounded);
    let inner = block.inner(menu_area);
    frame.render_widget(Clear, menu_area);
    frame.render_widget(block, menu_area);

    let rows = [
        ("Output device", String::new(), "O"),
        ("Bar color", app.palette.mode.label().to_string(), "C"),
        ("Sensitivity", format!("◀ {:.2}x ▶", app.gain), "↑/↓"),
        ("Bars", format!("◀ {} ▶", app.bar_count), "←/→"),
        (
            "Smoothing",
            (if app.smoothing { "on" } else { "off" }).to_string(),
            "S",
        ),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(row, (label, value, shortcut))| {
            let selected = row == app.menu_row;
            let style = if selected {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::White)
            };
            let left = format!(
                "{} {:<14}{}",
                if selected { ">" } else { " " },
                label,
                value
            );
            let pad = (inner.width as usize)
                .saturating_sub(left.chars().count() + shortcut.chars().count() + 1);
            Line::from(vec![
                Span::styled(left, style),
                Span::raw(" ".repeat(pad)),
                Span::styled(*shortcut, Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_color_popup(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let palette = &app.palette;
    // One extra line separates the mirror checkbox from the colors.
    let height = (Palette::ROWS as u16 + 3).min(area.height);
    // While the picker is open the popup and the picker are centered together.
    let group_width = if app.color_picker.is_some() && POPUP_WIDTH + PICKER_WIDTH <= area.width {
        POPUP_WIDTH + PICKER_WIDTH
    } else {
        POPUP_WIDTH.min(area.width)
    };
    let group = area.centered(Constraint::Length(group_width), Constraint::Length(height));
    let popup_area = Rect {
        width: POPUP_WIDTH.min(area.width),
        ..group
    };
    if popup_area.width < 3 || popup_area.height < 3 {
        return;
    }

    let block = Block::bordered()
        .title(" Bar Colors ")
        .title_alignment(Alignment::Center)
        .title_bottom(Line::from(" ←→ mode  Enter pick  Esc close ").centered())
        .border_type(BorderType::Rounded);
    let inner = block.inner(popup_area);
    frame.render_widget(Clear, popup_area);
    frame.render_widget(block, popup_area);

    let mut lines: Vec<Line> = (0..Palette::ROWS)
        .map(|row| {
            let label = palette.row_label(row);
            // In single-color mode the mids and treble just follow the bass.
            let grayed_out = !palette.row_selectable(row);
            let selected = row == app.color_row;
            let text_style = if grayed_out {
                Style::default().fg(Color::DarkGray)
            } else if row == Palette::MIRROR_ROW {
                // The checkbox row shows its state: green when on, yellow when off. The cursor
                // is shown by the `>` and bold text.
                let state = if palette.mirror_dimmed {
                    Color::Green
                } else {
                    Color::Yellow
                };
                let style = Style::default().fg(state);
                if selected { style.bold() } else { style }
            } else if selected {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::White)
            };
            let value = match palette.row_color(row) {
                Some(color) => color_value(color, text_style),
                None if row == Palette::MIRROR_ROW => {
                    let mark = if palette.mirror_dimmed { "x" } else { " " };
                    vec![Span::styled(format!("[{mark}] Darker"), text_style)]
                }
                None => vec![Span::styled(palette.mode.label(), text_style)],
            };
            let mut spans = vec![Span::styled(
                format!("{} {:<7}", if selected { ">" } else { " " }, label),
                text_style,
            )];
            // The mode is changed with the arrows, which the row shows around its name.
            if row == 0 {
                spans.push(Span::styled("◀ ", text_style));
                spans.extend(value);
                spans.push(Span::styled(" ▶", text_style));
            } else {
                spans.extend(value);
            }
            Line::from(spans)
        })
        .collect();
    lines.insert(Palette::MIRROR_ROW, Line::default());
    frame.render_widget(Paragraph::new(lines), inner);

    if let Some(picker) = &app.color_picker {
        render_color_picker(frame, popup_area, picker);
    }
}

/// Draws the color picker to the right of `popup_area`, or over its right edge when the terminal
/// has no room beside it: the saturation/value square, the hue strip beside it and the picked
/// color's hex value below.
fn render_color_picker(frame: &mut Frame, popup_area: Rect, picker: &ColorPicker) {
    let area = frame.area();
    let width = PICKER_WIDTH.min(area.width);
    let height = PICKER_HEIGHT.min(area.height);
    // Centered on the same horizontal axis as the popup, but kept inside the terminal.
    let centered_y = (popup_area.y + popup_area.height / 2).saturating_sub(height / 2);
    let y = centered_y.min(area.y + area.height - height);
    let x = if popup_area.right() + width <= area.right() {
        popup_area.right()
    } else {
        area.right() - width
    };
    let picker_area = Rect {
        x,
        y,
        width,
        height,
    };
    if picker_area.width < 3 || picker_area.height < 3 {
        return;
    }

    let block = Block::bordered()
        .title(" Color ")
        .title_alignment(Alignment::Center)
        .title_bottom(Line::from(" Tab:focus Enter:ok ").centered())
        .border_type(BorderType::Rounded);
    let inner = block.inner(picker_area);
    frame.render_widget(Clear, picker_area);
    frame.render_widget(block, picker_area);

    let focused = |part: PickerFocus| picker.focus == part;
    let buf = frame.buffer_mut();
    let mut paint = |col: usize, row: usize, glyph: char, fg: Color, bg: Color| {
        if col < inner.width as usize && row < inner.height as usize {
            buf[(inner.x + col as u16, inner.y + row as u16)]
                .set_char(glyph)
                .set_fg(fg)
                .set_bg(bg);
        }
    };

    // Saturation grows to the right and value upward; the cursor shows the picked spot.
    let (cursor_col, cursor_row) = picker.square_cursor();
    for row in 0..SQUARE_ROWS {
        for col in 0..SQUARE_COLS {
            let saturation = col as f32 / (SQUARE_COLS - 1) as f32;
            let value = 1.0 - row as f32 / (SQUARE_ROWS - 1) as f32;
            let cell = BarColor::from_hsv(picker.hue, saturation, value);
            if (col, row) == (cursor_col, cursor_row) {
                let glyph = if focused(PickerFocus::Square) {
                    '◆'
                } else {
                    '+'
                };
                paint(col, row, glyph, contrast_color(cell), cell.color());
            } else {
                paint(col, row, ' ', Color::Reset, cell.color());
            }
        }
    }

    // The hue strip runs from red at the top through the rainbow and back to red.
    let strip_col = SQUARE_COLS + 1;
    for row in 0..HUE_ROWS {
        let hue = row as f32 / (HUE_ROWS - 1) as f32 * 360.0;
        let cell = BarColor::from_hsv(hue, 1.0, 1.0).color();
        paint(strip_col, row, ' ', Color::Reset, cell);
        paint(strip_col + 1, row, ' ', Color::Reset, cell);
    }
    let marker_color = if focused(PickerFocus::Hue) {
        Color::Cyan
    } else {
        Color::White
    };
    paint(
        strip_col + 2,
        picker.hue_row(),
        '◀',
        marker_color,
        Color::Reset,
    );

    // A swatch of the picked color and its `#RRGGBB` value, for reading only.
    let hex_line = Line::from(vec![
        Span::styled("████", Style::default().fg(picker.color().color())),
        Span::raw(format!(" {}", picker.color().hex())),
    ]);
    if inner.height > SQUARE_ROWS as u16 + 1 {
        let hex_area = Rect {
            y: inner.y + SQUARE_ROWS as u16 + 1,
            height: 1,
            ..inner
        };
        frame.render_widget(Paragraph::new(hex_line), hex_area);
    }
}

/// Black or white, whichever reads better on top of `color`.
fn contrast_color(color: BarColor) -> Color {
    let luminance = 0.299 * color.r as f32 + 0.587 * color.g as f32 + 0.114 * color.b as f32;
    if luminance > 140.0 {
        Color::Black
    } else {
        Color::White
    }
}

/// A color swatch followed by the color's hex value.
fn color_value(color: BarColor, name_style: Style) -> Vec<Span<'static>> {
    vec![
        Span::styled("████", Style::default().fg(color.color())),
        Span::styled(format!(" {}", color.hex()), name_style),
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

/// Mirrored spectrum: bars grow upward from a center divider and are reflected below it.
///
/// Writes cells straight into the buffer. Building a `Paragraph` of styled spans meant one
/// `String` allocation per cell plus unicode-width measurement for centering, every frame,
/// which dominated the app's CPU use.
struct Soundwave<'a> {
    bars: &'a [f32],
    palette: &'a Palette,
    /// Height the loudest bars settle at, where the vertical gradient reaches its final color.
    peak_height: f32,
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

        // In vertical mode the color depends on the row, not on the bar, so it is worked out once
        // per row instead of once per cell.
        // The loudest bars only reach `peak_height`, which the manual sensitivity raises or
        // lowers (the mirror is attenuated on top of that), so the gradient ends there and the
        // final color is reached at ordinary levels.
        // The row of the final color is the topmost cell such a bar draws.
        let height_t = |row: usize, rows: usize, full_height: f32| {
            let last_row = (rows as f32 * full_height - 1.0).max(1.0);
            (row as f32 / last_row).min(1.0)
        };
        let (upper_row_colors, mirror_row_colors): (Vec<Color>, Vec<Color>) =
            if self.palette.is_vertical() {
                (
                    (0..upper_rows)
                        .map(|row| {
                            self.palette
                                .height_color(height_t(row, upper_rows, self.peak_height))
                                .color()
                        })
                        .collect(),
                    (0..mirror_rows)
                        .map(|row| {
                            self.palette.mirror_height_color(height_t(
                                row,
                                mirror_rows,
                                self.peak_height * MIRROR_ATTENUATION,
                            ))
                        })
                        .collect(),
                )
            } else {
                (Vec::new(), Vec::new())
            };

        for (i, &amplitude) in self.bars.iter().enumerate() {
            let color = self.palette.bar_color(i, bar_count).color();
            // Mirror reflection colors: softer/darker tones for depth effect
            let mirror_color = self.palette.mirror_bar_color(i, bar_count);
            let normalized = amplitude.clamp(0.0, 1.0);
            // Slight reflection attenuation for a more realistic mirror/water effect
            let mirror_amplitude = (amplitude * MIRROR_ATTENUATION).clamp(0.0, 1.0);

            // 1. Upper Visualizer (grows upward from the center line to the top of the screen)
            for row in 0..upper_rows {
                let threshold = row as f32 / upper_rows as f32;
                let glyph = bar_glyph(normalized, threshold, upper_rows, WAVE_CHARS);
                let y = divider_y - 1 - row as u16;
                let color = upper_row_colors.get(row).copied().unwrap_or(color);
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
                let mirror_color = mirror_row_colors.get(row).copied().unwrap_or(mirror_color);
                set_cell(i * 2, y, glyph, mirror_color);
                set_cell(i * 2 + 1, y, ' ', mirror_color);
            }
        }
    }
}

/// Draws the spectrum. `gain` is the manual sensitivity, which decides how tall a bar has to be
/// to show the final color of the vertical gradient.
pub fn draw_soundwave(frame: &mut Frame, area: Rect, bars: &[f32], palette: &Palette, gain: f32) {
    frame.render_widget(
        Soundwave {
            bars,
            palette,
            peak_height: loudest_bar_height(gain),
        },
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::ColorMode;
    use ratatui::{Terminal, backend::TestBackend};

    const SIZE: Size = Size::new(80, 24);

    fn render_bars(bars: &[f32], width: u16, height: u16) -> Buffer {
        render_bars_with(bars, width, height, &Palette::default())
    }

    fn render_bars_with(bars: &[f32], width: u16, height: u16, palette: &Palette) -> Buffer {
        render_bars_with_gain(bars, width, height, palette, 1.0)
    }

    fn render_bars_with_gain(
        bars: &[f32],
        width: u16,
        height: u16,
        palette: &Palette,
        gain: f32,
    ) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_soundwave(frame, frame.area(), bars, palette, gain))
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
        assert_eq!(buffer[(8, 0)].fg, Color::Rgb(0, 255, 255));
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
        let bands = Palette {
            mode: ColorMode::Bands,
            ..Palette::default()
        };
        let buffer = render_bars_with(&[1.0; 32], 40, 7, &bands);

        assert_eq!(buffer[(0, 0)].fg, Color::Rgb(0, 255, 255));
        assert_eq!(buffer[(39, 0)].fg, Color::Rgb(0, 205, 0));
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
    fn test_menu_shows_every_option_with_its_shortcut() {
        let mut app = App::new();
        app.open_menu();
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..12)
            .flat_map(|y| (0..60).map(move |x| (x, y)))
            .map(|(x, y)| buffer[(x, y)].symbol().to_string())
            .collect();
        for expected in [
            "Output device",
            "Bar color",
            "Sensitivity",
            "1.00x",
            "Bars",
            "Smoothing",
            "↑/↓",
            "←/→",
        ] {
            assert!(text.contains(expected), "{expected}");
        }
        assert_ne!(
            FrameState::capture(&app, SIZE),
            FrameState::capture(&App::new(), SIZE)
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
        assert_eq!(buffer[(8, 1)].fg, Color::Rgb(0, 255, 255));
    }

    #[test]
    fn test_color_picker_opens_to_the_right_of_the_popup_and_both_are_centered() {
        let mut app = App::new();
        app.open_color_popup();
        app.select_next_color_row();
        app.activate_color_row();

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        // The two boxes are centered together: 34 + 26 columns in 80 start at column 10.
        let corner_rows = |x: u16| -> (u16, u16) {
            let top = (0..24).find(|&y| buffer[(x, y)].symbol() == "╭").unwrap();
            let bottom = (0..24).find(|&y| buffer[(x, y)].symbol() == "╰").unwrap();
            (top, bottom)
        };
        let (popup_top, popup_bottom) = corner_rows(10);
        let (picker_top, picker_bottom) = corner_rows(44);
        // Both boxes share the same center row.
        assert_eq!(popup_top + popup_bottom, picker_top + picker_bottom);
        assert_eq!(picker_bottom - picker_top + 1, PICKER_HEIGHT);

        let text = |y: u16| -> String { (44..70).map(|x| buffer[(x, y)].symbol()).collect() };
        assert!(text(picker_top).contains("Color"));
        assert!(text(picker_bottom - 1).contains("#00FFFF"));

        // The square's top right corner is the full color: cyan, with the cursor on it.
        let square_x = 45 + SQUARE_COLS as u16 - 1;
        let cell = &buffer[(square_x, picker_top + 1)];
        assert_eq!(cell.symbol(), "◆");
        assert_eq!(cell.bg, Color::Rgb(0, 255, 255));
        // The hue strip starts at red and its marker sits on the cyan row.
        assert_eq!(
            buffer[(45 + SQUARE_COLS as u16 + 1, picker_top + 1)].bg,
            Color::Rgb(255, 0, 0)
        );
        let marker_y = picker_top + 1 + 5;
        assert_eq!(
            buffer[(45 + SQUARE_COLS as u16 + 3, marker_y)].symbol(),
            "◀"
        );
    }

    #[test]
    fn test_color_picker_cursor_moves_with_the_keys() {
        let mut app = App::new();
        app.open_color_popup();
        app.select_next_color_row();
        app.activate_color_row();
        app.edit_color_picker(|picker| picker.nudge(-100, 100, false));

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let top = (0..24).find(|&y| buffer[(44, y)].symbol() == "╭").unwrap();
        // Bottom left of the square is black.
        let cell = &buffer[(45, top + SQUARE_ROWS as u16)];
        assert_eq!(cell.symbol(), "◆");
        assert_eq!(cell.bg, Color::Rgb(0, 0, 0));
        assert_eq!(cell.fg, Color::White);
        assert_eq!(app.palette.bands[0], BarColor::rgb(0, 0, 0));
    }

    #[test]
    fn test_single_color_mode_grays_out_mids_and_treble_and_keeps_the_size() {
        let render_popup_rows = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
            terminal.draw(|frame| render(app, frame)).unwrap();
            let buffer = terminal.backend().buffer().clone();
            let top = (0..20).find(|&y| buffer[(23, y)].symbol() == "╭").unwrap();
            let bottom = (0..20).find(|&y| buffer[(23, y)].symbol() == "╰").unwrap();
            (bottom - top, top, buffer)
        };
        let mut app = App::new();
        app.open_color_popup();
        let (gradient_height, _, _) = render_popup_rows(&mut app);
        app.palette.mode = ColorMode::Single;
        let (height, top, buffer) = render_popup_rows(&mut app);
        assert_eq!(height, gradient_height);

        // Rows after the border: mode, bass, mids, treble. Mids and treble are dark gray text
        // with the bass color's swatch; the bass row is not grayed out.
        let swatch_x = 23 + 1 + 2 + 7;
        let label = |y: u16| buffer[(25, y)].fg;
        assert_eq!(label(top + 2), Color::White);
        assert_eq!(label(top + 3), Color::DarkGray);
        assert_eq!(label(top + 4), Color::DarkGray);
        assert_eq!(buffer[(swatch_x, top + 3)].fg, Color::Rgb(0, 255, 255));
        assert_eq!(buffer[(swatch_x, top + 4)].fg, Color::Rgb(0, 255, 255));
    }

    #[test]
    fn test_mirror_checkbox_and_plain_mirror_colors_are_drawn() {
        let mut app = App::new();
        app.open_color_popup();
        app.select_prev_color_row();
        app.activate_color_row();
        app.update_audio_bars(vec![1.0; 32]);
        for _ in 0..30 {
            app.tick();
        }

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let text: String = (0..24)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .map(|(x, y)| buffer[(x, y)].symbol().to_string())
            .collect();
        assert!(text.contains("[ ] Darker"));
        let color_of = |buffer: &Buffer, word: &str| {
            let line = |y: u16| -> String { (0..80).map(|x| buffer[(x, y)].symbol()).collect() };
            let row = (0..24).find(|&y| line(y).contains(word)).unwrap();
            let x = line(row).split(word).next().unwrap().chars().count() as u16;
            buffer[(x, row)].fg
        };
        // Off: both the label and the checkbox are yellow. On: green.
        assert_eq!(color_of(buffer, "Mirror"), Color::Yellow);
        assert_eq!(color_of(buffer, "Darker"), Color::Yellow);
        app.activate_color_row();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let on = terminal.backend().buffer();
        assert_eq!(color_of(on, "Mirror"), Color::Green);
        assert_eq!(color_of(on, "Darker"), Color::Green);
        app.activate_color_row();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        // The leftmost mirror cell has the same color as the bar above the divider.
        assert_eq!(buffer[(8, 23)].fg, Color::Rgb(0, 255, 255));
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
        app.edit_color_picker(|picker| picker.nudge(-1, 0, false));
        assert_ne!(picking, FrameState::capture(&app, SIZE));
    }

    #[test]
    fn test_color_notice_is_drawn_after_confirming() {
        let mut app = App::new();
        app.open_color_popup();
        app.change_color_mode(true);
        app.close_color_popup();

        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let top_row: String = (0..40).map(|x| buffer[(x, 0)].symbol()).collect();
        assert!(top_row.ends_with(" color: Gradient "), "{top_row:?}");
        assert_eq!(
            FrameState::capture(&app, SIZE).color_notice.as_deref(),
            Some("Gradient")
        );
    }

    #[test]
    fn test_gradient_mode_blends_the_colors_across_the_bars() {
        let mut app = App::new();
        app.palette.mode = ColorMode::Gradient;
        let buffer = render_bars_with(&[1.0; 32], 64, 7, &app.palette);

        // Bars are two columns wide: first is the bass color, last the treble color, and no two
        // neighbouring bars share a color.
        assert_eq!(buffer[(0, 0)].fg, Color::Rgb(0, 255, 255));
        assert_eq!(buffer[(62, 0)].fg, Color::Rgb(255, 0, 255));
        let colors: Vec<Color> = (0..32).map(|bar| buffer[(bar * 2, 0)].fg).collect();
        assert!(colors.windows(2).all(|pair| pair[0] != pair[1]));
        // The mirror is the same blend, darker.
        assert_eq!(buffer[(0, 4)].fg, Color::Rgb(0, 128, 128));
        assert_ne!(buffer[(20, 4)].fg, buffer[(22, 4)].fg);
    }

    #[test]
    fn test_vertical_mode_colors_every_bar_from_its_base_to_its_top() {
        let palette = Palette {
            mode: ColorMode::Vertical,
            bands: [
                BarColor::rgb(255, 0, 0),
                BarColor::rgb(0, 255, 0),
                BarColor::rgb(0, 0, 255),
            ],
            ..Palette::default()
        };
        // 11 rows: 5 above the divider, the divider, 5 in the mirror.
        let buffer = render_bars_with(&[1.0; 32], 64, 11, &palette);

        let column = |bar: u16, rows: std::ops::Range<u16>| -> Vec<Color> {
            rows.map(|y| buffer[(bar * 2, y)].fg).collect()
        };
        // Top to bottom above the divider (row 5). The gradient ends at the height the loudest
        // bars reach (0.75), so the top two rows already show the final color.
        let upper = column(0, 0..5);
        assert_eq!(upper[0], Color::Rgb(0, 0, 255));
        assert_eq!(upper[1], Color::Rgb(0, 0, 255));
        assert_eq!(upper[4], Color::Rgb(255, 0, 0));
        // The rows in between are real blends.
        assert_ne!(upper[2], upper[1]);
        assert_ne!(upper[2], upper[3]);
        assert_ne!(upper[3], upper[4]);
        // All bars share the same column of colors, unlike the horizontal modes.
        assert_eq!(column(0, 0..5), column(31, 0..5));
        // The mirror repeats the blend going down, darker, and ends sooner since it is
        // attenuated.
        let mirror = column(0, 6..11);
        assert_eq!(mirror[0], Color::Rgb(128, 0, 0));
        assert_eq!(mirror[3], Color::Rgb(0, 0, 128));
        assert_eq!(mirror[4], Color::Rgb(0, 0, 128));
    }

    #[test]
    fn test_vertical_final_color_is_reached_by_a_bar_at_the_peak_target() {
        let palette = Palette {
            mode: ColorMode::Vertical,
            bands: [
                BarColor::rgb(255, 0, 0),
                BarColor::rgb(0, 255, 0),
                BarColor::rgb(0, 0, 255),
            ],
            ..Palette::default()
        };
        let peak = loudest_bar_height(1.0);
        let buffer = render_bars_with(&[peak; 32], 64, 41, &palette);

        // 20 rows above the divider; the bar's topmost visible cell is the final color.
        let top_cell = (0..20)
            .map(|y| &buffer[(0, y)])
            .find(|cell| cell.symbol() != " ")
            .unwrap();
        assert_eq!(top_cell.fg, Color::Rgb(0, 0, 255));
    }

    #[test]
    fn test_lower_sensitivity_needs_shorter_bars_for_the_final_vertical_color() {
        let palette = Palette {
            mode: ColorMode::Vertical,
            bands: [
                BarColor::rgb(255, 0, 0),
                BarColor::rgb(0, 255, 0),
                BarColor::rgb(0, 0, 255),
            ],
            ..Palette::default()
        };
        let final_color = Color::Rgb(0, 0, 255);
        let top_color = |bar_height: f32, gain: f32| {
            let buffer = render_bars_with_gain(&[bar_height; 32], 64, 41, &palette, gain);
            (0..20)
                .map(|y| &buffer[(0, y)])
                .find(|cell| cell.symbol() != " ")
                .unwrap()
                .fg
        };

        // A bar a little under the usual peak height: at full sensitivity it has not quite
        // reached the final color, but with the gain turned down the peak is lower, so it has.
        let short_bar = loudest_bar_height(0.5);
        assert_ne!(top_color(short_bar, 1.0), final_color);
        assert_eq!(top_color(short_bar, 0.5), final_color);
        // Turning the gain up asks for taller bars.
        let usual = loudest_bar_height(1.0);
        assert_eq!(top_color(usual, 1.0), final_color);
        assert_ne!(top_color(usual, 2.0), final_color);
    }

    #[test]
    fn test_color_popup_labels_follow_the_mode() {
        let mut app = App::new();
        app.palette.mode = ColorMode::Vertical;
        app.open_color_popup();

        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|frame| render(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..20)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .map(|(x, y)| buffer[(x, y)].symbol().to_string())
            .collect();
        for word in [
            "Vertical gradient",
            "Base",
            "Middle",
            "Top",
            "←→ mode",
            "◀",
            "▶",
        ] {
            assert!(text.contains(word), "{word}");
        }
    }

    #[test]
    fn test_frame_state_changes_with_the_sensitivity() {
        let mut app = App::new();
        let before = FrameState::capture(&app, SIZE);
        app.increase_gain();
        assert_ne!(before.gain, FrameState::capture(&app, SIZE).gain);
    }

    #[test]
    fn test_frame_state_changes_with_the_color_mode() {
        let mut app = App::new();
        let before = FrameState::capture(&app, SIZE);
        app.palette.mode = ColorMode::Gradient;
        assert_ne!(before, FrameState::capture(&app, SIZE));
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
