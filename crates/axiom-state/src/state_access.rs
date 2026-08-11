//! What a computation declares it will read and write.

use axiom_kernel::StableHash;

use crate::state_id::StateId;
use crate::state_key::StateKey;

/// The states a computation may read, and the states it may write.
///
/// Two jobs, and the second is the reason this is data rather than documentation:
///
/// 1. It *constrains*. A [`crate::StateView`] opened with this refuses to read or
///    write anything undeclared, so a computation that says it needs the ball and
///    the batter cannot quietly start depending on the scoreboard.
/// 2. It *informs*. Because the declaration is an inspectable list of identities
///    rather than a type-level proof, a future scheduler can compute "A writes X,
///    B reads X, therefore B depends on A" without running either. That is what
///    [`Self::conflicts_with`] and [`Self::shared_states`] are for. **This layer
///    schedules nothing** — it only supplies the raw material.
///
/// Both lists are kept sorted and deduplicated, so a declaration's identity, its
/// bytes and its digest never depend on the order it was written in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StateAccess {
    reads: Vec<StateId>,
    writes: Vec<StateId>,
}

/// Insert into a sorted, deduplicated list. Already-present is a no-op, so
/// declaring the same state twice is idempotent.
fn admit(mut list: Vec<StateId>, id: StateId) -> Vec<StateId> {
    let vacancy = list.binary_search(&id).err();
    vacancy.map(|at| list.insert(at, id));
    list
}

impl StateAccess {
    /// Declare nothing.
    pub fn none() -> Self {
        StateAccess::default()
    }

    /// Also read `K`.
    pub fn read<K: StateKey>(self) -> Self {
        StateAccess {
            reads: admit(self.reads, K::id()),
            writes: self.writes,
        }
    }

    /// Also write `K`.
    ///
    /// A write does not imply a read: a computation that overwrites a state
    /// without consulting it has a genuinely different dependency shape, and
    /// collapsing the two would cost a future scheduler that distinction.
    pub fn write<K: StateKey>(self) -> Self {
        StateAccess {
            reads: self.reads,
            writes: admit(self.writes, K::id()),
        }
    }

    /// Declare by identity, for callers assembling access dynamically.
    pub fn from_ids(reads: &[StateId], writes: &[StateId]) -> Self {
        StateAccess {
            reads: reads.iter().fold(Vec::new(), |list, id| admit(list, *id)),
            writes: writes.iter().fold(Vec::new(), |list, id| admit(list, *id)),
        }
    }

    /// The declared reads, ascending.
    pub fn reads(&self) -> &[StateId] {
        &self.reads
    }

    /// The declared writes, ascending.
    pub fn writes(&self) -> &[StateId] {
        &self.writes
    }

    /// Whether `id` was declared as a read.
    pub fn declares_read(&self, id: StateId) -> bool {
        self.reads.binary_search(&id).is_ok()
    }

    /// Whether `id` was declared as a write.
    pub fn declares_write(&self, id: StateId) -> bool {
        self.writes.binary_search(&id).is_ok()
    }

    /// The states both declarations touch where at least one of them writes —
    /// exactly the set a scheduler would call a dependency.
    pub fn shared_states(&self, other: &StateAccess) -> Vec<StateId> {
        let mine: Vec<StateId> = self
            .writes
            .iter()
            .filter(|id| other.declares_read(**id) | other.declares_write(**id))
            .copied()
            .collect();
        let theirs = other
            .writes
            .iter()
            .filter(|id| self.declares_read(**id) | self.declares_write(**id))
            .copied();
        mine.into_iter()
            .chain(theirs)
            .fold(Vec::new(), |list, id| admit(list, id))
    }

    /// Whether these two computations may not run independently.
    pub fn conflicts_with(&self, other: &StateAccess) -> bool {
        !self.shared_states(other).is_empty()
    }

    /// The declaration's digest.
    pub fn hash(&self) -> StableHash {
        let words: Vec<u64> = core::iter::once(self.reads.len() as u64)
            .chain(self.reads.iter().copied().map(StateId::raw))
            .chain(core::iter::once(self.writes.len() as u64))
            .chain(self.writes.iter().copied().map(StateId::raw))
            .collect();
        StableHash::of_words(&words)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_kind::StateKind;

    struct Ball;
    impl StateKey for Ball {
        const PATH: &'static str = "access/ball";
        const KIND: StateKind = StateKind::Cell;
    }

    struct Batter;
    impl StateKey for Batter {
        const PATH: &'static str = "access/batter";
        const KIND: StateKind = StateKind::Cell;
    }

    struct Scoreboard;
    impl StateKey for Scoreboard {
        const PATH: &'static str = "access/scoreboard";
        const KIND: StateKind = StateKind::Cell;
    }

    #[test]
    fn an_empty_declaration_declares_nothing() {
        let none = StateAccess::none();
        assert!(none.reads().is_empty());
        assert!(none.writes().is_empty());
        assert!(!none.declares_read(Ball::id()));
        assert!(!none.declares_write(Ball::id()));
    }

    #[test]
    fn reads_and_writes_are_declared_separately() {
        let access = StateAccess::none().read::<Ball>().write::<Scoreboard>();
        assert!(access.declares_read(Ball::id()));
        assert!(!access.declares_write(Ball::id()));
        assert!(access.declares_write(Scoreboard::id()));
        assert!(
            !access.declares_read(Scoreboard::id()),
            "a write must not imply a read"
        );
    }

    #[test]
    fn declaration_order_does_not_reach_the_declaration() {
        let one = StateAccess::none().read::<Ball>().read::<Batter>();
        let other = StateAccess::none().read::<Batter>().read::<Ball>();
        assert_eq!(one, other);
        assert_eq!(one.hash(), other.hash());
    }

    #[test]
    fn declaring_the_same_state_twice_is_idempotent() {
        let once = StateAccess::none().read::<Ball>();
        let twice = StateAccess::none().read::<Ball>().read::<Ball>();
        assert_eq!(once, twice);
        assert_eq!(twice.reads().len(), 1);
    }

    #[test]
    fn declarations_are_stored_ascending() {
        let access = StateAccess::none()
            .read::<Scoreboard>()
            .read::<Ball>()
            .read::<Batter>();
        let mut sorted = access.reads().to_vec();
        sorted.sort_unstable();
        assert_eq!(access.reads(), sorted.as_slice());
    }

    #[test]
    fn a_declaration_can_be_assembled_from_identities() {
        let access = StateAccess::from_ids(&[Batter::id(), Ball::id()], &[Scoreboard::id()]);
        assert_eq!(access.reads().len(), 2);
        assert!(access.declares_read(Ball::id()));
        assert!(access.declares_write(Scoreboard::id()));
        assert_eq!(
            access,
            StateAccess::none()
                .read::<Ball>()
                .read::<Batter>()
                .write::<Scoreboard>()
        );
    }

    #[test]
    fn a_write_and_a_read_of_one_state_are_a_dependency() {
        let writer = StateAccess::none().write::<Ball>();
        let reader = StateAccess::none().read::<Ball>();
        assert!(writer.conflicts_with(&reader));
        assert!(reader.conflicts_with(&writer));
        assert_eq!(writer.shared_states(&reader), vec![Ball::id()]);
    }

    #[test]
    fn two_writers_of_one_state_are_a_dependency() {
        let one = StateAccess::none().write::<Ball>();
        let other = StateAccess::none().write::<Ball>();
        assert!(one.conflicts_with(&other));
    }

    #[test]
    fn two_readers_of_one_state_are_independent() {
        let one = StateAccess::none().read::<Ball>();
        let other = StateAccess::none().read::<Ball>();
        assert!(!one.conflicts_with(&other));
        assert!(one.shared_states(&other).is_empty());
    }

    #[test]
    fn computations_over_disjoint_states_are_independent() {
        let one = StateAccess::none().read::<Ball>().write::<Ball>();
        let other = StateAccess::none().read::<Batter>().write::<Batter>();
        assert!(!one.conflicts_with(&other));
    }

    #[test]
    fn shared_states_are_reported_once_and_ascending() {
        let one = StateAccess::none().write::<Ball>().write::<Scoreboard>();
        let other = StateAccess::none().write::<Ball>().read::<Scoreboard>();
        let shared = one.shared_states(&other);
        let mut expected = vec![Ball::id(), Scoreboard::id()];
        expected.sort_unstable();
        assert_eq!(shared, expected);
    }

    #[test]
    fn the_digest_is_stable_and_distinguishes_reads_from_writes() {
        let reader = StateAccess::none().read::<Ball>();
        let writer = StateAccess::none().write::<Ball>();
        assert_eq!(reader.hash(), StateAccess::none().read::<Ball>().hash());
        assert_ne!(reader.hash(), writer.hash());
    }
}
