//! The soccer-penalty **agent driver** — playing the deterministic session
//! through `axiom-agent`'s [`AgentApi::step`]. Native-only and gated behind the
//! `agent` feature, so the default workspace gates never compile it (mirrors the
//! growth agent).
//!
//! There is no hand-rolled decision logic. Every tick the driver:
//! 1. **observes** the current session/shot situation into a single
//!    reflex-selecting fact (aim toward the planned target, charge to the planned
//!    power, release, or continue),
//! 2. lets a `ScriptedBrain` **decide** through `axiom-agent` (fact kind → neutral
//!    `ActionIntent`), and
//! 3. **lowers** that neutral intent into a [`PenaltyInputIntent`] and advances
//!    the session.
//! The driver is the soccer-specific ends of the translation (session state →
//! neutral fact, neutral controls → penalty input) — exactly where the Module
//! Law keeps it. The brain and every internal agent type live inside
//! `axiom-agent` and are built fresh per decision (never named here), so the
//! module's one-facade rule is honored.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;

use crate::soccer_penalty::penalty_input::PenaltyInputIntent;
use crate::soccer_penalty::penalty_interaction::PenaltyShotFlightState;
use crate::soccer_penalty::penalty_session::{PenaltyLoopState, PenaltySessionState};

// Neutral control / axis codes the brain speaks (lowered to PenaltyInputIntent).
const AXIS_AIM_X: u32 = 1;
const AXIS_AIM_Y: u32 = 2;
const CTRL_CHARGE: u32 = 0b0001;
const CTRL_RELEASE: u32 = 0b0010;
const CTRL_CONTINUE: u32 = 0b0100;

// ActionIntent kind codes (mirrors axiom-agent's `action_intent.rs`).
const KIND_PRESS_CONTROL: u16 = 2;
const KIND_MOVE_AXIS: u16 = 4;

// Observation fact kinds — the reflex the ScriptedBrain maps to an intent.
const FACT_AIM_XP: u16 = 1;
const FACT_AIM_XN: u16 = 2;
const FACT_AIM_YP: u16 = 3;
const FACT_AIM_YN: u16 = 4;
const FACT_CHARGE: u16 = 5;
const FACT_RELEASE: u16 = 6;
const FACT_CONTINUE: u16 = 7;
const FACT_WAIT: u16 = 8;

/// The agent's stable id ("soccer" in ASCII) — deterministic, like everything else.
const AGENT_RAW_ID: u64 = 0x73_6f_63_63_65_72;

/// The per-round shot the agent aims for: a normalized target + a charge power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyShotPlan {
    pub target_x: i32,
    pub target_y: i32,
    pub power: i32,
}

impl PenaltyShotPlan {
    /// A clean scoring shot: mid-right, well inside the mouth, that clears the
    /// diving keeper (aim right 7, charge 8).
    pub const fn scoring() -> Self {
        Self { target_x: 56, target_y: 50, power: 64 }
    }
}

/// A deterministic soccer-penalty agent. It steers the reticle to a planned
/// target, charges to a planned power, releases, watches the result, and
/// continues — every per-tick control routed through `AgentApi::step`.
#[derive(Debug, Clone)]
pub struct PenaltyAgent {
    plans: Vec<PenaltyShotPlan>,
}

impl PenaltyAgent {
    /// An agent that plays every round with one plan.
    pub fn new(plan: PenaltyShotPlan) -> Self {
        Self { plans: vec![plan] }
    }

    /// An agent that cycles a fixed list of plans (one per round).
    pub fn with_plans(plans: Vec<PenaltyShotPlan>) -> Self {
        Self { plans }
    }

    fn plan(&self, session: &PenaltySessionState) -> PenaltyShotPlan {
        self.plans[(session.round_index as usize) % self.plans.len().max(1)]
    }

    /// Decide the input for one tick — the whole decision flows through
    /// `AgentApi::step`.
    pub fn decide(&self, session: &PenaltySessionState) -> PenaltyInputIntent {
        decide_intent(self.observe(session), session.shot.tick as u64)
    }

    /// Observe the current situation into a single reflex-selecting fact kind.
    fn observe(&self, session: &PenaltySessionState) -> u16 {
        match session.loop_state {
            PenaltyLoopState::SessionComplete => FACT_WAIT,
            PenaltyLoopState::BetweenRounds | PenaltyLoopState::RoundAwarded => FACT_CONTINUE,
            _ => self.observe_shot(session),
        }
    }

    fn observe_shot(&self, session: &PenaltySessionState) -> u16 {
        let plan = self.plan(session);
        let shot = &session.shot;
        match shot.state {
            PenaltyShotFlightState::Aiming | PenaltyShotFlightState::Charging => {
                if shot.aim.target_x < plan.target_x {
                    FACT_AIM_XP
                } else if shot.aim.target_x > plan.target_x {
                    FACT_AIM_XN
                } else if shot.aim.target_y < plan.target_y {
                    FACT_AIM_YP
                } else if shot.aim.target_y > plan.target_y {
                    FACT_AIM_YN
                } else if shot.power.power < plan.power {
                    FACT_CHARGE
                } else {
                    FACT_RELEASE
                }
            }
            // Ball in flight / resolving: wait and watch.
            _ => FACT_WAIT,
        }
    }

    /// Play a whole session to `SessionComplete` (with a safety cap), returning
    /// the final state and the number of ticks driven.
    pub fn play(&self, mut session: PenaltySessionState) -> (PenaltySessionState, u32) {
        let mut ticks = 0;
        while session.loop_state != PenaltyLoopState::SessionComplete && ticks < 4000 {
            let intent = self.decide(&session);
            session = session.advance(intent);
            ticks += 1;
        }
        (session, ticks)
    }
}

/// Run the `ScriptedBrain` for one observed fact through `AgentApi::step` and
/// lower the emitted neutral intent into a [`PenaltyInputIntent`].
fn decide_intent(fact_kind: u16, tick: u64) -> PenaltyInputIntent {
    let agent_id = AgentApi::create_agent_id(AGENT_RAW_ID);
    let profile = AgentApi::debug_perfect_profile();
    // The reflex table: observed fact kind → neutral action intent.
    let mut brain = AgentApi::scripted_brain(vec![
        AgentApi::script_rule(FACT_AIM_XP, AgentApi::move_axis_intent(AXIS_AIM_X, 100), FACT_AIM_XP),
        AgentApi::script_rule(FACT_AIM_XN, AgentApi::move_axis_intent(AXIS_AIM_X, -100), FACT_AIM_XN),
        AgentApi::script_rule(FACT_AIM_YP, AgentApi::move_axis_intent(AXIS_AIM_Y, 100), FACT_AIM_YP),
        AgentApi::script_rule(FACT_AIM_YN, AgentApi::move_axis_intent(AXIS_AIM_Y, -100), FACT_AIM_YN),
        AgentApi::script_rule(FACT_CHARGE, AgentApi::press_control_intent(CTRL_CHARGE), FACT_CHARGE),
        AgentApi::script_rule(FACT_RELEASE, AgentApi::press_control_intent(CTRL_RELEASE), FACT_RELEASE),
        AgentApi::script_rule(FACT_CONTINUE, AgentApi::press_control_intent(CTRL_CONTINUE), FACT_CONTINUE),
        AgentApi::script_rule(FACT_WAIT, AgentApi::noop_intent(), FACT_WAIT),
    ]);
    let mut memory = AgentApi::empty_memory(1);

    let mut builder = AgentApi::observation_builder(agent_id, Tick::new(tick), 1, 1, 0);
    let _ = builder.add_channel(AgentApi::channel_semantic());
    let _ = builder.add_fact(AgentApi::observation_fact(fact_kind, 0, 0, 0, 0, 0));
    let observation = builder.build();

    let step = RuntimeStep::new(FrameIndex::new(tick), Tick::new(tick), 16_666_667, 0);
    let (_report, queue) = AgentApi::step(agent_id, profile, &mut brain, &observation, &mut memory, step);

    let mut out = PenaltyInputIntent::NEUTRAL;
    queue.intents().iter().for_each(|intent| {
        let kind = intent.kind_code();
        (kind == KIND_MOVE_AXIS).then(|| {
            let value = intent.value() as i32;
            (intent.axis_code() == AXIS_AIM_X).then(|| out.aim_x_axis = value);
            (intent.axis_code() == AXIS_AIM_Y).then(|| out.aim_y_axis = value);
        });
        (kind == KIND_PRESS_CONTROL).then(|| {
            let control = intent.control_code();
            (control == CTRL_CHARGE).then(|| out.charge_pressed = true);
            (control == CTRL_RELEASE).then(|| out.release_pressed = true);
            (control == CTRL_CONTINUE).then(|| out.continue_pressed = true);
        });
    });
    out
}
