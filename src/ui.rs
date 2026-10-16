use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::app::App;

pub fn render(app: &mut App, frame: &mut Frame) {
    let vertical = Layout::vertical([
        Constraint::Percentage(25),
        Constraint::Percentage(75),
    ]);

    let horizontal = Layout::horizontal([
        Constraint::Percentage(5),
        Constraint::Percentage(90),
        Constraint::Percentage(5),
    ]);

    let [top, bottom] = frame.area().layout(&vertical);
    let [_left, middle, _right] = bottom.layout(&horizontal);

    render_paragraph(app, frame, top);
    draw_soundwave(frame, middle, &app.displayed_bars());
}

fn render_paragraph(app: &mut App, frame: &mut Frame, top_rect: Rect) {
    frame.render_widget(
        Paragraph::new(format!(
            "Pressione `Esc`, `Ctrl-C` ou `q` para sair.\n\
             Pressione `j` e `l` para alterar o contador.\n\
             Pressione `i` e `k` para aumentar ou diminuir a sensibilidade (Ganho: {:.2}x).\n\
             Contador: {}",
            app.gain, app.counter
        ))
        .block(
            Block::default()
                .title(" Visualizador de Áudio em Tempo Real (CPAL + RealFFT) ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center),
        top_rect,
    );
}

const WAVE_CHARS: &[char] = &[' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn render_soundwave(amplitudes: &[f32], total_rows: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if total_rows == 0 {
        return lines;
    }

    // Percorre de cima para baixo (da linha mais alta até a base da caixa)
    for row in (0..total_rows).rev() {
        let mut spans = Vec::new();
        // Define o limiar percentual de altura que esta linha representa (0.0 no chão, 1.0 no topo)
        let threshold = row as f32 / total_rows as f32;

        for (i, &amp) in amplitudes.iter().enumerate() {
            let normalized = amp.clamp(0.0, 1.0);

            // Calcula qual caractere usar com base na altura da barra e na linha atual
            let character = if normalized >= threshold + (1.0 / total_rows as f32) {
                '█'
            } else if normalized >= threshold {
                let fractional = (normalized - threshold) * total_rows as f32;
                let char_index = (fractional * (WAVE_CHARS.len() - 1) as f32).round() as usize;
                WAVE_CHARS[char_index.clamp(0, WAVE_CHARS.len() - 1)]
            } else {
                ' ' // Vazio acima da barra
            };

            // Gradiente de cor por frequência (graves ciano -> médios verde -> agudos magenta)
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

    lines
}

pub fn draw_soundwave(frame: &mut Frame, area: Rect, bars: &[f32]) {
    let rows = 18;
    let wave_lines = render_soundwave(bars, rows);

    let soundwave_widget = Paragraph::new(wave_lines)
        .alignment(Alignment::Center);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(rows as u16 + 2), // +2 para as bordas do bloco
        ])
        .split(area);

    frame.render_widget(soundwave_widget, chunks[1]);
}
