//! The headless autopilot: the deterministic "brain" that drives the user's
//! ball-carrier slot when no human is steering. It reads the authoritative
//! simulation and returns this tick's movement stick — a policy that carries the
//! ball toward the opponent goal while steering through the defense — plus the
//! decision of when to release a pass.
//!
//! It is a pure function of the simulation: no I/O, no wall clock, no
//! randomness, so an autopiloted run replays bit-for-bit like any other run.
//! The autopilot owns the four decisions a player makes: WHICH play to call at
//! the line ([`call_play`]), WHICH read to take in a decision window
//! ([`decide`]), WHERE to run ([`steer`]), and — for the ambient cone-aimed
//! throw — WHEN to release ([`should_throw`]).
//!
//! [`decide`] is also the prototype's **tuning instrument**: running the same
//! attempt loop under an impatient, a balanced and a greedy [`Patience`] is how
//! we check that waiting for the deep read really is a trade rather than a free
//! upgrade. If every patience profile posts the same numbers, the prototype has
//! failed its own design question.

use axiom::prelude::Vec2;

use crate::attempt::{AttemptPhase, AttemptStep, PlayerChoice, MAX_WINDOWS};
use crate::data::prototype::{concept, READ_COUNT};
use crate::field::OffensePoint;
use crate::player::PlayerSim;
use crate::state::SimState;

/// How far around the carrier a defender begins to influence steering, yards.
const THREAT_RADIUS: f32 = 11.0;
/// Candidate headings the policy scores, radians off straight-downfield
/// (negative = toward the offense's left, positive = toward its right).
const FAN: [f32; 9] = [-1.15, -0.8, -0.5, -0.25, 0.0, 0.25, 0.5, 0.8, 1.15];
/// How far off centre the steering treats as the sideline, yards.
const SIDELINE: f32 = 24.5;
/// A receiver is "open" once his nearest defender is farther than this, yards —
/// the band below which a throw risks an interception.
const OPEN_ENOUGH: f32 = 4.0;
/// A receiver must be at least this far downfield of the passer to be worth a
/// throw, yards (throwing flat or behind never advances the ball).
const THROW_LEAD: f32 = 3.0;

/// How patient a simulated quarterback is — the knob the balance harness
/// sweeps. A patient policy holds out for a deeper read and eats the sacks that
/// come with waiting; an impatient one takes the checkdown every time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patience {
    /// The shallowest read this policy will settle for (`0` = the quick out).
    pub floor: usize,
    /// How open a read must look before this policy takes it, `0..1`.
    pub demand: f32,
    /// Pocket pressure above which it abandons the read and runs, `0..1`.
    pub bail: f32,
}

impl Patience {
    /// Takes the first thing available.
    pub const IMPATIENT: Patience = Patience {
        floor: 0,
        demand: 0.20,
        bail: 0.92,
    };
    /// Waits for a real look but does not chase the deep shot.
    pub const BALANCED: Patience = Patience {
        floor: 0,
        demand: 0.48,
        bail: 0.80,
    };
    /// Refuses the checkdown and holds out for the intermediate or the post.
    pub const GREEDY: Patience = Patience {
        floor: 1,
        demand: 0.42,
        bail: 0.74,
    };
}

/// The choice this policy makes in an open decision window, or `None` to let the
/// window close and wait for a better look. `None` is a real decision with a
/// real cost — the rush is closer when the next window opens, and the window
/// after that is shorter.
/// The concept the autopilot always calls.
///
/// Fixed, and deliberately so. [`decide`] is the prototype's balance
/// instrument, and a policy that also shopped the PLAYBOOK would confound the
/// thing it measures: two patience profiles could post different numbers
/// because they ran different routes rather than because they waited
/// differently. The autopilot answers the read question and nothing else.
pub const AUTOPILOT_CONCEPT: usize = 0;

/// The play to call, while the offense is waiting on one (`None` otherwise).
///
/// An attempt does not start until a play is called, so a headless session has
/// to answer this or it stands at the line forever.
pub fn call_play(step: &AttemptStep) -> Option<usize> {
    matches!(step.phase, AttemptPhase::PlayCall).then_some(AUTOPILOT_CONCEPT)
}

pub fn decide(step: &AttemptStep, patience: Patience) -> Option<PlayerChoice> {
    if !step.phase.in_window() {
        return None;
    }
    let read = &step.read;
    let rewards = concept(read.concept).read_rewards;
    let max_reward = rewards[READ_COUNT - 1].max(1.0);
    let value = |r: usize| read.read(r).openness * rewards[r] / max_reward;
    let pick = (patience.floor.min(READ_COUNT - 1)..READ_COUNT)
        .filter(|r| read.read(*r).live)
        .max_by(|a, b| value(*a).total_cmp(&value(*b)))?;
    // The last window is the last chance: after it closes, nobody is asking
    // again and the rush finishes the job.
    let last_chance = step.windows >= MAX_WINDOWS;
    let panicking = read.pressure >= patience.bail;
    if read.read(pick).openness >= patience.demand {
        return Some(PlayerChoice::Throw(pick));
    }
    match (panicking, last_chance) {
        (true, _) => Some(PlayerChoice::Scramble),
        (false, true) => Some(PlayerChoice::Throw(pick)),
        (false, false) => None,
    }
}

/// This tick's movement stick for the autopilot, offense-relative (`x` = right,
/// `y` = downfield), each in `-1..=1`. Returns [`Vec2::ZERO`] whenever the
/// autopilot is steering no one — pre-snap, the ball in flight, the play dead,
/// or the defense in possession — leaving the AI intents untouched.
pub fn steer(sim: &SimState) -> Vec2 {
    let Some(id) = sim.controlled_player() else {
        return Vec2::ZERO;
    };
    let carrier = sim.players[id.index()];
    let here = sim.frame.from_world(carrier.pos);
    // Defenders still able to make a play, in offense-relative coordinates.
    let threats: Vec<OffensePoint> = sim
        .players
        .iter()
        .filter(|p| p.team != carrier.team && p.anim.can_act())
        .map(|p| sim.frame.from_world(p.pos))
        .collect();
    let best = FAN
        .iter()
        .map(|&angle| (angle, score_heading(angle, here, &threats)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(angle, _)| angle)
        .unwrap_or(0.0);
    // Offense-relative unit heading: `y` downfield (cos), `x` lateral (sin).
    Vec2::new(best.sin(), best.cos())
}

/// Score a candidate heading (radians off downfield) from the carrier at `here`:
/// reward downfield progress, punish running toward a defender or off the field.
fn score_heading(angle: f32, here: OffensePoint, threats: &[OffensePoint]) -> f32 {
    let dir_down = angle.cos();
    let dir_lat = angle.sin();
    // Downfield progress is the base reward; a backward heading scores negative.
    let mut score = dir_down * 2.0;
    // Steer off a near sideline: look a few yards along the heading and penalize
    // leaving the field.
    let ahead_lat = here.lateral + dir_lat * 6.0;
    score -= (ahead_lat.abs() - SIDELINE).max(0.0) * 1.5;
    for threat in threats {
        let rel_down = threat.downfield - here.downfield;
        let rel_lat = threat.lateral - here.lateral;
        let dist = (rel_down * rel_down + rel_lat * rel_lat).sqrt();
        // Only defenders ahead or beside, and within reach, threaten this run.
        if dist >= THREAT_RADIUS || rel_down <= -1.5 {
            continue;
        }
        let inv = 1.0 / dist.max(0.5);
        // How aligned the heading is with the defender's bearing (1 = straight
        // at him); running away from him costs nothing.
        let alignment = ((dir_down * rel_down + dir_lat * rel_lat) * inv).max(0.0);
        let closeness = (THREAT_RADIUS - dist) / THREAT_RADIUS;
        score -= alignment * closeness * 4.0;
    }
    score
}

/// Whether the autopilot should release the pass THIS tick: the quarterback is
/// holding a live ball and the receiver the simulation would throw to (the
/// nearest eligible one) is open and working downfield of the passer. Until then
/// the quarterback keeps scrambling and the routes keep developing.
pub fn should_throw(sim: &SimState) -> bool {
    let holding = sim.possession == Some(sim.quarterback);
    let Some(&target) = sim.throwable.first() else {
        return false;
    };
    let here = sim
        .frame
        .from_world(sim.players[sim.quarterback.index()].pos);
    let receiver = sim.players[target.index()];
    let spot = sim.frame.from_world(receiver.pos);
    holding
        && spot.downfield > here.downfield + THROW_LEAD
        && nearest_defender_distance(sim, receiver) > OPEN_ENOUGH
}

/// The distance from `receiver` to the nearest opposing player who can still
/// make a play, yards on the ground plane ([`f32::INFINITY`] if none can).
fn nearest_defender_distance(sim: &SimState, receiver: PlayerSim) -> f32 {
    sim.players
        .iter()
        .filter(|p| p.team != receiver.team && p.anim.can_act())
        .map(|p| {
            let dx = p.pos.x - receiver.pos.x;
            let dz = p.pos.z - receiver.pos.z;
            (dx * dx + dz * dz).sqrt()
        })
        .fold(f32::INFINITY, f32::min)
}
