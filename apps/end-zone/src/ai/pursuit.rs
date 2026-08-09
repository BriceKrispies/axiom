//! **One objective for the whole defense: get to the ball carrier before he
//! gets to the end zone.**
//!
//! Every defender solves the same problem here, and their differing
//! responsibilities ([`super::perception::Responsibility`]) only bias *which
//! shoulder* they take it from. That is the structural point, and it replaces an
//! arrangement where each responsibility computed its own absolute aim point
//! from its own formula — which meant nothing tied any of them to the objective,
//! and one of them (the deep safety) was free to contradict it entirely.
//!
//! It did. Measured over twenty carries, the safety spent 59% of his goal-side
//! ticks running AWAY from the ball at up to 9.7 yd/s, because his aim point was
//! `carrier + velocity × (distance × 1.6)`: backing up increased his distance,
//! which increased the lead, which pushed the aim point further downfield, which
//! backed him up more. A feedback loop with no exit, and no rule anywhere that
//! said "now go and get him" — because there was no shared objective to say it.
//!
//! ## The solve
//!
//! An **interception**, not a lead. For each moment ahead, ask where the carrier
//! will be and whether this defender can be there by then at his own top speed.
//! The earliest such point is the aim. It is the whole model, and everything
//! good about it falls out of the arithmetic rather than being written down as a
//! rule:
//!
//! * a defender **cannot run away from the ball**, because an interception point
//!   is by construction somewhere the carrier has not yet reached, and the path
//!   to it never leads backwards past him;
//! * a defender with the angle **holds it** rather than chasing the carrier's
//!   current position, because the earliest reachable point already accounts for
//!   his own speed;
//! * a defender who is **beaten** — no reachable point before the goal line —
//!   aims at the goal line itself, which is the only situation where running
//!   downfield of the carrier is right, and it is the last-ditch angle a real
//!   defender takes;
//! * a defender who is **faster** than the carrier converges on him directly,
//!   with no cushion to maintain and none to get stuck in.
//!
//! Nothing here needs to know whether it is covering deep, containing an edge or
//! filling a gap. Those are answers to "from which side", and they are applied
//! as a lateral nudge to this point by the caller — never as a different
//! destination.

use axiom::prelude::Vec3;

use crate::config::DT;
use crate::field::GOAL_LINE_Z;
use crate::player::PlayerSim;

/// How far ahead an interception is searched, in ticks (~2 s). Past this the
/// carrier's constant-velocity projection is fiction anyway.
const HORIZON_TICKS: u32 = 120;

/// How much of his own top speed a defender is assumed to be able to bring to
/// the interception.
///
/// Below `1.0` because he has to turn, accelerate and run a curve rather than
/// teleport along the straight line the solve measures. Assuming the full
/// figure makes him believe in interceptions he cannot actually make, so he aims
/// too far downfield and arrives late — the same mistake the old lead made, in
/// a smaller way.
const REACHABLE_FRACTION: f32 = 0.92;

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// The point this defender should run at to cut the carrier off.
///
/// `carrier_pos` / `carrier_vel` come from the defender's own **delayed**
/// perception, so his reaction latency is preserved: he intercepts where he
/// believes the carrier is going, not where it truly is.
pub fn intercept(
    defender: &PlayerSim,
    carrier_pos: Vec3,
    carrier_vel: Vec3,
    drive_sign: f32,
) -> Vec3 {
    let speed = (defender.archetype.max_speed * REACHABLE_FRACTION).max(0.1);
    let goal_z = drive_sign * GOAL_LINE_Z;

    // The earliest moment he can be where the carrier will be.
    let meeting = (0..=HORIZON_TICKS).find(|tick| {
        let seconds = *tick as f32 * DT;
        let there = flat(carrier_pos.add(carrier_vel.mul_scalar(seconds)));
        flat(there.subtract(defender.pos)).length() <= speed * seconds
    });

    match meeting {
        Some(tick) => {
            let seconds = tick as f32 * DT;
            let there = carrier_pos.add(carrier_vel.mul_scalar(seconds));
            // Never aim past the goal line: beyond it the play is already over,
            // and a point in the stands is not a defensive angle.
            Vec3::new(there.x, 0.0, clamp_to_goal(there.z, goal_z, drive_sign))
        }
        // Beaten: nowhere he can reach in time. The best remaining angle is the
        // goal line on the carrier's line — run for the corner and hope the
        // carrier has to turn. This is the ONE case where a defender correctly
        // ends up downfield of the ball.
        None => {
            let to_goal = (goal_z - carrier_pos.z) / carrier_vel.z.abs().max(0.5);
            let there = carrier_pos.add(carrier_vel.mul_scalar(to_goal.abs().min(4.0)));
            Vec3::new(there.x, 0.0, clamp_to_goal(there.z, goal_z, drive_sign))
        }
    }
}

/// Clamp a `z` so it never lies beyond the attacked goal line.
fn clamp_to_goal(z: f32, goal_z: f32, drive_sign: f32) -> f32 {
    let past = (z - goal_z) * drive_sign > 0.0;
    match past {
        true => goal_z,
        false => z,
    }
}
