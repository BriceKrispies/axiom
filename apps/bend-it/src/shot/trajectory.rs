//! Two projections, one trajectory.
//!
//! The top-down editor and the side editor are not two animations. They are two
//! views of a single world-space path, and this module is where that path is
//! built — once, deterministically, from a [`ShotIntent`]:
//!
//! ```text
//! forward(u) = lerp(ball, target, u)              the shot's straight spine
//! lateral(u) = bend.offset(u)  along the right axis     (the top-down editor)
//! height(u)  = loft.offset(u)  along up                 (the side editor)
//! ```
//!
//! summed into one `worldPosition(u)`, then **re-sampled by arc length** so the
//! stored points are evenly spaced along the path rather than evenly spaced in
//! the parameter. That resampling is what stops a Bézier from looking wrong: a
//! ball moving at a constant rate of `u` visibly slows through the bulge of a
//! curve, because that is where `u` covers the least distance.
//!
//! Two invariants hold by construction and are tested as such: the first sample
//! is exactly the ball, and the last sample is exactly the authored point in the
//! goal. Nothing downstream is permitted to steer toward either.

use axiom::prelude::Vec3;

use crate::pitch::GoalMouth;
use crate::shot::intent::ShotIntent;
use crate::tuning::Tuning;

/// How many raw evaluations back each stored sample, before arc-length
/// resampling. Denser is only spent once, at authoring time.
const OVERSAMPLE: usize = 6;

/// The ball's state at a moment of flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallState {
    pub position: Vec3,
    pub velocity: Vec3,
}

/// One canonical, arc-length-uniform world-space path.
#[derive(Debug, Clone, PartialEq)]
pub struct Trajectory {
    points: Vec<Vec3>,
    length: f32,
    duration: f32,
    decel: f32,
}

/// The right-hand axis of a shot running from `origin` to `target`: the
/// direction a positive bend pushes the ball.
///
/// With the camera behind the ball looking down `-Z`, this resolves to world
/// `+X`, which projects to the right of the screen — so "drag right, bend right"
/// is one identity rather than a sign convention to keep in step.
pub fn shot_right(origin: Vec3, target: Vec3) -> Vec3 {
    let forward = Vec3::new(target.x - origin.x, 0.0, target.z - origin.z);
    forward
        .normalize()
        .map(|f| Vec3::new(-f.z, 0.0, f.x))
        .unwrap_or(Vec3::UNIT_X)
}

/// Evaluate the un-resampled path at parameter `u`.
fn raw_point(origin: Vec3, target: Vec3, right: Vec3, intent: &ShotIntent, floor: f32) -> impl Fn(f32) -> Vec3 + '_ {
    move |u| {
        let u = u.clamp(0.0, 1.0);
        let base = origin.add(target.subtract(origin).mul_scalar(u));
        let lateral = right.mul_scalar(intent.bend.offset(u));
        let height = intent.loft.offset(u);
        Vec3::new(
            base.x + lateral.x,
            (base.y + height).max(floor),
            base.z + lateral.z,
        )
    }
}

impl Trajectory {
    /// Build the canonical path for an authored shot.
    pub fn build(origin: Vec3, target: Vec3, intent: &ShotIntent, tuning: &Tuning) -> Trajectory {
        let flight = &tuning.flight;
        let samples = flight.samples.max(8);
        let right = shot_right(origin, target);
        // The height curve is floored against the turf *before* the path is
        // built, so the clamp inside `raw_point` is a belt-and-braces guard and
        // never a shape the player can actually reach — a dip stays a smooth dip
        // rather than developing a flat spot along the ground.
        let base_height = |u: f32| origin.y + (target.y - origin.y) * u;
        let safe_loft = intent
            .loft
            .bounded(tuning.loft.min_offset, tuning.loft.max_offset)
            .floored(|u| (base_height(u) - flight.ball_radius).max(0.0));
        let bounded = ShotIntent {
            bend: intent
                .bend
                .bounded(tuning.bend.min_offset, tuning.bend.max_offset),
            loft: safe_loft,
            ..*intent
        };
        let eval = raw_point(origin, target, right, &bounded, flight.ball_radius);

        // Dense walk, accumulating arc length.
        let dense: Vec<Vec3> = (0..=samples * OVERSAMPLE)
            .map(|i| eval(i as f32 / (samples * OVERSAMPLE) as f32))
            .collect();
        let mut travelled = Vec::with_capacity(dense.len());
        let length = dense.iter().fold((0.0f32, dense[0]), |(sum, prev), &p| {
            let next = sum + p.subtract(prev).length();
            travelled.push(next);
            (next, p)
        })
        .0;

        // Resample at even arc length. Endpoints are copied verbatim rather than
        // interpolated, so no amount of floating point can drift the shot off the
        // ball or off the authored point.
        let mut cursor = 0usize;
        let mut points = Vec::with_capacity(samples + 1);
        points.push(dense[0]);
        (1..samples).for_each(|i| {
            let want = length * i as f32 / samples as f32;
            while (cursor + 1 < travelled.len()) && (travelled[cursor + 1] < want) {
                cursor += 1;
            }
            let a = travelled[cursor];
            let b = travelled[(cursor + 1).min(travelled.len() - 1)];
            let t = ((want - a) / (b - a).max(1.0e-6)).clamp(0.0, 1.0);
            let p0 = dense[cursor];
            let p1 = dense[(cursor + 1).min(dense.len() - 1)];
            points.push(p0.add(p1.subtract(p0).mul_scalar(t)));
        });
        points.push(target);

        Trajectory {
            points,
            length,
            duration: intent.duration(tuning),
            decel: flight.decel,
        }
    }

    /// The stored samples, evenly spaced along the path.
    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    /// Total path length, metres.
    pub fn length(&self) -> f32 {
        self.length
    }

    /// Flight time, seconds.
    pub fn duration(&self) -> f32 {
        self.duration
    }

    /// The point at arc-length fraction `s ∈ [0, 1]`.
    pub fn at_progress(&self, s: f32) -> Vec3 {
        let last = self.points.len() - 1;
        let scaled = s.clamp(0.0, 1.0) * last as f32;
        let index = (scaled.floor() as usize).min(last);
        let next = (index + 1).min(last);
        let t = scaled - index as f32;
        self.points[index].add(self.points[next].subtract(self.points[index]).mul_scalar(t))
    }

    /// How much of the path has been covered at time `t` seconds.
    ///
    /// A struck ball leaves hot and bleeds pace; an exponential ease-out with a
    /// gentle constant gives that without ever stalling, and lands exactly on
    /// `1.0` at the end of the flight.
    pub fn progress_at(&self, t: f32) -> f32 {
        let tau = (t / self.duration.max(1.0e-4)).clamp(0.0, 1.0);
        let k = self.decel.max(1.0e-4);
        (1.0 - (-k * tau).exp()) / (1.0 - (-k).exp())
    }

    /// Position and velocity at time `t` seconds into the flight.
    pub fn sample(&self, t: f32) -> BallState {
        let h = 1.0 / 240.0;
        let position = self.at_progress(self.progress_at(t));
        let ahead = self.at_progress(self.progress_at(t + h));
        let behind = self.at_progress(self.progress_at((t - h).max(0.0)));
        let span = (t + h) - (t - h).max(0.0);
        BallState {
            position,
            velocity: ahead.subtract(behind).mul_scalar(1.0 / span.max(1.0e-6)),
        }
    }
}

/// Resolve an authored intent against the pitch: where it starts, where it ends,
/// and the one path between them.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedShot {
    pub intent: ShotIntent,
    pub origin: Vec3,
    pub world_target: Vec3,
    pub trajectory: Trajectory,
}

impl ResolvedShot {
    pub fn build(origin: Vec3, intent: ShotIntent, mouth: &GoalMouth, tuning: &Tuning) -> Self {
        let world_target = mouth.to_world(intent.target.h, intent.target.v);
        let trajectory = Trajectory::build(origin, world_target, &intent, tuning);
        ResolvedShot {
            intent,
            origin,
            world_target,
            trajectory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::ball_spot;
    use crate::shot::curve::BendCurve;
    use crate::shot::intent::GoalTarget;

    fn resolved(bend: f32, loft: f32, target: GoalTarget) -> ResolvedShot {
        let tuning = Tuning::DEFAULT;
        let intent = ShotIntent {
            target,
            bend: BendCurve::through(0.5, bend, 0.14),
            loft: BendCurve::through(0.5, loft, 0.14),
        };
        ResolvedShot::build(
            ball_spot(tuning.flight.ball_radius),
            intent,
            &GoalMouth::new(tuning.goal.inset),
            &tuning,
        )
    }

    #[test]
    fn the_right_axis_points_at_screen_right() {
        // Ball up-pitch, goal at the origin: the shot runs down -Z and its right
        // hand is world +X.
        let right = shot_right(Vec3::new(0.0, 0.1, 11.0), Vec3::ZERO);
        assert!((right.x - 1.0).abs() < 1.0e-5, "{right:?}");
        // A degenerate shot (no ground travel) still yields a usable axis.
        assert_eq!(shot_right(Vec3::ZERO, Vec3::new(0.0, 2.0, 0.0)), Vec3::UNIT_X);
    }

    #[test]
    fn every_trajectory_starts_at_the_ball_and_ends_on_the_authored_point() {
        for (bend, loft, h, v) in [
            (0.0f32, 0.0f32, 0.0f32, 0.5f32),
            (4.6, 3.0, -1.0, 1.0),
            (-4.6, -1.5, 1.0, 0.0),
            (2.0, -1.2, 0.8, 0.05),
            (-30.0, 30.0, 0.3, 0.9),
        ] {
            let shot = resolved(bend, loft, GoalTarget::new(h, v));
            let points = shot.trajectory.points();
            assert_eq!(points[0], shot.origin, "bend {bend} loft {loft}");
            assert_eq!(
                *points.last().expect("a sampled path"),
                shot.world_target,
                "bend {bend} loft {loft}"
            );
            assert_eq!(shot.trajectory.at_progress(0.0), shot.origin);
            assert_eq!(shot.trajectory.at_progress(1.0), shot.world_target);
            assert_eq!(
                shot.trajectory.sample(0.0).position,
                shot.origin,
                "flight starts on the ball"
            );
            assert_eq!(
                shot.trajectory
                    .sample(shot.trajectory.duration())
                    .position,
                shot.world_target,
                "flight ends on the target"
            );
        }
    }

    #[test]
    fn no_trajectory_ever_goes_underground() {
        for loft in [-40.0f32, -4.0, -1.5, 0.0, 2.0, 40.0] {
            let shot = resolved(0.0, loft, GoalTarget::new(0.0, 0.05));
            shot.trajectory.points().iter().for_each(|p| {
                assert!(
                    p.y >= Tuning::DEFAULT.flight.ball_radius - 1.0e-4,
                    "loft {loft} dropped to {}",
                    p.y
                );
            });
        }
    }

    #[test]
    fn bend_moves_the_middle_of_the_path_but_not_its_ends() {
        let straight = resolved(0.0, 0.6, GoalTarget::new(0.0, 0.5));
        let curled = resolved(4.0, 0.6, GoalTarget::new(0.0, 0.5));
        let mid = |s: &ResolvedShot| s.trajectory.at_progress(0.5).x;
        assert!(mid(&curled) - mid(&straight) > 1.5);
        assert_eq!(straight.world_target, curled.world_target);
        // ... and the mirror bends the other way by the same amount.
        let mirrored = resolved(-4.0, 0.6, GoalTarget::new(0.0, 0.5));
        assert!((mid(&curled) + mid(&mirrored)).abs() < 1.0e-2);
    }

    #[test]
    fn loft_changes_the_height_of_the_path() {
        let low = resolved(0.0, 0.0, GoalTarget::new(0.0, 0.5));
        let high = resolved(0.0, 4.0, GoalTarget::new(0.0, 0.5));
        assert!(high.trajectory.at_progress(0.5).y - low.trajectory.at_progress(0.5).y > 2.0);
    }

    #[test]
    fn the_samples_are_evenly_spaced_along_the_path() {
        let shot = resolved(3.5, 2.5, GoalTarget::new(-0.6, 0.8));
        let points = shot.trajectory.points();
        let steps: Vec<f32> = points
            .windows(2)
            .map(|w| w[1].subtract(w[0]).length())
            .collect();
        let mean = steps.iter().sum::<f32>() / steps.len() as f32;
        steps.iter().for_each(|s| {
            assert!(
                (s - mean).abs() < mean * 0.12,
                "step {s} strays from the mean {mean}"
            );
        });
        assert!((shot.trajectory.length() - steps.iter().sum::<f32>()).abs() < 0.05);
    }

    #[test]
    fn the_ball_leaves_faster_than_it_arrives() {
        let shot = resolved(0.0, 0.6, GoalTarget::new(0.0, 0.5));
        let duration = shot.trajectory.duration();
        let early = shot.trajectory.sample(duration * 0.05).velocity.length();
        let late = shot.trajectory.sample(duration * 0.95).velocity.length();
        assert!(early > late, "{early} should exceed {late}");
        assert!(late > early * 0.4, "but it must not stall: {late} vs {early}");
        assert!(shot.trajectory.progress_at(-1.0) == 0.0);
        assert!((shot.trajectory.progress_at(duration * 2.0) - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn the_same_intent_always_builds_the_same_path() {
        let a = resolved(2.2, 1.4, GoalTarget::new(0.4, 0.7));
        let b = resolved(2.2, 1.4, GoalTarget::new(0.4, 0.7));
        assert_eq!(a, b);
    }
}
