#[derive(Debug)]
pub struct App {
    /// Should the application exit?
    pub should_quit: bool,
    /// Counter value.
    pub counter: u8,
    /// Tick counter.
    pub tick_counter: u64,
    /// Vector containing normalized and smoothed audio bar magnitudes (0.0 to 1.0).
    pub audio_bars: Vec<f32>,
    /// Vector containing the target values most recently received from the FFT.
    pub target_bars: Vec<f32>,
    /// Sensitivity gain multiplier for visualization.
    pub gain: f32,
    /// Attack factor: fast and damped rising response.
    pub attack_factor: f32,
    /// Decay factor: gradual and fluid falling response.
    pub decay_factor: f32,
    /// Whether to display the output device selection popup.
    pub show_popup: bool,
    /// List of available audio device names for display in the popup menu.
    pub devices: Vec<String>,
    /// Index of the currently highlighted device in the popup menu.
    pub selected_device_idx: usize,
    /// Index of the currently active audio device being captured.
    pub active_device_idx: usize,
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
            show_popup: false,
            devices: Vec::new(),
            selected_device_idx: 0,
            active_device_idx: 0,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs a new instance of [`App`] with device names and an active index.
    pub fn with_devices(devices: Vec<String>, active_device_idx: usize) -> Self {
        Self {
            devices,
            selected_device_idx: active_device_idx,
            active_device_idx,
            ..Self::default()
        }
    }

    /// Executes an animation cycle on each tick (~60 FPS).
    pub fn tick(&mut self) {
        self.tick_counter += 1;
        // Gradually decay targets for natural sound fade-out
        for target in &mut self.target_bars {
            *target = (*target * 0.94).max(0.0);
        }
        self.apply_smoothing();
    }

    /// Sets should_quit to true to quit the application.
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

    /// Moves the popup selection cursor down.
    pub fn select_next_device(&mut self) {
        if !self.devices.is_empty() {
            self.selected_device_idx = (self.selected_device_idx + 1) % self.devices.len();
        }
    }

    /// Moves the popup selection cursor up.
    pub fn select_prev_device(&mut self) {
        if !self.devices.is_empty() {
            if self.selected_device_idx == 0 {
                self.selected_device_idx = self.devices.len() - 1;
            } else {
                self.selected_device_idx -= 1;
            }
        }
    }

    /// Updates audio bars received from the FFT and applies smoothing.
    pub fn update_audio_bars(&mut self, bars: Vec<f32>) {
        if self.audio_bars.len() != bars.len() {
            self.audio_bars = bars.clone();
            self.target_bars = bars;
            return;
        }

        self.target_bars = bars;
        self.apply_smoothing();
    }

    /// Applies smooth interpolation (Attack / Decay) to prevent abrupt visual jumps.
    pub fn apply_smoothing(&mut self) {
        for (current, &target) in self.audio_bars.iter_mut().zip(self.target_bars.iter()) {
            if target > *current {
                // Attack: fast interpolation towards the target
                *current += (target - *current) * self.attack_factor;
            } else {
                // Decay: gradual and elastic falling decay
                *current -= (*current - target) * self.decay_factor;
            }

            if *current < 0.005 {
                *current = 0.0;
            } else if *current > 1.0 {
                *current = 1.0;
            }
        }
    }

    /// Increases visualization sensitivity/gain.
    pub fn increase_gain(&mut self) {
        self.gain = (self.gain + 0.25).min(10.0);
    }

    /// Decreases visualization sensitivity/gain.
    pub fn decrease_gain(&mut self) {
        self.gain = (self.gain - 0.25).max(0.1);
    }

    /// Returns audio bars with the current gain adjustment applied.
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
        assert_eq!(app.audio_bars[0], 0.0);

        let new_bars = vec![1.0; 32];
        app.update_audio_bars(new_bars);
        assert!(app.audio_bars[0] > 0.5 && app.audio_bars[0] < 1.0);

        let zero_bars = vec![0.0; 32];
        app.update_audio_bars(zero_bars);
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

    #[test]
    fn test_app_device_navigation() {
        let devices = vec![
            "Default Device".to_string(),
            "Headphones".to_string(),
            "HDMI Audio".to_string(),
        ];
        let mut app = App::with_devices(devices, 0);
        assert_eq!(app.selected_device_idx, 0);

        app.select_next_device();
        assert_eq!(app.selected_device_idx, 1);

        app.select_next_device();
        assert_eq!(app.selected_device_idx, 2);

        // Wrap-around forward
        app.select_next_device();
        assert_eq!(app.selected_device_idx, 0);

        // Wrap-around backward
        app.select_prev_device();
        assert_eq!(app.selected_device_idx, 2);
    }
}