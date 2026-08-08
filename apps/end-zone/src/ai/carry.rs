//! **How a ball carrier runs.** The heading policy every carrier uses — human
//! or not.
//!
//! This used to live in [`crate::autopilot`] as a *stick*: a headless policy
//! that produced the same `[-1,1]²` input a player's thumb would, and overwrote
//! the carrier's AI intent with it. That made sense while the human steered,
//! because then there were genuinely two drivers and the autopilot was standing
//! in for one of them.
//!
//! There is only one driver now. The running back runs downfield automatically
//! and the player's controls modify that run rather than replace it, so "which
//! way does a carrier run" is a question the **AI** answers, for everybody, all
//! the time — and it belongs here next to the other candidate generators rather
//! than in a harness. Moving it also deleted the duplicate: there is no longer a
//! version of this in the autopilot that could drift away from the one the game
//! actually plays.
//!
//! The policy itself is unchanged in shape: score a fixed fan of candidate
//! headings, reward downfield progress, punish running at a defender or off the
//! field, and take the best. Deterministic throughout — a fixed candidate set, a
//! fixed iteration order, ties broken toward the earlier heading.
//!
//! It is deliberately **good, not perfect**. A carrier who threaded every
//! defender would make the player's three moves decoration; one who ran straight
//! into the first tackler would make them mandatory. What it does is find the
//! sensible lane and leave the *encounter* — the man who has actually cut you
//! off — for the player to answer.

use axiom::prelude::Vec3;

use crate::field::{OffenseFrame, OffensePoint};
use crate::player::PlayerSim;

use super::brain::BrainCtx;

/// Candidate headings, radians off straight downfield (negative = toward the
/// offense's left).
const FAN: [f32; 9] = [-1.15, -0.8, -0.5, -0.25, 0.0, 0.25, 0.5, 0.8, 1.15];
/// How far around the carrier a defender begins to influence the heading, yd.
const THREAT_RADIUS: f32 = 11.0;
/// How far off centre the policy treats as the sideline, yd.
const SIDELINE: f32 = 24.5;
/// How far along the chosen heading the movement point is planted, yd.
const REACH: f32 = 7.0;
/// The designed hole stops pulling once the carrier is this far past it, yd.
const AIM_RELEASE: f32 = 1.0;
/// How strongly the designed hole pulls the early part of a run.
const AIM_WEIGHT: f32 = 2.4;

/// Where the carrier should run this tick, in world space.
///
/// `aim` is the play's designed hole, if it has one and the carrier has not yet
/// run past it — the reason a called play *gives you the opening* instead of
/// merely starting the same run three different ways.
pub fn carry_point(player: &PlayerSim, aim: Option<Vec3>, ctx: &BrainCtx<'_>) -> Vec3 {
    let frame = ctx.frame;
    let here = frame.from_world(player.pos);
    let live_aim = aim
        .map(|point| frame.from_world(point))
        .filter(|target| target.downfield > here.downfield + AIM_RELEASE);
    let threats: Vec<OffensePoint> = ctx
        .players
        .iter()
        .filter(|p| p.team != player.team && p.anim.can_act())
        .map(|p| frame.from_world(p.pos))
        .collect();
    let best = FAN
        .iter()
        .map(|&angle| (angle, score_heading(angle, here, live_aim, &threats)))
        .fold((0.0f32, f32::NEG_INFINITY), |best, (angle, score)| {
            match score > best.1 {
                true => (angle, score),
                false => best,
            }
        })
        .0;
    let heading = frame
        .forward()
        .mul_scalar(best.cos())
        .add(frame.right().mul_scalar(best.sin()));
    OffenseFrame::clamp_in_bounds(
        Vec3::new(
            player.pos.x + heading.x * REACH,
            0.0,
            player.pos.z + heading.z * REACH,
        ),
        ctx.tuning.bounds_margin,
    )
}

/// Score one candidate heading from the carrier at `here`: reward downfield
/// progress and the designed hole, punish running toward a defender or off the
/// field.
fn score_heading(
    angle: f32,
    here: OffensePoint,
    aim: Option<OffensePoint>,
    threats: &[OffensePoint],
) -> f32 {
    let dir_down = angle.cos();
    let dir_lat = angle.sin();
    // Downfield progress is the base reward; a backward heading scores negative.
    let mut score = dir_down * 2.0;
    // The play's hole, while it is still ahead: reward headings that point at it.
    if let Some(target) = aim {
        let to_lat = target.lateral - here.lateral;
        let to_down = target.downfield - here.downfield;
        let length = (to_lat * to_lat + to_down * to_down).sqrt().max(1.0e-3);
        score += AIM_WEIGHT * ((dir_lat * to_lat + dir_down * to_down) / length).max(0.0);
    }
    // Steer off a near sideline: look a few yards along the heading and punish
    // leaving the field.
    let ahead_lat = here.lateral + dir_lat * 6.0;
    score -= (ahead_lat.abs() - SIDELINE).max(0.0) * 1.5;
    threats
        .iter()
        .map(|threat| {
            let rel_down = threat.downfield - here.downfield;
            let rel_lat = threat.lateral - here.lateral;
            let dist = (rel_down * rel_down + rel_lat * rel_lat).sqrt();
            // Only defenders ahead or beside, and within reach, threaten a run.
            let counts = (dist < THREAT_RADIUS) & (rel_down > -1.5);
            let inv = 1.0 / dist.max(0.5);
            // How aligned the heading is with the defender's bearing (1 =
            // straight at him); running away from him costs nothing.
            let alignment = ((dir_down * rel_down + dir_lat * rel_lat) * inv).max(0.0);
            let closeness = (THREAT_RADIUS - dist).max(0.0) / THREAT_RADIUS;
            f32::from(u8::from(counts)) * alignment * closeness * 4.0
        })
        .fold(score, |acc, penalty| acc - penalty)
}
