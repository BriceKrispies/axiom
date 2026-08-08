//! **What makes a dodge a dodge, and a hurdle a hurdle.**
//!
//! Both success signals exist to answer a question a boolean cannot: *did the
//! move actually do anything?* Pressing juke while a defender happens to be
//! somewhere on the field is not a dodge, and leaving the ground near a defender
//! is not a hurdle. So each is a two-part claim — **evidence gathered at the
//! moment of the move**, and **a verdict reached later from what the field
//! actually did** — and the signal fires only when both halves hold.
//!
//! ## A dodge
//!
//! At the tick of the cut, every opposing player is tested against the runner's
//! **pre-juke** trajectory: not "is he near me" but *"if neither of us changed
//! anything, would he have got me?"* — both bodies projected forward at their
//! current velocities, looking for a closest approach inside tackling range
//! within the lookahead. Anyone who passes that is a [`ThreatSnapshot`]: a
//! defender with a credible imminent tackle on the line the runner was on.
//!
//! Then the field decides. The threat is credited as dodged only once the runner
//! is genuinely **past** him — downfield of him by a real margin, with the ball
//! still in his hands. If the defender tackles him, the threat is cancelled. If
//! neither happens inside the resolve window, it is dropped silently: whatever
//! happened afterwards was not the juke's doing, and a signal there would be a
//! lie.
//!
//! ## A hurdle
//!
//! While the runner is airborne, any opposing player who comes inside the
//! horizontal encounter region — near enough that on the ground they would be
//! colliding — is recorded as having gone *underneath*, together with the
//! daylight there was between them. The record is only kept if the runner's feet
//! were above a defender's tackling reach at the time; below that he is not over
//! anybody, he is merely in the air.
//!
//! The verdict comes at the landing: if he lands still carrying, every defender
//! who passed beneath is a cleared one. Landing on the turf without the ball is
//! not a hurdle, however high he got.

use axiom::prelude::Vec3;

use crate::config::DT;
use crate::data::{BehaviorTuning, RunbackTuning};
use crate::identity::PlayerId;
use crate::player::PlayerSim;

/// A defender who had a credible imminent tackle on the runner's pre-juke line,
/// captured at the tick of the cut and awaiting a verdict.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreatSnapshot {
    pub defender: PlayerId,
    /// Which way the cut went — carried so the success that eventually fires
    /// names the move the player actually made.
    pub via: crate::events::RunbackMoveCode,
    /// The tick the cut was made.
    pub juked_at: u64,
    /// How close he would have come to the *pre-juke* line, yd — the evidence
    /// that the tackle was real and not merely nearby.
    pub projected_gap: f32,
}

/// A defender who passed beneath a live leap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HurdleWatch {
    pub defender: PlayerId,
    /// The most daylight there was between the runner's feet and the defender's
    /// tackling reach during the encounter, yd.
    pub clearance: f32,
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Would `defender` have tackled `runner` if neither had changed anything?
///
/// Both are projected forward at their current velocities and sampled every
/// tick to the lookahead; the answer is the closest approach they would have
/// reached, or `None` if it never came inside tackling range. Sampling rather
/// than solving the quadratic keeps it in the same fixed-step arithmetic as the
/// rest of the simulation, so the answer is exactly reproducible.
pub fn imminent_tackle(
    runner: &PlayerSim,
    defender: &PlayerSim,
    tuning: &BehaviorTuning,
    runback: &RunbackTuning,
) -> Option<f32> {
    let start_gap = flat(defender.pos.subtract(runner.pos)).length();
    if start_gap > runback.dodge_threat_range {
        return None;
    }
    let relative_pos = flat(defender.pos.subtract(runner.pos));
    let relative_vel = flat(defender.vel.subtract(runner.vel));
    // The tackle must be *reachable*: an approach inside the tackler's range at
    // some point in the lookahead, from where both bodies are actually headed.
    (0..=runback.dodge_lookahead_ticks)
        .map(|t| {
            relative_pos
                .add(relative_vel.mul_scalar(t as f32 * DT))
                .length()
        })
        .fold(None::<f32>, |best, gap| {
            Some(best.map_or(gap, |b| b.min(gap)))
        })
        .filter(|closest| *closest <= tuning.tackle_range)
}

/// Every defender with a credible imminent tackle on the runner right now — the
/// threat set a cut is measured against. Ascending player order, so the
/// resulting event stream is stable.
pub fn snapshot_threats(
    runner: &PlayerSim,
    players: &[PlayerSim],
    tick: u64,
    via: crate::events::RunbackMoveCode,
    tuning: &BehaviorTuning,
    runback: &RunbackTuning,
) -> Vec<ThreatSnapshot> {
    players
        .iter()
        .filter(|p| p.team != runner.team && p.anim.can_act())
        .filter_map(|p| {
            imminent_tackle(runner, p, tuning, runback).map(|projected_gap| ThreatSnapshot {
                defender: p.id,
                via,
                juked_at: tick,
                projected_gap,
            })
        })
        .collect()
}

/// What a pending threat resolved to this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreatVerdict {
    /// Still unresolved — keep watching.
    Pending,
    /// The runner got past him: a confirmed dodge.
    Dodged,
    /// Nothing came of it inside the window, or the defender is out of the play.
    Expired,
}

/// Judge one pending threat against the field as it now is.
///
/// `carrying` is whether the runner still has the ball — the "play continued"
/// half of the claim, and the reason a runner who was tackled two ticks after
/// his cut is never credited with beating the man who tackled him.
pub fn judge_threat(
    threat: ThreatSnapshot,
    runner: &PlayerSim,
    defender: &PlayerSim,
    forward: Vec3,
    tick: u64,
    carrying: bool,
    runback: &RunbackTuning,
) -> ThreatVerdict {
    let elapsed = tick.saturating_sub(threat.juked_at);
    if !carrying || !runner.anim.can_act() {
        return ThreatVerdict::Expired;
    }
    // Downfield of the man who was about to make the tackle, by a real margin.
    let past = flat(runner.pos.subtract(defender.pos)).dot(forward);
    match (
        past >= runback.dodge_clear_yards,
        elapsed >= u64::from(runback.dodge_resolve_ticks),
    ) {
        (true, _) => ThreatVerdict::Dodged,
        (false, true) => ThreatVerdict::Expired,
        (false, false) => ThreatVerdict::Pending,
    }
}

/// Record every opposing player passing beneath a live leap this tick, folding
/// the sighting into `watches` (best clearance wins for a defender seen twice).
pub fn watch_hurdles(
    runner: &PlayerSim,
    players: &[PlayerSim],
    watches: &mut Vec<HurdleWatch>,
    runback: &RunbackTuning,
) {
    let clearance = runner.pos.y - runback.hurdle_min_height;
    if clearance <= 0.0 {
        return;
    }
    players
        .iter()
        .filter(|p| p.team != runner.team && p.anim.can_act())
        .filter(|p| {
            let reach =
                runner.archetype.body_radius + p.archetype.body_radius + runback.hurdle_reach;
            flat(p.pos.subtract(runner.pos)).length() <= reach
        })
        .for_each(|p| {
            match watches.iter_mut().find(|w| w.defender == p.id) {
                Some(existing) => existing.clearance = existing.clearance.max(clearance),
                None => watches.push(HurdleWatch {
                    defender: p.id,
                    clearance,
                }),
            }
        });
}
