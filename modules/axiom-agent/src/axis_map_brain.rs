//! A deterministic brain that maps perceived scalars onto **continuous control
//! axes** — the analogue counterpart of the hold-set brain.

use crate::action_intent::ActionIntent;
use crate::agent_brain::{AgentBrain, BrainDecision};
use crate::agent_id::AgentId;
use crate::agent_memory::AgentMemory;
use crate::agent_profile::AgentProfile;
use crate::decision_report::DecisionReport;
use crate::observation::Observation;

/// One perceived-scalar → control-axis binding: the neutral shape of a
/// proportional control law.
///
/// A binding says "when you perceive a fact of this kind, drive this axis by
/// `offset + value · gain`, held inside `[min, max]`". It carries no game noun
/// and no floating point: the perceived scalar, the gain (in thousandths, so a
/// gain of `1_000` is unity) and the limits are all integers, so the same
/// observation always produces the same axis value on every machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisBinding {
    fact_kind_code: u16,
    axis_code: u32,
    gain_milli: i64,
    offset: i64,
    min_value: i64,
    max_value: i64,
}

impl AxisBinding {
    /// A binding from facts of `fact_kind_code` to axis `axis_code`.
    pub const fn new(
        fact_kind_code: u16,
        axis_code: u32,
        gain_milli: i64,
        offset: i64,
        min_value: i64,
        max_value: i64,
    ) -> Self {
        AxisBinding {
            fact_kind_code,
            axis_code,
            gain_milli,
            offset,
            min_value,
            max_value,
        }
    }

    /// The observation-fact kind this binding reads.
    pub const fn fact_kind_code(self) -> u16 {
        self.fact_kind_code
    }

    /// The control axis this binding drives.
    pub const fn axis_code(self) -> u32 {
        self.axis_code
    }

    /// The proportional gain, in thousandths.
    pub const fn gain_milli(self) -> i64 {
        self.gain_milli
    }

    /// The constant added before the gain term.
    pub const fn offset(self) -> i64 {
        self.offset
    }

    /// The lower limit the produced axis value is held to.
    pub const fn min_value(self) -> i64 {
        self.min_value
    }

    /// The upper limit the produced axis value is held to.
    pub const fn max_value(self) -> i64 {
        self.max_value
    }

    /// The axis value this binding produces for a perceived `fact_value`.
    ///
    /// Saturating throughout and clamped with `max`/`min` rather than
    /// `i64::clamp`, so a caller who inverts the limits gets a defined value
    /// instead of a panic.
    pub(crate) fn apply(self, fact_value: i64) -> i64 {
        self.offset
            .saturating_add(fact_value.saturating_mul(self.gain_milli) / 1_000)
            .max(self.min_value)
            .min(self.max_value)
    }
}

/// A brain that emits **one `move_axis` intent per binding whose fact is
/// present**, every tick.
///
/// The substrate already had two ways to decide from an observation — the
/// scripted brain's first-matching-rule and the hold-set brain's fixed held
/// controls — but both emit *discrete* actions. Nothing could emit the
/// `move_axis` intents the action vocabulary has always carried, which left
/// every analogue control (a steering wheel, a throttle, a stick) reachable only
/// by an app quantising it into a control-code bitmask. This closes that: the
/// perceived value is the input, the binding table is the control law, and the
/// emitted axis values are the decision.
///
/// Emissions are clamped to the profile's `max_actions_per_tick` exactly like
/// the other brains — a zero budget emits nothing, reported as
/// [`DecisionReport::REASON_ACTION_BUDGET_ZERO`] — and an observation matching no
/// binding emits a single `Noop`, reported as
/// [`DecisionReport::REASON_NO_MATCHING_RULE`], exactly like the scripted brain.
///
/// Several bindings may name the *same* axis (a proportional term and a damping
/// term, say). Each emits its own intent; a consumer folds them together with
/// [`crate::action_queue::ActionQueue::axis_value`].
#[derive(Debug, Clone)]
pub struct AxisMapBrain {
    bindings: Vec<AxisBinding>,
}

impl AxisMapBrain {
    /// A brain driving `bindings`, evaluated in order.
    pub fn new(bindings: Vec<AxisBinding>) -> Self {
        AxisMapBrain { bindings }
    }
}

impl AgentBrain for AxisMapBrain {
    const KIND_CODE: u16 = DecisionReport::BRAIN_KIND_AXIS_MAP;

    fn decide(
        &mut self,
        _agent_id: AgentId,
        profile: AgentProfile,
        observation: &Observation,
        _memory: &AgentMemory,
    ) -> BrainDecision {
        let driven: Vec<ActionIntent> = self
            .bindings
            .iter()
            .filter_map(|binding| {
                observation
                    .first_fact_with_kind(binding.fact_kind_code())
                    .map(|fact| {
                        ActionIntent::move_axis(binding.axis_code(), binding.apply(fact.value()))
                    })
            })
            .collect();
        let has_match = !driven.is_empty();
        let max = profile.max_actions_per_tick() as usize;
        let budget_zero = max == 0;
        let fallback = ((!has_match) & (!budget_zero)).then(ActionIntent::noop);
        let emission: Vec<ActionIntent> = driven.into_iter().take(max).chain(fallback).collect();
        // Reason precedence, matching the scripted brain: a zero budget overrides
        // everything; otherwise "drove an axis" or "nothing to drive".
        let matched_reason = [
            DecisionReport::REASON_NO_MATCHING_RULE,
            DecisionReport::REASON_AXIS_MAP_EMITTED,
        ][has_match as usize];
        let reason =
            [matched_reason, DecisionReport::REASON_ACTION_BUDGET_ZERO][budget_zero as usize];
        BrainDecision::new(emission, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::ObservationFact;
    use axiom_kernel::Tick;

    const STEER_ERROR: u16 = 10;
    const YAW_RATE: u16 = 11;
    const STEER_AXIS: u32 = 1;

    fn observation(facts: Vec<ObservationFact>) -> Observation {
        Observation::from_parts(
            AgentId::from_raw(1),
            Tick::new(0),
            Vec::new(),
            Vec::new(),
            facts,
        )
    }

    fn fact(kind_code: u16, value: i64) -> ObservationFact {
        ObservationFact::new(kind_code, 0, 0, 0, 0, value)
    }

    fn decide(
        brain: &mut AxisMapBrain,
        profile: AgentProfile,
        observation: &Observation,
    ) -> BrainDecision {
        brain.decide(
            AgentId::from_raw(1),
            profile,
            observation,
            &AgentMemory::empty_with_capacity(1),
        )
    }

    #[test]
    fn a_perceived_scalar_becomes_a_scaled_axis_intent() {
        let mut brain = AxisMapBrain::new(vec![AxisBinding::new(
            STEER_ERROR,
            STEER_AXIS,
            2_400,
            0,
            -1_000_000,
            1_000_000,
        )]);
        let d = decide(
            &mut brain,
            AgentProfile::debug_perfect(),
            &observation(vec![fact(STEER_ERROR, 100)]),
        );
        assert_eq!(d.intents().len(), 1);
        assert_eq!(d.intents()[0].kind_code(), ActionIntent::KIND_MOVE_AXIS);
        assert_eq!(d.intents()[0].axis_code(), STEER_AXIS);
        assert_eq!(d.intents()[0].value(), 240);
        assert_eq!(d.reason_code(), DecisionReport::REASON_AXIS_MAP_EMITTED);
    }

    #[test]
    fn several_bindings_may_drive_one_axis_in_the_same_tick() {
        let mut brain = AxisMapBrain::new(vec![
            AxisBinding::new(STEER_ERROR, STEER_AXIS, 1_000, 0, -1_000, 1_000),
            AxisBinding::new(YAW_RATE, STEER_AXIS, -500, 0, -1_000, 1_000),
        ]);
        let d = decide(
            &mut brain,
            AgentProfile::debug_perfect(),
            &observation(vec![fact(STEER_ERROR, 400), fact(YAW_RATE, 200)]),
        );
        assert_eq!(d.intents().len(), 2);
        assert_eq!(d.intents()[0].value(), 400);
        assert_eq!(d.intents()[1].value(), -100);
    }

    #[test]
    fn a_binding_whose_fact_is_absent_drives_nothing() {
        let mut brain = AxisMapBrain::new(vec![
            AxisBinding::new(STEER_ERROR, STEER_AXIS, 1_000, 0, -1_000, 1_000),
            AxisBinding::new(YAW_RATE, STEER_AXIS, 1_000, 0, -1_000, 1_000),
        ]);
        let d = decide(
            &mut brain,
            AgentProfile::debug_perfect(),
            &observation(vec![fact(YAW_RATE, 7)]),
        );
        assert_eq!(d.intents().len(), 1);
        assert_eq!(d.intents()[0].value(), 7);
    }

    #[test]
    fn the_offset_and_limits_shape_the_produced_value() {
        let binding = AxisBinding::new(STEER_ERROR, STEER_AXIS, 1_000, 100, -50, 250);
        assert_eq!(binding.apply(0), 100);
        assert_eq!(binding.apply(1_000), 250, "clamped to the upper limit");
        assert_eq!(binding.apply(-1_000), -50, "clamped to the lower limit");
        assert_eq!(binding.apply(i64::MAX), 250, "saturating, not wrapping");
        assert_eq!(binding.fact_kind_code(), STEER_ERROR);
        assert_eq!(binding.axis_code(), STEER_AXIS);
        assert_eq!(binding.gain_milli(), 1_000);
        assert_eq!(binding.offset(), 100);
        assert_eq!(binding.min_value(), -50);
        assert_eq!(binding.max_value(), 250);
    }

    #[test]
    fn inverted_limits_produce_a_defined_value_rather_than_a_panic() {
        let binding = AxisBinding::new(STEER_ERROR, STEER_AXIS, 1_000, 0, 500, -500);
        assert_eq!(binding.apply(0), -500);
    }

    #[test]
    fn an_observation_matching_no_binding_emits_a_noop() {
        let mut brain = AxisMapBrain::new(vec![AxisBinding::new(
            STEER_ERROR, STEER_AXIS, 1_000, 0, -1_000, 1_000,
        )]);
        let d = decide(
            &mut brain,
            AgentProfile::debug_perfect(),
            &observation(vec![fact(YAW_RATE, 5)]),
        );
        assert_eq!(d.intents().len(), 1);
        assert_eq!(d.intents()[0].kind_code(), ActionIntent::KIND_NOOP);
        assert_eq!(d.reason_code(), DecisionReport::REASON_NO_MATCHING_RULE);
    }

    #[test]
    fn the_action_budget_clamps_the_emission_count() {
        let mut brain = AxisMapBrain::new(vec![
            AxisBinding::new(STEER_ERROR, STEER_AXIS, 1_000, 0, -1_000, 1_000),
            AxisBinding::new(YAW_RATE, STEER_AXIS, 1_000, 0, -1_000, 1_000),
        ]);
        let d = decide(
            &mut brain,
            AgentProfile::debug_perfect().with_action_budget(1),
            &observation(vec![fact(STEER_ERROR, 1), fact(YAW_RATE, 2)]),
        );
        assert_eq!(d.intents().len(), 1);
        assert_eq!(d.intents()[0].value(), 1);
    }

    #[test]
    fn a_zero_budget_emits_nothing_with_budget_zero_reason() {
        let mut brain = AxisMapBrain::new(vec![AxisBinding::new(
            STEER_ERROR, STEER_AXIS, 1_000, 0, -1_000, 1_000,
        )]);
        let d = decide(
            &mut brain,
            AgentProfile::debug_perfect().with_action_budget(0),
            &observation(vec![fact(STEER_ERROR, 1)]),
        );
        assert_eq!(d.intents().len(), 0);
        assert_eq!(d.reason_code(), DecisionReport::REASON_ACTION_BUDGET_ZERO);
    }

    #[test]
    fn a_zero_budget_with_no_match_still_reports_budget_zero() {
        let mut brain = AxisMapBrain::new(vec![AxisBinding::new(
            STEER_ERROR, STEER_AXIS, 1_000, 0, -1_000, 1_000,
        )]);
        let d = decide(
            &mut brain,
            AgentProfile::debug_perfect().with_action_budget(0),
            &observation(Vec::new()),
        );
        assert_eq!(d.intents().len(), 0);
        assert_eq!(d.reason_code(), DecisionReport::REASON_ACTION_BUDGET_ZERO);
    }

    #[test]
    fn derives_are_exercised() {
        let brain = AxisMapBrain::new(vec![AxisBinding::new(1, 1, 1, 0, 0, 1)]);
        let cloned = brain.clone();
        assert_eq!(cloned.bindings.len(), 1);
        assert!(format!("{brain:?}").contains("AxisMapBrain"));
        let binding = AxisBinding::new(1, 1, 1, 0, 0, 1);
        assert_eq!(binding, binding.clone());
        assert!(format!("{binding:?}").contains("AxisBinding"));
    }
}
