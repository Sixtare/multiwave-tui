#[derive(Debug)]
pub struct App {
    /// should the application exit?
    pub should_quit: bool,
    /// counter
    pub counter: u8,
    /// tick counter
    pub tick_counter: u64,
    /// Vetor com as magnitudes normalizadas e suavizadas das barras de áudio (0.0 a 1.0)
    pub audio_bars: Vec<f32>,
    /// Vetor com os valores alvo recebidos mais recentemente da FFT
    pub target_bars: Vec<f32>,
    /// Ganho de sensibilidade para a visualização
    pub gain: f32,
    /// Fator de subida (Attack): resposta rápida e amortecida
    pub attack_factor: f32,
    /// Fator de queda (Decay): decaimento gradual e fluido
    pub decay_factor: f32,
}

impl Default for App {
    fn default() -> Self {
        Self {
            should_quit: false,
            counter: 0,
            tick_counter: 0,
            audio_bars: vec![0.0; 32],
            target_bars: vec![0.0; 32],
            gain: 1.0,
            attack_factor: 0.65,
            decay_factor: 0.20,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Executa um ciclo de animação a cada tick (~60 FPS)
    pub fn tick(&mut self) {
        self.tick_counter += 1;
        // Puxa gradualmente os alvos para baixo para esmaecimento natural do som
        for target in &mut self.target_bars {
            *target = (*target * 0.94).max(0.0);
        }
        self.apply_smoothing();
    }

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
    /// Atualiza as barras de áudio recebidas da FFT e aplica suavização
    pub fn update_audio_bars(&mut self, bars: Vec<f32>) {
        if self.audio_bars.len() != bars.len() {
            self.audio_bars = bars.clone();
            self.target_bars = bars;
            return;
        }

        self.target_bars = bars;
        self.apply_smoothing();
    }

    /// Aplica interpolação suave (Attack / Decay) para evitar saltos visuais bruscos
    pub fn apply_smoothing(&mut self) {
        for (current, &target) in self.audio_bars.iter_mut().zip(self.target_bars.iter()) {
            if target > *current {
                // Subida: interpolação rápida em direção ao alvo
                *current += (target - *current) * self.attack_factor;
            } else {
                // Queda: decaimento gradual e elástico
                *current -= (*current - target) * self.decay_factor;
            }

            if *current < 0.005 {
                *current = 0.0;
            } else if *current > 1.0 {
                *current = 1.0;
            }
        }
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
    fn test_app_update_audio_bars_smoothing() {
        let mut app = App::default();
        // Inicializa com 0.0
        assert_eq!(app.audio_bars[0], 0.0);

        // Envia pico abrupto de 1.0
        let new_bars = vec![1.0; 32];
        app.update_audio_bars(new_bars);

        // A barra não deve pular instantaneamente para 1.0, mas sim subir suavemente
        assert!(app.audio_bars[0] > 0.5 && app.audio_bars[0] < 1.0);

        // Quando o alvo cai para 0.0
        let zero_bars = vec![0.0; 32];
        app.update_audio_bars(zero_bars);

        // A barra deve descer gradualmente sem despencar de vez para 0.0
        assert!(app.audio_bars[0] > 0.0);
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