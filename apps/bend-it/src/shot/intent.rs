//! What the player authored, as data — and nothing else.
//!
//! A [`ShotIntent`] is the *entire* instruction the player gives the kicker: a
//! point inside the goal and two deformations. It knows nothing about pointers,
//! panels, pixels, ticks, or the ball. That separation is the point: gesture code
//! writes one of these, the trajectory layer reads it, and neither can reach the
//! other. Replaying a shot is replaying four numbers and a target.

use crate::shot::curve::BendCurve;
use crate::tuning::Tuning;

/// The chosen finish, in normalized goal coordinates: `h ∈ [-1, +1]` across the
/// mouth, `v ∈ [0, +1]` up it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalTarget {
    pub h: f32,
    pub v: f32,
}

impl GoalTarget {
    pub fn new(h: f32, v: f32) -> Self {
        GoalTarget {
            h: h.clamp(-1.0, 1.0),
            v: v.clamp(0.0, 1.0),
        }
    }
}

impl Default for GoalTarget {
    /// The default aim is the middle of the goal, a little above the turf —
    /// somewhere no player would actually pick, so the first thing they do is
    /// move it.
    fn default() -> Self {
        GoalTarget::new(0.0, 0.42)
    }
}

/// The authored shot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotIntent {
    pub target: GoalTarget,
    /// The top-down projection: lateral offset, in metres, over shot progress.
    pub bend: BendCurve,
    /// The side projection: height offset, in metres, over shot progress.
    pub loft: BendCurve,
}

impl ShotIntent {
    /// The shot the editor opens on: aimed where the player last touched, dead
    /// straight, with the small natural arc a struck ball has anyway. It is
    /// deliberately a *plausible penalty* rather than a flat line — the player's
    /// first drag should feel like shaping a shot, not like inventing one.
    pub fn opening(target: GoalTarget) -> Self {
        ShotIntent {
            target,
            bend: BendCurve::STRAIGHT,
            loft: BendCurve::through(0.52, 0.55, 0.14),
        }
    }

    /// How hard the shot has been sculpted, `0..1` per axis. Flight time is read
    /// off these, which is what replaces a power meter.
    pub fn effort(&self, tuning: &Tuning) -> (f32, f32) {
        let bend = (self.bend.magnitude().abs() / tuning.bend.max_offset.max(1.0e-3)).min(1.0);
        let loft = (self.loft.magnitude().abs() / tuning.loft.max_offset.max(1.0e-3)).min(1.0);
        (bend, loft)
    }

    /// Flight time, seconds, inferred from the shape alone.
    ///
    /// Flatter and straighter is faster; a big curling loft trades pace for
    /// movement. The player never sees this number and never sets it — they set
    /// the shape, and the shape has consequences.
    pub fn duration(&self, tuning: &Tuning) -> f32 {
        let (bend, loft) = self.effort(tuning);
        let f = &tuning.flight;
        (f.base_duration + bend * f.bend_duration_gain + loft * f.loft_duration_gain)
            .clamp(f.min_duration, f.max_duration)
    }
}

impl Default for ShotIntent {
    fn default() -> Self {
        ShotIntent::opening(GoalTarget::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_is_always_inside_the_normalized_mouth() {
        assert_eq!(GoalTarget::new(-9.0, 9.0), GoalTarget { h: -1.0, v: 1.0 });
        assert_eq!(GoalTarget::new(9.0, -9.0), GoalTarget { h: 1.0, v: 0.0 });
        let default = GoalTarget::default();
        assert_eq!(default.h, 0.0);
        assert!(default.v > 0.0 && default.v < 1.0);
    }

    #[test]
    fn the_opening_shot_is_straight_with_a_natural_arc() {
        let intent = ShotIntent::opening(GoalTarget::new(0.5, 0.5));
        assert_eq!(intent.bend, BendCurve::STRAIGHT);
        assert!(intent.loft.magnitude() > 0.3);
        assert_eq!(ShotIntent::default().target, GoalTarget::default());
    }

    #[test]
    fn shape_sets_the_flight_time_and_nothing_else_does() {
        let tuning = Tuning::DEFAULT;
        let flat = ShotIntent {
            target: GoalTarget::new(0.0, 0.2),
            bend: BendCurve::STRAIGHT,
            loft: BendCurve::STRAIGHT,
        };
        let curled = ShotIntent {
            target: GoalTarget::new(0.0, 0.2),
            bend: BendCurve::through(0.5, tuning.bend.max_offset, 0.14),
            loft: BendCurve::through(0.5, tuning.loft.max_offset, 0.14),
        };
        assert!(flat.duration(&tuning) < curled.duration(&tuning));
        assert!(flat.duration(&tuning) >= tuning.flight.min_duration);
        assert!(curled.duration(&tuning) <= tuning.flight.max_duration);
        let (bend, loft) = curled.effort(&tuning);
        assert!((bend - 1.0).abs() < 1.0e-3);
        assert!((loft - 1.0).abs() < 1.0e-3);
        assert_eq!(flat.effort(&tuning), (0.0, 0.0));
    }
}
