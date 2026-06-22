//! The deterministic definition registry: data-defined simulation concepts.

use std::collections::{BTreeMap, BTreeSet};

use crate::fact::FactValue;
use crate::ids::DefinitionId;

/// The category of a [`Definition`]. sim-core stores the category but implements
/// none of these behaviors — later phases give them meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefinitionKind {
    /// A material (e.g. a future "iron").
    Material,
    /// A substance (e.g. a future "blood").
    Substance,
    /// A body plan.
    BodyPlan,
    /// A tissue.
    Tissue,
    /// A behavior.
    Behavior,
    /// A process definition.
    Process,
    /// An effect definition.
    Effect,
    /// A job definition.
    Job,
    /// A need definition.
    Need,
    /// A thought definition.
    Thought,
    /// An uncategorized definition.
    Generic,
}

/// A deterministic set of string tags, iterated in lexicographic order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagSet {
    tags: BTreeSet<String>,
}

impl TagSet {
    /// An empty tag set.
    pub fn new() -> Self {
        TagSet {
            tags: BTreeSet::new(),
        }
    }

    /// Builder: add a tag and return the set.
    pub fn with(mut self, tag: &str) -> Self {
        self.tags.insert(tag.to_string());
        self
    }

    /// Whether the set contains `tag`.
    pub fn contains(&self, tag: &str) -> bool {
        self.tags.contains(tag)
    }

    /// The tags, in lexicographic order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().map(String::as_str)
    }

    /// The number of tags.
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    /// Whether there are no tags.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }
}

/// A deterministic set of named properties, iterated in lexicographic order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PropertySet {
    properties: BTreeMap<String, FactValue>,
}

impl PropertySet {
    /// An empty property set.
    pub fn new() -> Self {
        PropertySet {
            properties: BTreeMap::new(),
        }
    }

    /// Builder: set a property and return the set.
    pub fn with(mut self, name: &str, value: FactValue) -> Self {
        self.properties.insert(name.to_string(), value);
        self
    }

    /// The value of a named property, if present.
    pub fn get(&self, name: &str) -> Option<FactValue> {
        self.properties.get(name).copied()
    }

    /// The properties, in lexicographic name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, FactValue)> {
        self.properties
            .iter()
            .map(|(name, value)| (name.as_str(), *value))
    }

    /// The number of properties.
    pub fn len(&self) -> usize {
        self.properties.len()
    }

    /// Whether there are no properties.
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }
}

/// A data-defined simulation concept: a kind, a durable name, tags, properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    id: DefinitionId,
    kind: DefinitionKind,
    name: String,
    tags: TagSet,
    properties: PropertySet,
}

impl Definition {
    /// The definition's deterministic id.
    pub const fn id(&self) -> DefinitionId {
        self.id
    }

    /// The definition's kind.
    pub const fn kind(&self) -> DefinitionKind {
        self.kind
    }

    /// The durable name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether this definition carries `tag`.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(tag)
    }

    /// The value of a named property, if present.
    pub fn property(&self, name: &str) -> Option<FactValue> {
        self.properties.get(name)
    }

    /// The tag set.
    pub fn tags(&self) -> &TagSet {
        &self.tags
    }

    /// The property set.
    pub fn properties(&self) -> &PropertySet {
        &self.properties
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// Derive a deterministic, order-independent id from a durable name (FNV-1a).
fn id_for(name: &str) -> DefinitionId {
    let hash = name.bytes().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(FNV_PRIME)
    });
    DefinitionId::from_raw(hash)
}

/// A deterministic registry of [`Definition`]s, keyed by [`DefinitionId`] and by
/// durable name. Ids are derived from the name (order-independent), so the same
/// definitions register to the same ids on every run.
#[derive(Debug, Clone, Default)]
pub struct DefinitionRegistry {
    by_id: BTreeMap<DefinitionId, Definition>,
    by_name: BTreeMap<String, DefinitionId>,
}

impl DefinitionRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        DefinitionRegistry {
            by_id: BTreeMap::new(),
            by_name: BTreeMap::new(),
        }
    }

    /// Register a definition. Returns its id, or `None` if the durable name (or
    /// its derived id) is already registered — duplicates are rejected cleanly.
    pub fn register(
        &mut self,
        kind: DefinitionKind,
        name: &str,
        tags: TagSet,
        properties: PropertySet,
    ) -> Option<DefinitionId> {
        let id = id_for(name);
        let free = !(self.by_name.contains_key(name) | self.by_id.contains_key(&id));
        free.then(|| {
            self.by_name.insert(name.to_string(), id);
            self.by_id.insert(
                id,
                Definition {
                    id,
                    kind,
                    name: name.to_string(),
                    tags,
                    properties,
                },
            );
        });
        free.then_some(id)
    }

    /// Look up a definition by id.
    pub fn get(&self, id: DefinitionId) -> Option<&Definition> {
        self.by_id.get(&id)
    }

    /// Look up a definition by durable name.
    pub fn by_name(&self, name: &str) -> Option<&Definition> {
        self.by_name.get(name).and_then(|id| self.by_id.get(id))
    }

    /// The id registered for a durable name, if any.
    pub fn id_of(&self, name: &str) -> Option<DefinitionId> {
        self.by_name.get(name).copied()
    }

    /// Definitions carrying `tag`, in ascending id order.
    pub fn by_tag<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Definition> {
        self.by_id
            .values()
            .filter(move |definition| definition.has_tag(tag))
    }

    /// Definitions whose `name` property equals `value`, in ascending id order.
    pub fn by_property<'a>(
        &'a self,
        name: &'a str,
        value: FactValue,
    ) -> impl Iterator<Item = &'a Definition> {
        self.by_id
            .values()
            .filter(move |definition| definition.property(name) == Some(value))
    }

    /// All definitions, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &Definition> {
        self.by_id.values()
    }

    /// The number of registered definitions.
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
    fn tagset_and_propertyset_are_deterministic() {
        let tags = TagSet::new()
            .with("flammable")
            .with("conductive")
            .with("flammable");
        assert!(tags.contains("flammable"));
        assert!(!tags.contains("brittle"));
        assert_eq!(tags.len(), 2, "duplicate tag collapses");
        let ordered: Vec<&str> = tags.iter().collect();
        assert_eq!(
            ordered,
            vec!["conductive", "flammable"],
            "lexicographic order"
        );
        assert!(!tags.is_empty());
        assert!(TagSet::new().is_empty());

        let props = PropertySet::new()
            .with("hardness", FactValue::Unsigned(5))
            .with("charge", FactValue::Signed(-1));
        assert_eq!(props.get("hardness"), Some(FactValue::Unsigned(5)));
        assert_eq!(props.get("missing"), None);
        assert_eq!(props.len(), 2);
        let names: Vec<&str> = props.iter().map(|(name, _)| name).collect();
        assert_eq!(names, vec!["charge", "hardness"]);
        assert!(!props.is_empty());
        assert!(PropertySet::new().is_empty());
    }

    #[test]
    fn register_rejects_duplicate_names() {
        let mut registry = DefinitionRegistry::new();
        assert!(registry.is_empty());
        let id = registry
            .register(
                DefinitionKind::Material,
                "iron",
                TagSet::new().with("metal"),
                PropertySet::new(),
            )
            .unwrap();
        assert_eq!(registry.len(), 1);
        // A duplicate durable name is rejected cleanly.
        assert!(registry
            .register(
                DefinitionKind::Substance,
                "iron",
                TagSet::new(),
                PropertySet::new()
            )
            .is_none());
        assert_eq!(
            registry.len(),
            1,
            "the registry is unchanged after a rejected duplicate"
        );
        assert_eq!(registry.id_of("iron"), Some(id));
        assert_eq!(registry.get(id).unwrap().kind(), DefinitionKind::Material);
        assert_eq!(registry.by_name("iron").unwrap().id(), id);
    }

    #[test]
    fn query_by_tag_and_property_is_ascending() {
        let mut registry = DefinitionRegistry::new();
        let iron = registry
            .register(
                DefinitionKind::Material,
                "iron",
                TagSet::new().with("solid"),
                PropertySet::new().with("hardness", FactValue::Unsigned(5)),
            )
            .unwrap();
        let _water = registry
            .register(
                DefinitionKind::Substance,
                "water",
                TagSet::new().with("liquid"),
                PropertySet::new().with("hardness", FactValue::Unsigned(0)),
            )
            .unwrap();
        let steel = registry
            .register(
                DefinitionKind::Material,
                "steel",
                TagSet::new().with("solid"),
                PropertySet::new().with("hardness", FactValue::Unsigned(5)),
            )
            .unwrap();
        let solids: Vec<DefinitionId> = registry.by_tag("solid").map(Definition::id).collect();
        assert!(solids.contains(&iron) && solids.contains(&steel) && solids.len() == 2);
        let hard: Vec<DefinitionId> = registry
            .by_property("hardness", FactValue::Unsigned(5))
            .map(Definition::id)
            .collect();
        assert!(hard.contains(&iron) && hard.contains(&steel) && hard.len() == 2);
        assert_eq!(registry.by_tag("gas").count(), 0);
        assert_eq!(
            registry
                .by_property("hardness", FactValue::Unsigned(99))
                .count(),
            0
        );
    }

    #[test]
    fn ids_are_order_independent_and_name_derived() {
        let mut a = DefinitionRegistry::new();
        a.register(
            DefinitionKind::Generic,
            "alpha",
            TagSet::new(),
            PropertySet::new(),
        );
        let beta_a = a
            .register(
                DefinitionKind::Generic,
                "beta",
                TagSet::new(),
                PropertySet::new(),
            )
            .unwrap();
        let mut b = DefinitionRegistry::new();
        let beta_b = b
            .register(
                DefinitionKind::Generic,
                "beta",
                TagSet::new(),
                PropertySet::new(),
            )
            .unwrap();
        assert_eq!(
            beta_a, beta_b,
            "ids derive from the name, not registration order"
        );
        assert_eq!(beta_a, id_for("beta"));
    }

    #[test]
    fn tags_and_properties_are_queryable_through_a_definition() {
        let mut registry = DefinitionRegistry::new();
        let id = registry
            .register(
                DefinitionKind::Material,
                "copper",
                TagSet::new().with("conductive"),
                PropertySet::new().with("hardness", FactValue::Unsigned(3)),
            )
            .unwrap();
        let definition = registry.get(id).unwrap();
        assert!(definition.has_tag("conductive"));
        assert!(!definition.has_tag("flammable"));
        assert_eq!(
            definition.property("hardness"),
            Some(FactValue::Unsigned(3))
        );
        assert_eq!(definition.property("missing"), None);
        assert_eq!(definition.name(), "copper");
        assert_eq!(definition.tags().len(), 1);
        assert_eq!(definition.properties().len(), 1);
        let names: Vec<&str> = registry.iter().map(Definition::name).collect();
        assert_eq!(names.len(), 1);
        assert!(registry.by_name("absent").is_none());
        assert!(registry.id_of("absent").is_none());
    }
}
