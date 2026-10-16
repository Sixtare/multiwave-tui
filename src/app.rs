#[derive(Debug)]
pub struct App {
    /// should the application exit?
    pub should_quit: bool,
    /// counter
    pub counter: u8,
    /// tick counter
    pub tick_counter: u64,
    /// Vetor com as magnitudes normalizadas das barras de áudio (0.0 a 1.0)
    pub audio_bars: Vec<f32>,
    /// Ganho de sensibilidade para a visualização
    pub gain: f32,
}

impl Default for App {
    fn default() -> Self {
        Self {
            should_quit: false,
            counter: 0,
            tick_counter: 0,
            audio_bars: vec![0.0; 32],
            gain: 1.0,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&self) {}

    /// Set should_quit to true to quit the application.
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn increment_counter(&mut self) {
        if let Some(res) = self.counter.checked_add(1) {
            self.counter = res;
        }
    }

    pub fn decrement_counter(&mut self) {
        if let Some(res) = self.counter.checked_sub(1) {
            self.counter = res;
        }
    }

    pub fn increment_tick_counter(&mut self) {
        self.tick_counter += 1;
    }

    /// Atualiza as barras de áudio recebidas da FFT
    pub fn update_audio_bars(&mut self, bars: Vec<f32>) {
        self.audio_bars = bars;
    }

    /// Aumenta a sensibilidade/ganho da visualização
    pub fn increase_gain(&mut self) {
        self.gain = (self.gain + 0.25).min(10.0);
    }

    /// Diminui a sensibilidade/ganho da visualização
    pub fn decrease_gain(&mut self) {
        self.gain = (self.gain - 0.25).max(0.1);
    }

    /// Retorna as barras de áudio com o ajuste de ganho atual aplicado
    pub fn displayed_bars(&self) -> Vec<f32> {
        self.audio_bars
            .iter()
            .map(|&amp| (amp * self.gain).clamp(0.0, 1.0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_increment_counter() {
        let mut app = App::default();
        app.increment_counter();
        assert_eq!(app.counter, 1);
    }

    #[test]
    fn test_app_decrement_counter() {
        let mut app = App::default();
        app.decrement_counter();
        assert_eq!(app.counter, 0);
    }

    #[test]
    fn test_app_update_audio_bars() {
        let mut app = App::default();
        let new_bars = vec![0.5; 32];
        app.update_audio_bars(new_bars);
        assert_eq!(app.audio_bars.len(), 32);
        assert_eq!(app.audio_bars[0], 0.5);
    }

    #[test]
    fn test_app_gain_adjustment() {
        let mut app = App::default();
        app.audio_bars = vec![0.4; 32];
        app.increase_gain();
        assert_eq!(app.gain, 1.25);
        assert_eq!(app.displayed_bars()[0], (0.4 * 1.25 as f32).clamp(0.0, 1.0));
    }
}