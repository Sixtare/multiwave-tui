use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};

use crate::app::App;

pub fn render(app: &mut App, frame: &mut Frame) {
    // Uses the full screen, centering the visualizer vertically and horizontally
    draw_soundwave(frame, frame.area(), &app.displayed_bars());
    if app.show_popup {
        render_popup(frame, app);
    }
}

// Characters for the upper bars (base-anchored)
const WAVE_CHARS: &[char] = &[' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// Characters for the lower mirror (top-anchored / upside down)
const INVERTED_WAVE_CHARS: &[char] = &[' ', '▔', '▀', '█'];

// Draws popup menu with available output audio devices
fn render_popup(frame: &mut Frame, app: &App) {
    let popup_block = Block::bordered()
        .title(" Select Output Device (↑/↓ to navigate, Enter to select, Esc/s to close) ")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Yellow));

    let items_count = app.devices.len().max(1);
    let popup_height = (items_count as u16 + 4).clamp(6, 18);
    let centered_area = frame.area().centered(
        Constraint::Percentage(70),
        Constraint::Length(popup_height),
    );

    // Clears out any background in the area before rendering the popup
    frame.render_widget(Clear, centered_area);

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
        .block(popup_block)
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, centered_area);
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
