//! The small orchestrator that steps one agent once.

use axiom_runtime::RuntimeStep;

use crate::action_intent::ActionIntent;
use crate::action_queue::ActionQueue;
use crate::agent_brain::AgentBrain;
use crate::agent_id::AgentId;
use crate::agent_memory::{AgentMemory, MemoryEntry};
use crate::agent_profile::AgentProfile;
use crate::decision_report::DecisionReport;
use crate::observation::Observation;

/// Steps a single agent through one `observe -> decide -> emit -> report` cycle.
/// It is a stateless orchestrator (no game loop, no system registration): given
/// the agent's identity, profile, a brain, the current observation, mutable
/// memory, and the deterministic [`RuntimeStep`] driving the tick, it runs the
/// brain, records one deterministic memory entry, hands back the emitted intents
/// as an [`ActionQueue`], and produces a [`DecisionReport`]. The step is generic
/// over the brain, so dispatch is monomorphized — no dynamic dispatch.
#[derive(Debug)]
pub struct AgentRuntime;

impl AgentRuntime {
    /// The memory key code under which each step records its decision reason.
    pub const MEMORY_KEY_DECISION: u32 = 1;

    /// Step `brain` once for `agent_id` against `observation`, stamping the
    /// decision with the tick carried by `step` and appending one decision entry
    /// to `memory`. Returns the report and the emitted intents in order.
    pub fn step<B: AgentBrain>(
        agent_id: AgentId,
        profile: AgentProfile,
        brain: &mut B,
        observation: &Observation,
        memory: &mut AgentMemory,
        step: RuntimeStep,
    ) -> (DecisionReport, ActionQueue) {
        let (intents, reason_code) = brain
            .decide(agent_id, profile, observation, memory)
            .into_parts();
        let tick = step.tick();
        let emitted_action_count = intents.len();
        let first_emitted_action_kind_code = intents
            .first()
            .map(|intent| intent.kind_code())
            .unwrap_or(ActionIntent::KIND_NOOP);
        memory.remember(MemoryEntry::new(
            tick,
            Self::MEMORY_KEY_DECISION,
            reason_code as i64,
        ));
        let report = DecisionReport::new(
            agent_id,
            tick,
            observation.fact_count(),
            observation.legal_action_count(),
            B::KIND_CODE,
            emitted_action_count,
            first_emitted_action_kind_code,
            reason_code,
        );
        (report, ActionQueue::from_intents(intents))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::ObservationFact;
    use crate::replay_brain::ReplayBrain;
    use crate::scripted_brain::{ScriptRule, ScriptedBrain};
    use axiom_kernel::{FrameIndex, Tick};

    fn step_at(raw_tick: u64) -> RuntimeStep {
        RuntimeStep::new(FrameIndex::new(0), Tick::new(raw_tick), 16_666_667, 0)
    }

    fn observation_with_fact(kind_code: u16) -> Observation {
        Observation::from_parts(
            AgentId::from_raw(1),
            Tick::new(0),
            Vec::new(),
            vec![10, 20],
            vec![ObservationFact::new(kind_code, 1, 0, 0, 0, 0)],
        )
    }

    #[test]
    fn step_produces_a_report_and_queue() {
        let mut brain = ScriptedBrain::new(vec![ScriptRule::new(
            100,
            ActionIntent::press_control(7),
            DecisionReport::REASON_MATCHED_RULE,
        )]);
        let mut memory = AgentMemory::empty_with_capacity(4);
        let (report, queue) = AgentRuntime::step(
            AgentId::from_raw(1),
            AgentProfile::debug_perfect(),
            &mut brain,
            &observation_with_fact(100),
            &mut memory,
            step_at(5),
        );
        assert_eq!(report.tick(), Tick::new(5));
        assert_eq!(
            report.selected_brain_kind_code(),
            DecisionReport::BRAIN_KIND_SCRIPTED
        );
        assert_eq!(report.observation_fact_count(), 1);
        assert_eq!(report.legal_action_count(), 2);
        assert_eq!(report.emitted_action_count(), 1);
        assert_eq!(
            report.first_emitted_action_kind_code(),
            ActionIntent::KIND_PRESS_CONTROL
        );
        assert_eq!(report.reason_code(), DecisionReport::REASON_MATCHED_RULE);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.intents()[0].control_code(), 7);
        assert_eq!(memory.len(), 1);
        assert_eq!(memory.entries()[0].tick(), Tick::new(5));
        assert_eq!(
            memory.entries()[0].key_code(),
            AgentRuntime::MEMORY_KEY_DECISION
        );
        assert_eq!(
            memory.entries()[0].value_code(),
            DecisionReport::REASON_MATCHED_RULE as i64
        );
    }

    #[test]
    fn no_match_reports_noop_first_kind() {
        let mut brain = ScriptedBrain::new(vec![ScriptRule::new(
            100,
            ActionIntent::press_control(7),
            DecisionReport::REASON_MATCHED_RULE,
        )]);
        let mut memory = AgentMemory::empty_with_capacity(4);
        let (report, _queue) = AgentRuntime::step(
            AgentId::from_raw(1),
            AgentProfile::debug_perfect(),
            &mut brain,
            &observation_with_fact(200),
            &mut memory,
            step_at(1),
        );
        assert_eq!(
            report.first_emitted_action_kind_code(),
            ActionIntent::KIND_NOOP
        );
        assert_eq!(
            report.reason_code(),
            DecisionReport::REASON_NO_MATCHING_RULE
        );
    }

    #[test]
    fn zero_budget_step_reports_no_emission_and_budget_zero_reason() {
        let mut brain = ScriptedBrain::new(vec![ScriptRule::new(
            100,
            ActionIntent::press_control(7),
            DecisionReport::REASON_MATCHED_RULE,
        )]);
        let mut memory = AgentMemory::empty_with_capacity(4);
        let (report, queue) = AgentRuntime::step(
            AgentId::from_raw(1),
            AgentProfile::debug_perfect().with_action_budget(0),
            &mut brain,
            &observation_with_fact(100),
            &mut memory,
            step_at(9),
        );
        assert_eq!(report.emitted_action_count(), 0);
        assert_eq!(
            report.first_emitted_action_kind_code(),
            ActionIntent::KIND_NOOP
        );
        assert_eq!(
            report.reason_code(),
            DecisionReport::REASON_ACTION_BUDGET_ZERO
        );
        assert!(queue.is_empty());
        assert_eq!(
            memory.entries()[0].value_code(),
            DecisionReport::REASON_ACTION_BUDGET_ZERO as i64
        );
    }

    #[test]
    fn replay_brain_step_reports_replay_kind() {
        let mut brain = ReplayBrain::new(vec![ActionIntent::press_control(3)]);
        let mut memory = AgentMemory::empty_with_capacity(2);
        let (report, queue) = AgentRuntime::step(
            AgentId::from_raw(1),
            AgentProfile::debug_perfect(),
            &mut brain,
            &observation_with_fact(100),
            &mut memory,
            step_at(2),
        );
        assert_eq!(
            report.selected_brain_kind_code(),
            DecisionReport::BRAIN_KIND_REPLAY
        );
        assert_eq!(report.reason_code(), DecisionReport::REASON_REPLAY_EMITTED);
        assert_eq!(queue.intents()[0].control_code(), 3);
    }

    #[test]
    fn hold_set_brain_step_emits_multiple_intents_in_one_tick() {
        use crate::hold_set_brain::HoldSetBrain;
        let mut brain = HoldSetBrain::new(vec![0b0001, 0b0100]);
        let mut memory = AgentMemory::empty_with_capacity(2);
        let (report, queue) = AgentRuntime::step(
            AgentId::from_raw(1),
            AgentProfile::debug_perfect(),
            &mut brain,
            &observation_with_fact(100),
            &mut memory,
            step_at(3),
        );
        assert_eq!(
            report.selected_brain_kind_code(),
            DecisionReport::BRAIN_KIND_HOLD_SET
        );
        assert_eq!(
            report.emitted_action_count(),
            2,
            "two controls → two intents this tick"
        );
        assert_eq!(
            report.first_emitted_action_kind_code(),
            ActionIntent::KIND_PRESS_CONTROL
        );
        assert_eq!(
            report.reason_code(),
            DecisionReport::REASON_HOLD_SET_EMITTED
        );
        assert_eq!(queue.len(), 2);
        assert_eq!(
            queue.combined_control_code(),
            0b0101,
            "the two held controls combine"
        );
    }

    #[test]
    fn debug_derive_is_exercised() {
        assert!(format!("{:?}", AgentRuntime).contains("AgentRuntime"));
    }
}
