//! A deterministic brain that holds a *set* of controls each tick.

use crate::action_intent::ActionIntent;
use crate::agent_brain::{AgentBrain, BrainDecision};
use crate::agent_id::AgentId;
use crate::agent_memory::AgentMemory;
use crate::agent_profile::AgentProfile;
use crate::decision_report::DecisionReport;
use crate::observation::Observation;

/// A brain that emits **one `press_control` intent per held control, every
/// tick** — so a single decision carries several simultaneous controls (e.g.
/// forward + turn).
/// This is the substrate's producer of genuine *multi-intent* decisions. The
/// replay brain emits one recorded intent per step and the scripted brain emits
/// the first matching rule's intent; neither lets one tick express more than one
/// action. The hold-set brain does: the runtime queues every emitted intent, and
/// a consumer folds them back together with
/// [`crate::action_queue::ActionQueue::combined_control_code`].
/// Emissions are clamped to the profile's `max_actions_per_tick` exactly like the
/// scripted brain — a zero budget emits nothing, reported as
/// [`DecisionReport::REASON_ACTION_BUDGET_ZERO`].
#[derive(Debug, Clone)]
pub struct HoldSetBrain {
    controls: Vec<u32>,
}

impl HoldSetBrain {
    /// A brain that holds the abstract control codes `controls` every tick.
    pub fn new(controls: Vec<u32>) -> Self {
        HoldSetBrain { controls }
    }
}

impl AgentBrain for HoldSetBrain {
    const KIND_CODE: u16 = DecisionReport::BRAIN_KIND_HOLD_SET;

    fn decide(
        &mut self,
        _agent_id: AgentId,
        profile: AgentProfile,
        _observation: &Observation,
        _memory: &AgentMemory,
    ) -> BrainDecision {
        let max = profile.max_actions_per_tick() as usize;
        let budget_zero = max == 0;
        let emission: Vec<ActionIntent> = self
            .controls
            .iter()
            .map(|code| ActionIntent::press_control(*code))
            .take(max)
            .collect();
        let reason = [
            DecisionReport::REASON_HOLD_SET_EMITTED,
            DecisionReport::REASON_ACTION_BUDGET_ZERO,
        ][budget_zero as usize];
        BrainDecision::new(emission, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Tick;

    fn decide(brain: &mut HoldSetBrain, profile: AgentProfile) -> BrainDecision {
        brain.decide(
            AgentId::from_raw(1),
            profile,
            &Observation::empty(AgentId::from_raw(1), Tick::new(0)),
            &AgentMemory::empty_with_capacity(1),
        )
    }

    #[test]
    fn emits_one_press_control_intent_per_held_control() {
        let mut brain = HoldSetBrain::new(vec![0b0001, 0b0100]);
        let d = decide(&mut brain, AgentProfile::debug_perfect());
        assert_eq!(d.intents().len(), 2);
        assert_eq!(d.intents()[0].kind_code(), ActionIntent::KIND_PRESS_CONTROL);
        assert_eq!(d.intents()[0].control_code(), 0b0001);
        assert_eq!(d.intents()[1].control_code(), 0b0100);
        assert_eq!(d.reason_code(), DecisionReport::REASON_HOLD_SET_EMITTED);
    }

    #[test]
    fn the_action_budget_clamps_the_emission_count() {
        let mut brain = HoldSetBrain::new(vec![1, 2, 4]);
        let d = decide(
            &mut brain,
            AgentProfile::debug_perfect().with_action_budget(2),
        );
        assert_eq!(d.intents().len(), 2);
        assert_eq!(d.intents()[0].control_code(), 1);
        assert_eq!(d.intents()[1].control_code(), 2);
    }

    #[test]
    fn an_empty_held_set_emits_nothing_with_the_hold_set_reason() {
        let mut brain = HoldSetBrain::new(Vec::new());
        let d = decide(&mut brain, AgentProfile::debug_perfect());
        assert_eq!(d.intents().len(), 0);
        assert_eq!(d.reason_code(), DecisionReport::REASON_HOLD_SET_EMITTED);
    }

    #[test]
    fn a_zero_budget_emits_nothing_with_budget_zero_reason() {
        let mut brain = HoldSetBrain::new(vec![1, 2]);
        let d = decide(
            &mut brain,
            AgentProfile::debug_perfect().with_action_budget(0),
        );
        assert_eq!(d.intents().len(), 0);
        assert_eq!(d.reason_code(), DecisionReport::REASON_ACTION_BUDGET_ZERO);
    }

    #[test]
    fn derives_are_exercised() {
        let brain = HoldSetBrain::new(vec![1]);
        let cloned = brain.clone();
        assert_eq!(cloned.controls.len(), 1);
        assert!(format!("{brain:?}").contains("HoldSetBrain"));
    }
}
