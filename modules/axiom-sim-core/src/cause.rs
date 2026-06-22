//! A deterministic reference to *why* a mutation happened.

use crate::ids::{CausalEventId, ProcessId, RuleId};

/// What caused a fact, relation, process, or causal event to come about.
///
/// A `CauseRef` points at the originator of a mutation: a prior causal event, a
/// process, a rule, or a direct command. It is a value type — stored on facts,
/// relations, processes, and causal events, and compared for equality when
/// querying a causal chain. It carries no prose; it is structured cause tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CauseRef {
    /// Caused by a previously recorded causal event.
    Event(CausalEventId),
    /// Caused by a running process.
    Process(ProcessId),
    /// Caused by a rule.
    Rule(RuleId),
    /// Caused by a direct, external command (no prior sim cause).
    Command,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_compare_by_identity() {
        assert_eq!(
            CauseRef::Process(ProcessId::from_raw(1)),
            CauseRef::Process(ProcessId::from_raw(1))
        );
        assert_ne!(
            CauseRef::Process(ProcessId::from_raw(1)),
            CauseRef::Process(ProcessId::from_raw(2))
        );
        assert_ne!(CauseRef::Command, CauseRef::Rule(RuleId::from_raw(1)));
        assert_ne!(
            CauseRef::Event(CausalEventId::from_raw(1)),
            CauseRef::Process(ProcessId::from_raw(1))
        );
    }

    #[test]
    fn command_cause_is_self_equal() {
        assert_eq!(CauseRef::Command, CauseRef::Command);
    }
}
