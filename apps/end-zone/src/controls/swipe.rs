//! **Swipe recognition — a pure function, in the deterministic core.**
//!
//! Phones are the point of this control scheme: the game is four discrete
//! decisions made at speed, which is exactly what a thumb is good at and exactly
//! what a virtual joystick is bad at. So there is no stick. There are four
//! flicks, and they mean the same four things the keyboard's four keys do.
//!
//! What lives here is the *recogniser*, and it lives on this side of the
//! platform boundary on purpose. `web/touch.rs` does nothing but read pointer
//! events off the DOM and hand over `(x, y)` in screen pixels; every judgement —
//! is this a swipe or a tap, which axis, was it deliberate — is made by this
//! file, which has no browser in it and can be driven from a test with a list of
//! numbers. A gesture layer written inside the wasm edge would be unreachable by
//! the native gate, and a gesture that cannot be tested is a gesture that
//! silently rots.
//!
//! ## The rules
//!
//! * **A gesture is deliberate.** It must travel at least [`MIN_TRAVEL`] pixels.
//!   Below that it is a tap, a tremor, or a thumb resting on the glass, and it
//!   produces nothing.
//! * **A gesture has one axis.** The dominant axis must beat the other by
//!   [`AXIS_RATIO`], so a sloppy diagonal is refused rather than guessed at.
//!   Refusing is the kind choice: a mis-read juke costs the play.
//! * **A gesture is one move.** It fires **once**, at the moment it becomes
//!   unambiguous — mid-drag, not on release, so the move happens when the thumb
//!   commits rather than when it lifts — and the rest of that drag is inert
//!   until the finger comes up. Holding a finger to the left of the screen does
//!   not juke left forever.
//! * **A gesture is screen-relative.** Up is up whichever way the phone is held,
//!   so the same four flicks work in portrait and in landscape with nothing to
//!   configure.
//!
//! Screen `y` grows **downward** (the DOM convention the edge reports in), so a
//! swipe *up* is a negative `dy`. That single sign is the only thing in here
//! that knows what a browser is.

use crate::runback::RunbackMove;

/// How far a gesture must travel to count as deliberate, in pixels.
///
/// Sized in raw pixels rather than as a fraction of the viewport because a thumb
/// is a fixed physical size: the flick that means "juke" is the same flick on a
/// small phone and a large one, and scaling it to the screen would make the
/// large phone require a longer, slower gesture for no reason.
pub const MIN_TRAVEL: f32 = 34.0;

/// How much the dominant axis must beat the other by for the gesture to be
/// unambiguous.
pub const AXIS_RATIO: f32 = 1.35;

/// What a pointer sample is reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipePhase {
    /// A finger went down.
    Down,
    /// A finger moved.
    Move,
    /// A finger came up (or was cancelled).
    Up,
}

/// One sample of a pointer, in screen pixels — the whole of what the platform
/// edge is trusted to know.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwipeSample {
    pub phase: SwipePhase,
    pub x: f32,
    pub y: f32,
}

/// The gesture recogniser: the origin of the drag in progress, and whether it
/// has already fired.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SwipeRecognizer {
    origin: Option<(f32, f32)>,
    fired: bool,
}

impl SwipeRecognizer {
    pub fn new() -> Self {
        SwipeRecognizer::default()
    }

    /// Feed one pointer sample; returns the move it completed, if any.
    ///
    /// Pure and total: the same sequence of samples always produces the same
    /// sequence of moves, which is what lets the mobile control scheme be proven
    /// by a native test rather than by a person with a phone.
    pub fn sample(&mut self, sample: SwipeSample) -> Option<RunbackMove> {
        match sample.phase {
            SwipePhase::Down => {
                self.origin = Some((sample.x, sample.y));
                self.fired = false;
                None
            }
            SwipePhase::Up => {
                self.origin = None;
                self.fired = false;
                None
            }
            SwipePhase::Move => {
                let (ox, oy) = self.origin?;
                if self.fired {
                    return None;
                }
                let wanted = classify(sample.x - ox, sample.y - oy)?;
                self.fired = true;
                Some(wanted)
            }
        }
    }

    /// Abandon any drag in progress (a run swap or a pause must not inherit a
    /// half-finished gesture).
    pub fn clear(&mut self) {
        self.origin = None;
        self.fired = false;
    }
}

/// Classify a completed displacement into a move, or refuse it.
///
/// Split out from the recogniser so the *decision* can be tested on its own,
/// without a gesture state machine around it.
pub fn classify(dx: f32, dy: f32) -> Option<RunbackMove> {
    let (ax, ay) = (dx.abs(), dy.abs());
    let horizontal = ax >= ay;
    let (dominant, other) = match horizontal {
        true => (ax, ay),
        false => (ay, ax),
    };
    let deliberate = dominant >= MIN_TRAVEL;
    let unambiguous = dominant >= other * AXIS_RATIO;
    if !(deliberate && unambiguous) {
        return None;
    }
    Some(match (horizontal, dx >= 0.0, dy >= 0.0) {
        (true, false, _) => RunbackMove::JukeLeft,
        (true, true, _) => RunbackMove::JukeRight,
        // Screen `y` grows downward: a swipe UP is negative.
        (false, _, false) => RunbackMove::Jump,
        (false, _, true) => RunbackMove::Shoulder,
    })
}
