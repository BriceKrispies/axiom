//! Deterministic generic transfer rules: how quantity moves between residues.

use std::collections::BTreeMap;

use crate::ids::TransferRuleId;
use crate::interaction::InteractionRoute;
use crate::quantity::Quantity;

const FIXED: u8 = 0;
const PERCENT: u8 = 1;
const ALL_UP_TO: u8 = 2;
const NONE: u8 = 3;

/// How much of an available amount a transfer moves. A tagged value so
/// [`Self::compute`] selects branchlessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferMode {
    kind: u8,
    amount: i64,
    basis_points: i64,
    max: i64,
}

impl TransferMode {
    /// Move a fixed amount (fails later if the source has less).
    pub const fn fixed(amount: i64) -> Self {
        TransferMode {
            kind: FIXED,
            amount,
            basis_points: 0,
            max: 0,
        }
    }

    /// Move a fraction of the available amount, in basis points (1% = 100 bp).
    pub const fn percentage(basis_points: i64) -> Self {
        TransferMode {
            kind: PERCENT,
            amount: 0,
            basis_points,
            max: 0,
        }
    }

    /// Move everything available, capped at `max`.
    pub const fn all_up_to(max: i64) -> Self {
        TransferMode {
            kind: ALL_UP_TO,
            amount: 0,
            basis_points: 0,
            max,
        }
    }

    /// Move nothing.
    pub const fn none() -> Self {
        TransferMode {
            kind: NONE,
            amount: 0,
            basis_points: 0,
            max: 0,
        }
    }

    /// Whether this mode is valid: a percentage must be in `0..=10000` bp and
    /// non-negative; other modes are always valid (negative fixed/max can't occur
    /// because a [`Quantity`] amount is non-negative, but we re-check defensively).
    pub fn is_valid(self) -> bool {
        let percent_ok = (self.basis_points >= 0) & (self.basis_points <= 10_000);
        [true, percent_ok, self.max >= 0, true][self.kind as usize]
    }

    /// The amount this mode would move given `available` (always `>= 0`). The
    /// caller still verifies the source actually holds the result.
    pub fn compute(self, available: i64) -> i64 {
        let percent = available.saturating_mul(self.basis_points) / 10_000;
        [self.amount, percent, available.min(self.max), 0][self.kind as usize]
    }
}

/// A rule moving quantity along an interaction route. `lossy` rules destroy the
/// moved amount instead of depositing it (explicit, non-conserving).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferRule {
    id: TransferRuleId,
    mode: TransferMode,
    route: InteractionRoute,
    lossy: bool,
}

impl TransferRule {
    /// This rule's stable id.
    pub const fn id(&self) -> TransferRuleId {
        self.id
    }
    /// The transfer mode.
    pub const fn mode(&self) -> TransferMode {
        self.mode
    }
    /// The route this rule applies to.
    pub const fn route(&self) -> InteractionRoute {
        self.route
    }
    /// Whether the rule destroys the moved amount (does not deposit it).
    pub const fn lossy(&self) -> bool {
        self.lossy
    }
}

/// The outcome of applying a transfer rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferOutcome {
    /// Quantity moved successfully.
    Applied,
    /// The source residue does not exist.
    InvalidSource,
    /// The interaction route does not match the rule's route.
    RouteMismatch,
    /// The source holds less than the rule wants to move.
    InsufficientQuantity,
    /// The source and target residues use incompatible units.
    IncompatibleUnits,
}

/// A structured transfer result: the outcome and, on success, the moved amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferResult {
    outcome: TransferOutcome,
    moved: Option<Quantity>,
}

impl TransferResult {
    /// Build a result.
    pub(crate) fn new(outcome: TransferOutcome, moved: Option<Quantity>) -> Self {
        TransferResult { outcome, moved }
    }

    /// The outcome.
    pub const fn outcome(&self) -> TransferOutcome {
        self.outcome
    }

    /// The amount moved (only present when [`TransferOutcome::Applied`]).
    pub const fn moved(&self) -> Option<Quantity> {
        self.moved
    }
}

/// A deterministic store of transfer rules, keyed/iterated by ascending id.
#[derive(Debug, Clone, Default)]
pub struct TransferRuleStore {
    rules: BTreeMap<TransferRuleId, TransferRule>,
    next: u64,
}

impl TransferRuleStore {
    /// Create an empty store. The first rule has id 1.
    pub fn new() -> Self {
        TransferRuleStore {
            rules: BTreeMap::new(),
            next: 1,
        }
    }

    /// Register a transfer rule, minting an id. Returns `None` for an invalid mode.
    pub fn register(
        &mut self,
        mode: TransferMode,
        route: InteractionRoute,
        lossy: bool,
    ) -> Option<TransferRuleId> {
        mode.is_valid().then(|| {
            let id = TransferRuleId::from_raw(self.next);
            self.next += 1;
            self.rules.insert(
                id,
                TransferRule {
                    id,
                    mode,
                    route,
                    lossy,
                },
            );
            id
        })
    }

    /// Borrow a rule by id, if present.
    pub fn get(&self, id: TransferRuleId) -> Option<&TransferRule> {
        self.rules.get(&id)
    }

    /// Remove a rule by id. Returns it if present.
    pub fn remove(&mut self, id: TransferRuleId) -> Option<TransferRule> {
        self.rules.remove(&id)
    }

    /// Rules applying to a given route, in ascending id order.
    pub fn by_route(&self, route: InteractionRoute) -> impl Iterator<Item = &TransferRule> {
        self.rules.values().filter(move |rule| rule.route == route)
    }

    /// All rules, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &TransferRule> {
        self.rules.values()
    }

    /// The number of rules.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the store holds no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_compute_amounts_deterministically() {
        assert_eq!(TransferMode::fixed(5).compute(10), 5);
        assert_eq!(TransferMode::percentage(2500).compute(40), 10); // 25% of 40
        assert_eq!(TransferMode::all_up_to(7).compute(10), 7);
        assert_eq!(TransferMode::all_up_to(7).compute(3), 3); // capped by available
        assert_eq!(TransferMode::none().compute(10), 0);
    }

    #[test]
    fn validity_rejects_out_of_range_percentages() {
        assert!(TransferMode::fixed(3).is_valid());
        assert!(TransferMode::percentage(10_000).is_valid());
        assert!(!TransferMode::percentage(10_001).is_valid());
        assert!(!TransferMode::percentage(-1).is_valid());
        assert!(TransferMode::all_up_to(0).is_valid());
        assert!(TransferMode::none().is_valid());
    }

    #[test]
    fn register_rejects_invalid_rules_and_queries_by_route() {
        let mut store = TransferRuleStore::new();
        assert!(store.is_empty());
        assert!(store
            .register(
                TransferMode::percentage(20_000),
                InteractionRoute::Touch,
                false
            )
            .is_none());
        let r1 = store
            .register(TransferMode::fixed(2), InteractionRoute::Touch, false)
            .unwrap();
        let _r2 = store
            .register(TransferMode::none(), InteractionRoute::Ingestion, false)
            .unwrap();
        let r3 = store
            .register(TransferMode::all_up_to(9), InteractionRoute::Touch, true)
            .unwrap();
        assert_eq!(r1.raw(), 1);
        let rule = store.get(r1).unwrap();
        assert_eq!(rule.route(), InteractionRoute::Touch);
        assert!(!rule.lossy());
        assert_eq!(rule.mode(), TransferMode::fixed(2));
        let touch: Vec<TransferRuleId> = store
            .by_route(InteractionRoute::Touch)
            .map(TransferRule::id)
            .collect();
        assert_eq!(touch, vec![r1, r3]);
        assert_eq!(store.iter().count(), 3);
        assert_eq!(store.remove(r1).unwrap().id(), r1);
        assert!(store.get(r1).is_none());
    }

    #[test]
    fn transfer_result_carries_outcome_and_moved() {
        let moved = Quantity::new(crate::quantity::QuantityUnit::Mass, 3);
        let result = TransferResult::new(TransferOutcome::Applied, moved);
        assert_eq!(result.outcome(), TransferOutcome::Applied);
        assert_eq!(result.moved(), moved);
        let fail = TransferResult::new(TransferOutcome::RouteMismatch, None);
        assert_eq!(fail.outcome(), TransferOutcome::RouteMismatch);
        assert_eq!(fail.moved(), None);
    }
}
