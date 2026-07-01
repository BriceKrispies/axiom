//! Change-detection queries over a [`TrackedColumn`] — `Added`/`Changed`/`Removed`
//! since a logical tick. A second `impl Query` block, so these read like the rest
//! of the query surface.

use axiom_kernel::EntityId;

use crate::query::Query;
use crate::tracked_column::{ChangeKind, TrackedColumn};

impl Query {
    /// Entities whose component was **added or changed** at or after `tick`, as
    /// `(entity, &value)` ascending. Mirrors Bevy's `Changed` (which includes
    /// additions). `tick` is inclusive — pass `last_seen + 1` for strictly-after.
    ///
    /// Removed entries pass the tick filter but drop out at the value lookup
    /// (their value is gone), so the result is exactly the live added-or-changed
    /// set.
    pub fn changed_since<T>(
        tracked: &TrackedColumn<T>,
        tick: u64,
    ) -> impl Iterator<Item = (EntityId, &T)> {
        tracked
            .changes()
            .filter(move |(_, _, t)| *t >= tick)
            .filter_map(move |(entity, _, _)| tracked.get(entity).map(move |value| (entity, value)))
    }

    /// Entities whose component was **newly added** at or after `tick`, as
    /// `(entity, &value)` ascending. Mirrors Bevy's `Added`. `tick` is inclusive.
    pub fn added_since<T>(
        tracked: &TrackedColumn<T>,
        tick: u64,
    ) -> impl Iterator<Item = (EntityId, &T)> {
        tracked
            .changes()
            .filter(move |(_, _, t)| *t >= tick)
            .filter_map(move |(entity, kind, _)| {
                (kind == ChangeKind::Added)
                    .then(|| tracked.get(entity))
                    .flatten()
                    .map(move |value| (entity, value))
            })
    }

    /// Entities whose component was **removed** at or after `tick`, as `entity`
    /// ids ascending. Mirrors Bevy's `RemovedComponents`. There is no value to
    /// yield — it is gone — so this yields ids only. `tick` is inclusive.
    pub fn removed_since<T>(
        tracked: &TrackedColumn<T>,
        tick: u64,
    ) -> impl Iterator<Item = EntityId> + '_ {
        tracked
            .changes()
            .filter(move |(_, kind, t)| (*kind == ChangeKind::Removed) & (*t >= tick))
            .map(|(entity, _, _)| entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(raw: u64) -> EntityId {
        EntityId::from_raw(raw)
    }

    /// A column with one entity per change kind at tick 5, plus an Added entry
    /// before the window (tick 2). Last-write-wins gives each entity one record.
    fn column() -> TrackedColumn<i32> {
        let mut col = TrackedColumn::new();
        col.insert(e(1), 10, 5);
        col.insert(e(2), 20, 1);
        col.insert(e(2), 21, 5);
        col.insert(e(3), 30, 1);
        col.remove(e(3), 5);
        col.insert(e(4), 40, 2);
        col
    }

    #[test]
    fn changed_since_yields_added_and_changed_dropping_removed_and_old() {
        let col = column();
        let got: Vec<(u64, i32)> = Query::changed_since(&col, 5)
            .map(|(id, v)| (id.raw(), *v))
            .collect();
        assert_eq!(got, vec![(1, 10), (2, 21)]);
    }

    #[test]
    fn added_since_yields_only_additions_in_the_window() {
        let col = column();
        let got: Vec<(u64, i32)> = Query::added_since(&col, 5)
            .map(|(id, v)| (id.raw(), *v))
            .collect();
        assert_eq!(got, vec![(1, 10)]);
    }

    #[test]
    fn removed_since_yields_only_removals_in_the_window() {
        let mut col = column();
        col.insert(e(5), 50, 1);
        col.remove(e(5), 2);
        let got: Vec<u64> = Query::removed_since(&col, 5).map(|id| id.raw()).collect();
        assert_eq!(got, vec![3]);
    }
}
