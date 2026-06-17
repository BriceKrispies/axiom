//! A sparse, deterministic column of one component type.

use std::collections::BTreeMap;

use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, KernelResult, Reflect, TypeSchema};

/// A sparse store of one component type `T`, keyed by [`EntityId`].
///
/// An entity appears in a column only if it has that component, so storage and
/// iteration cost only what is actually present — the "open" part of the open
/// component model: each component type is its own independently-addressable
/// column. Entries are held in a `BTreeMap`, so iteration is always in
/// ascending entity-id order on every platform.
#[derive(Debug, Clone)]
pub struct ComponentColumn<T> {
    entries: BTreeMap<EntityId, T>,
}

impl<T> ComponentColumn<T> {
    /// Create an empty column.
    pub fn new() -> Self {
        ComponentColumn {
            entries: BTreeMap::new(),
        }
    }

    /// Set the component for `entity`, returning the previous value if any.
    pub fn insert(&mut self, entity: EntityId, component: T) -> Option<T> {
        self.entries.insert(entity, component)
    }

    /// Borrow `entity`'s component, if present.
    pub fn get(&self, entity: EntityId) -> Option<&T> {
        self.entries.get(&entity)
    }

    /// Mutably borrow `entity`'s component, if present.
    pub fn get_mut(&mut self, entity: EntityId) -> Option<&mut T> {
        self.entries.get_mut(&entity)
    }

    /// Whether `entity` has this component.
    pub fn contains(&self, entity: EntityId) -> bool {
        self.entries.contains_key(&entity)
    }

    /// Remove `entity`'s component, returning it if present.
    pub fn remove(&mut self, entity: EntityId) -> Option<T> {
        self.entries.remove(&entity)
    }

    /// Iterate `(entity, &component)` in ascending entity-id order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &T)> {
        self.entries.iter().map(|(id, c)| (*id, c))
    }

    /// The number of entities in this column.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the column has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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
        writer.write_u32(self.entries.len() as u32);
        self.entries.iter().for_each(|(entity, component)| {
            entity.reflect_write(writer);
            component.reflect_write(writer);
        });
    }

    /// Read a column previously written with [`Self::reflect_write`].
    pub fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        reader.read_u32().and_then(|count| {
            (0..count).try_fold(ComponentColumn::new(), |mut column, _| {
                EntityId::reflect_read(reader)
                    .and_then(|entity| {
                        T::reflect_read(reader).map(|component| (entity, component))
                    })
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
}
