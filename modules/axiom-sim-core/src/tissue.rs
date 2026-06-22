//! Generic tissue definitions: reusable material-like layers a body part has.
//!
//! Tissue definitions are pure data (kind + durable name + tags + typed numeric
//! properties). sim-core implements no biology — no bleeding, infection, pain, or
//! healing. Later phases give the kinds/tags meaning.

use std::collections::{BTreeMap, BTreeSet};

use crate::ids::TissueId;

/// The category of a tissue. Opaque to sim-core; later phases assign meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TissueKind {
    /// A covering layer (e.g. a future skin/hide).
    Covering,
    /// Muscle.
    Muscle,
    /// Bone.
    Bone,
    /// Nerve.
    Nerve,
    /// Blood.
    Blood,
    /// An organ tissue.
    Organ,
    /// A fluid tissue.
    Fluid,
    /// An uncategorized tissue.
    Generic,
}

const TISSUE_KINDS: [TissueKind; 8] = [
    TissueKind::Covering,
    TissueKind::Muscle,
    TissueKind::Bone,
    TissueKind::Nerve,
    TissueKind::Blood,
    TissueKind::Organ,
    TissueKind::Fluid,
    TissueKind::Generic,
];

impl TissueKind {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<TissueKind> {
        TISSUE_KINDS.get(code as usize).copied()
    }

    /// The kind's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// A typed numeric property key for a tissue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TissueProperty(u32);

impl TissueProperty {
    /// Construct from a deterministic code.
    pub const fn new(code: u32) -> Self {
        TissueProperty(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// A reference to a tissue definition placed at an ordinal depth in a body part,
/// outermost first. Pure data — the index orders layers deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TissueLayer {
    tissue: TissueId,
    depth: u32,
}

impl TissueLayer {
    /// A layer of `tissue` at ordinal `depth` (0 = outermost).
    pub const fn new(tissue: TissueId, depth: u32) -> Self {
        TissueLayer { tissue, depth }
    }

    /// The referenced tissue definition.
    pub const fn tissue(self) -> TissueId {
        self.tissue
    }

    /// The layer depth (0 = outermost).
    pub const fn depth(self) -> u32 {
        self.depth
    }
}

/// A data-defined tissue: a kind, durable name, tags, and typed properties.
#[derive(Debug, Clone)]
pub struct TissueDefinition {
    id: TissueId,
    kind: TissueKind,
    name: String,
    tags: BTreeSet<String>,
    properties: BTreeMap<u32, i64>,
}

impl TissueDefinition {
    /// The deterministic id.
    pub const fn id(&self) -> TissueId {
        self.id
    }

    /// The tissue kind.
    pub const fn kind(&self) -> TissueKind {
        self.kind
    }

    /// The durable name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether this tissue carries `tag`.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(tag)
    }

    /// A typed property value, if present.
    pub fn property(&self, key: TissueProperty) -> Option<i64> {
        self.properties.get(&key.code()).copied()
    }

    /// The tags, lexicographic.
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().map(String::as_str)
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// Derive a deterministic id from a durable name (FNV-1a).
fn id_for(name: &str) -> TissueId {
    let hash = name.bytes().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(FNV_PRIME)
    });
    TissueId::from_raw(hash)
}

/// A deterministic registry of tissue definitions, keyed by id and durable name.
#[derive(Debug, Clone, Default)]
pub struct TissueRegistry {
    by_id: BTreeMap<TissueId, TissueDefinition>,
    by_name: BTreeMap<String, TissueId>,
}

impl TissueRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        TissueRegistry {
            by_id: BTreeMap::new(),
            by_name: BTreeMap::new(),
        }
    }

    /// Register a tissue definition. Returns its id, or `None` if the durable name
    /// (or derived id) is already registered.
    pub fn register(
        &mut self,
        kind: TissueKind,
        name: &str,
        tags: &[&str],
        properties: &[(TissueProperty, i64)],
    ) -> Option<TissueId> {
        let id = id_for(name);
        let free = !(self.by_name.contains_key(name) | self.by_id.contains_key(&id));
        let tag_set = tags.iter().fold(BTreeSet::new(), |mut set, tag| {
            set.insert(tag.to_string());
            set
        });
        let prop_map = properties
            .iter()
            .fold(BTreeMap::new(), |mut map, (key, value)| {
                map.insert(key.code(), *value);
                map
            });
        free.then(|| {
            self.by_name.insert(name.to_string(), id);
            self.by_id.insert(
                id,
                TissueDefinition {
                    id,
                    kind,
                    name: name.to_string(),
                    tags: tag_set,
                    properties: prop_map,
                },
            );
        });
        free.then_some(id)
    }

    /// Look up by id.
    pub fn get(&self, id: TissueId) -> Option<&TissueDefinition> {
        self.by_id.get(&id)
    }

    /// Look up by durable name.
    pub fn by_name(&self, name: &str) -> Option<&TissueDefinition> {
        self.by_name.get(name).and_then(|id| self.by_id.get(id))
    }

    /// The id registered for a durable name, if any.
    pub fn id_of(&self, name: &str) -> Option<TissueId> {
        self.by_name.get(name).copied()
    }

    /// Tissues carrying `tag`, ascending by id.
    pub fn by_tag<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a TissueDefinition> {
        self.by_id
            .values()
            .filter(move |tissue| tissue.has_tag(tag))
    }

    /// Tissues whose typed property equals `value`, ascending by id.
    pub fn by_property(
        &self,
        key: TissueProperty,
        value: i64,
    ) -> impl Iterator<Item = &TissueDefinition> {
        self.by_id
            .values()
            .filter(move |tissue| tissue.property(key) == Some(value))
    }

    /// All tissues, ascending by id.
    pub fn iter(&self) -> impl Iterator<Item = &TissueDefinition> {
        self.by_id.values()
    }

    /// The number of registered tissues.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_codes_validate_and_round_trip() {
        assert_eq!(TissueKind::from_code(0), Some(TissueKind::Covering));
        assert_eq!(TissueKind::from_code(7), Some(TissueKind::Generic));
        assert_eq!(TissueKind::from_code(8), None);
        assert_eq!(TissueKind::Bone.code(), 2);
        assert_eq!(
            TissueKind::from_code(TissueKind::Blood.code()),
            Some(TissueKind::Blood)
        );
    }

    #[test]
    fn tissue_layer_carries_ordinal() {
        let layer = TissueLayer::new(TissueId::from_raw(3), 1);
        assert_eq!(layer.tissue(), TissueId::from_raw(3));
        assert_eq!(layer.depth(), 1);
        assert!(TissueLayer::new(TissueId::from_raw(3), 0) < layer);
    }

    #[test]
    fn register_rejects_duplicates_and_derives_name_id() {
        let mut registry = TissueRegistry::new();
        assert!(registry.is_empty());
        let id = registry
            .register(
                TissueKind::Covering,
                "test-covering",
                &["can-hold-residue", "protective"],
                &[(TissueProperty::new(1), 5)],
            )
            .unwrap();
        assert_eq!(id, id_for("test-covering"));
        assert_eq!(registry.len(), 1);
        assert!(registry
            .register(TissueKind::Muscle, "test-covering", &[], &[])
            .is_none());
        assert_eq!(registry.id_of("test-covering"), Some(id));
        let tissue = registry.get(id).unwrap();
        assert_eq!(tissue.kind(), TissueKind::Covering);
        assert_eq!(tissue.name(), "test-covering");
        assert!(tissue.has_tag("protective"));
        assert!(!tissue.has_tag("vital"));
        assert_eq!(tissue.property(TissueProperty::new(1)), Some(5));
        assert_eq!(tissue.property(TissueProperty::new(9)), None);
        assert_eq!(tissue.tags().count(), 2);
        assert_eq!(registry.by_name("test-covering").unwrap().id(), id);
        assert!(registry.by_name("absent").is_none());
        assert!(registry.id_of("absent").is_none());
    }

    #[test]
    fn query_by_tag_and_property_is_ascending() {
        let mut registry = TissueRegistry::new();
        let a = registry
            .register(
                TissueKind::Covering,
                "a",
                &["absorbent"],
                &[(TissueProperty::new(1), 2)],
            )
            .unwrap();
        let _b = registry
            .register(
                TissueKind::Bone,
                "b",
                &["structural"],
                &[(TissueProperty::new(1), 9)],
            )
            .unwrap();
        let c = registry
            .register(
                TissueKind::Muscle,
                "c",
                &["absorbent"],
                &[(TissueProperty::new(1), 2)],
            )
            .unwrap();
        let absorbent: Vec<TissueId> = registry
            .by_tag("absorbent")
            .map(TissueDefinition::id)
            .collect();
        assert!(absorbent.contains(&a) && absorbent.contains(&c) && absorbent.len() == 2);
        let two: Vec<TissueId> = registry
            .by_property(TissueProperty::new(1), 2)
            .map(TissueDefinition::id)
            .collect();
        assert!(two.contains(&a) && two.contains(&c) && two.len() == 2);
        assert_eq!(registry.by_tag("vital").count(), 0);
        assert_eq!(registry.iter().count(), 3);
    }
}
