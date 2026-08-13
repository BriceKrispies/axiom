//! What one agent tick decided, and the module's account of how.
//!
//! Split out of [`super::agent`] so each file stays narrowly owned. Pure
//! relocation — the shapes are unchanged.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;
use super::agent::Perception;
use crate::showcase::ShowcaseRun;
use crate::attempt::AttemptStep;
use crate::autopilot::{self, Aggression};
use crate::identity::PlayerId;
use crate::runback::RunbackMove;
use crate::state::SimState;

/// One tick of agent output: what it decided, and the module's own account of
/// how it decided it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgentDecision {
    /// The concept to call, while the offense is waiting on one.
    pub call_play: Option<usize>,
    /// The move to make, if the geometry asked for one.
    pub wanted: Option<RunbackMove>,
    /// `axiom-agent`'s reason code for this decision.
    pub reason_code: u16,
    /// How many player-equivalent intents the agent emitted.
    pub emitted: usize,
    /// What it saw (so the trace can show the *why* next to the *what*).
    pub perception: Perception,
}

/// The machine-readable observation of the game — everything an agent needs to
/// reason about a carry, with no pixels involved.
///
/// It is deliberately a *superset* of what the agent's own brain reads. The
/// brain needs five booleans; a reader of the trace, a test, or a future
/// smarter brain needs the whole picture, and the cost of publishing it is one
/// struct built from state the simulation already holds.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentObservation {
    pub tick: u64,
    /// The attempt loop's phase (`play-call`, `mesh`, `exchange`, `carrying`…).
    pub phase: &'static str,
    /// 1-based carry number.
    pub carry: u32,
    /// The concept the offense is lined up in.
    pub concept: &'static str,
    /// The player the human/agent controls.
    pub controlled: Option<PlayerId>,
    /// Who currently holds the ball.
    pub possession: Option<PlayerId>,
    /// Whether the controlled runner has it (control is live).
    pub carrying: bool,
    /// World position of the runner, yards.
    pub position: (f32, f32, f32),
    /// Normalized field position: `0..1` sideline to sideline, `0..1` own goal
    /// line to the attacked one.
    pub normalized: (f32, f32),
    /// Yards to the goal line he is attacking.
    pub yards_to_goal: f32,
    /// Ground speed, yd/s, and the full velocity.
    pub speed: f32,
    pub velocity: (f32, f32),
    /// The move in progress, if any.
    pub action: Option<&'static str>,
    /// Airborne under his own leap, and how high his feet are (yd).
    pub airborne: bool,
    pub height: f32,
    /// Whether a leap may begin now, and the ticks left if not.
    pub jump_available: bool,
    pub jump_cooldown_left: u64,
    /// **The charge tell**, machine-readable: the defender the shoulder would go
    /// through right now, and how decisively. `None` when there is no window.
    ///
    /// This is exactly the value the player is shown on the field. An agent that
    /// had to infer it from pixels — or from its own copy of the contest — could
    /// be told something different from the human, which would make the two
    /// incomparable.
    pub charge_target: Option<PlayerId>,
    pub charge_overload: f32,
    /// Nearby opposing players, nearest first: who, how far (yd), and the
    /// bearing in the offense frame (`lateral`, `downfield`, unit).
    pub threats: Vec<ThreatView>,
    /// Confirmed successes on this carry.
    pub dodges: u32,
    pub broken: u32,
    pub hurdled: u32,
    /// Whether the play is over, and how.
    pub play_over: bool,
    pub outcome: Option<&'static str>,
}

/// One nearby opponent, as the agent sees him.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreatView {
    pub id: PlayerId,
    pub distance: f32,
    /// Unit bearing in the offense frame: `+lateral` is the runner's right,
    /// `+downfield` is toward the end zone he is attacking.
    pub lateral: f32,
    pub downfield: f32,
    /// Closing speed along that bearing, yd/s.
    pub closing: f32,
    /// Whether he is squared up to meet a hit, `0..=1`.
    pub brace: f32,
}

/// How far around the runner an opponent is worth reporting, yd.
const OBSERVED_RADIUS: f32 = 16.0;

/// Build the machine-readable observation for this tick.
pub fn observe(sim: &SimState, step: &AttemptStep) -> AgentObservation {
    let back = step.runback.back;
    let runner = back.map(|id| sim.players[id.index()]);
    let pos = runner.map(|p| p.pos).unwrap_or(sim.ball.pos);
    let vel = runner.map(|p| p.vel).unwrap_or(axiom::prelude::Vec3::ZERO);
    let sign = sim.frame.direction.sign();
    let threats = runner.map(|r| nearby(sim, &r)).unwrap_or_default();
    AgentObservation {
        tick: sim.tick,
        phase: step.phase.label(),
        carry: step.attempt,
        concept: crate::data::concept(step.concept).name,
        controlled: sim.controlled_player().filter(|id| Some(*id) == back),
        possession: sim.possession,
        carrying: sim.back_is_carrying(),
        position: (pos.x, pos.y, pos.z),
        normalized: (
            (pos.x / crate::field::coordinates::FIELD_WIDTH) + 0.5,
            (crate::field::coordinates::z_to_yards_from_own_goal(pos.z, sim.frame.direction) / 100.0)
                .clamp(-0.2, 1.2),
        ),
        yards_to_goal: crate::field::coordinates::GOAL_LINE_Z - pos.z * sign,
        speed: runner.map(|p| p.speed()).unwrap_or(0.0),
        velocity: (vel.x, vel.z),
        action: step.runback.active.map(|m| m.label()),
        airborne: step.runback.airborne,
        height: step.runback.height,
        jump_available: step.runback.jump_available,
        jump_cooldown_left: step.runback.jump_cooldown_left,
        charge_target: step.runback.charge_window.map(|w| w.defender),
        charge_overload: step.runback.charge_window.map(|w| w.overload).unwrap_or(0.0),
        threats,
        dodges: step.runback.dodges,
        broken: step.runback.broken,
        hurdled: step.runback.hurdled,
        play_over: sim.end_reason.is_some(),
        outcome: step.last.map(|record| record.outcome.label()),
    }
}

/// Opposing players inside [`OBSERVED_RADIUS`], nearest first.
fn nearby(sim: &SimState, runner: &crate::player::PlayerSim) -> Vec<ThreatView> {
    let mut seen: Vec<ThreatView> = sim
        .players
        .iter()
        .filter(|p| p.team != runner.team && p.anim.can_act())
        .filter_map(|p| {
            let to = axiom::prelude::Vec3::new(p.pos.x - runner.pos.x, 0.0, p.pos.z - runner.pos.z);
            let distance = to.length();
            (distance <= OBSERVED_RADIUS).then(|| {
                let unit = to.mul_scalar(1.0 / distance.max(1.0e-4));
                ThreatView {
                    id: p.id,
                    distance,
                    lateral: unit.dot(sim.frame.right()),
                    downfield: unit.dot(sim.frame.forward()),
                    closing: axiom::prelude::Vec3::new(
                        runner.vel.x - p.vel.x,
                        0.0,
                        runner.vel.z - p.vel.z,
                    )
                    .dot(unit),
                    brace: p.facing_dir().dot(unit.mul_scalar(-1.0)).max(0.0),
                }
            })
        })
        .collect();
    seen.sort_by(|a, b| a.distance.total_cmp(&b.distance).then(a.id.0.cmp(&b.id.0)));
    seen
}
