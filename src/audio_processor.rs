use realfft::{RealFftPlanner, RealToComplex};
use std::f32::consts::PI;
use std::sync::Arc;

/// Processador de áudio responsável por receber amostras brutas (intercaladas),
/// aplicar downmix, acumulação, FFT (via realfft), cálculo de magnitude,
/// binning logarítmico de frequências e compensação de ganho para médios e agudos.
pub struct AudioProcessor {
    channels: usize,
    fft_size: usize,
    sample_rate: u32,
    num_bars: usize,
    pub gain: f32,
    sample_accumulator: Vec<f32>,
    fft_plan: Arc<dyn RealToComplex<f32>>,
    input_buffer: Vec<f32>,
    output_buffer: Vec<realfft::num_complex::Complex<f32>>,
    window: Vec<f32>,
    bin_ranges: Vec<(usize, usize)>,
    /// For bars narrower than one FFT bin: the lower bin and the fraction towards the next one.
    interpolation: Vec<Option<(usize, f32)>>,
    frequency_weights: Vec<f32>,
}

/// Lowest and highest frequencies covered by the bars, in Hz.
const MIN_FREQ: f32 = 40.0;
const MAX_FREQ: f32 = 16000.0;

/// Per-bar frequency layout derived from the bar count and the stream's sample rate.
struct BandLayout {
    bin_ranges: Vec<(usize, usize)>,
    interpolation: Vec<Option<(usize, f32)>>,
    frequency_weights: Vec<f32>,
}

impl BandLayout {
    fn new(sample_rate: u32, fft_size: usize, num_bars: usize) -> Self {
        let sample_rate = sample_rate as f32;
        let total_bins = fft_size / 2; // 512 usable bins for fft_size 1024
        let bins_per_hz = fft_size as f32 / sample_rate;

        let mut bin_ranges = Vec::with_capacity(num_bars);
        let mut interpolation = Vec::with_capacity(num_bars);
        for i in 0..num_bars {
            let f_start = MIN_FREQ * (MAX_FREQ / MIN_FREQ).powf(i as f32 / num_bars as f32);
            let f_end = MIN_FREQ * (MAX_FREQ / MIN_FREQ).powf((i + 1) as f32 / num_bars as f32);

            let bin_start = (f_start * bins_per_hz).round() as usize;
            let bin_start = bin_start.clamp(1, total_bins.saturating_sub(1));

            let bin_end = (f_end * bins_per_hz).round() as usize;
            let bin_end = bin_end.clamp(bin_start + 1, total_bins);

            bin_ranges.push((bin_start, bin_end));

            // A bar narrower than one FFT bin would repeat its neighbours' value as a plateau.
            // Read it at its center frequency, interpolating between the two surrounding bins.
            if (f_end - f_start) * bins_per_hz < 1.0 {
                let center = (f_start * f_end).sqrt() * bins_per_hz;
                let lower = (center.floor() as usize).clamp(1, total_bins.saturating_sub(1));
                interpolation.push(Some((lower, (center - lower as f32).clamp(0.0, 1.0))));
            } else {
                interpolation.push(None);
            }
        }

        // Frequency compensation curve (treble/mid boost): bar 0 (lowest bass) has weight 1.0 so
        // bass is not reduced, and the other bars get a progressive gain to counter the natural
        // 1/f energy roll-off.
        let mut frequency_weights = Vec::with_capacity(num_bars);
        for i in 0..num_bars {
            let t = if num_bars > 1 {
                i as f32 / (num_bars - 1) as f32
            } else {
                0.0
            };
            // 1.0 on bar 0 up to ~6.0 on the highest bar
            frequency_weights.push(1.0 + 5.0 * t.powf(0.85));
        }

        Self {
            bin_ranges,
            interpolation,
            frequency_weights,
        }
    }
}

impl AudioProcessor {
    /// Cria uma nova instância de `AudioProcessor`.
    ///
    /// - `channels`: número de canais de áudio recebidos (ex: 1 para mono, 2 para estéreo).
    /// - `sample_rate`: taxa de amostragem real do stream, em Hz.
    /// - `fft_size`: tamanho do bloco da FFT (deve ser potência de 2, ex: 1024).
    /// - `num_bars`: quantidade de barras para a visualização na TUI (ex: 16 ou 32).
    /// - `gain`: multiplicador de ganho para normalização de volume.
    pub fn new(
        channels: usize,
        sample_rate: u32,
        fft_size: usize,
        num_bars: usize,
        gain: f32,
    ) -> Self {
        assert!(sample_rate > 0, "sample_rate must be greater than zero");
        assert!(
            fft_size > 0 && (fft_size & (fft_size - 1)) == 0,
            "fft_size deve ser potência de 2"
        );
        assert!(
            num_bars > 0 && num_bars <= fft_size / 2,
            "num_bars inválido para o fft_size"
        );

        let mut planner = RealFftPlanner::<f32>::new();
        let fft_plan = planner.plan_fft_forward(fft_size);

        let input_buffer = fft_plan.make_input_vec();
        let output_buffer = fft_plan.make_output_vec();

        // Janela de Hann pré-calculada para suavizar as bordas das amostras e reduzir vazamento espectral
        let window: Vec<f32> = (0..fft_size)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (fft_size - 1) as f32).cos()))
            .collect();

        let layout = BandLayout::new(sample_rate, fft_size, num_bars);

        Self {
            channels: channels.max(1),
            fft_size,
            sample_rate,
            num_bars,
            gain,
            sample_accumulator: Vec::with_capacity(fft_size * 2),
            fft_plan,
            input_buffer,
            output_buffer,
            window,
            bin_ranges: layout.bin_ranges,
            interpolation: layout.interpolation,
            frequency_weights: layout.frequency_weights,
        }
    }

    /// Changes the number of bars produced from now on, keeping the buffered samples.
    pub fn set_num_bars(&mut self, num_bars: usize) {
        assert!(
            num_bars > 0 && num_bars <= self.fft_size / 2,
            "num_bars out of range for the fft_size"
        );
        if num_bars == self.num_bars {
            return;
        }

        let layout = BandLayout::new(self.sample_rate, self.fft_size, num_bars);
        self.num_bars = num_bars;
        self.bin_ranges = layout.bin_ranges;
        self.interpolation = layout.interpolation;
        self.frequency_weights = layout.frequency_weights;
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
                self.sample_accumulator.push(self.downmix_frame(frame));
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

            if self
                .fft_plan
                .process(&mut self.input_buffer, &mut self.output_buffer)
                .is_ok()
            {
                // 4. Cálculo de Magnitude (Módulo)
                let magnitudes: Vec<f32> = self.output_buffer.iter().map(|c| c.norm()).collect();

                let mut bars = Vec::with_capacity(self.num_bars);

                // 5. Agrupamento de Frequências (Binning com espaçamento acústico)
                for i in 0..self.num_bars {
                    let magnitude = if let Some((lower, fraction)) = self.interpolation[i] {
                        magnitudes[lower] * (1.0 - fraction) + magnitudes[lower + 1] * fraction
                    } else {
                        let (start, end) = self.bin_ranges[i];
                        let slice = if start < magnitudes.len() {
                            let actual_end = end.min(magnitudes.len());
                            &magnitudes[start..actual_end]
                        } else {
                            &[]
                        };

                        if !slice.is_empty() {
                            let avg = slice.iter().sum::<f32>() / slice.len() as f32;
                            let max = slice.iter().cloned().fold(0.0, f32::max);
                            // Combina a média com o pico da faixa para destacar transientes e pratos
                            0.5 * avg + 0.5 * max
                        } else {
                            0.0
                        }
                    };

                    // 6. Normalização com compensação de resposta para médios e agudos
                    let normalized_amp = (magnitude / (self.fft_size as f32 / 2.0))
                        * self.gain
                        * self.frequency_weights[i];

                    let clamped = normalized_amp.clamp(0.0, 1.0);
                    bars.push(clamped);
                }

                results.push(bars);
            }
        }

        results
    }

    /// Downsamples an interleaved frame to mono.
    ///
    /// For multichannel layouts (5.1/7.1), only the front left/right channels are used.
    /// Averaging every channel would mix in center, LFE and surround content and could
    /// make the visualizer respond mostly to the subwoofer channel.
    fn downmix_frame(&self, frame: &[f32]) -> f32 {
        match frame {
            [] => 0.0,
            [mono] => *mono,
            [left, right, ..] => (left + right) * 0.5,
        }
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
        let sample_rate = 44_100;
        let mut processor = AudioProcessor::new(2, sample_rate, fft_size, num_bars, gain);

        // Gera 2048 amostras estéreo de uma senoide pura de 440 Hz (médio-grave / nota Lá)
        let sample_rate = sample_rate as f32;
        let freq = 440.0;
        let mut samples = Vec::new();

        for i in 0..2048 {
            let t = i as f32 / sample_rate;
            let val = (2.0 * PI * freq * t).sin();
            samples.push(val); // canal esquerdo
            samples.push(val); // canal direito
        }

        let bars_list = processor.process_interleaved_samples(&samples);
        assert_eq!(bars_list.len(), 2);
        for bars in bars_list {
            assert_eq!(bars.len(), num_bars);
            let total_energy: f32 = bars.iter().sum();
            assert!(total_energy > 0.0);
            for &val in &bars {
                assert!((0.0..=1.0).contains(&val));
            }
        }
    }

    #[test]
    fn test_treble_compensation() {
        let fft_size = 1024;
        let num_bars = 32;
        let processor = AudioProcessor::new(2, 48_000, fft_size, num_bars, 1.0);

        // A barra 0 (grave) deve ter peso exatamente 1.0
        assert_eq!(processor.frequency_weights[0], 1.0);

        // Barras superiores devem ter pesos progressivamente maiores
        for i in 1..num_bars {
            assert!(
                processor.frequency_weights[i] >= processor.frequency_weights[i - 1],
                "O peso da barra {} deveria ser maior ou igual à barra {}",
                i,
                i - 1
            );
        }

        // A última barra (mais aguda) deve ter um boost substancial (ex: >= 5.0)
        assert!(processor.frequency_weights[num_bars - 1] >= 5.0);
    }

    #[test]
    fn test_bin_ranges_use_stream_sample_rate() {
        let at_44k = AudioProcessor::new(2, 44_100, 1024, 32, 1.0);
        let at_48k = AudioProcessor::new(2, 48_000, 1024, 32, 1.0);

        assert_ne!(at_44k.bin_ranges, at_48k.bin_ranges);
    }

    #[test]
    fn test_narrow_bars_are_interpolated_instead_of_repeating_a_bin() {
        let processor = AudioProcessor::new(2, 48_000, 1024, 256, 1.0);

        // With 256 bars the lowest ones are far narrower than one ~47 Hz FFT bin.
        assert!(processor.interpolation[0].is_some());
        assert!(processor.interpolation[255].is_none());

        // Interpolated bars read distinct positions between bins instead of one shared bin.
        let positions: Vec<f32> = processor
            .interpolation
            .iter()
            .flatten()
            .map(|&(lower, fraction)| lower as f32 + fraction)
            .collect();
        let distinct = positions
            .windows(2)
            .filter(|pair| pair[1] > pair[0])
            .count();
        assert!(
            distinct * 2 > positions.len(),
            "{distinct} of {}",
            positions.len()
        );
        assert!(positions.windows(2).all(|pair| pair[1] >= pair[0]));
    }

    #[test]
    fn test_interpolated_bar_blends_neighbouring_bins() {
        let mut processor = AudioProcessor::new(2, 48_000, 1024, 32, 1.0);
        // Bar 5 is narrower than a bin at 32 bars; put it midway between bins 3 and 4.
        processor.interpolation[5] = Some((3, 0.5));

        // A 3.5-bin sine (~164 Hz) leaks equally into bins 3 and 4.
        let freq = 3.5 * 48_000.0 / 1024.0;
        let samples: Vec<f32> = (0..1024)
            .flat_map(|i| {
                let value = (2.0 * PI * freq * i as f32 / 48_000.0).sin();
                [value, value]
            })
            .collect();
        let bars = processor.process_interleaved_samples(&samples).remove(0);

        assert!(bars[5] > 0.0);
    }

    #[test]
    fn test_set_num_bars_changes_output_length() {
        let mut processor = AudioProcessor::new(2, 48_000, 1024, 32, 1.0);
        let samples = vec![0.5; 2048];
        assert_eq!(processor.process_interleaved_samples(&samples)[0].len(), 32);

        processor.set_num_bars(96);

        let bars = processor.process_interleaved_samples(&samples);
        assert_eq!(bars[0].len(), 96);
        assert_eq!(processor.frequency_weights.len(), 96);
        assert_eq!(processor.frequency_weights[0], 1.0);
        assert!(processor.frequency_weights[95] >= 5.0);
    }

    #[test]
    fn test_multichannel_downmix_uses_front_channels() {
        let processor = AudioProcessor::new(6, 48_000, 1024, 32, 1.0);
        let frame = [1.0, 0.0, 0.75, 0.75, 0.75, 0.75];

        // Only front left/right participate: (1.0 + 0.0) / 2.
        assert_eq!(processor.downmix_frame(&frame), 0.5);
        assert_eq!(processor.downmix_frame(&[0.25]), 0.25);
        assert_eq!(processor.downmix_frame(&[]), 0.0);
    }
}
