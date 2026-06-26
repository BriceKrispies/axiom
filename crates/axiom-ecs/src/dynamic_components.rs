//! An app-blind, type-erased dynamic component store — safe, `Reflect`-backed.

use std::collections::BTreeMap;

use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, KernelResult, Reflect, TypeSchema};

/// One component type's column: its schema (for description) and the per-entity
/// serialized bytes.
#[derive(Debug)]
struct DynColumn {
    schema: TypeSchema,
    entries: BTreeMap<EntityId, Vec<u8>>,
}

/// A store of components whose types the storage was never told about at
/// compile time, keyed by each type's [`Reflect`] schema name.
///
/// This is the **app-blind** path: any module or agent can `insert`/`get` a
/// `T: Reflect` without the store — or the app — naming `T`. It pays for that
/// flexibility with serialization: components live as bytes, so [`Self::get`]
/// deserializes to an **owned `T`** (there is no borrowed `&T`). In exchange it
/// is entirely safe and deterministic — no `unsafe`, no `Any`, no `downcast` —
/// and a type mismatch surfaces as a clean [`KernelResult`] error, never
/// undefined behavior.
///
/// The static [`crate::World`] remains the zero-cost *borrowed* path for the hot
/// loop; this serves the app-blind / cold cases (modded content, tooling,
/// agent-authored components).
///
/// ## Shape
/// All the type-agnostic storage branching (which column, which entity, present
/// or absent) lives in a **monomorphic byte core** (`put_bytes` / `get_bytes` /
/// `has_bytes` / `take_bytes`). The generic methods are a thin typed shell whose
/// only job is `Reflect` (de)serialization. That split is deliberate: the only
/// thing that genuinely must be generic is turning a `T` into bytes and back, so
/// that is the only thing that is — keeping the branching logic in one place,
/// exercised once, instead of smeared across every instantiation.
#[derive(Debug, Default)]
pub struct DynamicComponents {
    columns: BTreeMap<&'static str, DynColumn>,
}

impl DynamicComponents {
    /// Create an empty store.
    pub fn new() -> Self {
        DynamicComponents {
            columns: BTreeMap::new(),
        }
    }

    // ---- monomorphic byte core: all the type-agnostic storage branching ----

    /// Store `bytes` for `entity` under the column named `name`, creating the
    /// column (recording `schema`) on first use.
    fn put_bytes(
        &mut self,
        name: &'static str,
        schema: TypeSchema,
        entity: EntityId,
        bytes: Vec<u8>,
    ) {
        self.columns
            .entry(name)
            .or_insert_with(|| DynColumn {
                schema,
                entries: BTreeMap::new(),
            })
            .entries
            .insert(entity, bytes);
    }

    /// The bytes stored for `entity` under `name`, if any.
    fn get_bytes(&self, name: &'static str, entity: EntityId) -> Option<&[u8]> {
        self.columns
            .get(name)
            .and_then(|column| column.entries.get(&entity).map(Vec::as_slice))
    }

    /// Whether `entity` has bytes stored under `name`.
    fn has_bytes(&self, name: &'static str, entity: EntityId) -> bool {
        self.columns
            .get(name)
            .is_some_and(|column| column.entries.contains_key(&entity))
    }

    /// Remove `entity`'s bytes under `name`, returning whether they existed.
    fn take_bytes(&mut self, name: &'static str, entity: EntityId) -> bool {
        self.columns
            .get_mut(name)
            .is_some_and(|column| column.entries.remove(&entity).is_some())
    }

    // ---- typed shell: the only generic part is Reflect (de)serialization ----

    /// Set `entity`'s component of type `T`, serializing it. The type is keyed
    /// by its `Reflect` schema name; the first insert of a type records its
    /// schema for [`Self::describe`].
    pub fn insert<T: Reflect>(&mut self, entity: EntityId, value: T) {
        let mut writer = BinaryWriter::new();
        value.reflect_write(&mut writer);
        self.put_bytes(T::SCHEMA.name(), T::SCHEMA, entity, writer.into_bytes());
    }

    /// Read `entity`'s component of type `T`, deserializing an owned value.
    ///
    /// `Ok(None)` if the type or entity is absent; `Ok(Some(value))` on a clean
    /// decode; `Err(..)` if the stored bytes do not decode as `T` (e.g. a
    /// schema-name collision with a differently-shaped type) — a graceful
    /// failure, never UB.
    pub fn get<T: Reflect>(&self, entity: EntityId) -> KernelResult<Option<T>> {
        self.get_bytes(T::SCHEMA.name(), entity)
            .map(|bytes| T::reflect_read(&mut BinaryReader::new(bytes)))
            .transpose()
    }

    /// Whether `entity` has a component of type `T`.
    pub fn contains<T: Reflect>(&self, entity: EntityId) -> bool {
        self.has_bytes(T::SCHEMA.name(), entity)
    }

    /// Remove `entity`'s component of type `T`, returning whether it existed.
    pub fn remove<T: Reflect>(&mut self, entity: EntityId) -> bool {
        self.take_bytes(T::SCHEMA.name(), entity)
    }

    /// Describe the store: per registered component type, its name, schema, and
    /// entry count — in ascending name order.
    pub fn describe(&self) -> Vec<(&'static str, TypeSchema, usize)> {
        self.columns
            .iter()
            .map(|(name, column)| (*name, column.schema, column.entries.len()))
            .collect()
    }

    /// The number of distinct component types stored.
    pub fn component_types(&self) -> usize {
        self.columns.len()
    }

    /// Whether no component types are stored.
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::FieldSchema;

    fn e(raw: u64) -> EntityId {
        EntityId::from_raw(raw)
    }

    // Two types deliberately sharing a schema name but with different shapes —
    // the name-collision hazard. `Marker` serializes to zero bytes; `One`
    // serializes to four. Reading the wrong one must fail gracefully. `One` is
    // the workhorse the typed-shell tests use so the generic methods have a
    // single, fully-exercised instantiation.
    struct Marker;
    impl Reflect for Marker {
        const SCHEMA: TypeSchema = TypeSchema::new("Clash", &[]);
        fn reflect_write(&self, _w: &mut BinaryWriter) {}
        fn reflect_read(_r: &mut BinaryReader<'_>) -> KernelResult<Self> {
            Ok(Marker)
        }
    }
    struct One(u32);
    impl Reflect for One {
        const SCHEMA: TypeSchema = TypeSchema::new("Clash", &[FieldSchema::new("a", "u32")]);
        fn reflect_write(&self, w: &mut BinaryWriter) {
            self.0.reflect_write(w);
        }
        fn reflect_read(r: &mut BinaryReader<'_>) -> KernelResult<Self> {
            Ok(One(u32::reflect_read(r)?))
        }
    }

    #[test]
    fn new_and_default_are_empty() {
        assert!(DynamicComponents::new().is_empty());
        assert!(DynamicComponents::default().is_empty());
        assert_eq!(DynamicComponents::new().component_types(), 0);
    }

    #[test]
    fn inserts_and_gets_owned_values_by_type() {
        let mut store = DynamicComponents::new();
        // Absent type entirely (get_bytes `None` arm; get<One> skips its closure).
        assert_eq!(store.get::<One>(e(1)).unwrap().map(|o| o.0), None);

        store.insert(e(1), One(10));
        store.insert(e(2), One(20)); // second insert: put_bytes `or_insert_with` skipped

        assert_eq!(store.component_types(), 1);
        assert!(!store.is_empty());
        // Present (get_bytes Some+Some; get<One> closure -> Ok).
        assert_eq!(store.get::<One>(e(1)).unwrap().map(|o| o.0), Some(10));
        assert_eq!(store.get::<One>(e(2)).unwrap().map(|o| o.0), Some(20));
        // Present type, absent entity (get_bytes Some+None).
        assert_eq!(store.get::<One>(e(3)).unwrap().map(|o| o.0), None);
    }

    #[test]
    fn contains_and_remove() {
        let mut store = DynamicComponents::new();
        // Absent type (has_bytes / take_bytes `None` arm).
        assert!(!store.contains::<One>(e(1)));
        assert!(!store.remove::<One>(e(1)));

        store.insert(e(1), One(5));
        assert!(store.contains::<One>(e(1))); // present
        assert!(!store.contains::<One>(e(2))); // present type, absent entity

        assert!(store.remove::<One>(e(1))); // present -> removed
        assert!(!store.contains::<One>(e(1)));
        assert!(!store.remove::<One>(e(1))); // already gone (present type, absent entity)
    }

    #[test]
    fn describe_lists_types_schemas_and_counts() {
        let mut store = DynamicComponents::new();
        store.insert(e(1), One(1));
        store.insert(e(2), One(2));
        let description = store.describe();
        assert_eq!(description.len(), 1);
        assert_eq!(description[0].0, "Clash");
        assert_eq!(description[0].1.name(), "Clash");
        assert_eq!(description[0].2, 2);
    }

    #[test]
    fn type_mismatch_fails_gracefully_not_unsafely() {
        // `Marker` inserted first records the "Clash" column (covers the
        // `put_bytes` create arm for the Marker instantiation), then `One`
        // shares the same column name with a wider shape.
        let mut store = DynamicComponents::new();
        store.insert(e(1), Marker); // 0 bytes under "Clash"
        store.insert(e(2), One(5)); // 4 bytes under "Clash"

        // Each type reads its own bytes back cleanly (closure -> Ok).
        assert!(store.get::<Marker>(e(1)).unwrap().is_some());
        assert_eq!(store.get::<One>(e(2)).unwrap().map(|o| o.0), Some(5));

        // Reading `One` from the zero-byte `Marker` entry wants 4 bytes it
        // doesn't have -> a clean decode error, never UB (closure -> Err).
        assert!(store.get::<One>(e(1)).is_err());
    }
}
