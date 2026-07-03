//! A timed clip event carrying an opaque, game-defined code.

use axiom_kernel::Tick;

/// An event fired at an exact [`Tick`] on a clip's timeline. The `code` is an
/// **opaque** `u32` the *game* assigns and interprets — a footstep, a strike, a
/// sound cue. The animation mechanism only carries and reports codes at ticks;
/// it never names what a code *means* (that would be gameplay meaning leaking
/// into a mechanism). The app maps a code back to its concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipEvent {
    at: Tick,
    code: u32,
}

impl ClipEvent {
    /// An event with opaque `code` at `at`.
    pub(crate) fn new(at: Tick, code: u32) -> Self {
        ClipEvent { at, code }
    }

    /// The tick this event fires at.
    pub(crate) fn at(self) -> Tick {
        self.at
    }

    /// The opaque game-defined code.
    pub(crate) fn code(self) -> u32 {
        self.code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_keeps_tick_and_code() {
        let e = ClipEvent::new(Tick::new(12), 7);
        assert_eq!(e.at(), Tick::new(12));
        assert_eq!(e.code(), 7);
    }
}
