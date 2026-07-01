//! A deterministic replay brain.

use crate::action_intent::ActionIntent;
use crate::agent_brain::{AgentBrain, BrainDecision};
use crate::agent_id::AgentId;
use crate::agent_memory::AgentMemory;
use crate::agent_profile::AgentProfile;
use crate::decision_report::DecisionReport;
use crate::observation::Observation;

/// A brain that replays a pre-recorded sequence of intents, one per step.
///
/// It ignores the observation entirely: step `n` emits the `n`-th recorded
/// intent. An empty recording, or a step past the end of the recording, emits a
/// single `Noop`. The cursor advances by one each step (saturating), so the
/// emitted sequence is fully deterministic.
#[derive(Debug, Clone)]
pub struct ReplayBrain {
    recorded: Vec<ActionIntent>,
    cursor: usize,
}

impl ReplayBrain {
    /// A replay brain over `recorded`, starting at the first intent.
    pub fn new(recorded: Vec<ActionIntent>) -> Self {
        ReplayBrain { recorded, cursor: 0 }
    }
}

impl AgentBrain for ReplayBrain {
    const KIND_CODE: u16 = DecisionReport::BRAIN_KIND_REPLAY;

    fn decide(
        &mut self,
        _agent_id: AgentId,
        _profile: AgentProfile,
        _observation: &Observation,
        _memory: &AgentMemory,
    ) -> BrainDecision {
        let next = self.recorded.get(self.cursor).copied();
        let recorded_is_empty = self.recorded.is_empty();
        self.cursor = self.cursor.saturating_add(1);
        let has = next.is_some();
        // Reason precedence: an emitted intent wins; otherwise exhaustion is
        // "empty" for a never-populated recording or "complete" past its end.
        let reason = [
            [
                DecisionReport::REASON_REPLAY_COMPLETE,
                DecisionReport::REASON_REPLAY_EMPTY,
            ][recorded_is_empty as usize],
            DecisionReport::REASON_REPLAY_EMITTED,
        ][has as usize];
        let emission = vec![next.unwrap_or_else(ActionIntent::noop)];
        BrainDecision::new(emission, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Tick;

    fn decide(brain: &mut ReplayBrain) -> BrainDecision {
        brain.decide(
            AgentId::from_raw(1),
            AgentProfile::debug_perfect(),
            &Observation::empty(AgentId::from_raw(1), Tick::new(0)),
            &AgentMemory::empty_with_capacity(1),
        )
    }

    #[test]
    fn empty_replay_emits_noop_with_replay_empty_reason() {
        let mut brain = ReplayBrain::new(Vec::new());
        let d = decide(&mut brain);
        assert_eq!(d.intents().len(), 1);
        assert_eq!(d.intents()[0].kind_code(), ActionIntent::KIND_NOOP);
        assert_eq!(d.reason_code(), DecisionReport::REASON_REPLAY_EMPTY);
        let again = decide(&mut brain);
        assert_eq!(again.reason_code(), DecisionReport::REASON_REPLAY_EMPTY);
    }

    #[test]
    fn emits_recorded_actions_in_order() {
        let mut brain = ReplayBrain::new(vec![
            ActionIntent::press_control(1),
            ActionIntent::press_control(2),
        ]);
        let first = decide(&mut brain);
        let second = decide(&mut brain);
        assert_eq!(first.intents()[0].control_code(), 1);
        assert_eq!(first.reason_code(), DecisionReport::REASON_REPLAY_EMITTED);
        assert_eq!(second.intents()[0].control_code(), 2);
        assert_eq!(second.reason_code(), DecisionReport::REASON_REPLAY_EMITTED);
    }

    #[test]
    fn emits_noop_with_replay_complete_reason_after_completion() {
        let mut brain = ReplayBrain::new(vec![ActionIntent::press_control(1)]);
        let _consumed = decide(&mut brain);
        let after = decide(&mut brain);
        assert_eq!(after.intents()[0].kind_code(), ActionIntent::KIND_NOOP);
        assert_eq!(after.reason_code(), DecisionReport::REASON_REPLAY_COMPLETE);
    }

    #[test]
    fn derives_are_exercised() {
        let brain = ReplayBrain::new(vec![ActionIntent::noop()]);
        let cloned = brain.clone();
        assert!(format!("{brain:?}").contains("ReplayBrain"));
        assert_eq!(cloned.recorded.len(), 1);
    }
}
