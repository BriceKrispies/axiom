//! The directional-swipe synthesizer: pointer samples in, one completed gesture
//! direction out. The internal arm of [`crate::InputState`] that turns a frame's
//! neutral pointer `(position, is_down)` samples into a discrete flick.

use axiom_math::Vec2;

/// Minimum swipe length, as a fraction of the surface's shorter edge, before a
/// completed drag counts as a directional swipe (shorter drags are ignored).
const SWIPE_MIN_FRACTION: f32 = 0.10;
/// Floor for divisors derived from positions, so a degenerate (zero-area)
/// surface can never yield a zero threshold.
const TINY: f32 = 1.0e-6;

/// Holds the small per-gesture state a directional swipe needs: where the
/// current gesture began and the swiping pointer's latest position, so the
/// gesture's displacement can be measured on lift. Two instances driven with the
/// same surface and samples reach byte-identical results.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwipeSynth {
    /// Where the current swipe gesture began (surface pixels); `None` between
    /// gestures.
    start: Option<Vec2>,
    /// The latest position of the swiping pointer, so the gesture's displacement
    /// can be measured at lift (when no sample remains).
    last: Option<Vec2>,
}

impl SwipeSynth {
    /// A fresh synthesizer with no gesture in progress.
    pub const fn new() -> Self {
        SwipeSynth {
            start: None,
            last: None,
        }
    }

    /// Fold one frame's pointer samples into the gesture. While a pointer is
    /// down the gesture's start and latest positions are tracked; on lift (no
    /// down sample this call), if the start→end displacement exceeds a fraction
    /// of the surface's shorter edge, its **unit direction** (`+x` right, `+y`
    /// down) is returned — exactly one direction per completed swipe. `None`
    /// mid-gesture, for a too-short flick, or with no gesture in progress.
    pub fn fold(&mut self, surface: Vec2, pointers: &[(Vec2, bool)]) -> Option<Vec2> {
        let down = pointers.iter().filter(|p| p.1).map(|p| p.0).next();
        down.iter().for_each(|pos| {
            self.start = self.start.or(Some(*pos));
            self.last = Some(*pos);
        });

        let lifted = down.is_none();
        let threshold = (surface.x.min(surface.y) * SWIPE_MIN_FRACTION).max(TINY);
        let result = lifted
            .then_some(self.start.zip(self.last))
            .flatten()
            .map(|(start, last)| last.subtract(start))
            .filter(|d| d.length() >= threshold)
            .and_then(|d| d.normalize().ok());

        self.start = [self.start, None][usize::from(lifted)];
        self.last = [self.last, None][usize::from(lifted)];
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1000×600 landscape surface: the swipe threshold is 600 · 0.10 = 60 px.
    fn surface() -> Vec2 {
        Vec2::new(1000.0, 600.0)
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
    }

    #[test]
    fn no_gesture_in_progress_is_none() {
        let mut synth = SwipeSynth::new();
        assert_eq!(synth.fold(surface(), &[]), None);
    }

    #[test]
    fn mid_gesture_is_none_until_lift() {
        let mut synth = SwipeSynth::new();
        assert_eq!(synth.fold(surface(), &[(Vec2::new(700.0, 300.0), true)]), None);
    }

    #[test]
    fn horizontal_drag_returns_a_unit_right_direction() {
        let mut synth = SwipeSynth::new();
        synth.fold(surface(), &[(Vec2::new(700.0, 300.0), true)]);
        synth.fold(surface(), &[(Vec2::new(800.0, 300.0), true)]);
        let dir = synth.fold(surface(), &[]).expect("a completed swipe");
        assert!(approx(dir.x, 1.0));
        assert!(approx(dir.y, 0.0));
    }

    #[test]
    fn a_too_short_flick_is_not_a_swipe() {
        let mut synth = SwipeSynth::new();
        synth.fold(surface(), &[(Vec2::new(300.0, 300.0), true)]);
        synth.fold(surface(), &[(Vec2::new(320.0, 300.0), true)]);
        assert_eq!(synth.fold(surface(), &[]), None);
    }

    #[test]
    fn gesture_state_resets_so_a_second_swipe_is_independent() {
        let mut synth = SwipeSynth::new();
        synth.fold(surface(), &[(Vec2::new(200.0, 300.0), true)]);
        synth.fold(surface(), &[(Vec2::new(320.0, 300.0), true)]);
        let first = synth.fold(surface(), &[]).expect("first swipe");
        assert!(approx(first.x, 1.0));
        synth.fold(surface(), &[(Vec2::new(500.0, 100.0), true)]);
        synth.fold(surface(), &[(Vec2::new(500.0, 250.0), true)]);
        let second = synth.fold(surface(), &[]).expect("second swipe");
        assert!(approx(second.x, 0.0));
        assert!(approx(second.y, 1.0));
    }

    #[test]
    fn degenerate_zero_surface_stays_finite() {
        let mut synth = SwipeSynth::new();
        synth.fold(Vec2::ZERO, &[(Vec2::new(5.0, 0.0), true)]);
        synth.fold(Vec2::ZERO, &[(Vec2::new(50.0, 0.0), true)]);
        let dir = synth.fold(Vec2::ZERO, &[]).expect("a finite swipe");
        assert!(dir.x.is_finite() && dir.y.is_finite());
    }
}
