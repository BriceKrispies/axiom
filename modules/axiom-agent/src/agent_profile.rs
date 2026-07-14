//! A deterministic profile of an agent's human-like control limits.

/// The fixed control-limit parameters of one agent.
/// Every field is a plain integer or fixed-point quantity — there is no
/// randomness here and no noisy behavior is implemented yet. The profile is a
/// pure *parameter block*: a future decision stage may read these limits to
/// shape its output, but in this scaffold only [`Self::max_actions_per_tick`] is
/// consumed (to clamp how many actions a brain may emit in one step). The rest
/// are carried as the documented, stable contract a later stage will honor.
/// Fixed-point units keep the contract deterministic and float-free: angles in
/// milli-degrees (`1000` = one degree) and positions in micro-units (`1_000_000`
/// = one world unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentProfile {
    reaction_delay_ticks: u32,
    max_actions_per_tick: u32,
    attention_slots: u32,
    memory_ticks: u32,
    max_turn_milli_degrees_per_tick: u32,
    aim_error_milli_degrees: u32,
    movement_error_microunits: u32,
}

impl AgentProfile {
    /// A flawless reference agent: zero reaction delay, zero aim/movement error,
    /// generous action and attention budgets. The deterministic baseline tests
    /// and tools compare against.
    pub const fn debug_perfect() -> Self {
        AgentProfile {
            reaction_delay_ticks: 0,
            max_actions_per_tick: 8,
            attention_slots: 16,
            memory_ticks: 256,
            max_turn_milli_degrees_per_tick: 180_000,
            aim_error_milli_degrees: 0,
            movement_error_microunits: 0,
        }
    }

    /// A plausible human-like default: a short reaction delay, a small action
    /// budget, limited attention, a bounded turn rate, and non-zero aim/movement
    /// error parameters (declared only — no noisy behavior is applied yet).
    pub const fn human_like_default() -> Self {
        AgentProfile {
            reaction_delay_ticks: 12,
            max_actions_per_tick: 3,
            attention_slots: 4,
            memory_ticks: 120,
            max_turn_milli_degrees_per_tick: 9_000,
            aim_error_milli_degrees: 500,
            movement_error_microunits: 2_000,
        }
    }

    /// A copy of this profile with its action budget overridden. An app uses
    /// this to throttle (or, at `0`, freeze) a *deciding* agent — the scripted
    /// brain honors the budget; the replay brain reproduces its recording
    /// verbatim and is unaffected. Every other field is preserved.
    pub const fn with_action_budget(self, max_actions_per_tick: u32) -> Self {
        AgentProfile {
            max_actions_per_tick,
            ..self
        }
    }

    /// Ticks of delay before the agent reacts to a new observation.
    pub const fn reaction_delay_ticks(self) -> u32 {
        self.reaction_delay_ticks
    }

    /// The most actions a brain may emit in a single step.
    pub const fn max_actions_per_tick(self) -> u32 {
        self.max_actions_per_tick
    }

    /// How many distinct subjects the agent can attend to at once.
    pub const fn attention_slots(self) -> u32 {
        self.attention_slots
    }

    /// How many ticks of history the agent's memory is meant to span.
    pub const fn memory_ticks(self) -> u32 {
        self.memory_ticks
    }

    /// The maximum turn rate, in milli-degrees per tick.
    pub const fn max_turn_milli_degrees_per_tick(self) -> u32 {
        self.max_turn_milli_degrees_per_tick
    }

    /// The aim error parameter, in milli-degrees.
    pub const fn aim_error_milli_degrees(self) -> u32 {
        self.aim_error_milli_degrees
    }

    /// The movement error parameter, in micro-units.
    pub const fn movement_error_microunits(self) -> u32 {
        self.movement_error_microunits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_perfect_has_stable_expected_values() {
        let p = AgentProfile::debug_perfect();
        assert_eq!(p.reaction_delay_ticks(), 0);
        assert_eq!(p.max_actions_per_tick(), 8);
        assert_eq!(p.attention_slots(), 16);
        assert_eq!(p.memory_ticks(), 256);
        assert_eq!(p.max_turn_milli_degrees_per_tick(), 180_000);
        assert_eq!(p.aim_error_milli_degrees(), 0);
        assert_eq!(p.movement_error_microunits(), 0);
    }

    #[test]
    fn human_like_default_has_stable_expected_values() {
        let p = AgentProfile::human_like_default();
        assert_eq!(p.reaction_delay_ticks(), 12);
        assert_eq!(p.max_actions_per_tick(), 3);
        assert_eq!(p.attention_slots(), 4);
        assert_eq!(p.memory_ticks(), 120);
        assert_eq!(p.max_turn_milli_degrees_per_tick(), 9_000);
        assert_eq!(p.aim_error_milli_degrees(), 500);
        assert_eq!(p.movement_error_microunits(), 2_000);
    }

    #[test]
    fn the_two_profiles_differ() {
        assert_ne!(
            AgentProfile::debug_perfect(),
            AgentProfile::human_like_default()
        );
    }

    #[test]
    fn with_action_budget_overrides_only_the_budget() {
        let base = AgentProfile::debug_perfect();
        let frozen = base.with_action_budget(0);
        assert_eq!(frozen.max_actions_per_tick(), 0);
        assert_eq!(frozen.reaction_delay_ticks(), base.reaction_delay_ticks());
        assert_eq!(frozen.attention_slots(), base.attention_slots());
        assert_eq!(frozen.memory_ticks(), base.memory_ticks());
        assert_eq!(
            frozen.max_turn_milli_degrees_per_tick(),
            base.max_turn_milli_degrees_per_tick()
        );
        assert_eq!(
            frozen.aim_error_milli_degrees(),
            base.aim_error_milli_degrees()
        );
        assert_eq!(
            frozen.movement_error_microunits(),
            base.movement_error_microunits()
        );
        assert_eq!(base.with_action_budget(2).max_actions_per_tick(), 2);
    }

    #[test]
    fn derives_are_exercised() {
        let p = AgentProfile::debug_perfect();
        let c = p;
        assert_eq!(p, c);
        assert!(format!("{p:?}").contains("AgentProfile"));
    }
}
