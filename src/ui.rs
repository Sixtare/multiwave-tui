use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};

use crate::app::App;

pub fn render(app: &mut App, frame: &mut Frame) {
    match app.show_popup {
        false => draw_soundwave(frame, frame.area(), &app.displayed_bars()),
        // Uses the full screen, centering the visualizer vertically and horizontally
        true => render_popup(frame, app),
    }
    
    if let Some(error) = &app.audio_error {
        render_audio_error(frame, error);
    }
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
    let centered_area = area.centered(
        Constraint::Percentage(70),
        Constraint::Length(popup_height),
    );

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

/// Renders normal bars (upward) combined with the mirror effect (downward / upside down).
fn render_mirrored_soundwave(
    amplitudes: &[f32],
    upper_rows: usize,
    mirror_rows: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if upper_rows == 0 && mirror_rows == 0 {
        return lines;
    }

    // 1. Upper Visualizer (grows upward from the center line to the top of the screen)
    for row in (0..upper_rows).rev() {
        let mut spans = Vec::new();
        let threshold = row as f32 / upper_rows as f32;

        for (i, &amp) in amplitudes.iter().enumerate() {
            let normalized = amp.clamp(0.0, 1.0);

            let character = if normalized >= threshold + (1.0 / upper_rows as f32) {
                '█'
            } else if normalized >= threshold {
                let fractional = (normalized - threshold) * upper_rows as f32;
                let char_index = (fractional * (WAVE_CHARS.len() - 1) as f32).round() as usize;
                WAVE_CHARS[char_index.clamp(0, WAVE_CHARS.len() - 1)]
            } else {
                ' '
            };

            // Vibrant gradient: bass cyan -> mids green -> treble magenta
            let color = if i < amplitudes.len() / 3 {
                Color::Cyan
            } else if i < (amplitudes.len() * 2) / 3 {
                Color::Green
            } else {
                Color::Magenta
            };

            spans.push(Span::styled(
                format!("{} ", character),
                Style::default().fg(color),
            ));
        }

        lines.push(Line::from(spans));
    }

    // 2. Subtle center divider line (mirror horizon at screen center)
    let divider_spans: Vec<Span<'static>> = vec![Span::styled(
        "──".repeat(amplitudes.len()),
        Style::default().fg(Color::DarkGray),
    )];
    lines.push(Line::from(divider_spans));

    // 3. Mirror Effect (upside down, grows down towards screen bottom)
    for row in 0..mirror_rows {
        let mut spans = Vec::new();
        let threshold = row as f32 / mirror_rows as f32;

        for (i, &amp) in amplitudes.iter().enumerate() {
            // Slight reflection attenuation for a more realistic mirror/water effect
            let mirror_amp = (amp * 0.85).clamp(0.0, 1.0);

            let character = if mirror_amp >= threshold + (1.0 / mirror_rows as f32) {
                '█'
            } else if mirror_amp >= threshold {
                let fractional = (mirror_amp - threshold) * mirror_rows as f32;
                let char_index =
                    (fractional * (INVERTED_WAVE_CHARS.len() - 1) as f32).round() as usize;
                INVERTED_WAVE_CHARS[char_index.clamp(0, INVERTED_WAVE_CHARS.len() - 1)]
            } else {
                ' '
            };

            // Mirror reflection colors: softer/darker tones for depth effect
            let mirror_color = if i < amplitudes.len() / 3 {
                Color::Rgb(0, 120, 160)
            } else if i < (amplitudes.len() * 2) / 3 {
                Color::Rgb(30, 130, 70)
            } else {
                Color::Rgb(130, 40, 130)
            };

            spans.push(Span::styled(
                format!("{} ", character),
                Style::default().fg(mirror_color),
            ));
        }

        lines.push(Line::from(spans));
    }

    lines
}

pub fn draw_soundwave(frame: &mut Frame, area: Rect, bars: &[f32]) {
    let available_height = area.height as usize;
    if available_height < 3 {
        return;
    }

    // Reserves 1 line for the center horizon/divider
    let usable_lines = available_height.saturating_sub(1);

    // Complete symmetrical division: no artificial clamp limits.
    // The divider line is fixed at the exact center of the screen and bars
    // utilize 100% of the available vertical height (reaching the top and bottom).
    let upper_rows = usable_lines / 2;
    let mirror_rows = usable_lines / 2;
    let content_lines = upper_rows + 1 + mirror_rows;

    let wave_lines = render_mirrored_soundwave(bars, upper_rows, mirror_rows);
    let soundwave_widget = Paragraph::new(wave_lines).alignment(Alignment::Center);

    // Vertically centers while filling the entire screen
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(content_lines as u16),
            Constraint::Fill(1),
        ])
        .split(area);

    frame.render_widget(soundwave_widget, chunks[1]);
}
