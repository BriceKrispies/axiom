//! What the keeper *decides*: one reading of the early flight, turned into a
//! commitment it then has to live with.
//!
//! Split from the keeper itself because deciding and moving are different jobs.
//! Everything here is a pure function of a trajectory, a moment, and the keeper's
//! own limits — no state, nothing it can write. The dive that executes the
//! decision lives next door in [`super::keeper`].

use axiom::prelude::Vec3;

use crate::shot::Trajectory;
use crate::tuning::KeeperTuning;

use super::keeper::HIP_HEIGHT;

/// What the keeper decided, once, at the end of its reaction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeeperRead {
    /// Where the keeper believes the ball will cross the goal plane.
    pub predicted: Vec3,
    /// Where it is actually able to get to, given its reach and its execution.
    pub aim: Vec3,
    /// Signed lateral commitment, `-1..1`.
    pub lean: f32,
    /// Where the hands are thrown, `-1` low to `+1` high.
    pub height_bias: f32,
    /// Seconds the dive takes to reach full extension (a long dive is slower —
    /// the keeper has a speed, not a teleport).
    pub extend_time: f32,
    /// Flight time at which the read was taken.
    pub at: f32,
}

/// Take the one reading, and turn it into a commitment.
pub(super) fn take_read(
    home: Vec3,
    expectation: (f32, f32),
    trajectory: &Trajectory,
    t: f32,
    tuning: &KeeperTuning,
) -> KeeperRead {
    take_read_with(home, expectation, trajectory, t, tuning, tuning.read_fidelity)
}

/// The same, at an explicit fidelity — the correction reads better than the
/// first look because it has had a beat of flight to watch.
pub(super) fn take_read_with(
    home: Vec3,
    expectation: (f32, f32),
    trajectory: &Trajectory,
    t: f32,
    tuning: &KeeperTuning,
    fidelity: f32,
) -> KeeperRead {
    let seen = predict_crossing(trajectory, t, fidelity, tuning.read_gravity);
    // What it has been beaten by lately pulls the height it reads toward that
    // memory. Put four in the same place and the keeper stops being surprised
    // there — which is what makes any one authored shape a good answer rather
    // than a solved one.
    let (remembered, weight) = expectation;
    let predicted = Vec3::new(
        seen.x,
        seen.y + (remembered - seen.y) * weight,
        seen.z,
    );

    // What the keeper can actually get to: its dive reaches only so far
    // sideways and only so high, and it executes its own plan imperfectly.
    let desire = predicted.x - home.x;
    let travel = desire.clamp(-tuning.dive_distance, tuning.dive_distance) * tuning.execution;
    let ceiling = HIP_HEIGHT + tuning.vertical_reach;
    // The vertical commitment is hedged toward standing height: see
    // `KeeperTuning::vertical_trust`.
    let trusted =
        HIP_HEIGHT + (predicted.y - HIP_HEIGHT) * tuning.vertical_trust.clamp(0.0, 1.0);
    let aim = Vec3::new(home.x + travel, trusted.clamp(0.10, ceiling), predicted.z);
    let lean = (desire / tuning.dive_distance.max(1.0e-3)).clamp(-1.0, 1.0);
    // Where the hands go. Around hip height the keeper stays square; the higher
    // or lower the read, the more committed the hands are — and a keeper who
    // commits low to a lob is a keeper who has already lost.
    let height_bias = (((trusted - HIP_HEIGHT) / 0.85) * 1.4).clamp(-1.0, 1.0);
    let travel_time = travel.abs() / tuning.dive_speed.max(1.0e-3);
    KeeperRead {
        predicted,
        aim,
        lean,
        height_bias,
        extend_time: tuning.extend_time.max(travel_time),
        at: t,
    }
}

/// Extrapolate where the ball crosses the goal plane, from one instant of
/// flight.
///
/// The keeper's mental model is the one a human has: **a ball flying
/// ballistically from where it is right now** — the pace and direction it can
/// see, falling under gravity. It carries no model of the movement the striker
/// put on it, because that movement has not happened yet.
///
/// `fidelity` is how much of the shot's real behaviour it additionally
/// anticipates, blended in on top. At `0` it is a pure ballistic read and every
/// bend, dip and late lift is a surprise; at `1` it knows exactly where the ball
/// finishes. The shipping value sits low, which is what makes *where* the player
/// puts the peak of a curve a real decision:
///
/// * a curve that breaks after the read is lateral movement the keeper never
///   saw, so it dives to where the ball *was* going;
/// * a shot that climbs late reads as flat, and the keeper commits under it;
/// * a shot that is steeply up at the read and dips afterwards reads as high,
///   and the keeper commits over it.
pub fn predict_crossing(
    trajectory: &Trajectory,
    t: f32,
    fidelity: f32,
    gravity: f32,
) -> Vec3 {
    let now = trajectory.sample(t);
    // Time to the goal plane at the pace it is going.
    let closing = now.velocity.z.min(-0.25);
    let dt = (now.position.z / -closing).clamp(0.0, 3.0);
    let ballistic = Vec3::new(
        now.position.x + now.velocity.x * dt,
        (now.position.y + now.velocity.y * dt - 0.5 * gravity.max(0.0) * dt * dt).max(0.0),
        0.0,
    );
    let truth = trajectory.at_progress(1.0);
    let blend = fidelity.clamp(0.0, 1.0);
    ballistic.add(truth.subtract(ballistic).mul_scalar(blend))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::{ball_spot, GoalMouth};
    use crate::shot::{BendCurve, GoalTarget, ResolvedShot, ShotIntent};
    use crate::tuning::Tuning;

    /// The shipping read gravity, so a test reads what the keeper reads.
    const GRAVITY: f32 = Tuning::DEFAULT.keeper.read_gravity;

    fn shot(bend: f32, loft: f32, h: f32, v: f32) -> ResolvedShot {
        shaped(bend, 0.5, loft, 0.5, h, v)
    }

    /// A shot with explicit peak positions, so a test can say "this one breaks
    /// late" rather than only "this one breaks".
    fn shaped(bend: f32, bend_at: f32, loft: f32, loft_at: f32, h: f32, v: f32) -> ResolvedShot {
        let tuning = Tuning::DEFAULT;
        ResolvedShot::build(
            ball_spot(tuning.flight.ball_radius),
            ShotIntent {
                target: GoalTarget::new(h, v),
                bend: BendCurve::through(bend_at, bend, 0.14),
                loft: BendCurve::through(loft_at, loft, 0.14),
            },
            &GoalMouth::new(tuning.goal.inset),
            &tuning,
        )
    }

    #[test]
    fn fidelity_spans_a_pure_ballistic_read_and_a_perfect_one() {
        let s = shot(3.5, 2.4, -0.7, 0.6);
        let perfect = predict_crossing(&s.trajectory, 0.17, 1.0, GRAVITY);
        assert!(
            perfect.subtract(s.world_target).length() < 1.0e-4,
            "a perfect reader is exactly right: {perfect:?}"
        );
        let blind = predict_crossing(&s.trajectory, 0.17, 0.0, GRAVITY);
        assert!(
            blind.subtract(s.world_target).length() > 1.0,
            "a purely ballistic reader is not"
        );
        assert_eq!(blind.z, 0.0, "the read is always on the goal plane");
        // Out-of-range fidelity clamps rather than extrapolating past the truth.
        assert_eq!(predict_crossing(&s.trajectory, 0.17, 4.0, GRAVITY), perfect);
    }

    #[test]
    fn a_bend_is_what_a_low_fidelity_reader_misses() {
        let straight = shot(0.0, 0.6, -0.7, 0.5);
        let curled = shot(4.2, 0.6, -0.7, 0.5);
        assert_eq!(straight.world_target, curled.world_target);
        let fidelity = Tuning::DEFAULT.keeper.read_fidelity;
        let read_straight = predict_crossing(&straight.trajectory, 0.17, fidelity, GRAVITY);
        let read_curled = predict_crossing(&curled.trajectory, 0.17, fidelity, GRAVITY);
        let err_straight = (read_straight.x - straight.world_target.x).abs();
        let err_curled = (read_curled.x - curled.world_target.x).abs();
        assert!(
            err_curled > err_straight + 1.0,
            "the curve should fool the read: {err_curled} vs {err_straight}"
        );
    }

    #[test]
    fn the_vertical_shape_decides_how_high_the_keeper_reads_it() {
        let fidelity = Tuning::DEFAULT.keeper.read_fidelity;
        let read = |s: &ResolvedShot| predict_crossing(&s.trajectory, 0.17, fidelity, GRAVITY).y;
        // The same top-corner endpoint, two vertical shapes. The keeper mis-reads
        // both — it cannot see the rest of the flight — but it mis-reads them in
        // opposite directions, which is what makes the height editor a decision
        // rather than a dial.
        let driven = shaped(0.0, 0.5, 0.0, 0.5, 0.0, 0.92);
        let arced = shaped(0.0, 0.5, 1.6, 0.5, 0.0, 0.92);
        assert_eq!(driven.world_target, arced.world_target);
        let flat_err = read(&driven) - driven.world_target.y;
        let arc_err = read(&arced) - arced.world_target.y;
        assert!(
            flat_err < 0.0,
            "a driven shot is read as arriving lower than it does: {flat_err}"
        );
        assert!(
            arc_err > 0.0,
            "an arced shot is read as arriving higher than it does: {arc_err}"
        );
        assert!(
            (flat_err - arc_err).abs() > 0.8,
            "and the two reads are far apart: {flat_err} vs {arc_err}"
        );
        // Where the arc peaks moves the read as well, so the two ends of the
        // height editor are genuinely different instructions.
        let early = read(&shaped(0.0, 0.5, 2.6, 0.28, 0.0, 0.55));
        let late = read(&shaped(0.0, 0.5, 2.6, 0.78, 0.0, 0.55));
        assert!(
            (early - late).abs() > 0.25,
            "an early peak reads {early} and a late one {late}"
        );
    }

}
