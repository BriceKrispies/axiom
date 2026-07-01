//! Composable presence filters over query iterators — `With`/`Without`/`Or`.

use axiom_kernel::EntityId;

use crate::component_column::ComponentColumn;

/// Extracts the entity id from a query item, so presence filters apply to any
/// query arity.
///
/// Sealed by construction: it is `pub` only so the blanket [`QueryFilterExt`]
/// impl's bound is nameable, but it lives in this private module and is never
/// re-exported, so external code can neither name it nor implement it for foreign
/// item types. It is implemented for the `(EntityId, …)` tuples every
/// [`crate::Query`] yields.
pub trait QueryItem {
    /// The entity this item belongs to.
    fn entity(&self) -> EntityId;
}

impl<A> QueryItem for (EntityId, A) {
    fn entity(&self) -> EntityId {
        self.0
    }
}

impl<A, B> QueryItem for (EntityId, A, B) {
    fn entity(&self) -> EntityId {
        self.0
    }
}

impl<A, B, C> QueryItem for (EntityId, A, B, C) {
    fn entity(&self) -> EntityId {
        self.0
    }
}

impl<A, B, C, D> QueryItem for (EntityId, A, B, C, D) {
    fn entity(&self) -> EntityId {
        self.0
    }
}

/// Presence-filter adapters that compose with any [`crate::Query`] iterator.
///
/// Every query yields items whose first element is the [`EntityId`]; these
/// adapters keep or drop an item by whether some *other* column has that entity —
/// the engine's branchless equivalent of Bevy's `With` / `Without` / `Or` query
/// filters. They chain (`Query::two(..).with(&c).without(&d)`) because each
/// adapter yields the same item type it received.
pub trait QueryFilterExt: Iterator + Sized
where
    Self::Item: QueryItem,
{
    /// Keep only items whose entity also has component `C`.
    fn with<C>(self, column: &ComponentColumn<C>) -> impl Iterator<Item = Self::Item> {
        self.filter(move |item| column.contains(item.entity()))
    }

    /// Keep only items whose entity does *not* have component `C`.
    fn without<C>(self, column: &ComponentColumn<C>) -> impl Iterator<Item = Self::Item> {
        self.filter(move |item| !column.contains(item.entity()))
    }

    /// Keep only items whose entity has component `C` *or* component `D` — the
    /// branchless `Or` of two presence checks (bitwise `|`, both always evaluated).
    fn with_either<C, D>(
        self,
        c: &ComponentColumn<C>,
        d: &ComponentColumn<D>,
    ) -> impl Iterator<Item = Self::Item> {
        self.filter(move |item| c.contains(item.entity()) | d.contains(item.entity()))
    }
}

impl<I> QueryFilterExt for I
where
    I: Iterator + Sized,
    I::Item: QueryItem,
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_registry::EntityRegistry;
    use crate::query::Query;

    fn ids<I: Iterator<Item = (EntityId, V)>, V>(iter: I) -> Vec<u64> {
        iter.map(|(e, _)| e.raw()).collect()
    }

    #[test]
    fn with_keeps_only_entities_that_have_the_column() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
        let mut base: ComponentColumn<i32> = ComponentColumn::new();
        base.insert(a, 10);
        base.insert(b, 20);
        let mut tag: ComponentColumn<()> = ComponentColumn::new();
        tag.insert(a, ());
        assert_eq!(ids(Query::one(&registry, &base).with(&tag)), vec![1]);
    }

    #[test]
    fn without_keeps_only_entities_missing_the_column() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
        let mut base: ComponentColumn<i32> = ComponentColumn::new();
        base.insert(a, 10);
        base.insert(b, 20);
        let mut tag: ComponentColumn<()> = ComponentColumn::new();
        tag.insert(a, ());
        assert_eq!(ids(Query::one(&registry, &base).without(&tag)), vec![2]);
    }

    #[test]
    fn with_either_covers_all_presence_combinations() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
        let c_e = registry.spawn();
        let d_e = registry.spawn();
        let mut base: ComponentColumn<i32> = ComponentColumn::new();
        [a, b, c_e, d_e].iter().for_each(|&e| {
            base.insert(e, 0);
        });
        let mut c: ComponentColumn<()> = ComponentColumn::new();
        let mut d: ComponentColumn<()> = ComponentColumn::new();
        c.insert(a, ());
        d.insert(b, ());
        c.insert(c_e, ());
        d.insert(c_e, ());
        assert_eq!(
            ids(Query::one(&registry, &base).with_either(&c, &d)),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn filters_chain_and_apply_to_every_query_arity() {
        let mut registry = EntityRegistry::new();
        let a = registry.spawn();
        let b = registry.spawn();
        let mut x: ComponentColumn<i32> = ComponentColumn::new();
        let mut y: ComponentColumn<i32> = ComponentColumn::new();
        let mut z: ComponentColumn<i32> = ComponentColumn::new();
        let mut w: ComponentColumn<i32> = ComponentColumn::new();
        let mut keep: ComponentColumn<()> = ComponentColumn::new();
        let mut drop: ComponentColumn<()> = ComponentColumn::new();
        [a, b].iter().for_each(|&e| {
            x.insert(e, 1);
            y.insert(e, 2);
            z.insert(e, 3);
            w.insert(e, 4);
        });
        keep.insert(a, ());
        keep.insert(b, ());
        drop.insert(b, ());

        assert_eq!(
            ids(Query::one(&registry, &x).with(&keep).without(&drop)),
            vec![1]
        );
        assert_eq!(
            Query::two(&registry, &x, &y)
                .with(&keep)
                .without(&drop)
                .map(|(e, _, _)| e.raw())
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(
            Query::three(&registry, &x, &y, &z)
                .without(&drop)
                .map(|(e, _, _, _)| e.raw())
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(
            Query::four(&registry, &x, &y, &z, &w)
                .with(&keep)
                .map(|(e, _, _, _, _)| e.raw())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }
}
