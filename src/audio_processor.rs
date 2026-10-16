use std::f32::consts::PI;
use std::sync::Arc;
use realfft::{RealFftPlanner, RealToComplex};

/// Processador de áudio responsável por receber amostras brutas (intercaladas),
/// aplicar downmix, acumulação, FFT (via realfft), cálculo de magnitude,
/// binning de frequências e normalização de ganho.
pub struct AudioProcessor {
    channels: usize,
    fft_size: usize,
    num_bars: usize,
    pub gain: f32,
    sample_accumulator: Vec<f32>,
    fft_plan: Arc<dyn RealToComplex<f32>>,
    input_buffer: Vec<f32>,
    output_buffer: Vec<realfft::num_complex::Complex<f32>>,
    window: Vec<f32>,
}

impl AudioProcessor {
    /// Cria uma nova instância de `AudioProcessor`.
    /// 
    /// - `channels`: número de canais de áudio recebidos (ex: 1 para mono, 2 para estéreo).
    /// - `fft_size`: tamanho do bloco da FFT (deve ser potência de 2, ex: 1024).
    /// - `num_bars`: quantidade de barras para a visualização na TUI (ex: 16 ou 32).
    /// - `gain`: multiplicador de ganho para normalização de volume.
    pub fn new(channels: usize, fft_size: usize, num_bars: usize, gain: f32) -> Self {
        assert!(fft_size > 0 && (fft_size & (fft_size - 1)) == 0, "fft_size deve ser potência de 2");
        assert!(num_bars > 0 && num_bars <= fft_size / 2, "num_bars inválido para o fft_size");

        let mut planner = RealFftPlanner::<f32>::new();
        let fft_plan = planner.plan_fft_forward(fft_size);

        let input_buffer = fft_plan.make_input_vec();
        let output_buffer = fft_plan.make_output_vec();

        // Janela de Hann pré-calculada para suavizar as bordas das amostras e reduzir vazamento espectral
        let window: Vec<f32> = (0..fft_size)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (fft_size - 1) as f32).cos()))
            .collect();

        Self {
            channels: channels.max(1),
            fft_size,
            num_bars,
            gain,
            sample_accumulator: Vec::with_capacity(fft_size * 2),
            fft_plan,
            input_buffer,
            output_buffer,
            window,
        }
    }

    /// Processa um bloco de amostras cruas intercaladas vindas do CPAL.
    /// Retorna uma lista de blocos de barras gerados (cada bloco contendo `num_bars` valores entre 0.0 e 1.0).
    pub fn process_interleaved_samples(&mut self, interleaved: &[f32]) -> Vec<Vec<f32>> {
        let mut results = Vec::new();

        // 1. Downmix de Canais (Estéreo para Mono)
        if self.channels == 1 {
            self.sample_accumulator.extend_from_slice(interleaved);
        } else {
            for frame in interleaved.chunks(self.channels) {
                let mono_sample = if frame.len() == 1 {
                    frame[0]
                } else {
                    // Média matemática dos canais disponíveis
                    frame.iter().sum::<f32>() / frame.len() as f32
                };
                self.sample_accumulator.push(mono_sample);
            }
        }

        // 2. Acumulação de Amostras (Buffering)
        // Enquanto houver pelo menos `fft_size` amostras acumuladas, processa um bloco de FFT
        while self.sample_accumulator.len() >= self.fft_size {
            // Fatiar o bloco de tamanho `fft_size`
            let chunk = self.sample_accumulator.drain(0..self.fft_size);

            // 3. Execução da Transformada Rápida de Fourier (realfft)
            // Copiar o bloco de amostras aplicando a janela de Hann
            for (idx, sample) in chunk.enumerate() {
                self.input_buffer[idx] = sample * self.window[idx];
            }

            if self.fft_plan.process(&mut self.input_buffer, &mut self.output_buffer).is_ok() {
                // 4. Cálculo de Magnitude (Módulo)
                // O buffer de saída tem tamanho fft_size / 2 + 1 (ex: 513 para 1024)
                let magnitudes: Vec<f32> = self.output_buffer
                    .iter()
                    .map(|c| c.norm())
                    .collect();

                // 5. Agrupamento de Frequências (Binning)
                // Desconsideramos o bin 0 (DC offset) para focar nas frequências audíveis
                let usable_bins = &magnitudes[1..];
                let bin_count = usable_bins.len();
                let chunk_size = bin_count / self.num_bars;

                let mut bars = Vec::with_capacity(self.num_bars);

                for i in 0..self.num_bars {
                    let start = i * chunk_size;
                    let end = if i == self.num_bars - 1 {
                        bin_count
                    } else {
                        (i + 1) * chunk_size
                    };

                    let slice = &usable_bins[start..end];
                    let avg_magnitude = if !slice.is_empty() {
                        slice.iter().sum::<f32>() / slice.len() as f32
                    } else {
                        0.0
                    };

                    // 6. Normalização e Ajuste de Ganho
                    // Normalizamos pela metade do tamanho da FFT para compensar a escala de energia
                    let normalized_amp = (avg_magnitude / (self.fft_size as f32 / 2.0)) * self.gain;
                    let clamped = normalized_amp.clamp(0.0, 1.0);
                    bars.push(clamped);
                }

                results.push(bars);
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_processor_sin_wave() {
        let fft_size = 1024;
        let num_bars = 16;
        let gain = 2.0;
        let mut processor = AudioProcessor::new(2, fft_size, num_bars, gain);

        // Gera 2048 amostras estéreo de uma senoide pura de baixa frequência
        // 44100 Hz, seno de 440 Hz
        let sample_rate = 44100.0;
        let freq = 440.0;
        let mut samples = Vec::new();

        for i in 0..2048 {
            let t = i as f32 / sample_rate;
            let val = (2.0 * PI * freq * t).sin();
            samples.push(val); // canal esquerdo
            samples.push(val); // canal direito
        }

        let bars_list = processor.process_interleaved_samples(&samples);
        // Com 2048 amostras estéreo = 2048 frames mono => 2 blocos de 1024
        assert_eq!(bars_list.len(), 2);
        for bars in bars_list {
            assert_eq!(bars.len(), num_bars);
            // Deve haver energia significativa nas primeiras barras (graves/médios baixos)
            let total_energy: f32 = bars.iter().sum();
            assert!(total_energy > 0.0);
            for &val in &bars {
                assert!(val >= 0.0 && val <= 1.0);
            }
        }
    }
}
