//! What the player authored, as data — and nothing else.
//!
//! A [`ShotIntent`] is the *entire* instruction the player gives the kicker: a
//! point inside the goal and two deformations. It knows nothing about pointers,
//! panels, pixels, ticks, or the ball. That separation is the point: gesture code
//! writes one of these, the trajectory layer reads it, and neither can reach the
//! other. Replaying a shot is replaying four numbers and a target.

use crate::shot::path::ShotPath;
use crate::stroke::Pace;
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
    /// The shape of the flight: how far it runs off the straight line ball →
    /// point, across and up, sampled along the shot. Whatever was drawn.
    pub shape: ShotPath,
    /// How hard it was hit — read from the *tempo* of the drawing rather than
    /// from its shape, so the same line drawn quickly and slowly is the same shot
    /// struck with different conviction.
    pub pace: Pace,
}

impl ShotIntent {
    /// A shot authored from a pair of curves rather than from a hand.
    ///
    /// The way everything without fingers describes a shape: the shot matrix
    /// sweeping its parameter space, the agent picking a corner, a test saying
    /// "one that breaks late". The curves are a *generator* here and nothing
    /// more — no drawing is ever put through them.
    pub fn curved(
        target: GoalTarget,
        bend: crate::shot::curve::BendCurve,
        loft: crate::shot::curve::BendCurve,
        pace: Pace,
    ) -> ShotIntent {
        ShotIntent {
            target,
            shape: ShotPath::from_curves(bend, loft),
            pace,
        }
    }

    /// The shot the editor opens on: aimed where the player last touched, dead
    /// straight, with the small natural arc a struck ball has anyway. It is
    /// deliberately a *plausible penalty* rather than a flat line — the player's
    /// first drag should feel like shaping a shot, not like inventing one.
    pub fn opening(target: GoalTarget) -> Self {
        ShotIntent {
            target,
            shape: ShotPath::from_curves(
                crate::shot::curve::BendCurve::STRAIGHT,
                crate::shot::curve::BendCurve::through(0.52, 0.55, 0.14),
            ),
            pace: Pace::STEADY,
        }
    }

    /// How hard the shot has been sculpted, `0..1` per axis. Flight time is read
    /// off these, which is what replaces a power meter.
    pub fn effort(&self, tuning: &Tuning) -> (f32, f32) {
        let (bend, loft) = self.shape.reach();
        (
            (bend.abs() / tuning.bend.max_offset.max(1.0e-3)).min(1.0),
            (loft.abs() / tuning.loft.max_offset.max(1.0e-3)).min(1.0),
        )
    }

    /// Which way the shot bends, and how hard — signed, because it decides which
    /// side of the ball the boot has to come across.
    pub fn across(&self, tuning: &Tuning) -> f32 {
        let (bend, _) = self.shape.reach();
        bend.signum() * self.effort(tuning).0
    }

    /// How fast the ball leaves the boot, metres per second.
    ///
    /// This is the number a shot is actually authored in; flight time is derived
    /// from it rather than the other way round. Tempo sets it — a flick is struck
    /// harder than a careful stroke — and shape takes some back, because bending
    /// and lifting a ball costs pace: the boot's energy goes into the movement
    /// instead of into the flight.
    ///
    /// It is bounded at both ends by construction, so no drawing can produce a
    /// shot slower than a firm penalty or faster than a very good one.
    pub fn launch_speed(&self, tuning: &Tuning) -> f32 {
        let f = &tuning.flight;
        let (bend, loft) = self.effort(tuning);
        let hit = (self.pace.speed.clamp(0.0, 1.0) - f.shape_cost * (bend + loft) * 0.5)
            .clamp(0.0, 1.0);
        f.slow_launch + (f.fast_launch - f.slow_launch) * hit
    }

    /// How sharply this shot bleeds pace through its flight.
    pub fn decay(&self, tuning: &Tuning) -> f32 {
        self.pace.decay(tuning.flight.decel, &tuning.pace)
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
    use crate::shot::curve::BendCurve;

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
        let (bend, lift) = intent.shape.reach();
        assert_eq!(bend, 0.0, "it does not bend");
        assert!(lift > 0.3, "but it has the arc a struck ball has anyway");
        assert_eq!(ShotIntent::default().target, GoalTarget::default());
    }

    #[test]
    fn tempo_sets_how_hard_it_is_hit_and_shape_takes_some_back() {
        let tuning = Tuning::DEFAULT;
        let with = |bend: f32, loft: f32, speed: f32| {
            ShotIntent::curved(
                GoalTarget::new(0.0, 0.2),
                BendCurve::through(0.5, bend, 0.14),
                BendCurve::through(0.5, loft, 0.14),
                crate::stroke::Pace { speed, easing: 0.0 },
            )
        };
        let flat = with(0.0, 0.0, 1.0);
        let curled = with(tuning.bend.max_offset, tuning.loft.max_offset, 1.0);
        assert!(
            flat.launch_speed(&tuning) > curled.launch_speed(&tuning),
            "movement costs pace"
        );
        // Whatever was drawn, the ball leaves inside the range a penalty is
        // actually struck at: never a floated 35 km/h, never an impossible 200.
        [0.0f32, 0.5, 1.0].into_iter().for_each(|speed| {
            [0.0f32, 1.0, tuning.bend.max_offset]
                .into_iter()
                .for_each(|shape| {
                    let v = with(shape, shape, speed).launch_speed(&tuning);
                    assert!(
                        (tuning.flight.slow_launch..=tuning.flight.fast_launch).contains(&v),
                        "speed {speed} shape {shape} left at {v} m/s"
                    );
                });
        });
        assert_eq!(flat.launch_speed(&tuning), tuning.flight.fast_launch);
        assert_eq!(
            with(0.0, 0.0, 0.0).launch_speed(&tuning),
            tuning.flight.slow_launch
        );
        // Full effort, to within where the samples fall: a sampled shape finds
        // its extreme at a sample rather than exactly at the curve's peak.
        let (bend, loft) = curled.effort(&tuning);
        assert!((bend - 1.0).abs() < 0.02, "bend effort {bend}");
        assert!((loft - 1.0).abs() < 0.02, "loft effort {loft}");
        assert_eq!(flat.effort(&tuning), (0.0, 0.0));
    }
}
