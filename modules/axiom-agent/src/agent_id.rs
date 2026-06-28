//! Stable identity for one agent.

/// A deterministic, opaque identifier for an agent in an [`crate::AgentApi`]
/// session.
///
/// It is a plain `u64` newtype — there is no kernel "agent id" primitive to
/// reuse, and borrowing an unrelated kernel id (a handle, an entity) would be a
/// ceremonial dependency on a concept this module does not model. Construction
/// is pure data: the same raw value always yields the same id, with no clock and
/// no randomness, so an id is safe to store in reports and replay logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentId(u64);

impl AgentId {
    /// Construct an id from its raw value.
    pub const fn from_raw(raw: u64) -> Self {
        AgentId(raw)
    }

    /// The raw value backing this id.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_raw_round_trips() {
        assert_eq!(AgentId::from_raw(42).raw(), 42);
        assert_eq!(AgentId::from_raw(0).raw(), 0);
    }

    #[test]
    fn construction_is_deterministic() {
        assert_eq!(AgentId::from_raw(7), AgentId::from_raw(7));
    }

    #[test]
    fn ordering_and_equality_are_numeric() {
        assert!(AgentId::from_raw(1) < AgentId::from_raw(2));
        assert_ne!(AgentId::from_raw(1), AgentId::from_raw(2));
    }

    #[test]
    fn derives_are_exercised() {
        let a = AgentId::from_raw(3);
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, AgentId::from_raw(4));
        assert!(format!("{a:?}").contains("AgentId"));
    }
}
