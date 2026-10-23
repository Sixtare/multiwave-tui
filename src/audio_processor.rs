use realfft::{RealFftPlanner, RealToComplex};
use std::f32::consts::PI;
use std::sync::Arc;

/// Lowest and highest frequencies covered by the bars, in Hz.
pub const MIN_FREQ: f32 = 40.0;
pub const MAX_FREQ: f32 = 16000.0;

/// Samples per analysis step; the treble FFT size. One bars frame comes out per step.
const FFT_SIZE: usize = 1024;
/// FFT size for the bass bars. Four times longer than [`FFT_SIZE`], so the narrow bass bars
/// span several bins instead of two or three. It still advances one [`FFT_SIZE`] step at a time
/// over the most recent samples, so the update rate does not change.
const BASS_FFT_SIZE: usize = 4096;
/// Bars centered below this frequency, in Hz, are read from the bass FFT.
const BASS_CROSSOVER_HZ: f32 = 150.0;

/// Most bars the processor can produce: one per bin of the treble FFT.
pub const MAX_BAR_COUNT: usize = FFT_SIZE / 2;

/// Height scale: the top of a bar is 0 dB, the level of a full-scale sine and the bottom is `DB_WINDOW` dB below it. Bar height is
/// linear in dB between the two.
pub const DB_WINDOW: f32 = 50.0;
const FLOOR_DB: f32 = -DB_WINDOW;

/// Pink noise falls 3 dB per octave, so each octave is boosted by this much to display it flat.
const PINK_SLOPE_DB_PER_OCT: f32 = 3.01;
/// Extra boost per octave on top of the pink-noise slope: music is typically darker than pink
/// noise, so without it the treble would still sit below the bass.
pub const EXTRA_SLOPE_DB_PER_OCT: f32 = 1.5;
/// Frequency at which the tilt has no effect.
const TILT_PIVOT_HZ: f32 = 1000.0;

/// Height the automatic sensitivity holds the loudest bar at (1.0 is the top). The headroom above
/// it is left for transients and for the manual sensitivity, so loud music doesn't fill the screen.
const PEAK_TARGET_HEIGHT: f32 = 0.75;

/// A frame whose loudest sample is below this (about -80 dBFS) counts as silence.
const SILENCE_PEAK: f32 = 1e-4;
/// Automatic sensitivity: how fast it drops when a bar overshoots and rises when none does.
/// Same rates as cava, tuned for a reference of 66 frames per second.
const SENSITIVITY_DOWN_RATE: f32 = 0.02;
const SENSITIVITY_UP_RATE: f32 = 0.001;
/// Until a bar overshoots for the first time, the sensitivity climbs this much faster (in the
/// same units as the rates above), so a quiet start fills the display within a fraction of a
/// second. It stops at `INITIAL_RAMP_MAX_SENSITIVITY` so a noise floor can't send it through
/// the roof.
const SENSITIVITY_INITIAL_UP_RATE: f32 = 0.1;
const INITIAL_RAMP_MAX_SENSITIVITY: f32 = 10.0;
const REFERENCE_FRAMES_PER_SECOND: f32 = 66.0;
const MIN_SENSITIVITY: f32 = 0.01;
const MAX_SENSITIVITY: f32 = 100.0;

/// Which FFT a bar is read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Band {
    Bass,
    Treble,
}

/// How one bar is read from the spectrum.
#[derive(Clone, Copy, Debug)]
struct Bar {
    band: Band,
    /// Range of FFT bins averaged (together with their peak) into the bar.
    bins: (usize, usize),
    /// For bars narrower than one bin: the lower bin and the fraction towards the next one.
    interpolation: Option<(usize, f32)>,
    /// Linear gain of the frequency tilt at the bar's center frequency.
    tilt: f32,
    /// Linear gain (at most 1.0) that cancels how much the average-plus-peak rule overstates
    /// noise in a wide bar, so noise reads the same whatever the bar's width.
    bias_correction: f32,
}

impl Bar {
    /// Total linear gain applied to the bar's magnitude before the sensitivity.
    fn gain(&self) -> f32 {
        self.tilt * self.bias_correction
    }
}

/// How much `0.5 * average + 0.5 * peak` overstates the mean magnitude of noise over `bins`
/// bins: 1.0 for a single bin, growing with the width because the peak of more bins is higher.
///
/// Noise magnitudes follow a Rayleigh distribution, whose mean is sqrt(pi/2) (for sigma = 1) and
/// whose expected maximum over n bins is the integral of `1 - (1 - exp(-x^2 / 2))^n`.
fn noise_bias(bins: usize) -> f32 {
    if bins <= 1 {
        return 1.0;
    }

    const STEPS: usize = 400;
    const UPPER: f32 = 8.0;
    let dx = UPPER / STEPS as f32;
    let expected_max: f32 = (0..STEPS)
        .map(|step| {
            let x = (step as f32 + 0.5) * dx;
            (1.0 - (1.0 - (-x * x / 2.0).exp()).powi(bins as i32)) * dx
        })
        .sum();
    let mean = (PI / 2.0).sqrt();
    0.5 + 0.5 * expected_max / mean
}

/// Per-bar layout derived from the bar count and the stream's sample rate.
#[derive(Debug)]
struct BandLayout {
    bars: Vec<Bar>,
}

impl BandLayout {
    fn new(sample_rate: u32, num_bars: usize, extra_slope_db_per_oct: f32) -> Self {
        let sample_rate = sample_rate as f32;
        let slope = PINK_SLOPE_DB_PER_OCT + extra_slope_db_per_oct;

        let bars = (0..num_bars)
            .map(|i| {
                let f_start = MIN_FREQ * (MAX_FREQ / MIN_FREQ).powf(i as f32 / num_bars as f32);
                let f_end = MIN_FREQ * (MAX_FREQ / MIN_FREQ).powf((i + 1) as f32 / num_bars as f32);
                let center = (f_start * f_end).sqrt();

                let (band, fft_size) = if center < BASS_CROSSOVER_HZ {
                    (Band::Bass, BASS_FFT_SIZE)
                } else {
                    (Band::Treble, FFT_SIZE)
                };
                let total_bins = fft_size / 2;
                let bins_per_hz = fft_size as f32 / sample_rate;

                let bin_start = ((f_start * bins_per_hz).round() as usize).clamp(1, total_bins - 1);
                let bin_end =
                    ((f_end * bins_per_hz).round() as usize).clamp(bin_start + 1, total_bins);

                // A bar narrower than one FFT bin would repeat its neighbours' value as a
                // plateau. Read it at its center frequency, interpolating between the two
                // surrounding bins.
                let interpolation = ((f_end - f_start) * bins_per_hz < 1.0).then(|| {
                    let position = center * bins_per_hz;
                    let lower = (position.floor() as usize).clamp(1, total_bins - 1);
                    (lower, (position - lower as f32).clamp(0.0, 1.0))
                });

                let tilt_db = slope * (center / TILT_PIVOT_HZ).log2();
                // An interpolated bar reads a single value, which has no peak to overstate.
                let bias = if interpolation.is_some() {
                    1.0
                } else {
                    noise_bias(bin_end - bin_start)
                };
                Bar {
                    band,
                    bins: (bin_start, bin_end),
                    interpolation,
                    tilt: 10f32.powf(tilt_db / 20.0),
                    bias_correction: 1.0 / bias,
                }
            })
            .collect();

        Self { bars }
    }
}

/// One windowed FFT and its magnitudes.
struct Spectrum {
    fft_plan: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<realfft::num_complex::Complex<f32>>,
    window: Vec<f32>,
    /// Magnitudes of the last analyzed block, already scaled (see [`Spectrum::new`]).
    magnitudes: Vec<f32>,
    scale: f32,
}

impl Spectrum {
    /// Prepares an FFT of `size` samples.
    ///
    /// Magnitudes are scaled so that a full-scale sine reads 1.0 (0 dB) in the treble FFT, and
    /// so that noise reads the same level whatever the FFT size. The bass FFT therefore shows
    /// a full-scale tone 6 dB hotter, in exchange for noise-like content staying continuous
    /// across the crossover.
    fn new(size: usize) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft_plan = planner.plan_fft_forward(size);
        let input = fft_plan.make_input_vec();
        let output = fft_plan.make_output_vec();
        let magnitudes = vec![0.0; output.len()];

        // Hann window: smooths the block edges to reduce spectral leakage.
        let window = (0..size)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (size - 1) as f32).cos()))
            .collect();

        Self {
            fft_plan,
            input,
            output,
            window,
            magnitudes,
            scale: 4.0 / ((FFT_SIZE * size) as f32).sqrt(),
        }
    }

    fn analyze(&mut self, samples: &[f32]) {
        for ((input, &sample), &window) in self.input.iter_mut().zip(samples).zip(&self.window) {
            *input = sample * window;
        }

        if self
            .fft_plan
            .process(&mut self.input, &mut self.output)
            .is_ok()
        {
            for (magnitude, bin) in self.magnitudes.iter_mut().zip(&self.output) {
                *magnitude = bin.norm() * self.scale;
            }
        } else {
            self.magnitudes.fill(0.0);
        }
    }
}

/// Turns raw interleaved samples into bar heights.
///
/// Pipeline: mono downmix, then two Hann-windowed FFTs (a long one for the bass, a short one for
/// the rest), a per-bar magnitude, a frequency tilt that flattens pink noise, automatic
/// sensitivity, and finally a dB scale mapped to heights from 0.0 to 1.0.
pub struct AudioProcessor {
    channels: usize,
    sample_rate: u32,
    num_bars: usize,
    extra_slope_db_per_oct: f32,
    /// Mono samples not yet analyzed (always fewer than `FFT_SIZE` between calls).
    sample_accumulator: Vec<f32>,
    /// The most recent `BASS_FFT_SIZE` mono samples, oldest first.
    history: Vec<f32>,
    treble: Spectrum,
    bass: Spectrum,
    layout: BandLayout,
    /// Automatic sensitivity multiplier; it follows the loudness of the music.
    sensitivity: f32,
    auto_sensitivity: bool,
    /// Whether a bar has overshot yet; before that, the sensitivity climbs fast.
    has_overshot: bool,
    /// The user's manual sensitivity, applied on top of the automatic one.
    manual_gain: f32,
    frames_per_second: f32,
}

impl AudioProcessor {
    /// Creates a processor for a stream.
    ///
    /// - `channels`: interleaved channel count (1 for mono, 2 for stereo, ...).
    /// - `sample_rate`: the stream's real sample rate, in Hz.
    /// - `num_bars`: number of bars, between 1 and [`MAX_BAR_COUNT`].
    pub fn new(channels: usize, sample_rate: u32, num_bars: usize) -> Self {
        Self::with_extra_slope(channels, sample_rate, num_bars, EXTRA_SLOPE_DB_PER_OCT)
    }

    fn with_extra_slope(
        channels: usize,
        sample_rate: u32,
        num_bars: usize,
        extra_slope_db_per_oct: f32,
    ) -> Self {
        assert!(sample_rate > 0, "sample_rate must be greater than zero");
        assert!(
            num_bars > 0 && num_bars <= MAX_BAR_COUNT,
            "num_bars out of range"
        );

        Self {
            channels: channels.max(1),
            sample_rate,
            num_bars,
            extra_slope_db_per_oct,
            sample_accumulator: Vec::with_capacity(FFT_SIZE * 2),
            history: vec![0.0; BASS_FFT_SIZE],
            treble: Spectrum::new(FFT_SIZE),
            bass: Spectrum::new(BASS_FFT_SIZE),
            layout: BandLayout::new(sample_rate, num_bars, extra_slope_db_per_oct),
            sensitivity: 1.0,
            auto_sensitivity: true,
            has_overshot: false,
            manual_gain: 1.0,
            frames_per_second: sample_rate as f32 / FFT_SIZE as f32,
        }
    }

    /// Changes the number of bars produced from now on, keeping the buffered samples and the
    /// automatic sensitivity.
    pub fn set_num_bars(&mut self, num_bars: usize) {
        assert!(
            num_bars > 0 && num_bars <= MAX_BAR_COUNT,
            "num_bars out of range"
        );
        if num_bars == self.num_bars {
            return;
        }

        self.num_bars = num_bars;
        self.layout = BandLayout::new(self.sample_rate, num_bars, self.extra_slope_db_per_oct);
    }

    /// Sets the user's manual sensitivity multiplier (1.0 leaves the levels unchanged).
    pub fn set_manual_gain(&mut self, gain: f32) {
        self.manual_gain = gain.max(f32::EPSILON);
    }

    /// The current automatic sensitivity multiplier.
    pub fn sensitivity(&self) -> f32 {
        self.sensitivity
    }

    /// Processes a block of raw interleaved samples from CPAL.
    ///
    /// Returns one frame of `num_bars` heights (0.0 to 1.0) for every `FFT_SIZE` mono samples
    /// completed by this call.
    pub fn process_interleaved_samples(&mut self, interleaved: &[f32]) -> Vec<Vec<f32>> {
        // 1. Channel downmix to mono
        if self.channels == 1 {
            self.sample_accumulator.extend_from_slice(interleaved);
        } else {
            for frame in interleaved.chunks(self.channels) {
                self.sample_accumulator.push(self.downmix_frame(frame));
            }
        }

        // 2. Analyze one step at a time while enough samples are buffered
        let mut results = Vec::new();
        while self.sample_accumulator.len() >= FFT_SIZE {
            let step = &self.sample_accumulator[..FFT_SIZE];
            let peak = step
                .iter()
                .fold(0.0, |peak: f32, sample| peak.max(sample.abs()));

            self.history.copy_within(FFT_SIZE.., 0);
            self.history[BASS_FFT_SIZE - FFT_SIZE..].copy_from_slice(step);
            self.sample_accumulator.drain(..FFT_SIZE);

            self.treble
                .analyze(&self.history[BASS_FFT_SIZE - FFT_SIZE..]);
            self.bass.analyze(&self.history);

            results.push(self.compute_bars(peak));
        }

        results
    }

    /// Reads every bar out of the analyzed spectra and adapts the automatic sensitivity.
    fn compute_bars(&mut self, peak: f32) -> Vec<f32> {
        let manual_offset = 20.0 * self.manual_gain.log10() / DB_WINDOW;
        let mut overshoot = false;
        let mut bars = Vec::with_capacity(self.num_bars);

        for bar in &self.layout.bars {
            let magnitudes = match bar.band {
                Band::Bass => &self.bass.magnitudes,
                Band::Treble => &self.treble.magnitudes,
            };

            let magnitude = if let Some((lower, fraction)) = bar.interpolation {
                magnitudes[lower] * (1.0 - fraction) + magnitudes[lower + 1] * fraction
            } else {
                let slice = &magnitudes[bar.bins.0..bar.bins.1.min(magnitudes.len())];
                let average = slice.iter().sum::<f32>() / slice.len() as f32;
                let max = slice.iter().cloned().fold(0.0, f32::max);
                // Mixes the average with the band's peak to bring out transients and cymbals
                0.5 * average + 0.5 * max
            };

            let level = magnitude * bar.gain() * self.sensitivity;
            let height = level_to_height(level);
            overshoot |= height > PEAK_TARGET_HEIGHT;
            bars.push((height + manual_offset).clamp(0.0, 1.0));
        }

        self.adapt_sensitivity(overshoot, peak < SILENCE_PEAK);
        bars
    }

    /// Lowers the sensitivity quickly when a bar overshoots the top and raises it slowly when
    /// none does (fast, up to a limit, until the first overshoot). Silence leaves it alone, so
    /// a pause does not crank it up.
    fn adapt_sensitivity(&mut self, overshoot: bool, silent: bool) {
        if !self.auto_sensitivity || silent {
            return;
        }

        let rate = REFERENCE_FRAMES_PER_SECOND / self.frames_per_second;
        if overshoot {
            self.has_overshot = true;
            self.sensitivity *= 1.0 - SENSITIVITY_DOWN_RATE * rate;
        } else if !self.has_overshot && self.sensitivity < INITIAL_RAMP_MAX_SENSITIVITY {
            self.sensitivity = (self.sensitivity * (1.0 + SENSITIVITY_INITIAL_UP_RATE * rate))
                .min(INITIAL_RAMP_MAX_SENSITIVITY);
        } else {
            self.sensitivity *= 1.0 + SENSITIVITY_UP_RATE * rate;
        }
        self.sensitivity = self.sensitivity.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY);
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

/// Maps a linear level (1.0 = full-scale sine) to a bar height; 1.0 is the top, 0.0 the floor.
/// Not clamped, so overshoot can be detected.
fn level_to_height(level: f32) -> f32 {
    (20.0 * level.max(1e-9).log10() - FLOOR_DB) / DB_WINDOW
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 48_000;

    struct Rng(u32);

    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            (self.0 >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        }
    }

    fn scale_to_rms(samples: &mut [f32], rms: f32) {
        let current = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        samples.iter_mut().for_each(|s| *s *= rms / current);
    }

    /// Pink noise (-3 dB/octave) from Paul Kellet's economy filter.
    fn pink_noise(len: usize, rms: f32) -> Vec<f32> {
        let mut rng = Rng(12345);
        let (mut b0, mut b1, mut b2) = (0.0f32, 0.0f32, 0.0f32);
        let mut samples: Vec<f32> = (0..len)
            .map(|_| {
                let white = rng.next();
                b0 = 0.99765 * b0 + white * 0.0990460;
                b1 = 0.96300 * b1 + white * 0.2965164;
                b2 = 0.57000 * b2 + white * 1.0526913;
                b0 + b1 + b2 + white * 0.1848
            })
            .collect();
        scale_to_rms(&mut samples, rms);
        samples
    }

    fn sine(frequency: f32, amplitude: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| amplitude * (2.0 * PI * frequency * i as f32 / SAMPLE_RATE as f32).sin())
            .collect()
    }

    /// Mean height of each bar over all frames, converted to dB relative to the window top.
    fn mean_levels_db(processor: &mut AudioProcessor, samples: &[f32]) -> Vec<f32> {
        let frames = processor.process_interleaved_samples(samples);
        let mut mean = vec![0.0f32; processor.num_bars];
        for frame in &frames {
            for (total, height) in mean.iter_mut().zip(frame) {
                *total += height / frames.len() as f32;
            }
        }
        mean.iter()
            .map(|height| height * DB_WINDOW - DB_WINDOW)
            .collect()
    }

    fn spread(levels: &[f32]) -> f32 {
        let max = levels.iter().cloned().fold(f32::MIN, f32::max);
        let min = levels.iter().cloned().fold(f32::MAX, f32::min);
        max - min
    }

    fn processor_without_agc(num_bars: usize, extra_slope: f32) -> AudioProcessor {
        let mut processor = AudioProcessor::with_extra_slope(1, SAMPLE_RATE, num_bars, extra_slope);
        processor.auto_sensitivity = false;
        processor
    }

    #[test]
    fn test_audio_processor_sin_wave() {
        let num_bars = 16;
        let mut processor = AudioProcessor::new(2, 44_100, num_bars);

        // 2048 stereo samples of a pure 440 Hz sine
        let mut samples = Vec::new();
        for i in 0..2048 {
            let value = (2.0 * PI * 440.0 * i as f32 / 44_100.0).sin();
            samples.push(value);
            samples.push(value);
        }

        let bars_list = processor.process_interleaved_samples(&samples);
        assert_eq!(bars_list.len(), 2);
        for bars in bars_list {
            assert_eq!(bars.len(), num_bars);
            assert!(bars.iter().sum::<f32>() > 0.0);
            assert!(bars.iter().all(|value| (0.0..=1.0).contains(value)));
        }
    }

    #[test]
    fn test_pink_noise_is_flat_without_the_extra_tilt() {
        let mut processor = processor_without_agc(32, 0.0);
        let levels = mean_levels_db(&mut processor, &pink_noise(SAMPLE_RATE as usize * 10, 0.1));

        let spread = spread(&levels);
        assert!(spread < 3.0, "{spread} dB across the bars: {levels:.1?}");
    }

    #[test]
    fn test_pink_noise_stays_flat_whatever_the_bar_count() {
        for num_bars in [32, 64, 128, 256] {
            let mut processor = processor_without_agc(num_bars, 0.0);
            let levels =
                mean_levels_db(&mut processor, &pink_noise(SAMPLE_RATE as usize * 10, 0.1));
            let spread = spread(&levels);
            assert!(spread < 3.5, "{num_bars} bars: {spread} dB");
        }
    }

    #[test]
    fn test_extra_tilt_lifts_the_treble_over_the_bass() {
        let noise = pink_noise(SAMPLE_RATE as usize * 10, 0.1);
        let flat = mean_levels_db(&mut processor_without_agc(32, 0.0), &noise);
        let tilted = mean_levels_db(
            &mut processor_without_agc(32, EXTRA_SLOPE_DB_PER_OCT),
            &noise,
        );

        // Highest bar versus lowest: about 8.4 octaves apart.
        let octaves = (MAX_FREQ / MIN_FREQ).log2() * 31.0 / 32.0;
        let measured = (tilted[31] - tilted[0]) - (flat[31] - flat[0]);
        let expected = EXTRA_SLOPE_DB_PER_OCT * octaves;
        assert!(
            (measured - expected).abs() < 2.0,
            "{measured} vs {expected}"
        );
    }

    #[test]
    fn test_full_scale_sine_reaches_the_top_after_removing_the_tilt() {
        // 256 bars are narrow enough that a tone is not diluted by averaging.
        let mut processor = processor_without_agc(256, 0.0);
        // A frequency centered on a treble-FFT bin, so there is no scalloping loss.
        let frequency = 64.0 * SAMPLE_RATE as f32 / FFT_SIZE as f32;
        let frames = processor.process_interleaved_samples(&sine(frequency, 1.0, 8192));
        let bars = frames.last().unwrap();

        let (index, height) = bars
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        let tilt_db = 20.0 * processor.layout.bars[index].gain().log10();
        // 0 dB is the level of a full-scale sine. A tone reads a little below it: the window
        // spreads it over neighbouring bins and the bar averages them with its peak.
        let level_db = height * DB_WINDOW - DB_WINDOW - tilt_db;
        assert!(
            (-4.5..=0.5).contains(&level_db),
            "{level_db} dB at bar {index}"
        );
    }

    #[test]
    fn test_levels_follow_decibels() {
        let mut quiet = processor_without_agc(32, 0.0);
        let mut loud = processor_without_agc(32, 0.0);
        let frequency = 64.0 * SAMPLE_RATE as f32 / FFT_SIZE as f32;
        let quiet_bars = quiet
            .process_interleaved_samples(&sine(frequency, 0.05, 8192))
            .pop()
            .unwrap();
        let loud_bars = loud
            .process_interleaved_samples(&sine(frequency, 0.5, 8192))
            .pop()
            .unwrap();

        // 20 dB louder input is 20/60 higher on the bar.
        let peak = |bars: &[f32]| bars.iter().cloned().fold(0.0, f32::max);
        let difference = peak(&loud_bars) - peak(&quiet_bars);
        assert!((difference - 20.0 / DB_WINDOW).abs() < 0.03, "{difference}");
    }

    #[test]
    fn test_bass_resolution_separates_close_tones() {
        let mut processor = processor_without_agc(64, 0.0);
        let len = 16_384;
        let samples: Vec<f32> = sine(60.0, 0.3, len)
            .iter()
            .zip(sine(100.0, 0.3, len))
            .map(|(low, high)| low + high)
            .collect();
        let bars = processor
            .process_interleaved_samples(&samples)
            .pop()
            .unwrap();

        let bar_at = |frequency: f32| {
            let position = (frequency / MIN_FREQ).ln() / (MAX_FREQ / MIN_FREQ).ln();
            (position * 64.0) as usize
        };
        let (low, middle, high) = (bar_at(60.0), bar_at(80.0), bar_at(100.0));
        let dip_db = (bars[low].min(bars[high]) - bars[middle]) * DB_WINDOW;
        assert!(dip_db > 6.0, "only {dip_db} dB between the tones");
    }

    #[test]
    fn test_narrow_bars_are_interpolated_instead_of_repeating_a_bin() {
        let processor = AudioProcessor::new(2, SAMPLE_RATE, 256);
        let bars = &processor.layout.bars;

        // Even with the long bass FFT, the lowest of 256 bars is narrower than a bin.
        assert!(bars[0].interpolation.is_some());
        assert!(bars[255].interpolation.is_none());

        let positions: Vec<f32> = bars
            .iter()
            .filter(|bar| bar.band == Band::Bass)
            .filter_map(|bar| bar.interpolation)
            .map(|(lower, fraction)| lower as f32 + fraction)
            .collect();
        assert!(positions.windows(2).all(|pair| pair[1] >= pair[0]));
        assert!(positions.last().unwrap() > positions.first().unwrap());
    }

    #[test]
    fn test_bass_bars_come_from_the_long_fft_and_the_rest_from_the_short_one() {
        let processor = AudioProcessor::new(2, SAMPLE_RATE, 32);
        let bars = &processor.layout.bars;

        assert_eq!(bars[0].band, Band::Bass);
        assert_eq!(bars[31].band, Band::Treble);
        let crossover = bars.iter().filter(|bar| bar.band == Band::Bass).count();
        assert!((6..=8).contains(&crossover), "{crossover} bass bars");
        assert!(
            bars.iter()
                .skip(crossover)
                .all(|bar| bar.band == Band::Treble)
        );
    }

    #[test]
    fn test_tilt_rises_by_the_slope_every_octave() {
        let processor = AudioProcessor::new(2, SAMPLE_RATE, 256);
        let bars = &processor.layout.bars;

        // 256 bars over 8.64 octaves: bars 29.6 apart make one octave.
        let step = (256.0f32 / (MAX_FREQ / MIN_FREQ).log2()).round() as usize;
        let ratio_db = 20.0 * (bars[100 + step].tilt / bars[100].tilt).log10();
        let expected = PINK_SLOPE_DB_PER_OCT + EXTRA_SLOPE_DB_PER_OCT;
        assert!(
            (ratio_db - expected).abs() < 0.3,
            "{ratio_db} vs {expected}"
        );
    }

    #[test]
    fn test_bin_ranges_use_stream_sample_rate() {
        let at_44k = AudioProcessor::new(2, 44_100, 32);
        let at_48k = AudioProcessor::new(2, 48_000, 32);

        let ranges = |processor: &AudioProcessor| {
            processor
                .layout
                .bars
                .iter()
                .map(|bar| bar.bins)
                .collect::<Vec<_>>()
        };
        assert_ne!(ranges(&at_44k), ranges(&at_48k));
    }

    #[test]
    fn test_set_num_bars_changes_output_length() {
        let mut processor = AudioProcessor::new(2, SAMPLE_RATE, 32);
        let samples = vec![0.5; 2048];
        assert_eq!(processor.process_interleaved_samples(&samples)[0].len(), 32);

        processor.set_num_bars(96);

        let bars = processor.process_interleaved_samples(&samples);
        assert_eq!(bars[0].len(), 96);
        assert_eq!(processor.layout.bars.len(), 96);
    }

    #[test]
    fn test_sensitivity_drops_when_a_bar_overshoots_and_recovers_slowly() {
        let mut processor = AudioProcessor::new(1, SAMPLE_RATE, 32);
        // The tilt lifts a 10 kHz tone about 15 dB, so a full-scale one overshoots the top.
        let loud = sine(10_000.0, 1.0, FFT_SIZE * 40);
        processor.process_interleaved_samples(&loud);
        let after_loud = processor.sensitivity();
        assert!(after_loud < 0.7, "{after_loud}");

        // Music gets quieter: the climb back is far slower than the drop was.
        let quiet = sine(10_000.0, 0.001, FFT_SIZE * 40);
        processor.process_interleaved_samples(&quiet);
        let after_quiet = processor.sensitivity();
        assert!(after_quiet > after_loud);
        assert!(
            after_quiet < after_loud * 1.2,
            "{after_quiet} vs {after_loud}"
        );
    }

    #[test]
    fn test_loud_music_settles_with_its_peak_at_the_target_height() {
        let mut processor = AudioProcessor::new(1, SAMPLE_RATE, 32);
        let frames = processor.process_interleaved_samples(&sine(10_000.0, 1.0, FFT_SIZE * 200));

        let last = frames.last().unwrap();
        let top = last.iter().cloned().fold(0.0, f32::max);
        assert!((0.65..0.85).contains(&top), "{top}");
        // Settles where the loudest bar sits at the target height, leaving headroom above it.
        assert!(
            (0.05..0.4).contains(&processor.sensitivity()),
            "{}",
            processor.sensitivity()
        );
    }

    #[test]
    fn test_quiet_start_ramps_up_fast_but_not_beyond_the_limit() {
        let mut processor = AudioProcessor::new(1, SAMPLE_RATE, 32);
        let quiet = pink_noise(FFT_SIZE * 60, 0.003);

        let frames = processor.process_interleaved_samples(&quiet);

        // Reaches the limit within a second (47 frames per second), then only creeps on ...
        let sensitivity = processor.sensitivity();
        assert!(sensitivity >= INITIAL_RAMP_MAX_SENSITIVITY, "{sensitivity}");
        assert!(
            sensitivity < INITIAL_RAMP_MAX_SENSITIVITY * 1.1,
            "{sensitivity}"
        );
        // ... and brightens the display along the way.
        let mean = |frame: &Vec<f32>| frame.iter().sum::<f32>() / frame.len() as f32;
        assert!(mean(frames.last().unwrap()) > mean(&frames[0]));
    }

    #[test]
    fn test_a_very_quiet_track_keeps_climbing_slowly_past_the_ramp_limit() {
        let mut processor = AudioProcessor::new(1, SAMPLE_RATE, 32);
        processor.process_interleaved_samples(&pink_noise(FFT_SIZE * 60, 0.0001));
        let at_limit = processor.sensitivity();

        processor.process_interleaved_samples(&pink_noise(FFT_SIZE * 470, 0.0001));

        // Ten more seconds with no overshoot ever: still rising, but slowly.
        assert!(processor.sensitivity() > at_limit * 1.1);
        assert!(processor.sensitivity() < at_limit * 3.0);
    }

    #[test]
    fn test_after_the_first_overshoot_the_sensitivity_climbs_slowly() {
        let mut processor = AudioProcessor::new(1, SAMPLE_RATE, 32);
        processor.process_interleaved_samples(&sine(10_000.0, 1.0, FFT_SIZE * 10));
        assert!(processor.has_overshot);
        let before = processor.sensitivity();

        processor.process_interleaved_samples(&pink_noise(FFT_SIZE * 47, 0.001));

        // One second of quiet music: well under +1 dB, not the +20 dB of the initial ramp.
        let gained_db = 20.0 * (processor.sensitivity() / before).log10();
        assert!(gained_db > 0.0 && gained_db < 1.0, "{gained_db} dB");
    }

    #[test]
    fn test_silence_leaves_the_sensitivity_alone() {
        let mut processor = AudioProcessor::new(1, SAMPLE_RATE, 32);
        processor.sensitivity = 0.37;

        let frames = processor.process_interleaved_samples(&vec![0.0; FFT_SIZE * 50]);

        assert_eq!(processor.sensitivity(), 0.37);
        assert!(frames.iter().flatten().all(|&height| height == 0.0));
    }

    #[test]
    fn test_set_num_bars_keeps_the_sensitivity() {
        let mut processor = AudioProcessor::new(1, SAMPLE_RATE, 32);
        processor.sensitivity = 0.4;

        processor.set_num_bars(64);

        assert_eq!(processor.sensitivity(), 0.4);
    }

    #[test]
    fn test_manual_gain_shifts_levels_by_its_decibels() {
        let samples = pink_noise(SAMPLE_RATE as usize * 4, 0.2);
        let base = mean_levels_db(&mut processor_without_agc(32, 0.0), &samples);

        let mut boosted = processor_without_agc(32, 0.0);
        boosted.set_manual_gain(2.0);
        let boosted = mean_levels_db(&mut boosted, &samples);

        // Doubling the amplitude is +6.02 dB on every bar that stays inside the window.
        for (base, boosted) in base.iter().zip(&boosted) {
            assert!((boosted - base - 6.02).abs() < 0.3, "{base} -> {boosted}");
        }
    }

    #[test]
    fn test_manual_gain_does_not_fight_the_automatic_sensitivity() {
        let mut processor = AudioProcessor::new(1, SAMPLE_RATE, 32);
        processor.set_manual_gain(0.5);
        let frames = processor.process_interleaved_samples(&sine(3000.0, 0.2, FFT_SIZE * 100));

        // The automatic part is decided before the manual gain, so turning the gain down
        // lowers the bars instead of being undone by the automatic sensitivity.
        let top = frames.last().unwrap().iter().cloned().fold(0.0, f32::max);
        assert!(top < 1.0);
        assert!(
            processor.sensitivity() >= 1.0,
            "{}",
            processor.sensitivity()
        );
    }

    #[test]
    fn test_multichannel_downmix_uses_front_channels() {
        let processor = AudioProcessor::new(6, SAMPLE_RATE, 32);
        let frame = [1.0, 0.0, 0.75, 0.75, 0.75, 0.75];

        // Only front left/right participate: (1.0 + 0.0) / 2.
        assert_eq!(processor.downmix_frame(&frame), 0.5);
        assert_eq!(processor.downmix_frame(&[0.25]), 0.25);
        assert_eq!(processor.downmix_frame(&[]), 0.0);
    }
}
