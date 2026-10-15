use ratatui::{
    Frame, layout::{Alignment, Constraint, Direction, Layout, Rect}, style::{Color, Style, Stylize}, text::{Line, Span}, widgets::{Bar, BarChart, Block, BorderType, Borders, Paragraph},
};

use crate::app::App;

pub fn render(app: &mut App, frame: &mut Frame) {
    
    let vertical: Layout = Layout::vertical([
        Constraint::Percentage(30), 
        Constraint::Percentage(70)]);

    let horizontal: Layout = Layout::horizontal([
        Constraint::Percentage(5), 
        Constraint::Percentage(90),
        Constraint::Percentage(5)]);

    
    
    let [top, bottom] = frame.area().layout(&vertical);
    let [left, middle, right] = bottom.layout(&horizontal);

    render_paragraph(app, frame, top);
    draw_demo_soundwave(frame, middle, app.tick_counter);
}

fn render_paragraph(app: &mut App, frame: &mut Frame, top_rect: Rect){
    frame.render_widget(
        Paragraph::new(format!(
            "
        Press `Esc`, `Ctrl-C` or `q` to stop running.\n\
        Press `j` and `l` to switch between bars respectively.\n\
        Press 'i' and 'k' to increase and decrease bar height. \n\
        Counter: {}
      ",
            app.counter
        ))
        .block(
            Block::default()
                .title("Audio visualizer")
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

        for &amp in amplitudes {
            let normalized = amp.clamp(0.0, 1.0);

            // Calcula qual caractere usar com base na altura da barra e na linha atual
            // Se a amplitude for muito alta para esta linha, pinta o bloco cheio ('█')
            // Se estiver na ponta exata da barra, usa um caractere parcial do WAVE_CHARS para dar suavidade!
            let character = if normalized >= threshold + (1.0 / total_rows as f32) {
                '█'
            } else if normalized >= threshold {
                // Parte fracionária para suavizar a ponta da barra
                let fractional = (normalized - threshold) * total_rows as f32;
                let char_index = (fractional * (WAVE_CHARS.len() - 1) as f32).round() as usize;
                WAVE_CHARS[char_index.clamp(0, WAVE_CHARS.len() - 1)]
            } else {
                ' ' // Vazio acima da barra
            };

            spans.push(Span::styled(
                format!("{} ", character), // O espaço mantém o espaçamento horizontal entre as colunas
                Style::default().fg(Color::Cyan),
            ));
        }

        lines.push(Line::from(spans));
    }

    lines
}

pub fn draw_demo_soundwave(frame: &mut Frame, area: Rect, tick: u64) {
    // 1. Simulação de 16 barras de áudio usando a função seno para criar um efeito de onda fluida    
    let num_bars = 32;
    let mut mock_amplitudes = vec![0.0; num_bars];

    for (i, amp) in mock_amplitudes.iter_mut().enumerate() {
        // Cria um movimento orgânico baseado no "tick" (tempo/frames) atual
        let wave = ((tick as f64 * 0.2) + (i as f64 * 0.5)).sin();
        // Converte o seno de [-1, 1] para [0.1, 1.0] para evitar barras totalmente mortas
        *amp = ((wave + 1.0) / 2.0) as f32 * 0.9 + 0.1;
    }

    // 2. Gera a linha de texto com as ondas sonoras
    let wave_line = render_soundwave(&mock_amplitudes,20);

    // 3. Cria o Paragraph envolvendo o gráfico
    let soundwave_widget = Paragraph::new(wave_line)
        .alignment(Alignment::Center);

    // 4. Renderiza na tela
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),       // Espaço livre em cima
            Constraint::Length(20),     // O Paragraph terá exatamente 5 linhas de altura no fundo
        ])
        .split(area);

    frame.render_widget(soundwave_widget, chunks[1]);
}
