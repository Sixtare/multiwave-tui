//! The animated bar heights: bars that chase the targets the audio side reports.

/// Target decay per tick at the reference rate, so a bar falls on its own when audio stops.
const TARGET_DECAY: f32 = 0.94;
/// Bars below this height snap to zero, so silence settles and stops redrawing.
const SNAP_TO_ZERO: f32 = 0.005;

#[derive(Debug, Clone, PartialEq)]
pub struct BarMotion {
    /// Displayed heights, 0.0..=1.0.
    bars: Vec<f32>,
    /// Heights the displayed bars move towards.
    targets: Vec<f32>,
}

impl BarMotion {
    /// `len` bars, all at rest.
    pub fn new(len: usize) -> Self {
        Self {
            bars: vec![0.0; len],
            targets: vec![0.0; len],
        }
    }

    /// Bars and targets set directly, for tests that start from a given state.
    #[cfg(test)]
    pub fn from_parts(bars: Vec<f32>, targets: Vec<f32>) -> Self {
        assert_eq!(bars.len(), targets.len());
        Self { bars, targets }
    }

    /// The displayed heights.
    pub fn bars(&self) -> &[f32] {
        &self.bars
    }

    /// The heights the bars are moving towards.
    pub fn targets(&self) -> &[f32] {
        &self.targets
    }

    /// Takes new targets. A frame of another length (computed before a bar count change and
    /// still in flight) is dropped; returns whether it was taken.
    pub fn set_targets(&mut self, targets: Vec<f32>) -> bool {
        if targets.len() != self.bars.len() {
            return false;
        }
        self.targets = targets;
        true
    }

    /// Stretches the bars and targets to `len`, so the display does not blank out while the
    /// audio side catches up with a new bar count.
    pub fn resize(&mut self, len: usize) {
        self.bars = resample(&self.bars, len);
        self.targets = resample(&self.targets, len);
    }

    /// Lets the targets fall as `steps` reference ticks would (1.0 for one).
    pub fn decay_targets(&mut self, steps: f32) {
        let decay = TARGET_DECAY.powf(steps);
        for target in &mut self.targets {
            *target = (*target * decay).max(0.0);
        }
    }

    /// Moves every bar towards its target as `steps` reference ticks would (1.0 for one): by
    /// the `attack` fraction of the gap going up and the `decay` fraction going down, both
    /// given per reference tick.
    pub fn step(&mut self, attack: f32, decay: f32, steps: f32) {
        let (attack, decay) = if steps == 1.0 {
            (attack, decay)
        } else {
            (
                1.0 - (1.0 - attack).powf(steps),
                1.0 - (1.0 - decay).powf(steps),
            )
        };
        for (current, &target) in self.bars.iter_mut().zip(&self.targets) {
            let factor = if target > *current { attack } else { decay };
            *current += (target - *current) * factor;

            if *current < SNAP_TO_ZERO {
                *current = 0.0;
            } else if *current > 1.0 {
                *current = 1.0;
            }
        }
    }
}

fn resample(bars: &[f32], len: usize) -> Vec<f32> {
    if bars.is_empty() {
        return vec![0.0; len];
    }
    (0..len).map(|i| bars[i * bars.len() / len]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bars_rise_faster_than_they_fall() {
        let mut motion = BarMotion::new(2);
        assert!(motion.set_targets(vec![1.0, 0.0]));
        motion.step(0.5, 0.1, 1.0);
        assert_eq!(motion.bars(), [0.5, 0.0]);

        assert!(motion.set_targets(vec![0.0, 0.0]));
        motion.step(0.5, 0.1, 1.0);
        assert!((motion.bars()[0] - 0.45).abs() < 1e-6);
    }

    #[test]
    fn test_two_half_steps_match_one_whole_step() {
        let mut whole = BarMotion::new(1);
        whole.set_targets(vec![1.0]);
        whole.step(0.4, 0.1, 1.0);

        let mut halves = BarMotion::new(1);
        halves.set_targets(vec![1.0]);
        halves.step(0.4, 0.1, 0.5);
        halves.step(0.4, 0.1, 0.5);

        assert!((whole.bars()[0] - halves.bars()[0]).abs() < 1e-6);
    }

    #[test]
    fn test_frames_of_the_wrong_length_are_dropped() {
        let mut motion = BarMotion::new(4);
        assert!(!motion.set_targets(vec![1.0; 3]));
        assert_eq!(motion.targets(), [0.0; 4]);
    }

    #[test]
    fn test_resize_stretches_the_current_heights() {
        let mut motion = BarMotion::new(2);
        motion.set_targets(vec![1.0, 0.5]);
        motion.step(1.0, 1.0, 1.0);

        motion.resize(4);

        assert_eq!(motion.bars(), [1.0, 1.0, 0.5, 0.5]);
        assert_eq!(motion.targets(), [1.0, 1.0, 0.5, 0.5]);
    }

    #[test]
    fn test_small_heights_snap_to_zero() {
        let mut motion = BarMotion::new(1);
        motion.set_targets(vec![0.004]);
        motion.step(1.0, 1.0, 1.0);
        assert_eq!(motion.bars(), [0.0]);
    }
}
