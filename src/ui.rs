use ratatui::{
    Frame, layout::{Alignment, Constraint, Layout, Rect}, style::{Color, Style, Stylize}, text::{Line, Span}, widgets::{Bar, BarChart, Block, BorderType, Borders, Paragraph},
};

use crate::app::App;

pub fn render(app: &mut App, frame: &mut Frame) {
    
    let vertical = Layout::vertical([
        Constraint::Percentage(30), 
        Constraint::Percentage(70)]);

    let horizontal = Layout::horizontal([
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

fn render_soundwave(amplitudes: &[f32]) -> Line<'static> {
    let mut spans = Vec::new();

    for &amp in amplitudes {
        let normalized = amp.clamp(0.0, 1.0);
        let index = (normalized * (WAVE_CHARS.len() - 1) as f32).round() as usize;
        let character = WAVE_CHARS[index];

        spans.push(Span::styled(
            format!("{} ", character), // O espaço garante o espaçamento entre as barras
            Style::default().fg(Color::Cyan),
        ));
    }

    Line::from(spans)
}

pub fn draw_demo_soundwave(f: &mut Frame, area: Rect, tick: u64) {
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
    let wave_line = render_soundwave(&mock_amplitudes);

    // 3. Cria o Paragraph envolvendo o gráfico
    let soundwave_widget = Paragraph::new(wave_line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Equalizador (Demonstração) ")
                .title_alignment(Alignment::Center)
                
        )
        .alignment(Alignment::Center);

    // 4. Renderiza na tela
    f.render_widget(soundwave_widget, area);
}
