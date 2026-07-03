//! A named span of a clip's timeline carrying an opaque, game-defined code.

use axiom_kernel::Tick;

/// A half-open tick span `[start, end)` of a clip, tagged with an **opaque**
/// `u32` `code` the game assigns (a wind-up phase, a follow-through, …). Like
/// [`crate::clip_event::ClipEvent`], the mechanism carries and reports codes but
/// never names what they mean. A span with `start >= end` simply never contains
/// any tick (a harmless empty phase), so no validation error is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipPhase {
    start: Tick,
    end: Tick,
    code: u32,
}

impl ClipPhase {
    /// A phase spanning `[start, end)` with opaque `code`.
    pub(crate) fn new(start: Tick, end: Tick, code: u32) -> Self {
        ClipPhase { start, end, code }
    }

    /// The opaque game-defined code.
    pub(crate) fn code(self) -> u32 {
        self.code
    }

    /// Whether `tick` falls in `[start, end)`.
    pub(crate) fn contains(self, tick: Tick) -> bool {
        (tick.raw() >= self.start.raw()) & (tick.raw() < self.end.raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_contains_is_half_open() {
        let p = ClipPhase::new(Tick::new(4), Tick::new(8), 2);
        assert_eq!(p.code(), 2);
        assert!(!p.contains(Tick::new(3)));
        assert!(p.contains(Tick::new(4)));
        assert!(p.contains(Tick::new(7)));
        assert!(!p.contains(Tick::new(8)));
    }

    #[test]
    fn empty_span_contains_nothing() {
        let p = ClipPhase::new(Tick::new(8), Tick::new(4), 0);
        assert!(!p.contains(Tick::new(6)));
    }
}
