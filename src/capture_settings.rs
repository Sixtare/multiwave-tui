//! What the UI can change while a capture stream keeps running.
//!
//! The UI thread sets values and the capture callback reads them on every block, so changing a
//! setting never rebuilds the stream.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::audio_processor::{AudioProcessor, MAX_BAR_COUNT};

#[derive(Debug)]
pub struct CaptureSettings {
    /// Number of bars the callback produces.
    bar_count: AtomicUsize,
    /// The user's manual sensitivity, as `f32` bits.
    manual_gain_bits: AtomicU32,
    /// Whether the treble bars are smoothed over time.
    smoothing: AtomicBool,
}

impl CaptureSettings {
    /// Settings for `num_bars` bars, neutral sensitivity and no smoothing.
    pub fn new(num_bars: usize) -> Self {
        Self {
            bar_count: AtomicUsize::new(num_bars.clamp(1, MAX_BAR_COUNT)),
            manual_gain_bits: AtomicU32::new(1.0f32.to_bits()),
            smoothing: AtomicBool::new(false),
        }
    }

    /// Changes how many bars the running capture produces, effective from the next audio chunk.
    pub fn set_num_bars(&self, num_bars: usize) {
        self.bar_count
            .store(num_bars.clamp(1, MAX_BAR_COUNT), Ordering::Relaxed);
    }

    /// Changes the manual sensitivity of the running capture, effective from the next block.
    pub fn set_gain(&self, gain: f32) {
        self.manual_gain_bits
            .store(gain.to_bits(), Ordering::Relaxed);
    }

    /// Turns the treble smoothing of the running capture on or off, effective from the next block.
    pub fn set_smoothing(&self, on: bool) {
        self.smoothing.store(on, Ordering::Relaxed);
    }

    /// A processor for a stream with `channels` interleaved channels at `sample_rate` Hz,
    /// configured with the current settings.
    pub fn new_processor(&self, channels: usize, sample_rate: u32) -> AudioProcessor {
        let mut processor = AudioProcessor::new(
            channels,
            sample_rate,
            self.bar_count.load(Ordering::Relaxed),
        );
        self.apply_to(&mut processor);
        processor
    }

    /// Brings `processor` up to date with the current settings; the capture callback calls it
    /// before every block.
    pub fn apply_to(&self, processor: &mut AudioProcessor) {
        processor.set_num_bars(self.bar_count.load(Ordering::Relaxed));
        processor.set_manual_gain(f32::from_bits(
            self.manual_gain_bits.load(Ordering::Relaxed),
        ));
        processor.set_smoothing(self.smoothing.load(Ordering::Relaxed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BARS: usize = 64;

    #[test]
    fn test_new_processor_starts_with_the_current_settings() {
        let settings = CaptureSettings::new(BARS);
        settings.set_num_bars(40);

        let mut processor = settings.new_processor(2, 48_000);
        let frames = processor.process_interleaved_samples(&vec![0.1; 2048]);

        assert_eq!(frames[0].len(), 40);
    }

    #[test]
    fn test_apply_to_follows_later_changes() {
        let settings = CaptureSettings::new(BARS);
        let mut processor = settings.new_processor(2, 48_000);

        settings.set_num_bars(48);
        settings.apply_to(&mut processor);
        let frames = processor.process_interleaved_samples(&vec![0.1; 2048]);

        assert_eq!(frames[0].len(), 48);
    }

    #[test]
    fn test_bar_count_is_clamped() {
        let settings = CaptureSettings::new(0);
        assert_eq!(
            settings
                .new_processor(2, 48_000)
                .process_interleaved_samples(&vec![0.1; 2048])[0]
                .len(),
            1
        );

        settings.set_num_bars(10_000);
        let mut processor = settings.new_processor(2, 48_000);
        assert_eq!(
            processor.process_interleaved_samples(&vec![0.1; 2048])[0].len(),
            MAX_BAR_COUNT
        );
    }
}
