//! A component column that records per-entity changes against logical ticks.

use std::collections::BTreeMap;

use axiom_kernel::EntityId;

/// What happened to an entity's component since the change log was last cleared.
///
/// The last change wins per entity within a window: an insert then a mutation
/// reads as [`Changed`](Self::Changed); an insert then a removal reads as
/// [`Removed`](Self::Removed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// The component was newly added to an entity that did not have it.
    Added,
    /// An existing component value was overwritten or mutably borrowed.
    Changed,
    /// The component was removed from an entity that had it.
    Removed,
}

/// A sparse column of component type `T` that records change events.
///
/// It is the deterministic change-detection primitive future rendering,
/// networking, and replay systems consume: every mutation is tagged with the
/// logical `tick` it happened on (a simulation/frame tick the caller supplies —
/// never wall-clock time), and [`changes`](Self::changes) yields the recorded
/// events in ascending entity-id order. It is a standalone primitive, so existing
/// plain [`crate::ComponentColumn`] storage is untouched.
#[derive(Debug, Clone)]
pub struct TrackedColumn<T> {
    values: BTreeMap<EntityId, T>,
    changes: BTreeMap<EntityId, (ChangeKind, u64)>,
}

impl<T> TrackedColumn<T> {
    /// Create an empty tracked column.
    pub fn new() -> Self {
        TrackedColumn {
            values: BTreeMap::new(),
            changes: BTreeMap::new(),
        }
    }

    /// Set `entity`'s component at logical `tick`, recording [`ChangeKind::Added`]
    /// if the entity did not have it or [`ChangeKind::Changed`] if it did. Returns
    /// the previous value, if any.
    pub fn insert(&mut self, entity: EntityId, value: T, tick: u64) -> Option<T> {
        let kind =
            [ChangeKind::Added, ChangeKind::Changed][self.values.contains_key(&entity) as usize];
        self.changes.insert(entity, (kind, tick));
        self.values.insert(entity, value)
    }

    /// Borrow `entity`'s component, if present, without recording a change.
    pub fn get(&self, entity: EntityId) -> Option<&T> {
        self.values.get(&entity)
    }

    /// Mutably borrow `entity`'s component, recording [`ChangeKind::Changed`] at
    /// `tick` when the component is present.
    pub fn get_mut(&mut self, entity: EntityId, tick: u64) -> Option<&mut T> {
        let present = self.values.contains_key(&entity);
        present.then(|| self.changes.insert(entity, (ChangeKind::Changed, tick)));
        self.values.get_mut(&entity)
    }

    /// Remove `entity`'s component, recording [`ChangeKind::Removed`] at `tick`
    /// when it existed. Returns the removed value, if any.
    pub fn remove(&mut self, entity: EntityId, tick: u64) -> Option<T> {
        let removed = self.values.remove(&entity);
        removed
            .is_some()
            .then(|| self.changes.insert(entity, (ChangeKind::Removed, tick)));
        removed
    }

    /// Whether `entity` currently has this component.
    pub fn contains(&self, entity: EntityId) -> bool {
        self.values.contains_key(&entity)
    }

    /// The recorded changes since the last [`clear_changes`](Self::clear_changes),
    /// as `(entity, kind, tick)` in ascending entity-id order.
    pub fn changes(&self) -> impl Iterator<Item = (EntityId, ChangeKind, u64)> + '_ {
        self.changes
            .iter()
            .map(|(entity, (kind, tick))| (*entity, *kind, *tick))
    }

    /// Discard the recorded change log (values are kept).
    pub fn clear_changes(&mut self) {
        self.changes.clear();
    }

    /// The number of entities holding this component.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether no entities hold this component.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl<T> Default for TrackedColumn<T> {
    fn default() -> Self {
        TrackedColumn::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(raw: u64) -> EntityId {
        EntityId::from_raw(raw)
    }

    fn recorded(column: &TrackedColumn<i32>) -> Vec<(u64, ChangeKind, u64)> {
        column
            .changes()
            .map(|(entity, kind, tick)| (entity.raw(), kind, tick))
            .collect()
    }

    #[test]
    fn new_and_default_are_empty() {
        let a: TrackedColumn<i32> = TrackedColumn::new();
        let b: TrackedColumn<i32> = TrackedColumn::default();
        assert!(a.is_empty());
        assert_eq!(a.len(), 0);
        assert!(b.is_empty());
        assert!(recorded(&a).is_empty());
    }

    #[test]
    fn insert_marks_added_then_changed() {
        let mut column = TrackedColumn::new();
        assert_eq!(column.insert(e(1), 10, 5), None);
        assert_eq!(recorded(&column), vec![(1, ChangeKind::Added, 5)]);
        assert_eq!(column.insert(e(1), 20, 6), Some(10));
        assert_eq!(recorded(&column), vec![(1, ChangeKind::Changed, 6)]);
        assert_eq!(column.get(e(1)), Some(&20));
    }

    #[test]
    fn get_mut_marks_changed_only_when_present() {
        let mut column = TrackedColumn::new();
        column.insert(e(1), 1, 0);
        column.clear_changes();
        assert!(column.get_mut(e(2), 9).is_none());
        assert!(recorded(&column).is_empty());
        *column.get_mut(e(1), 7).unwrap() = 42;
        assert_eq!(recorded(&column), vec![(1, ChangeKind::Changed, 7)]);
        assert_eq!(column.get(e(1)), Some(&42));
    }

    #[test]
    fn remove_marks_removed_only_when_present() {
        let mut column = TrackedColumn::new();
        column.insert(e(1), 1, 0);
        column.clear_changes();
        assert_eq!(column.remove(e(2), 3), None);
        assert!(recorded(&column).is_empty());
        assert_eq!(column.remove(e(1), 4), Some(1));
        assert_eq!(recorded(&column), vec![(1, ChangeKind::Removed, 4)]);
        assert!(!column.contains(e(1)));
    }

    #[test]
    fn changes_are_ascending_and_clearable() {
        let mut column = TrackedColumn::new();
        column.insert(e(3), 30, 1);
        column.insert(e(1), 10, 1);
        column.insert(e(2), 20, 1);
        let ids: Vec<u64> = column
            .changes()
            .map(|(entity, _, _)| entity.raw())
            .collect();
        assert_eq!(ids, vec![1, 2, 3], "change log is ascending by entity id");
        column.clear_changes();
        assert!(recorded(&column).is_empty());
        assert_eq!(column.len(), 3, "clearing changes keeps values");
    }
}
