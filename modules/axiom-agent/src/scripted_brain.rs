//! A tiny deterministic scripted brain.

use crate::action_intent::ActionIntent;
use crate::agent_brain::{AgentBrain, BrainDecision};
use crate::agent_id::AgentId;
use crate::agent_memory::AgentMemory;
use crate::agent_profile::AgentProfile;
use crate::decision_report::DecisionReport;
use crate::observation::Observation;

/// One scripted rule: if the observation carries a fact of `fact_kind_code`,
/// emit `intent` and report `reason_code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptRule {
    fact_kind_code: u16,
    intent: ActionIntent,
    reason_code: u16,
}

impl ScriptRule {
    /// A rule matching facts of `fact_kind_code`, emitting `intent`, and
    /// reporting `reason_code` when it fires (conventionally
    /// [`crate::decision_report::DecisionReport::REASON_MATCHED_RULE`]).
    pub const fn new(fact_kind_code: u16, intent: ActionIntent, reason_code: u16) -> Self {
        ScriptRule {
            fact_kind_code,
            intent,
            reason_code,
        }
    }

    /// The fact kind this rule matches.
    pub const fn fact_kind_code(self) -> u16 {
        self.fact_kind_code
    }

    /// The intent this rule emits.
    pub const fn intent(self) -> ActionIntent {
        self.intent
    }

    /// The reason code this rule reports when it fires.
    pub const fn reason_code(self) -> u16 {
        self.reason_code
    }
}

/// A deterministic brain driven by an ordered list of rules.
///
/// It is not a scripting language: it is a fixed table. The **first** rule whose
/// fact kind appears in the observation wins and its intent is emitted (clamped
/// to the profile's `max_actions_per_tick`). If no rule matches, the brain emits
/// a single `Noop`.
#[derive(Debug, Clone)]
pub struct ScriptedBrain {
    rules: Vec<ScriptRule>,
}

impl ScriptedBrain {
    /// A scripted brain evaluating `rules` in order.
    pub fn new(rules: Vec<ScriptRule>) -> Self {
        ScriptedBrain { rules }
    }
}

impl AgentBrain for ScriptedBrain {
    const KIND_CODE: u16 = DecisionReport::BRAIN_KIND_SCRIPTED;

    fn decide(
        &mut self,
        _agent_id: AgentId,
        profile: AgentProfile,
        observation: &Observation,
        _memory: &AgentMemory,
    ) -> BrainDecision {
        let matched = self
            .rules
            .iter()
            .find(|rule| {
                observation
                    .first_fact_with_kind(rule.fact_kind_code())
                    .is_some()
            })
            .copied();
        let has_match = matched.is_some();
        let max = profile.max_actions_per_tick() as usize;
        let budget_zero = max == 0;
        // Reason precedence: a zero budget overrides everything; otherwise the
        // matched rule's own reason (already defaulting to "no matching rule").
        let rule_reason = matched
            .map(|rule| rule.reason_code())
            .unwrap_or(DecisionReport::REASON_NO_MATCHING_RULE);
        let reason = [rule_reason, DecisionReport::REASON_ACTION_BUDGET_ZERO][budget_zero as usize];
        let fallback = ((!has_match) & (!budget_zero)).then(ActionIntent::noop);
        let emission: Vec<ActionIntent> = matched
            .map(|rule| rule.intent())
            .into_iter()
            .take(max)
            .chain(fallback)
            .collect();
        BrainDecision::new(emission, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Tick;

    fn observation_with_fact(kind_code: u16) -> Observation {
        Observation::from_parts(
            AgentId::from_raw(1),
            Tick::new(0),
            Vec::new(),
            Vec::new(),
            vec![crate::observation::ObservationFact::new(
                kind_code, 1, 0, 0, 0, 0,
            )],
        )
    }

    fn empty_observation() -> Observation {
        Observation::empty(AgentId::from_raw(1), Tick::new(0))
    }

    fn decide(
        brain: &mut ScriptedBrain,
        observation: &Observation,
        profile: AgentProfile,
    ) -> BrainDecision {
        brain.decide(
            AgentId::from_raw(1),
            profile,
            observation,
            &AgentMemory::empty_with_capacity(1),
        )
    }

    const MATCHED: u16 = DecisionReport::REASON_MATCHED_RULE;

    #[test]
    fn empty_brain_emits_noop_with_no_matching_rule_reason() {
        let mut brain = ScriptedBrain::new(Vec::new());
        let d = decide(
            &mut brain,
            &empty_observation(),
            AgentProfile::debug_perfect(),
        );
        assert_eq!(d.intents().len(), 1);
        assert_eq!(d.intents()[0].kind_code(), ActionIntent::KIND_NOOP);
        assert_eq!(d.reason_code(), DecisionReport::REASON_NO_MATCHING_RULE);
    }

    #[test]
    fn matching_fact_emits_configured_intent_and_rule_reason() {
        let mut brain = ScriptedBrain::new(vec![ScriptRule::new(
            100,
            ActionIntent::press_control(7),
            MATCHED,
        )]);
        let d = decide(
            &mut brain,
            &observation_with_fact(100),
            AgentProfile::debug_perfect(),
        );
        assert_eq!(d.intents().len(), 1);
        assert_eq!(d.intents()[0].kind_code(), ActionIntent::KIND_PRESS_CONTROL);
        assert_eq!(d.intents()[0].control_code(), 7);
        assert_eq!(d.reason_code(), DecisionReport::REASON_MATCHED_RULE);
    }

    #[test]
    fn a_rules_custom_reason_code_propagates_to_the_decision() {
        let mut brain = ScriptedBrain::new(vec![ScriptRule::new(
            100,
            ActionIntent::press_control(7),
            42,
        )]);
        let d = decide(
            &mut brain,
            &observation_with_fact(100),
            AgentProfile::debug_perfect(),
        );
        assert_eq!(d.reason_code(), 42);
    }

    #[test]
    fn first_matching_rule_wins() {
        let mut brain = ScriptedBrain::new(vec![
            ScriptRule::new(100, ActionIntent::press_control(1), MATCHED),
            ScriptRule::new(100, ActionIntent::press_control(2), MATCHED),
        ]);
        let d = decide(
            &mut brain,
            &observation_with_fact(100),
            AgentProfile::debug_perfect(),
        );
        assert_eq!(
            d.intents()[0].control_code(),
            1,
            "first rule in order must win"
        );
    }

    #[test]
    fn non_matching_observation_falls_back_to_noop() {
        let mut brain = ScriptedBrain::new(vec![ScriptRule::new(
            100,
            ActionIntent::press_control(1),
            MATCHED,
        )]);
        let d = decide(
            &mut brain,
            &observation_with_fact(200),
            AgentProfile::debug_perfect(),
        );
        assert_eq!(d.intents()[0].kind_code(), ActionIntent::KIND_NOOP);
        assert_eq!(d.reason_code(), DecisionReport::REASON_NO_MATCHING_RULE);
    }

    #[test]
    fn emitted_count_never_exceeds_the_action_budget() {
        let mut brain = ScriptedBrain::new(vec![ScriptRule::new(
            100,
            ActionIntent::press_control(1),
            MATCHED,
        )]);
        for profile in [
            AgentProfile::debug_perfect(),
            AgentProfile::human_like_default(),
        ] {
            let d = decide(&mut brain, &observation_with_fact(100), profile);
            assert_eq!(d.intents().len(), 1, "a single matched action is emitted");
            assert!(
                d.intents().len() <= profile.max_actions_per_tick() as usize,
                "emitted count must respect max_actions_per_tick",
            );
        }
    }

    #[test]
    fn zero_action_budget_emits_nothing_with_budget_zero_reason() {
        let frozen = AgentProfile::debug_perfect().with_action_budget(0);
        let mut matching = ScriptedBrain::new(vec![ScriptRule::new(
            100,
            ActionIntent::press_control(1),
            MATCHED,
        )]);
        let d = decide(&mut matching, &observation_with_fact(100), frozen);
        assert_eq!(d.intents().len(), 0, "a zero budget emits no action");
        assert_eq!(d.reason_code(), DecisionReport::REASON_ACTION_BUDGET_ZERO);
        let mut nonmatching = ScriptedBrain::new(vec![ScriptRule::new(
            100,
            ActionIntent::press_control(1),
            MATCHED,
        )]);
        let d2 = decide(&mut nonmatching, &observation_with_fact(200), frozen);
        assert_eq!(d2.intents().len(), 0);
        assert_eq!(d2.reason_code(), DecisionReport::REASON_ACTION_BUDGET_ZERO);
    }

    #[test]
    fn rule_accessors_round_trip() {
        let rule = ScriptRule::new(42, ActionIntent::wait_ticks(3), 7);
        assert_eq!(rule.fact_kind_code(), 42);
        assert_eq!(rule.intent().ticks(), 3);
        assert_eq!(rule.reason_code(), 7);
    }

    #[test]
    fn derives_are_exercised() {
        let rule = ScriptRule::new(1, ActionIntent::noop(), MATCHED);
        let copy = rule;
        assert_eq!(rule, copy);
        assert_ne!(rule, ScriptRule::new(2, ActionIntent::noop(), MATCHED));
        assert!(format!("{rule:?}").contains("ScriptRule"));
        let brain = ScriptedBrain::new(vec![rule]);
        let cloned = brain.clone();
        assert!(format!("{brain:?}").contains("ScriptedBrain"));
        assert_eq!(cloned.rules.len(), 1);
    }
}
