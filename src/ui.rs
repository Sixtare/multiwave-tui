use ratatui::{
    Frame, layout::{Alignment, Constraint, Layout}, style::{Color, Style, Stylize}, widgets::{Bar, BarChart, Block, BorderType, Borders, Paragraph},
};

use crate::app::App;

pub fn render(app: &mut App, frame: &mut Frame) {
    render_paragraph(app, frame);
    render_bars(frame);  
}

fn render_paragraph(app: &mut App, frame: &mut Frame){
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
        frame.area(),
    ); 
}

fn render_bars(frame: &mut Frame){
    let vertical = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).spacing(1);
    let horizontal = Layout::horizontal([Constraint::Length(28), Constraint::Fill(1)]).spacing(1);
    let [top, main] = frame.area().layout(&vertical);
    let [left, right] = main.layout(&horizontal);

    let bars = vec![
        Bar::with_label("Red", 30).red(),
        Bar::with_label("Blue", 20).blue(),
        Bar::with_label("Green", 15).green(),
        Bar::with_label("Yellow", 10).yellow(),
    ];
    let chart = BarChart::vertical(bars).bar_width(6);
    frame.render_widget(chart, right);
}