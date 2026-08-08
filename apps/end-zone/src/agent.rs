//! **Playing End Zone with the reusable `axiom-agent` module.**
//!
//! The agent is not a simulation of a player; it *is* a player. Every tick it
//! runs the module's real loop and the intents that come out are lowered into
//! the same [`crate::showcase::ShowcaseRun`] entry points a keyboard reaches:
//!
//! ```text
//! sim state --perceive--> Observation (integer facts)
//!            --axiom-agent decide--> press_control intents
//!            --lower--> select_concept / RunbackMove --> the real game
//! ```
//!
//! # What is the agent's, and what is the app's
//!
//! **The app owns perception.** Which defender is the encounter, how fast the
//! two are closing, whether he is squared up, whether the leap is off cooldown —
//! that is the runner's *eyes*, and every part of it names an End Zone noun
//! `axiom-agent` must never learn. It is the same read the headless policy in
//! [`crate::autopilot`] uses, through the same [`crate::autopilot::encounter`]
//! function, so the agent and the game can never be judging different fields.
//!
//! **The agent owns the decision.** Which of the four verbs answers this
//! geometry is a *priority ordering over present facts*, and that is exactly
//! what the module's scripted brain is: an ordered rule table where the first
//! rule whose fact is present wins. The ordering — go through him if you can,
//! else over him, else round him — is written as data in [`decide_one_step`] and
//! evaluated by the module. There is no hand-rolled decision anywhere in this
//! file: cut the agent out and nothing is ever pressed.
//!
//! # Why the scripted brain and not the axis-map brain
//!
//! Burnt Rubber's driver steers a wheel, so it needs the analogue brain. A
//! running back has four buttons and presses at most one of them, so the
//! discrete brain is the right shape — and `press_control` is precisely the
//! neutral intent for "the player pushed the thing", which is what makes the
//! lowering below a translation rather than a special case.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;

use crate::attempt::AttemptStep;
use crate::autopilot::{self, Aggression};
use crate::identity::PlayerId;
use crate::runback::RunbackMove;
use crate::state::SimState;

/// The app's control vocabulary: the meaning this app assigns to a neutral
/// `press_control` code. `axiom-agent` carries the `u32` opaquely.
pub const CONTROL_CALL_PLAY: u32 = 1;
pub const CONTROL_JUKE_LEFT: u32 = 2;
pub const CONTROL_JUKE_RIGHT: u32 = 3;
pub const CONTROL_SHOULDER: u32 = 4;
pub const CONTROL_JUMP: u32 = 5;

/// The app's observation-fact vocabulary: what the back can *see*. A fact is
/// present only when there is something to perceive, which is what lets the
/// priority live in the rule ORDER rather than in a comparison — "there is a man
/// I can run through" and "there is a man I must go round" are different
/// sightings, not one number with a threshold.
pub const FACT_AWAITING_CALL: u16 = 1;
pub const FACT_RUN_THROUGH: u16 = 2;
pub const FACT_GO_OVER: u16 = 3;
pub const FACT_CUT_LEFT: u16 = 4;
pub const FACT_CUT_RIGHT: u16 = 5;

/// Fixed-point scale: one whole unit (a yard, a yard per second) is a million.
const MICRO: f32 = 1_000_000.0;

/// The stable agent id this single-runner session uses.
const AGENT_RAW_ID: u64 = 0x454e_445a; // "ENDZ"

/// The engine's fixed 60 Hz step delta in integer nanoseconds.
const FIXED_DELTA_NANOS: u64 = crate::config::FIXED_STEP_NANOS;

/// What the agent perceived this tick, before it is encoded as integer facts.
///
/// Split out so perception is testable on its own: it is a pure function of the
/// simulation and the attempt view, with no agent machinery in it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Perception {
    /// The offense is at the line waiting for a play to be called.
    pub awaiting_call: bool,
    /// There is a man in range with enough momentum behind the runner, and he is
    /// not squared up to meet it — the charge's own conditions, read the way the
    /// resolution will read them.
    pub run_through: Option<f32>,
    /// There is a man in range the runner can go over, and the leap is ready.
    pub go_over: Option<f32>,
    /// There is a man in range coming from the runner's right (cut left) or his
    /// left (cut right).
    pub cut_left: Option<f32>,
    pub cut_right: Option<f32>,
}

/// Look at the field and measure what a back would need to know.
///
/// The three "can I" questions are asked against the same [`Aggression`] profile
/// the headless policy uses, so the agent's *standards* are tunable data rather
/// than a constant buried in a decision.
pub fn perceive(sim: &SimState, step: &AttemptStep, policy: Aggression) -> Perception {
    let awaiting_call = step.phase.accepts_call();
    let Some(seen) = autopilot::encounter(sim, step)
        .filter(|_| step.phase.controllable())
        .filter(|seen| seen.gap <= policy.react_range)
    else {
        return Perception {
            awaiting_call,
            ..Perception::default()
        };
    };
    let run_through = (seen.closing >= policy.charge_speed && seen.brace <= policy.charge_max_brace)
        .then_some(seen.closing);
    let go_over =
        (run_through.is_none() && policy.will_jump && step.runback.jump_available).then_some(seen.gap);
    let cut = (run_through.is_none() && go_over.is_none()).then_some(seen.gap);
    Perception {
        awaiting_call,
        run_through,
        go_over,
        cut_left: cut.filter(|_| seen.from_right),
        cut_right: cut.filter(|_| !seen.from_right),
    }
}

/// One full `observe → decide → emit → lower` cycle: what the agent does with
/// one tick of the game.
///
/// Returns the play to call (if the offense is waiting on one) and the move to
/// make (if the geometry asks for one), plus the module's own reason code and
/// how many intents it emitted — the numbers the trace prints so a reader can
/// see the agent, and not the app, doing the deciding.
pub fn decide_one_step(
    sim: &SimState,
    step: &AttemptStep,
    policy: Aggression,
    tick: u64,
) -> AgentDecision {
    let agent_id = AgentApi::create_agent_id(AGENT_RAW_ID);
    let profile = AgentApi::debug_perfect_profile();
    let seen = perceive(sim, step, policy);

    // Observe.
    let mut builder = AgentApi::observation_builder(agent_id, Tick::new(tick), 2, 5, 0);
    let _ = builder.add_channel(AgentApi::channel_geometric());
    let _ = builder.add_channel(AgentApi::channel_semantic());
    [
        seen.awaiting_call.then_some((FACT_AWAITING_CALL, 1.0)),
        seen.run_through.map(|v| (FACT_RUN_THROUGH, v)),
        seen.go_over.map(|v| (FACT_GO_OVER, v)),
        seen.cut_left.map(|v| (FACT_CUT_LEFT, v)),
        seen.cut_right.map(|v| (FACT_CUT_RIGHT, v)),
    ]
    .into_iter()
    .flatten()
    .for_each(|(kind, value)| {
        let _ = builder.add_fact(AgentApi::observation_fact(
            kind,
            0,
            0,
            0,
            0,
            (value * MICRO) as i64,
        ));
    });
    let observation = builder.build();

    // Decide. The ORDER of these rules is the entire policy: call the play if
    // one is wanted; otherwise go through the man if you can, over him if you
    // cannot, and round him if you can do neither.
    let matched = AgentApi::REASON_MATCHED_RULE;
    let mut brain = AgentApi::scripted_brain(vec![
        AgentApi::script_rule(
            FACT_AWAITING_CALL,
            AgentApi::press_control_intent(CONTROL_CALL_PLAY),
            matched,
        ),
        AgentApi::script_rule(
            FACT_RUN_THROUGH,
            AgentApi::press_control_intent(CONTROL_SHOULDER),
            matched,
        ),
        AgentApi::script_rule(
            FACT_GO_OVER,
            AgentApi::press_control_intent(CONTROL_JUMP),
            matched,
        ),
        AgentApi::script_rule(
            FACT_CUT_LEFT,
            AgentApi::press_control_intent(CONTROL_JUKE_LEFT),
            matched,
        ),
        AgentApi::script_rule(
            FACT_CUT_RIGHT,
            AgentApi::press_control_intent(CONTROL_JUKE_RIGHT),
            matched,
        ),
    ]);
    let mut memory = AgentApi::empty_memory(1);
    let runtime_step = RuntimeStep::new(
        FrameIndex::new(tick),
        Tick::new(tick),
        FIXED_DELTA_NANOS,
        0,
    );
    let (report, queue) = AgentApi::step(
        agent_id,
        profile,
        &mut brain,
        &observation,
        &mut memory,
        runtime_step,
    );

    // Lower. A neutral control code becomes the exact input a human produces —
    // the play call, or one of the four moves. This is the only place a
    // decision reaches the game.
    let control = queue.combined_control_code();
    AgentDecision {
        call_play: (control & bit(CONTROL_CALL_PLAY) != 0)
            .then_some(crate::autopilot::AUTOPILOT_CONCEPT),
        wanted: lower(control),
        reason_code: report.reason_code(),
        emitted: report.emitted_action_count(),
        perception: seen,
    }
}

/// The bit a control code occupies in the queue's folded bitmask.
fn bit(control: u32) -> u32 {
    1u32 << control
}

/// Turn the queue's folded control bitmask into the game's move vocabulary.
fn lower(control: u32) -> Option<RunbackMove> {
    [
        (CONTROL_SHOULDER, RunbackMove::Shoulder),
        (CONTROL_JUMP, RunbackMove::Jump),
        (CONTROL_JUKE_LEFT, RunbackMove::JukeLeft),
        (CONTROL_JUKE_RIGHT, RunbackMove::JukeRight),
    ]
    .into_iter()
    .find(|(code, _)| control & bit(*code) != 0)
    .map(|(_, wanted)| wanted)
}

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
            (pos.x / crate::field::FIELD_WIDTH) + 0.5,
            (crate::field::z_to_yards_from_own_goal(pos.z, sim.frame.direction) / 100.0)
                .clamp(-0.2, 1.2),
        ),
        yards_to_goal: crate::field::GOAL_LINE_Z - pos.z * sign,
        speed: runner.map(|p| p.speed()).unwrap_or(0.0),
        velocity: (vel.x, vel.z),
        action: step.runback.active.map(|m| m.label()),
        airborne: step.runback.airborne,
        height: step.runback.height,
        jump_available: step.runback.jump_available,
        jump_cooldown_left: step.runback.jump_cooldown_left,
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
