//! Driving one agent tick: the reaction buffer, and the decision cycle.
//!
//! Split out of [`super::agent`], which owns the control vocabulary and the
//! perception read, so each file stays narrowly owned. Pure relocation.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;
use super::agent::{
    AGENT_RAW_ID, CONTROL_CALL_PLAY, CONTROL_JUKE_LEFT, CONTROL_JUKE_RIGHT, CONTROL_JUMP,
    CONTROL_SHOULDER, FACT_AWAITING_CALL, FACT_CUT_LEFT, FACT_CUT_RIGHT, FACT_GO_OVER,
    FACT_RUN_THROUGH, FIXED_DELTA_NANOS, MICRO,
};
use super::agent::{perceive, Perception};
use super::agent_report::observe;
use super::agent_report::{AgentDecision, AgentObservation};
use crate::showcase::ShowcaseRun;
use crate::attempt::AttemptStep;
use crate::autopilot::{self, Aggression};
use crate::identity::PlayerId;
use crate::runback::RunbackMove;
use crate::state::SimState;

/// How much sight the delay line remembers, in ticks.
///
/// Four seconds at the fixed step. Comfortably longer than any reaction time
/// worth simulating, so the clamp inside the buffer is a safety net rather than
/// something a normal run ever touches.
const REACTION_MEMORY_TICKS: usize = 240;

/// The default reaction time, in milliseconds.
///
/// A human on a visual cue is somewhere around 200–300 ms to *begin* moving, and
/// meaningfully slower when the cue has to be recognised and chosen between
/// rather than merely noticed — which is what this agent does every tick. 500 ms
/// is a deliberately unhurried player: it is the setting that makes the agent's
/// runs comparable to a person's, and it is why every window in this game is
/// tuned to be longer than a flicker.
pub const DEFAULT_REACTION_MILLIS: u32 = 500;

/// What one tick of the agent did, handed to the caller's observer.
#[derive(Debug)]
pub struct TickReport<'a> {
    pub observation: &'a AgentObservation,
    /// The encounter read this tick, with its projection and the charge it
    /// predicts — the raw material behind every decision, published so a trace
    /// can show *why* a move was or was not chosen instead of only *that* it
    /// was not.
    pub encounter: Option<crate::runback::Encounter>,
    pub decision: AgentDecision,
    pub events: &'a [crate::events::StampedEvent],
    pub ledger: crate::attempt::AttemptLedger,
    pub phase: crate::attempt::AttemptPhase,
}

/// **Play End Zone.** Drives `run` through the agent for up to `carries`
/// carries, calling `observer` once per tick.
///
/// # Why the loop lives here and not in the binary
///
/// Reaction latency is *history*, so it needs a delay line that survives across
/// ticks — and `axiom-agent` seals every contract type behind `AgentApi`, which
/// means the buffer can be held in a local but can never be named in a struct
/// field or a function signature outside the module. That is the module boundary
/// working exactly as designed, and it has one consequence: the tick loop and
/// the delay line must be in the same scope. So the loop is here, where the
/// agent is, and the binary keeps what a binary should — argument parsing and
/// printing.
///
/// # The latency
///
/// Every tick the agent *perceives* the current observation and then *decides on
/// the one it could see `reaction_millis` ago*. It is a delay line, not a filter:
/// nothing is smoothed, dropped or invented, so the agent acts on a true picture
/// of the world, just an old one. Everything human about that falls out for
/// free — it commits to threats that have already moved, and it misses windows
/// that opened and closed inside its own latency.
pub fn drive(
    run: &mut ShowcaseRun,
    policy: Aggression,
    reaction_millis: u32,
    carries: u32,
    observer: &mut dyn FnMut(TickReport<'_>),
) {
    let agent_id = AgentApi::create_agent_id(AGENT_RAW_ID);
    let profile = AgentApi::profile_with_reaction_millis(
        AgentApi::debug_perfect_profile(),
        reaction_millis,
        FIXED_DELTA_NANOS,
    );
    let latency = AgentApi::reaction_ticks_for_millis(reaction_millis, FIXED_DELTA_NANOS);
    let mut reaction =
        AgentApi::reaction_buffer(agent_id, Tick::new(0), REACTION_MEMORY_TICKS);
    let mut memory = AgentApi::empty_memory(1);
    // The decision table, in priority order — the whole policy, as data. The
    // ORDER is the policy: call the play if one is wanted; otherwise go through
    // the man if you can, over him if you cannot, and round him if you can do
    // neither. The module evaluates it; nothing in this file decides anything.
    // Written inline because `ScriptRule` is sealed behind the facade.
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
    let mut resolved = 0u32;

    for _ in 0..(u64::from(carries) * 900) {
        let Some(step) = run.attempt() else { break };
        let tick = run.sim.tick;
        let seen = observe(&run.sim, &step);
        let perception = perceive(&run.sim, &step, policy, latency);

        // Perceive now — the observation is built inline because every type in
        // it is sealed behind the facade and so cannot be a helper's return
        // type. A fact is present only when there is something to perceive,
        // which is what lets the priority live in the rule ORDER rather than in
        // a comparison.
        let mut builder = AgentApi::observation_builder(agent_id, Tick::new(tick), 2, 5, 0);
        let _ = builder.add_channel(AgentApi::channel_geometric());
        let _ = builder.add_channel(AgentApi::channel_semantic());
        [
            perception.awaiting_call.then_some((FACT_AWAITING_CALL, 1.0)),
            perception.run_through.map(|v| (FACT_RUN_THROUGH, v)),
            perception.go_over.map(|v| (FACT_GO_OVER, v)),
            perception.cut_left.map(|v| (FACT_CUT_LEFT, v)),
            perception.cut_right.map(|v| (FACT_CUT_RIGHT, v)),
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
        AgentApi::perceive(&mut reaction, builder.build());
        // ...decide on what could be seen a reaction time ago.
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
            AgentApi::reacted(&reaction, profile),
            &mut memory,
            runtime_step,
        );
        let control = queue.combined_control_code();
        let decision = AgentDecision {
            call_play: (control & CONTROL_CALL_PLAY != 0)
                .then_some(crate::autopilot::AUTOPILOT_CONCEPT),
            wanted: lower(control),
            reason_code: report.reason_code(),
            emitted: report.emitted_action_count(),
            perception,
        };

        // Lower into the real controls a person uses, and nothing else.
        if let Some(play) = decision.call_play {
            run.select_concept(play);
        }
        if let Some(wanted) = decision.wanted {
            run.command(wanted);
        }

        let out = run.step(&[]);
        let ledger = run.ledger().unwrap_or_default();
        let encounter = autopilot::encounter(&run.sim, &step);
        observer(TickReport {
            observation: &seen,
            encounter,
            decision,
            events: &out.events,
            ledger,
            phase: step.phase,
        });

        if ledger.attempts > resolved {
            resolved = ledger.attempts;
            if resolved >= carries {
                break;
            }
        }
    }
}

/// Turn the queue's folded control bitmask into the game's move vocabulary.
///
/// The codes are distinct BIT FLAGS, not 1/2/3/4/5, because the queue folds a
/// tick's presses with a bitwise OR — it models "which controls are held", which
/// is a set. With sequential integers the codes alias (`3` is `1|2`), so a single
/// juke-right press reads back as "call the play AND juke left". Getting that
/// wrong is silent: the agent decides correctly, emits correctly, and the
/// lowering hands the game nonsense.
fn lower(control: u32) -> Option<RunbackMove> {
    [
        (CONTROL_SHOULDER, RunbackMove::Shoulder),
        (CONTROL_JUMP, RunbackMove::Jump),
        (CONTROL_JUKE_LEFT, RunbackMove::JukeLeft),
        (CONTROL_JUKE_RIGHT, RunbackMove::JukeRight),
    ]
    .into_iter()
    .find(|(code, _)| control & *code != 0)
    .map(|(_, wanted)| wanted)
}
