use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::App;

pub fn render(app: &mut App, frame: &mut Frame) {
    // Utiliza a tela inteira, centralizando o visualizador vertical e horizontalmente
    draw_soundwave(frame, frame.area(), &app.displayed_bars());
}

// Caracteres para as barras superiores (ancoradas na base)
const WAVE_CHARS: &[char] = &[' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// Caracteres para o espelho inferior (ancorados no topo / de cabeça para baixo)
const INVERTED_WAVE_CHARS: &[char] = &[' ', '▔', '▀', '█'];

/// Renderiza as barras normais (para cima) combinadas com o efeito de espelho (de cabeça para baixo).
fn render_mirrored_soundwave(
    amplitudes: &[f32],
    upper_rows: usize,
    mirror_rows: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if upper_rows == 0 && mirror_rows == 0 {
        return lines;
    }

    // 1. Visualizador Superior (cresce para cima a partir da linha central até o topo da tela)
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

            // Gradiente vibrante: graves ciano -> médios verde -> agudos magenta
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

    // 2. Linha divisória central sutil (horizonte do espelho no centro da tela)
    let divider_spans: Vec<Span<'static>> = vec![Span::styled(
        "──".repeat(amplitudes.len()),
        Style::default().fg(Color::DarkGray),
    )];
    lines.push(Line::from(divider_spans));

    // 3. Efeito Espelho / Mirror (de cabeça para baixo, cresce até o fundo da tela)
    for row in 0..mirror_rows {
        let mut spans = Vec::new();
        let threshold = row as f32 / mirror_rows as f32;

        for (i, &amp) in amplitudes.iter().enumerate() {
            // Leve atenuação no reflexo para um efeito mais realista de espelho/água
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

            // Cores do reflexo: tons mais suaves/esmaecidos para efeito de profundidade
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

    // Reserva 1 linha para o horizonte/divisor central
    let usable_lines = available_height.saturating_sub(1);

    // Divisão simétrica completa: sem limite artificial de clamp.
    // A linha divisória fica fixada no meio exato da tela e as barras
    // utilizam 100% da altura vertical disponível (subindo até o topo e descendo até a base).
    let upper_rows = usable_lines / 2;
    let mirror_rows = usable_lines / 2;
    let content_lines = upper_rows + 1 + mirror_rows;

    let wave_lines = render_mirrored_soundwave(bars, upper_rows, mirror_rows);
    let soundwave_widget = Paragraph::new(wave_lines).alignment(Alignment::Center);

    // Centraliza verticalmente preenchendo a tela inteira
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
