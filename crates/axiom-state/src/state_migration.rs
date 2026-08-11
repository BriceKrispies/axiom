//! Carrying a snapshot forward across schema versions.

use axiom_kernel::{SchemaVersion, StableHash};

use crate::state_error::StateError;
use crate::state_error_code::StateErrorCode;
use crate::state_schema_id::version_word;
use crate::state_snapshot::StateSnapshot;
use crate::StateResult;

/// One version step.
///
/// The step is a **plain function pointer**, not a closure and not a trait
/// object: a migration that could capture state would be able to produce a
/// different result on its second run, which is exactly what a migration must
/// never do. A `fn(&StateSnapshot) -> StateResult<StateSnapshot>` has nothing to
/// capture.
#[derive(Debug, Clone, Copy)]
pub struct StateMigration {
    from: SchemaVersion,
    to: SchemaVersion,
    step: fn(&StateSnapshot) -> StateResult<StateSnapshot>,
}

/// A step's identity is the version transition it performs, not the address of
/// the function that performs it: function-pointer addresses are not guaranteed
/// unique, so comparing them would be meaningless. Two steps claiming the same
/// transition are the same step as far as a plan is concerned — a chain offering
/// two different routes from one version to another would be ambiguous, which is
/// precisely what `StateMigrationPlan::new` exists to reject.
impl PartialEq for StateMigration {
    fn eq(&self, other: &Self) -> bool {
        (self.from == other.from) & (self.to == other.to)
    }
}

impl Eq for StateMigration {}

impl StateMigration {
    /// Declare a step from one version to the next.
    pub const fn new(
        from: SchemaVersion,
        to: SchemaVersion,
        step: fn(&StateSnapshot) -> StateResult<StateSnapshot>,
    ) -> Self {
        StateMigration { from, to, step }
    }

    /// The version this step consumes.
    pub const fn from(&self) -> SchemaVersion {
        self.from
    }

    /// The version this step produces.
    pub const fn to(&self) -> SchemaVersion {
        self.to
    }

    /// Run the step.
    pub fn run(&self, snapshot: &StateSnapshot) -> StateResult<StateSnapshot> {
        (self.step)(snapshot)
    }

    /// The step's identity, for recording which migrations a run performed.
    pub fn identity(&self) -> StableHash {
        StableHash::of_words(&[version_word(self.from), version_word(self.to)])
    }
}

/// An ordered chain of version steps.
///
/// Sequential only, on purpose. An arbitrary migration *graph* needs
/// pathfinding, and two paths between the same versions could produce two
/// different answers — non-determinism built into the thing whose whole job is
/// to be reproducible. A chain composes by folding, is trivially deterministic,
/// and matches how schema versions actually evolve. A version that needs no
/// change gets an explicit identity step, which keeps the chain total.
///
/// The plan is a value the caller owns and passes in. There is no registry to
/// look a migration up in, and nothing is discovered at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StateMigrationPlan {
    steps: Vec<StateMigration>,
}

impl StateMigrationPlan {
    /// Build a plan, rejecting a chain with a gap or one that does not advance.
    pub fn new(steps: &[StateMigration]) -> StateResult<Self> {
        steps
            .windows(2)
            .find(|pair| pair[0].to() != pair[1].from())
            .map_or(Ok(()), |_| Err(gap()))
            .and_then(|()| {
                steps
                    .iter()
                    .find(|step| step.to() <= step.from())
                    .map_or(Ok(()), |_| Err(gap()))
            })
            .map(|()| StateMigrationPlan {
                steps: steps.to_vec(),
            })
    }

    /// The steps, in order.
    pub fn steps(&self) -> &[StateMigration] {
        &self.steps
    }

    /// Carry `snapshot` forward to `target`.
    ///
    /// Deterministic and repeatable: the same snapshot and the same plan always
    /// produce the same result, because each step is a pure function of its
    /// input and the chain is walked in one fixed order.
    pub fn migrate(
        &self,
        snapshot: &StateSnapshot,
        target: SchemaVersion,
    ) -> StateResult<StateSnapshot> {
        self.steps
            .iter()
            .position(|step| step.from() == snapshot.version())
            .ok_or(unsupported())
            .and_then(|start| {
                self.steps[start..]
                    .iter()
                    .take_while(|step| step.from() < target)
                    .try_fold(snapshot.clone(), |current, step| step.run(&current))
            })
            .and_then(|migrated| {
                (migrated.version() == target)
                    .then_some(migrated)
                    .ok_or(unsupported())
            })
    }
}

fn gap() -> StateError {
    StateError::new(
        StateErrorCode::UnsupportedMigration,
        "the migration chain has a gap, or a step that does not advance the version",
    )
}

fn unsupported() -> StateError {
    StateError::new(
        StateErrorCode::UnsupportedMigration,
        "this plan has no sequential path from the snapshot's version to the target",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_decl::StateDecl;
    use crate::state_key::{CellKey, StateKey};
    use crate::state_kind::StateKind;
    use crate::state_schema::StateSchema;
    use crate::state_snapshot_builder::StateSnapshotBuilder;

    struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "migrate/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    /// Added at v2 — the shape change the migration exists to perform.
    struct Solved;
    impl StateKey for Solved {
        const PATH: &'static str = "migrate/solved";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Solved {
        type Value = bool;
    }

    fn v1() -> SchemaVersion {
        SchemaVersion::new(1, 0)
    }

    fn v2() -> SchemaVersion {
        SchemaVersion::new(2, 0)
    }

    fn v3() -> SchemaVersion {
        SchemaVersion::new(3, 0)
    }

    fn schema_v1() -> StateSchema {
        StateSchema::build("migrate", v1(), &[StateDecl::cell::<Tick>()]).expect("valid")
    }

    fn schema_v2() -> StateSchema {
        StateSchema::build(
            "migrate",
            v2(),
            &[StateDecl::cell::<Tick>(), StateDecl::cell::<Solved>()],
        )
        .expect("valid")
    }

    fn schema_v3() -> StateSchema {
        StateSchema::build(
            "migrate",
            v3(),
            &[StateDecl::cell::<Tick>(), StateDecl::cell::<Solved>()],
        )
        .expect("valid")
    }

    fn snapshot_v1() -> StateSnapshot {
        StateSnapshotBuilder::new(&schema_v1())
            .with_cell::<Tick>(&7)
            .build()
            .expect("complete")
    }

    /// v1 -> v2: carry the tick forward, default the new flag.
    fn one_to_two(old: &StateSnapshot) -> StateResult<StateSnapshot> {
        old.cell::<Tick>().and_then(|tick| {
            StateSnapshotBuilder::new(&schema_v2())
                .with_cell::<Tick>(&tick)
                .with_cell::<Solved>(&false)
                .build()
        })
    }

    /// v2 -> v3: a version bump that changes no shape.
    fn two_to_three(old: &StateSnapshot) -> StateResult<StateSnapshot> {
        old.cell::<Tick>().and_then(|tick| {
            old.cell::<Solved>().and_then(|solved| {
                StateSnapshotBuilder::new(&schema_v3())
                    .with_cell::<Tick>(&tick)
                    .with_cell::<Solved>(&solved)
                    .build()
            })
        })
    }

    /// A step that always fails, to prove a failure propagates.
    fn always_fails(_old: &StateSnapshot) -> StateResult<StateSnapshot> {
        Err(StateError::new(
            StateErrorCode::UnsupportedMigration,
            "this step cannot run",
        ))
    }

    fn plan() -> StateMigrationPlan {
        StateMigrationPlan::new(&[
            StateMigration::new(v1(), v2(), one_to_two),
            StateMigration::new(v2(), v3(), two_to_three),
        ])
        .expect("a well-formed chain")
    }

    #[test]
    fn a_step_reports_its_versions_and_identity() {
        let step = StateMigration::new(v1(), v2(), one_to_two);
        assert_eq!(step.from(), v1());
        assert_eq!(step.to(), v2());
        assert_eq!(step.identity(), StateMigration::new(v1(), v2(), one_to_two).identity());
        assert_ne!(
            step.identity(),
            StateMigration::new(v2(), v3(), two_to_three).identity()
        );
    }

    #[test]
    fn one_step_carries_data_forward_and_defaults_what_is_new() {
        let migrated = plan().migrate(&snapshot_v1(), v2()).expect("migrates");
        assert_eq!(migrated.version(), v2());
        assert_eq!(migrated.cell::<Tick>(), Ok(7));
        assert_eq!(migrated.cell::<Solved>(), Ok(false));
    }

    #[test]
    fn several_steps_chain_to_the_target() {
        let migrated = plan().migrate(&snapshot_v1(), v3()).expect("migrates");
        assert_eq!(migrated.version(), v3());
        assert_eq!(migrated.cell::<Tick>(), Ok(7));
    }

    #[test]
    fn migrating_is_repeatable_and_byte_identical() {
        let once = plan().migrate(&snapshot_v1(), v3()).expect("migrates");
        let twice = plan().migrate(&snapshot_v1(), v3()).expect("migrates");
        assert_eq!(once, twice);
        assert_eq!(once.to_bytes(), twice.to_bytes());
        assert_eq!(once.hash(), twice.hash());
    }

    #[test]
    fn migrating_does_not_touch_the_original() {
        let original = snapshot_v1();
        let _ = plan().migrate(&original, v3()).expect("migrates");
        assert_eq!(original.version(), v1());
        assert_eq!(original.cell::<Tick>(), Ok(7));
    }

    #[test]
    fn the_steps_are_reported_in_order() {
        let plan = plan();
        assert_eq!(plan.steps().len(), 2);
        assert_eq!(plan.steps()[0].from(), v1());
        assert_eq!(plan.steps()[1].to(), v3());
    }

    #[test]
    fn a_chain_with_a_gap_is_rejected() {
        let error = StateMigrationPlan::new(&[
            StateMigration::new(v1(), v2(), one_to_two),
            StateMigration::new(v3(), SchemaVersion::new(4, 0), two_to_three),
        ])
        .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UnsupportedMigration);
    }

    #[test]
    fn a_step_that_does_not_advance_is_rejected() {
        let error =
            StateMigrationPlan::new(&[StateMigration::new(v2(), v1(), one_to_two)]).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UnsupportedMigration);

        let standing_still =
            StateMigrationPlan::new(&[StateMigration::new(v1(), v1(), one_to_two)]).unwrap_err();
        assert_eq!(standing_still.code(), StateErrorCode::UnsupportedMigration);
    }

    #[test]
    fn an_empty_plan_is_well_formed_but_migrates_nothing() {
        let empty = StateMigrationPlan::new(&[]).expect("well formed");
        assert!(empty.steps().is_empty());
        assert_eq!(
            empty.migrate(&snapshot_v1(), v2()).unwrap_err().code(),
            StateErrorCode::UnsupportedMigration
        );
    }

    #[test]
    fn a_snapshot_at_an_unknown_version_is_rejected() {
        let unrelated = StateSnapshotBuilder::new(&schema_v3())
            .with_cell::<Tick>(&1)
            .with_cell::<Solved>(&true)
            .build()
            .expect("complete");
        let error = plan().migrate(&unrelated, v3()).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UnsupportedMigration);
    }

    #[test]
    fn a_target_the_chain_cannot_reach_is_rejected() {
        let error = plan()
            .migrate(&snapshot_v1(), SchemaVersion::new(9, 0))
            .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UnsupportedMigration);
    }

    #[test]
    fn a_failing_step_propagates_rather_than_producing_a_half_migrated_snapshot() {
        let broken = StateMigrationPlan::new(&[StateMigration::new(v1(), v2(), always_fails)])
            .expect("well formed");
        assert_eq!(
            broken.migrate(&snapshot_v1(), v2()).unwrap_err().code(),
            StateErrorCode::UnsupportedMigration
        );
    }

    #[test]
    fn a_migrated_snapshot_serializes_stably() {
        let migrated = plan().migrate(&snapshot_v1(), v2()).expect("migrates");
        let restored = StateSnapshot::from_bytes(&migrated.to_bytes(), v2()).expect("round trip");
        assert_eq!(restored, migrated);
    }
}
