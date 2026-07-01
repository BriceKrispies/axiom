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

    /// Live entities that have all three of `A`, `B`, and `C`, as
    /// `(entity, &a, &b, &c)` ascending — the three-way intersection.
    pub fn three<'a, A, B, C>(
        registry: &'a EntityRegistry,
        a: &'a ComponentColumn<A>,
        b: &'a ComponentColumn<B>,
        c: &'a ComponentColumn<C>,
    ) -> impl Iterator<Item = (EntityId, &'a A, &'a B, &'a C)> {
        a.iter()
            .filter_map(move |(entity, av)| b.get(entity).map(move |bv| (entity, av, bv)))
            .filter_map(move |(entity, av, bv)| c.get(entity).map(move |cv| (entity, av, bv, cv)))
            .filter(move |(entity, _, _, _)| registry.contains(*entity))
    }

    /// Live entities that have all four of `A`, `B`, `C`, and `D`, as
    /// `(entity, &a, &b, &c, &d)` ascending — the four-way intersection.
    pub fn four<'a, A, B, C, D>(
        registry: &'a EntityRegistry,
        a: &'a ComponentColumn<A>,
        b: &'a ComponentColumn<B>,
        c: &'a ComponentColumn<C>,
        d: &'a ComponentColumn<D>,
    ) -> impl Iterator<Item = (EntityId, &'a A, &'a B, &'a C, &'a D)> {
        a.iter()
            .filter_map(move |(entity, av)| b.get(entity).map(move |bv| (entity, av, bv)))
            .filter_map(move |(entity, av, bv)| c.get(entity).map(move |cv| (entity, av, bv, cv)))
            .filter_map(move |(entity, av, bv, cv)| {
                d.get(entity).map(move |dv| (entity, av, bv, cv, dv))
            })
            .filter(move |(entity, _, _, _, _)| registry.contains(*entity))
    }

    /// Live entities that have `A`, each paired with `B` if present:
    /// `(entity, &a, Option<&b>)` ascending — a left join, the optional-component
    /// query. The base set is entities with `A`; `B` is looked up per entity.
    pub fn two_opt<'a, A, B>(
        registry: &'a EntityRegistry,
        a: &'a ComponentColumn<A>,
        b: &'a ComponentColumn<B>,
    ) -> impl Iterator<Item = (EntityId, &'a A, Option<&'a B>)> {
        a.iter()
            .filter(move |(entity, _)| registry.contains(*entity))
            .map(move |(entity, av)| (entity, av, b.get(entity)))
    }

    /// Live entities that have component `T`, as `(entity, &mut component)`
    /// ascending. The registry is borrowed immutably and the column mutably, so
    /// there is no aliasing.
    ///
    /// This is the only *mutable* query: a multi-column mutable join (yielding
    /// `&mut A` and `&mut B` from two columns at once) cannot be expressed as a
    /// safe `Iterator` (it would need a lending iterator or `unsafe`). To mutate
    /// across columns, drive one column — collect the ids you need, or iterate one
    /// mutably — and reach the others by id with [`ComponentColumn::get_mut`].
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
        let query = <Query as Default>::default();
        assert!(format!("{query:?}").contains("Query"));
    }

    #[test]
    fn one_yields_live_entities_in_order() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
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
        registry.despawn(b);
        let got: Vec<u64> = Query::one(&registry, &values)
            .map(|(id, _)| id.raw())
            .collect();
        assert_eq!(got, vec![1], "a row for a dead entity is excluded");
    }

    #[test]
    fn two_yields_intersection_in_order() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
        let c = registry.spawn();
        let mut names: ComponentColumn<&'static str> = ComponentColumn::new();
        names.insert(a, "a");
        names.insert(b, "b");
        names.insert(c, "c");
        let mut scores: ComponentColumn<i32> = ComponentColumn::new();
        scores.insert(c, 30);
        scores.insert(a, 10);
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

    #[test]
    fn three_yields_intersection_excluding_missing_and_dead() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn(); // 1: has all three
        let b = registry.spawn(); // 2: missing `cs` -> filter_map c None arm
        let c = registry.spawn(); // 3: missing `bs` -> filter_map b None arm
        let d = registry.spawn(); // 4: has all three but is despawned
        let mut xs: ComponentColumn<i32> = ComponentColumn::new();
        let mut ys: ComponentColumn<i32> = ComponentColumn::new();
        let mut zs: ComponentColumn<i32> = ComponentColumn::new();
        [a, b, c, d].iter().for_each(|&e| {
            xs.insert(e, 1);
        });
        xs.insert(a, 10);
        ys.insert(a, 20);
        zs.insert(a, 30);
        ys.insert(b, 21); // b: x,y but no z
        zs.insert(c, 32); // c: x,z but no y
        ys.insert(d, 24);
        zs.insert(d, 34);
        registry.despawn(d);
        let got: Vec<(u64, i32, i32, i32)> = Query::three(&registry, &xs, &ys, &zs)
            .map(|(id, x, y, z)| (id.raw(), *x, *y, *z))
            .collect();
        assert_eq!(got, vec![(1, 10, 20, 30)]);
    }

    #[test]
    fn four_yields_intersection_excluding_missing_and_dead() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn(); // 1: all four
        let b = registry.spawn(); // 2: missing the 4th -> filter_map d None arm
        let c = registry.spawn(); // 3: all four but despawned
        let mut w: ComponentColumn<i32> = ComponentColumn::new();
        let mut x: ComponentColumn<i32> = ComponentColumn::new();
        let mut y: ComponentColumn<i32> = ComponentColumn::new();
        let mut z: ComponentColumn<i32> = ComponentColumn::new();
        [a, b, c].iter().for_each(|&e| {
            w.insert(e, 1);
            x.insert(e, 2);
            y.insert(e, 3);
        });
        z.insert(a, 4);
        z.insert(c, 4); // b has no z
        registry.despawn(c);
        let got: Vec<(u64, i32, i32, i32, i32)> = Query::four(&registry, &w, &x, &y, &z)
            .map(|(id, w, x, y, z)| (id.raw(), *w, *x, *y, *z))
            .collect();
        assert_eq!(got, vec![(1, 1, 2, 3, 4)]);
    }

    #[test]
    fn two_opt_pairs_present_and_absent_and_excludes_dead() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn(); // 1: has b (Some)
        let b = registry.spawn(); // 2: no b (None arm)
        let c = registry.spawn(); // 3: has a-column but despawned
        let mut xs: ComponentColumn<i32> = ComponentColumn::new();
        let mut ys: ComponentColumn<&'static str> = ComponentColumn::new();
        xs.insert(a, 10);
        xs.insert(b, 20);
        xs.insert(c, 30);
        ys.insert(a, "a");
        registry.despawn(c);
        let got: Vec<(u64, i32, Option<&str>)> = Query::two_opt(&registry, &xs, &ys)
            .map(|(id, x, y)| (id.raw(), *x, y.copied()))
            .collect();
        assert_eq!(got, vec![(1, 10, Some("a")), (2, 20, None)]);
    }
}
