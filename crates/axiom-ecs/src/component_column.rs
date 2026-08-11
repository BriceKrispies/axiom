//! A sparse, deterministic column of one component type.

use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, KernelResult, Reflect, TypeSchema};

/// The `sparse` entry for a slot that this column holds no component for.
///
/// `u32::MAX` rather than an `Option<u32>`: the sparse array is one entry per
/// entity slot in the whole world, and halving its width is the difference
/// between a cache line carrying 16 indices and carrying 8. A column with
/// `u32::MAX` live components is not a thing this engine can produce.
const ABSENT: u32 = u32::MAX;

/// Take the parked value, which the caller has already established is present.
///
/// A named helper purely so [`ComponentColumn::insert`]'s two paths read as the
/// two things they are; `expect` states the invariant at the one place it holds.
fn take<T>(slot: &mut Option<T>) -> T {
    slot.take().expect("the insert path takes the value once")
}

/// A sparse store of one component type `T`, keyed by [`EntityId`].
///
/// An entity appears in a column only if it has that component, so storage and
/// iteration cost only what is actually present — the "open" part of the open
/// component model: each component type is its own independently-addressable
/// column.
///
/// # Why a sparse set and not a `BTreeMap`
///
/// This was a `BTreeMap<EntityId, T>` until 2026-08-11. The map was correct and
/// deterministic, and it was also the engine's single largest frame cost.
///
/// The reason is the access *pattern*, not the container. The hot loop in the
/// renderer is "for every entity, fetch its components" — `SceneSnapshot`
/// rebuilds itself each frame by walking every node and asking eight separate
/// columns for that node's entry. Against a B-tree each of those is an O(log n)
/// descent through pointer-linked nodes scattered across the heap, so a frame in
/// Burnt Rubber (~1000 spawned entities × 8 columns) spent it in roughly ten
/// thousand independent tree searches. Measured on a throttled browser profile
/// that was **~16% of total main-thread self time** in `BTreeMap` operations
/// alone, plus a further slice in the iterator adapters walking them.
///
/// A sparse set answers the same question by array indexing:
///
/// ```text
///   sparse[entity slot]  ->  index into dense   (O(1), one load)
///   dense[index]         ->  (entity, component)
/// ```
///
/// This is sound *here* specifically because entity ids are dense slot indices
/// from a reusing allocator (see `EntityRegistry`), so `sparse` stays
/// proportional to the live entity count rather than to a sprawling id space.
/// A column keyed by hashed or generational ids could not do this.
///
/// `dense` is kept in ascending entity-id order, so iteration is deterministic
/// on every platform — the property the `BTreeMap` was originally chosen for is
/// preserved by construction, and iteration is now over one contiguous slice
/// instead of a tree walk.
#[derive(Debug, Clone)]
pub struct ComponentColumn<T> {
    /// `(entity, component)` pairs in ascending entity-id order.
    dense: Vec<(EntityId, T)>,
    /// Entity slot → index into `dense`, or [`ABSENT`].
    sparse: Vec<u32>,
}

impl<T> ComponentColumn<T> {
    /// Create an empty column.
    pub fn new() -> Self {
        ComponentColumn {
            dense: Vec::new(),
            sparse: Vec::new(),
        }
    }

    /// This entity's index into `dense`, if it has this component.
    ///
    /// One bounds-checked load and one comparison. A slot past the end of
    /// `sparse` is simply absent, which is what lets the array lag behind the
    /// world's slot count without a separate length invariant to maintain.
    fn index_of(&self, entity: EntityId) -> Option<usize> {
        self.sparse
            .get(entity.raw() as usize)
            .copied()
            .filter(|&index| index != ABSENT)
            .map(|index| index as usize)
    }

    /// Set the component for `entity`, returning the previous value if any.
    pub fn insert(&mut self, entity: EntityId, component: T) -> Option<T> {
        let found = self.dense.binary_search_by_key(&entity, |(id, _)| *id);
        let at = found.unwrap_or_else(|slot| slot);
        // `component` has to reach exactly one of two paths, and Rust will not
        // let two closures both own it. Parking it in a slot and `take`-ing it
        // lets the paths run as plain sequential statements — no branch, and no
        // path where it could be moved twice. Whichever path runs first empties
        // the slot, so the other is a no-op.
        let mut incoming = Some(component);
        // Present: overwrite in place. This is the per-frame path — every
        // transform update lands here — so it must stay a single move with no
        // shifting and no re-indexing. (It was a `Vec::splice` over a 1-element
        // range, which reads beautifully and cost 13% of the frame: splice pays
        // for drain machinery even when it replaces one element with one
        // element.)
        let replaced = found
            .ok()
            .map(|index| std::mem::replace(&mut self.dense[index].1, take(&mut incoming)));
        // Absent: an ordered insertion, which shifts the tail and therefore
        // invalidates every index from here on. Rare by comparison — this is
        // spawn, not steady state.
        incoming.take().map(|component| {
            self.dense.insert(at, (entity, component));
            self.reindex_from(at);
        });
        replaced
    }

    /// Rebuild `sparse` for `dense[from..]`, growing it to cover those slots.
    fn reindex_from(&mut self, from: usize) {
        let Self { dense, sparse } = self;
        let needed = dense
            .iter()
            .skip(from)
            .map(|(entity, _)| entity.raw() as usize + 1)
            .max()
            .unwrap_or(0);
        sparse.resize(sparse.len().max(needed), ABSENT);
        dense
            .iter()
            .enumerate()
            .skip(from)
            .for_each(|(index, (entity, _))| {
                sparse[entity.raw() as usize] = index as u32;
            });
    }

    /// Borrow `entity`'s component, if present.
    pub fn get(&self, entity: EntityId) -> Option<&T> {
        self.index_of(entity).map(|index| &self.dense[index].1)
    }

    /// Mutably borrow `entity`'s component, if present.
    pub fn get_mut(&mut self, entity: EntityId) -> Option<&mut T> {
        let at = self.index_of(entity);
        at.map(|index| &mut self.dense[index].1)
    }

    /// Whether `entity` has this component.
    pub fn contains(&self, entity: EntityId) -> bool {
        self.index_of(entity).is_some()
    }

    /// Remove `entity`'s component, returning it if present.
    pub fn remove(&mut self, entity: EntityId) -> Option<T> {
        let at = self.index_of(entity);
        at.map(|index| {
            let (removed, component) = self.dense.remove(index);
            self.sparse[removed.raw() as usize] = ABSENT;
            self.reindex_from(index);
            component
        })
    }

    /// Iterate `(entity, &component)` in ascending entity-id order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &T)> {
        self.dense.iter().map(|(entity, c)| (*entity, c))
    }

    /// Iterate `(entity, &mut component)` in ascending entity-id order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (EntityId, &mut T)> {
        self.dense.iter_mut().map(|(entity, c)| (*entity, c))
    }

    /// The number of entities in this column.
    pub fn len(&self) -> usize {
        self.dense.len()
    }

    /// Whether the column has no entries.
    pub fn is_empty(&self) -> bool {
        self.dense.is_empty()
    }
}

impl<T> Default for ComponentColumn<T> {
    fn default() -> Self {
        ComponentColumn::new()
    }
}

impl<T: Reflect> ComponentColumn<T> {
    /// The schema of the component type stored in this column.
    pub fn schema(&self) -> TypeSchema {
        T::SCHEMA
    }

    /// Serialize the column: entry count, then each `(entity, component)` in
    /// ascending entity-id order.
    pub fn reflect_write(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.dense.len() as u32);
        self.dense.iter().for_each(|(entity, component)| {
            entity.reflect_write(writer);
            component.reflect_write(writer);
        });
    }

    /// Read a column previously written with [`Self::reflect_write`].
    pub fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        reader.read_u32().and_then(|count| {
            (0..count).try_fold(ComponentColumn::new(), |mut column, _| {
                EntityId::reflect_read(reader)
                    .and_then(|entity| T::reflect_read(reader).map(|component| (entity, component)))
                    .map(|(entity, component)| {
                        column.insert(entity, component);
                        column
                    })
            })
        })
    }
}

#[cfg(test)]
mod reflect_tests {
    use super::*;

    fn e(raw: u64) -> EntityId {
        EntityId::from_raw(raw)
    }

    #[test]
    fn schema_is_the_component_schema() {
        let col: ComponentColumn<u32> = ComponentColumn::new();
        assert_eq!(col.schema(), <u32 as Reflect>::SCHEMA);
    }

    #[test]
    fn empty_and_populated_columns_round_trip() {
        let empty: ComponentColumn<u32> = ComponentColumn::new();
        let mut w = BinaryWriter::new();
        empty.reflect_write(&mut w);
        let decoded =
            ComponentColumn::<u32>::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        assert!(decoded.is_empty());

        let mut col: ComponentColumn<u32> = ComponentColumn::new();
        col.insert(e(1), 10);
        col.insert(e(3), 30);
        let mut w = BinaryWriter::new();
        col.reflect_write(&mut w);
        let decoded =
            ComponentColumn::<u32>::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded.get(e(1)), Some(&10));
        assert_eq!(decoded.get(e(3)), Some(&30));
    }

    #[test]
    fn truncation_at_every_prefix_is_err() {
        let mut col: ComponentColumn<u32> = ComponentColumn::new();
        col.insert(e(1), 10);
        col.insert(e(2), 20);
        let mut w = BinaryWriter::new();
        col.reflect_write(&mut w);
        let bytes = w.into_bytes();
        for len in 0..bytes.len() {
            assert!(
                ComponentColumn::<u32>::reflect_read(&mut BinaryReader::new(&bytes[..len]))
                    .is_err()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(raw: u64) -> EntityId {
        EntityId::from_raw(raw)
    }

    #[test]
    fn insert_returns_previous_and_overwrites() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        assert_eq!(col.insert(e(1), 10), None);
        assert_eq!(col.insert(e(1), 20), Some(10));
        assert_eq!(col.get(e(1)), Some(&20));
        assert_eq!(col.len(), 1);
    }

    #[test]
    fn default_column_is_empty() {
        let col: ComponentColumn<i32> = ComponentColumn::default();
        assert!(col.is_empty());
        assert_eq!(col.len(), 0);
    }

    #[test]
    fn get_get_mut_contains_present_and_absent() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        col.insert(e(2), 7);
        assert!(col.contains(e(2)));
        assert_eq!(col.get(e(2)), Some(&7));
        *col.get_mut(e(2)).unwrap() = 9;
        assert_eq!(col.get(e(2)), Some(&9));
        assert!(!col.contains(e(5)));
        assert!(col.get(e(5)).is_none());
        assert!(col.get_mut(e(5)).is_none());
    }

    #[test]
    fn remove_present_and_absent() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        col.insert(e(1), 1);
        assert_eq!(col.remove(e(1)), Some(1));
        assert_eq!(col.remove(e(1)), None);
        assert!(col.is_empty());
    }

    #[test]
    fn iter_is_ascending_by_entity_id() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        col.insert(e(3), 30);
        col.insert(e(1), 10);
        col.insert(e(2), 20);
        let ids: Vec<(u64, i32)> = col.iter().map(|(id, v)| (id.raw(), *v)).collect();
        assert_eq!(ids, vec![(1, 10), (2, 20), (3, 30)]);
    }

    #[test]
    fn iter_mut_is_ascending_and_mutates() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        col.insert(e(2), 20);
        col.insert(e(1), 10);
        let ids: Vec<u64> = col
            .iter_mut()
            .map(|(id, v)| {
                *v += 1;
                id.raw()
            })
            .collect();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(col.get(e(1)), Some(&11));
        assert_eq!(col.get(e(2)), Some(&21));
    }

    // --- sparse-set invariants -------------------------------------------
    //
    // The tests above prove the column's *contract*, which is storage-agnostic.
    // These prove the thing that can actually break now: that `sparse` still
    // points where `dense` moved to.

    /// An insertion in the middle shifts every later entry, so every later
    /// entity's index must be rewritten. Getting this wrong is silent — the
    /// column returns the *neighbour's* component rather than erring.
    #[test]
    fn inserting_in_the_middle_reindexes_the_shifted_tail() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        col.insert(e(1), 10);
        col.insert(e(4), 40);
        col.insert(e(9), 90);
        col.insert(e(2), 20);
        col.insert(e(5), 50);
        [(1, 10), (2, 20), (4, 40), (5, 50), (9, 90)]
            .into_iter()
            .for_each(|(id, want)| assert_eq!(col.get(e(id)), Some(&want), "entity {id}"));
        let order: Vec<u64> = col.iter().map(|(id, _)| id.raw()).collect();
        assert_eq!(order, vec![1, 2, 4, 5, 9]);
    }

    /// The same hazard on the way out.
    #[test]
    fn removing_from_the_middle_reindexes_the_shifted_tail() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        (1..=6).for_each(|id| {
            col.insert(e(id), (id * 10) as i32);
        });
        assert_eq!(col.remove(e(3)), Some(30));
        assert!(!col.contains(e(3)));
        [(1, 10), (2, 20), (4, 40), (5, 50), (6, 60)]
            .into_iter()
            .for_each(|(id, want)| assert_eq!(col.get(e(id)), Some(&want), "entity {id}"));
        assert_eq!(col.len(), 5);
    }

    /// A slot returned to the free list and handed out again must land in a
    /// clean entry, not inherit the previous occupant's component.
    #[test]
    fn a_reused_slot_starts_absent() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        col.insert(e(2), 20);
        col.insert(e(7), 70);
        assert_eq!(col.remove(e(7)), Some(70));
        assert!(col.get(e(7)).is_none());
        assert_eq!(col.insert(e(7), 77), None, "the slot was genuinely empty");
        assert_eq!(col.get(e(7)), Some(&77));
    }

    /// `sparse` lags the id space until something is stored, so a query for a
    /// slot past its end must read as absent rather than panic.
    #[test]
    fn a_slot_past_the_sparse_array_is_absent() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        col.insert(e(1), 10);
        assert!(col.get(e(10_000)).is_none());
        assert!(col.get_mut(e(10_000)).is_none());
        assert!(!col.contains(e(10_000)));
        assert_eq!(col.remove(e(10_000)), None);
    }

    /// Overwriting is the per-frame path (a transform update), and it must not
    /// disturb any other entity's index.
    #[test]
    fn overwriting_leaves_every_other_index_intact() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        (1..=8).for_each(|id| {
            col.insert(e(id), (id * 10) as i32);
        });
        (1..=8).rev().for_each(|id| {
            assert_eq!(col.insert(e(id), (id * 100) as i32), Some((id * 10) as i32));
        });
        (1..=8).for_each(|id| assert_eq!(col.get(e(id)), Some(&((id * 100) as i32))));
        assert_eq!(col.len(), 8);
    }

    /// Emptying the column completely and refilling it must work — the tail
    /// re-index has an empty-tail case that `max()` has to survive.
    #[test]
    fn emptying_and_refilling_round_trips() {
        let mut col: ComponentColumn<i32> = ComponentColumn::new();
        (1..=4).for_each(|id| {
            col.insert(e(id), id as i32);
        });
        (1..=4).for_each(|id| {
            assert_eq!(col.remove(e(id)), Some(id as i32));
        });
        assert!(col.is_empty());
        col.insert(e(3), 33);
        assert_eq!(col.get(e(3)), Some(&33));
        assert_eq!(col.len(), 1);
    }
}
