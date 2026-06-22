//! Minimal deterministic queries over component columns.

use axiom_kernel::EntityId;

use crate::component_column::ComponentColumn;
use crate::entity_registry::EntityRegistry;

/// Deterministic iteration helpers over one or two component columns, filtered to
/// live entities.
///
/// This is the smallest useful query surface: iterate a single component, the
/// intersection of two components, or a single component mutably. Every query
/// yields entities in ascending id order (the columns are ascending), excludes
/// entities missing a required component, and excludes entities that are not live
/// in the registry — so a query never observes a stale or partially-built entity.
/// It is intentionally not an archetype engine: no filters beyond presence, no
/// scheduling, no parallelism.
#[derive(Debug, Clone, Copy, Default)]
pub struct Query;

impl Query {
    /// Live entities that have component `T`, as `(entity, &component)` ascending.
    pub fn one<'a, T>(
        registry: &'a EntityRegistry,
        column: &'a ComponentColumn<T>,
    ) -> impl Iterator<Item = (EntityId, &'a T)> {
        column
            .iter()
            .filter(move |(entity, _)| registry.contains(*entity))
    }

    /// Live entities that have both `A` and `B`, as `(entity, &a, &b)` ascending —
    /// the entity-id intersection of the two columns.
    pub fn two<'a, A, B>(
        registry: &'a EntityRegistry,
        a: &'a ComponentColumn<A>,
        b: &'a ComponentColumn<B>,
    ) -> impl Iterator<Item = (EntityId, &'a A, &'a B)> {
        a.iter()
            .filter_map(move |(entity, av)| b.get(entity).map(move |bv| (entity, av, bv)))
            .filter(move |(entity, _, _)| registry.contains(*entity))
    }

    /// Live entities that have component `T`, as `(entity, &mut component)`
    /// ascending. The registry is borrowed immutably and the column mutably, so
    /// there is no aliasing.
    pub fn one_mut<'a, T>(
        registry: &'a EntityRegistry,
        column: &'a mut ComponentColumn<T>,
    ) -> impl Iterator<Item = (EntityId, &'a mut T)> {
        column
            .iter_mut()
            .filter(move |(entity, _)| registry.contains(*entity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_default_are_available() {
        let query = Query::default();
        assert!(format!("{query:?}").contains("Query"));
    }

    #[test]
    fn one_yields_live_entities_in_order() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn(); // 1
        let b = registry.spawn(); // 2
        let mut values: ComponentColumn<i32> = ComponentColumn::new();
        values.insert(b, 20);
        values.insert(a, 10);
        let got: Vec<(u64, i32)> = Query::one(&registry, &values)
            .map(|(id, v)| (id.raw(), *v))
            .collect();
        assert_eq!(got, vec![(1, 10), (2, 20)]);
    }

    #[test]
    fn one_excludes_dead_entities() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
        let mut values: ComponentColumn<i32> = ComponentColumn::new();
        values.insert(a, 10);
        values.insert(b, 20);
        // Despawn b in the registry only (simulate a stale column row).
        registry.despawn(b);
        let got: Vec<u64> = Query::one(&registry, &values)
            .map(|(id, _)| id.raw())
            .collect();
        assert_eq!(got, vec![1], "a row for a dead entity is excluded");
    }

    #[test]
    fn two_yields_intersection_in_order() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn(); // 1
        let b = registry.spawn(); // 2
        let c = registry.spawn(); // 3
        let mut names: ComponentColumn<&'static str> = ComponentColumn::new();
        names.insert(a, "a");
        names.insert(b, "b");
        names.insert(c, "c");
        let mut scores: ComponentColumn<i32> = ComponentColumn::new();
        scores.insert(c, 30);
        scores.insert(a, 10);
        // b has a name but no score -> excluded from the intersection.
        let got: Vec<(u64, &str, i32)> = Query::two(&registry, &names, &scores)
            .map(|(id, n, s)| (id.raw(), *n, *s))
            .collect();
        assert_eq!(got, vec![(1, "a", 10), (3, "c", 30)]);
    }

    #[test]
    fn two_excludes_dead_entities() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
        let mut names: ComponentColumn<&'static str> = ComponentColumn::new();
        names.insert(a, "a");
        names.insert(b, "b");
        let mut scores: ComponentColumn<i32> = ComponentColumn::new();
        scores.insert(a, 10);
        scores.insert(b, 20);
        registry.despawn(b);
        let got: Vec<u64> = Query::two(&registry, &names, &scores)
            .map(|(id, _, _)| id.raw())
            .collect();
        assert_eq!(got, vec![1]);
    }

    #[test]
    fn one_mut_mutates_live_entities_in_order() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
        let mut values: ComponentColumn<i32> = ComponentColumn::new();
        values.insert(a, 1);
        values.insert(b, 2);
        let ids: Vec<u64> = Query::one_mut(&registry, &mut values)
            .map(|(id, v)| {
                *v *= 10;
                id.raw()
            })
            .collect();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(values.get(a), Some(&10));
        assert_eq!(values.get(b), Some(&20));
    }

    #[test]
    fn one_mut_excludes_dead_entities() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
        let mut values: ComponentColumn<i32> = ComponentColumn::new();
        values.insert(a, 1);
        values.insert(b, 2);
        registry.despawn(a);
        let ids: Vec<u64> = Query::one_mut(&registry, &mut values)
            .map(|(id, v)| {
                *v += 100;
                id.raw()
            })
            .collect();
        assert_eq!(ids, vec![2], "the dead entity's row is skipped");
        assert_eq!(values.get(a), Some(&1), "skipped row is untouched");
        assert_eq!(values.get(b), Some(&102));
    }
}
