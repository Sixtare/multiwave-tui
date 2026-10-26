//! Short on-screen announcements ("40 bars", "smoothing on") that expire after a while.

use crate::app::DEFAULT_FPS;

/// How long a notice stays on screen, in ticks (~1.5 s at 60 FPS; other frame rates scale it).
pub const NOTICE_TICKS: u64 = 90;

/// What a notice announces. Each kind expires independently, so changing two settings at once
/// shows both.
#[derive(Clone, Copy, Debug)]
pub enum NoticeKind {
    Bars,
    Gain,
    Smoothing,
    BarGap,
    Mirror,
    Fps,
    Color,
}

const KINDS: usize = 8;

/// When each kind of notice stops being shown.
#[derive(Debug, Default)]
pub struct Notices {
    until: [u64; KINDS],
}

impl Notices {
    /// Starts (or restarts) the notice of `kind` at tick `now`; it lasts the same time at every
    /// frame rate.
    pub fn show(&mut self, kind: NoticeKind, now: u64, fps: u32) {
        self.until[kind as usize] = now + NOTICE_TICKS * fps as u64 / DEFAULT_FPS as u64;
    }

    /// Whether the notice of `kind` is still on screen at tick `now`.
    pub fn is_active(&self, kind: NoticeKind, now: u64) -> bool {
        now < self.until[kind as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notice_expires_after_its_duration() {
        let mut notices = Notices::default();
        assert!(!notices.is_active(NoticeKind::Bars, 0));

        notices.show(NoticeKind::Bars, 10, DEFAULT_FPS);

        assert!(notices.is_active(NoticeKind::Bars, 10 + NOTICE_TICKS - 1));
        assert!(!notices.is_active(NoticeKind::Bars, 10 + NOTICE_TICKS));
    }

    #[test]
    fn test_kinds_are_independent() {
        let mut notices = Notices::default();
        notices.show(NoticeKind::Gain, 0, DEFAULT_FPS);

        assert!(notices.is_active(NoticeKind::Gain, 1));
        assert!(!notices.is_active(NoticeKind::Fps, 1));
    }

    #[test]
    fn test_duration_scales_with_the_frame_rate() {
        let mut notices = Notices::default();
        notices.show(NoticeKind::Color, 0, 120);

        assert!(notices.is_active(NoticeKind::Color, 2 * NOTICE_TICKS - 1));
        assert!(!notices.is_active(NoticeKind::Color, 2 * NOTICE_TICKS));
    }
}
