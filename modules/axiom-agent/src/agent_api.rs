//! [`AgentApi`] — the module's single public facade.
//!
//! Every neutral agent contract is constructed and stepped through this one
//! type. The contract types it returns and accepts (ids, profiles, memory,
//! observations, intents, queues, brains, reports) are sealed in private
//! modules: a caller holds them as opaque values and only ever names `AgentApi`.

use axiom_runtime::RuntimeStep;

use crate::action_intent::ActionIntent;
use crate::action_queue::ActionQueue;
use crate::agent_brain::AgentBrain;
use crate::agent_id::AgentId;
use crate::agent_memory::AgentMemory;
use crate::agent_profile::AgentProfile;
use crate::agent_runtime::AgentRuntime;
use crate::decision_report::DecisionReport;
use crate::observation::{Observation, ObservationFact};
use crate::observation_builder::ObservationBuilder;
use crate::observation_channel::ObservationChannel;
use crate::scripted_brain::{ScriptRule, ScriptedBrain};
use crate::replay_brain::ReplayBrain;
use axiom_kernel::Tick;

/// The deterministic embodied-agent facade — the only public type in the module.
///
/// It is a stateless constructor and orchestrator: it builds the neutral
/// contracts and steps a brain once. It holds no world, no clock, and no global
/// state.
#[derive(Debug)]
pub struct AgentApi;

/// The canonical decision-report vocabulary, surfaced through the facade so a
/// consumer can interpret `DecisionReport::reason_code()` and
/// `selected_brain_kind_code()` symbolically instead of by magic number. These
/// alias the sealed `DecisionReport` constants; the report type itself stays
/// behind the facade.
impl AgentApi {
    /// No brain decided (the absent/default brain kind).
    pub const BRAIN_KIND_NONE: u16 = DecisionReport::BRAIN_KIND_NONE;
    /// The scripted brain decided.
    pub const BRAIN_KIND_SCRIPTED: u16 = DecisionReport::BRAIN_KIND_SCRIPTED;
    /// The replay brain decided.
    pub const BRAIN_KIND_REPLAY: u16 = DecisionReport::BRAIN_KIND_REPLAY;
    /// Unset / no reason recorded.
    pub const REASON_NO_REASON: u16 = DecisionReport::REASON_NO_REASON;
    /// No scripted rule matched.
    pub const REASON_NO_MATCHING_RULE: u16 = DecisionReport::REASON_NO_MATCHING_RULE;
    /// A scripted rule matched and its intent was selected.
    pub const REASON_MATCHED_RULE: u16 = DecisionReport::REASON_MATCHED_RULE;
    /// A replay step emitted a recorded intent.
    pub const REASON_REPLAY_EMITTED: u16 = DecisionReport::REASON_REPLAY_EMITTED;
    /// A replay step had an empty recording.
    pub const REASON_REPLAY_EMPTY: u16 = DecisionReport::REASON_REPLAY_EMPTY;
    /// A replay step ran past the end of a non-empty recording.
    pub const REASON_REPLAY_COMPLETE: u16 = DecisionReport::REASON_REPLAY_COMPLETE;
    /// The action budget was zero, so no action could be emitted.
    pub const REASON_ACTION_BUDGET_ZERO: u16 = DecisionReport::REASON_ACTION_BUDGET_ZERO;
}

impl AgentApi {
    // --- identity & profiles ---

    /// Create an agent id from a raw value.
    pub fn create_agent_id(raw: u64) -> AgentId {
        AgentId::from_raw(raw)
    }

    /// The flawless reference control profile.
    pub fn debug_perfect_profile() -> AgentProfile {
        AgentProfile::debug_perfect()
    }

    /// A plausible human-like default control profile.
    pub fn human_like_profile() -> AgentProfile {
        AgentProfile::human_like_default()
    }

    /// A copy of `profile` with its per-tick action budget overridden — used to
    /// throttle, or (at `0`) freeze, an agent without rebuilding its other
    /// control limits.
    pub fn profile_with_action_budget(profile: AgentProfile, max_actions_per_tick: u32) -> AgentProfile {
        profile.with_action_budget(max_actions_per_tick)
    }

    // --- memory ---

    /// An empty memory bounded to `capacity` entries.
    pub fn empty_memory(capacity: usize) -> AgentMemory {
        AgentMemory::empty_with_capacity(capacity)
    }

    // --- observation ---

    /// An empty observation for `agent_id` at `tick`.
    pub fn empty_observation(agent_id: AgentId, tick: Tick) -> Observation {
        Observation::empty(agent_id, tick)
    }

    /// A bounds-enforcing observation builder.
    pub fn observation_builder(
        agent_id: AgentId,
        tick: Tick,
        max_channels: usize,
        max_facts: usize,
        max_legal_actions: usize,
    ) -> ObservationBuilder {
        ObservationBuilder::new(agent_id, tick, max_channels, max_facts, max_legal_actions)
    }

    /// A neutral observation fact.
    pub fn observation_fact(
        kind_code: u16,
        subject_code: u32,
        x: i64,
        y: i64,
        z: i64,
        value: i64,
    ) -> ObservationFact {
        ObservationFact::new(kind_code, subject_code, x, y, z, value)
    }

    /// The `semantic` perception channel.
    pub fn channel_semantic() -> ObservationChannel {
        ObservationChannel::Semantic
    }

    /// The `geometric` perception channel.
    pub fn channel_geometric() -> ObservationChannel {
        ObservationChannel::Geometric
    }

    /// The `screen_sample` perception channel (a label for a future app/tool).
    pub fn channel_screen_sample() -> ObservationChannel {
        ObservationChannel::ScreenSample
    }

    /// The `replay` perception channel.
    pub fn channel_replay() -> ObservationChannel {
        ObservationChannel::Replay
    }

    /// The `debug` perception channel.
    pub fn channel_debug() -> ObservationChannel {
        ObservationChannel::Debug
    }
}

/// Action-intent constructors (low-level player-equivalent + high-level data).
impl AgentApi {
    /// A no-op intent.
    pub fn noop_intent() -> ActionIntent {
        ActionIntent::noop()
    }

    /// Hold position for `ticks` ticks.
    pub fn wait_ticks_intent(ticks: u32) -> ActionIntent {
        ActionIntent::wait_ticks(ticks)
    }

    /// Begin holding abstract control `control_code`.
    pub fn press_control_intent(control_code: u32) -> ActionIntent {
        ActionIntent::press_control(control_code)
    }

    /// Stop holding abstract control `control_code`.
    pub fn release_control_intent(control_code: u32) -> ActionIntent {
        ActionIntent::release_control(control_code)
    }

    /// Drive movement axis `axis_code` by `value`.
    pub fn move_axis_intent(axis_code: u32, value: i64) -> ActionIntent {
        ActionIntent::move_axis(axis_code, value)
    }

    /// Drive look axis `axis_code` by `value`.
    pub fn look_axis_intent(axis_code: u32, value: i64) -> ActionIntent {
        ActionIntent::look_axis(axis_code, value)
    }

    /// Move an abstract pointer to `(x, y)`.
    pub fn pointer_move_intent(x: i64, y: i64) -> ActionIntent {
        ActionIntent::pointer_move(x, y)
    }

    /// Begin a pointer contact with button `control_code`.
    pub fn pointer_down_intent(control_code: u32) -> ActionIntent {
        ActionIntent::pointer_down(control_code)
    }

    /// End a pointer contact with button `control_code`.
    pub fn pointer_up_intent(control_code: u32) -> ActionIntent {
        ActionIntent::pointer_up(control_code)
    }

    // --- action intents (high-level, data only) ---

    /// Orient toward subject `subject_code`.
    pub fn look_at_subject_intent(subject_code: u32) -> ActionIntent {
        ActionIntent::look_at_subject(subject_code)
    }

    /// Orient toward point `(x, y, z)`.
    pub fn look_at_point_intent(x: i64, y: i64, z: i64) -> ActionIntent {
        ActionIntent::look_at_point(x, y, z)
    }

    /// Move toward subject `subject_code`.
    pub fn move_toward_subject_intent(subject_code: u32) -> ActionIntent {
        ActionIntent::move_toward_subject(subject_code)
    }

    /// Move toward point `(x, y, z)`.
    pub fn move_toward_point_intent(x: i64, y: i64, z: i64) -> ActionIntent {
        ActionIntent::move_toward_point(x, y, z)
    }

    /// Interact with subject `subject_code`.
    pub fn interact_with_subject_intent(subject_code: u32) -> ActionIntent {
        ActionIntent::interact_with_subject(subject_code)
    }

    /// Use affordance `affordance_code`.
    pub fn use_affordance_intent(affordance_code: u32) -> ActionIntent {
        ActionIntent::use_affordance(affordance_code)
    }

    /// Focus an attention slot on subject `subject_code`.
    pub fn focus_attention_intent(subject_code: u32) -> ActionIntent {
        ActionIntent::focus_attention(subject_code)
    }
}

/// Queue, brain, and stepping construction.
impl AgentApi {
    /// An empty action queue bounded to `capacity` intents.
    pub fn action_queue(capacity: usize) -> ActionQueue {
        ActionQueue::empty_with_capacity(capacity)
    }

    // --- brains ---

    /// A scripted-brain rule: match facts of `fact_kind_code`, emit `intent`,
    /// and report `reason_code` when the rule fires.
    pub fn script_rule(fact_kind_code: u16, intent: ActionIntent, reason_code: u16) -> ScriptRule {
        ScriptRule::new(fact_kind_code, intent, reason_code)
    }

    /// A scripted brain evaluating `rules` in order.
    pub fn scripted_brain(rules: Vec<ScriptRule>) -> ScriptedBrain {
        ScriptedBrain::new(rules)
    }

    /// A replay brain over the recorded `intents`.
    pub fn replay_brain(intents: Vec<ActionIntent>) -> ReplayBrain {
        ReplayBrain::new(intents)
    }

    // --- stepping ---

    /// Step `brain` once: observe, decide, emit player-equivalent intents, and
    /// produce a deterministic decision report. The step's tick stamps the
    /// report and the recorded memory entry.
    pub fn step<B: AgentBrain>(
        agent_id: AgentId,
        profile: AgentProfile,
        brain: &mut B,
        observation: &Observation,
        memory: &mut AgentMemory,
        step: RuntimeStep,
    ) -> (DecisionReport, ActionQueue) {
        AgentRuntime::step(agent_id, profile, brain, observation, memory, step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_derive_is_exercised() {
        assert!(format!("{:?}", AgentApi).contains("AgentApi"));
    }
}
