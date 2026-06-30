//! A deterministic record of one agent decision.

use axiom_kernel::Tick;

use crate::agent_id::AgentId;

/// The replayable record of a single `observe -> decide -> emit` step.
///
/// Its identity is entirely numeric: codes and counts, never human strings. Two
/// reports built from the same inputs compare equal, so a decision is a stable,
/// inspectable artifact. The `reason_code` explains *why* the decision came out
/// as it did, drawn from the `REASON_*` constants below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionReport {
    agent_id: AgentId,
    tick: Tick,
    observation_fact_count: usize,
    legal_action_count: usize,
    selected_brain_kind_code: u16,
    emitted_action_count: usize,
    first_emitted_action_kind_code: u16,
    reason_code: u16,
}

impl DecisionReport {
    /// No brain decided (the absent/default brain kind).
    pub const BRAIN_KIND_NONE: u16 = 0;
    /// The scripted brain decided.
    pub const BRAIN_KIND_SCRIPTED: u16 = 1;
    /// The replay brain decided.
    pub const BRAIN_KIND_REPLAY: u16 = 2;
    /// The hold-set brain decided (emits one press-control intent per held
    /// control, so one decision carries several simultaneous controls).
    pub const BRAIN_KIND_HOLD_SET: u16 = 3;

    /// Unset / no reason recorded.
    pub const REASON_NO_REASON: u16 = 0;
    /// No scripted rule matched; the brain fell back to a no-op.
    pub const REASON_NO_MATCHING_RULE: u16 = 1;
    /// A scripted rule matched and its intent was selected.
    pub const REASON_MATCHED_RULE: u16 = 2;
    /// A replay step emitted a recorded intent.
    pub const REASON_REPLAY_EMITTED: u16 = 3;
    /// A replay step had an empty recording.
    pub const REASON_REPLAY_EMPTY: u16 = 4;
    /// A replay step ran past the end of a non-empty recording.
    pub const REASON_REPLAY_COMPLETE: u16 = 5;
    /// The action budget was zero, so no action could be emitted.
    pub const REASON_ACTION_BUDGET_ZERO: u16 = 6;
    /// A hold-set step emitted one press-control intent per held control.
    pub const REASON_HOLD_SET_EMITTED: u16 = 7;

    /// Assemble a report. Built only by the agent runtime, from a completed
    /// decision.
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        agent_id: AgentId,
        tick: Tick,
        observation_fact_count: usize,
        legal_action_count: usize,
        selected_brain_kind_code: u16,
        emitted_action_count: usize,
        first_emitted_action_kind_code: u16,
        reason_code: u16,
    ) -> Self {
        DecisionReport {
            agent_id,
            tick,
            observation_fact_count,
            legal_action_count,
            selected_brain_kind_code,
            emitted_action_count,
            first_emitted_action_kind_code,
            reason_code,
        }
    }

    /// The agent this decision belongs to.
    pub const fn agent_id(self) -> AgentId {
        self.agent_id
    }

    /// The tick the decision was made at.
    pub const fn tick(self) -> Tick {
        self.tick
    }

    /// How many facts the observation carried.
    pub const fn observation_fact_count(self) -> usize {
        self.observation_fact_count
    }

    /// How many actions were legal for the agent.
    pub const fn legal_action_count(self) -> usize {
        self.legal_action_count
    }

    /// The kind code of the brain that decided.
    pub const fn selected_brain_kind_code(self) -> u16 {
        self.selected_brain_kind_code
    }

    /// How many actions the decision emitted.
    pub const fn emitted_action_count(self) -> usize {
        self.emitted_action_count
    }

    /// The kind of the first emitted action (or `noop` if none was emitted).
    pub const fn first_emitted_action_kind_code(self) -> u16 {
        self.first_emitted_action_kind_code
    }

    /// Why the decision came out as it did.
    pub const fn reason_code(self) -> u16 {
        self.reason_code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_intent::ActionIntent;

    fn report() -> DecisionReport {
        DecisionReport::new(
            AgentId::from_raw(1),
            Tick::new(7),
            3,
            2,
            DecisionReport::BRAIN_KIND_SCRIPTED,
            1,
            ActionIntent::KIND_PRESS_CONTROL,
            DecisionReport::REASON_MATCHED_RULE,
        )
    }

    #[test]
    fn accessors_return_constructed_parts() {
        let r = report();
        assert_eq!(r.agent_id(), AgentId::from_raw(1));
        assert_eq!(r.tick(), Tick::new(7));
        assert_eq!(r.observation_fact_count(), 3);
        assert_eq!(r.legal_action_count(), 2);
        assert_eq!(r.selected_brain_kind_code(), DecisionReport::BRAIN_KIND_SCRIPTED);
        assert_eq!(r.emitted_action_count(), 1);
        assert_eq!(r.first_emitted_action_kind_code(), ActionIntent::KIND_PRESS_CONTROL);
        assert_eq!(r.reason_code(), DecisionReport::REASON_MATCHED_RULE);
    }

    #[test]
    fn brain_kind_and_reason_codes_have_exact_stable_values() {
        // The canonical code tables — pinned to their exact numbers so a
        // renumbering cannot pass silently.
        assert_eq!(DecisionReport::BRAIN_KIND_NONE, 0);
        assert_eq!(DecisionReport::BRAIN_KIND_SCRIPTED, 1);
        assert_eq!(DecisionReport::BRAIN_KIND_REPLAY, 2);
        assert_eq!(DecisionReport::BRAIN_KIND_HOLD_SET, 3);
        assert_eq!(DecisionReport::REASON_NO_REASON, 0);
        assert_eq!(DecisionReport::REASON_NO_MATCHING_RULE, 1);
        assert_eq!(DecisionReport::REASON_MATCHED_RULE, 2);
        assert_eq!(DecisionReport::REASON_REPLAY_EMITTED, 3);
        assert_eq!(DecisionReport::REASON_REPLAY_EMPTY, 4);
        assert_eq!(DecisionReport::REASON_REPLAY_COMPLETE, 5);
        assert_eq!(DecisionReport::REASON_ACTION_BUDGET_ZERO, 6);
        assert_eq!(DecisionReport::REASON_HOLD_SET_EMITTED, 7);
    }

    #[test]
    fn reason_codes_are_distinct() {
        let codes = [
            DecisionReport::REASON_NO_REASON,
            DecisionReport::REASON_NO_MATCHING_RULE,
            DecisionReport::REASON_MATCHED_RULE,
            DecisionReport::REASON_REPLAY_EMITTED,
            DecisionReport::REASON_REPLAY_EMPTY,
            DecisionReport::REASON_REPLAY_COMPLETE,
            DecisionReport::REASON_ACTION_BUDGET_ZERO,
            DecisionReport::REASON_HOLD_SET_EMITTED,
        ];
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len());
    }

    #[test]
    fn derives_are_exercised() {
        let r = report();
        let c = r;
        assert_eq!(r, c);
        assert_ne!(
            r,
            DecisionReport::new(
                AgentId::from_raw(2),
                Tick::new(7),
                3,
                2,
                DecisionReport::BRAIN_KIND_SCRIPTED,
                1,
                ActionIntent::KIND_PRESS_CONTROL,
                DecisionReport::REASON_MATCHED_RULE,
            )
        );
        assert!(format!("{r:?}").contains("DecisionReport"));
    }
}
