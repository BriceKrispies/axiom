//! Typed material/substance classifiers and properties, layered on definitions.
//!
//! Identity, durable name, duplicate rejection, string tags, and generic
//! properties all live in the Phase-2 [`crate::DefinitionRegistry`]
//! (`DefinitionKind::Material` / `Substance`). This catalog adds the *typed*
//! material/substance overlay — an opaque classifier kind and typed numeric
//! properties keyed by code, so durable identity is never only a tag string.

use std::collections::BTreeMap;

use crate::ids::DefinitionId;

/// Define a `u32`-backed deterministic classifier/key newtype.
macro_rules! code_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u32);

        impl $name {
            /// Construct from a deterministic code.
            pub const fn new(code: u32) -> Self {
                $name(code)
            }

            /// The raw code.
            pub const fn code(self) -> u32 {
                self.0
            }
        }
    };
}

code_newtype!(
    MaterialKind,
    "An opaque classifier for a material (e.g. metal vs mineral)."
);
code_newtype!(
    SubstanceKind,
    "An opaque classifier for a substance (e.g. liquid vs powder)."
);
code_newtype!(MaterialProperty, "A typed property key for a material.");
code_newtype!(SubstanceProperty, "A typed property key for a substance.");

/// One catalog entry: whether the definition is a substance (vs material), its
/// classifier code, and its typed numeric properties.
#[derive(Debug, Clone)]
struct CatalogEntry {
    is_substance: bool,
    classifier: u32,
    properties: BTreeMap<u32, i64>,
}

/// The typed material/substance overlay over the definition registry.
#[derive(Debug, Clone, Default)]
pub struct MaterialCatalog {
    entries: BTreeMap<DefinitionId, CatalogEntry>,
}

impl MaterialCatalog {
    /// Create an empty catalog.
    pub fn new() -> Self {
        MaterialCatalog {
            entries: BTreeMap::new(),
        }
    }

    /// Catalog a definition as a material with a classifier and typed properties.
    /// Returns whether it was newly cataloged (`false` if already present).
    pub fn register_material(
        &mut self,
        definition: DefinitionId,
        kind: MaterialKind,
        properties: &[(MaterialProperty, i64)],
    ) -> bool {
        let free = !self.entries.contains_key(&definition);
        let props = properties
            .iter()
            .fold(BTreeMap::new(), |mut map, (key, value)| {
                map.insert(key.code(), *value);
                map
            });
        free.then(|| {
            self.entries.insert(
                definition,
                CatalogEntry {
                    is_substance: false,
                    classifier: kind.code(),
                    properties: props,
                },
            )
        });
        free
    }

    /// Catalog a definition as a substance with a classifier and typed properties.
    pub fn register_substance(
        &mut self,
        definition: DefinitionId,
        kind: SubstanceKind,
        properties: &[(SubstanceProperty, i64)],
    ) -> bool {
        let free = !self.entries.contains_key(&definition);
        let props = properties
            .iter()
            .fold(BTreeMap::new(), |mut map, (key, value)| {
                map.insert(key.code(), *value);
                map
            });
        free.then(|| {
            self.entries.insert(
                definition,
                CatalogEntry {
                    is_substance: true,
                    classifier: kind.code(),
                    properties: props,
                },
            )
        });
        free
    }

    /// The material classifier of a definition, if it is cataloged as a material.
    pub fn material_kind(&self, definition: DefinitionId) -> Option<MaterialKind> {
        self.entries
            .get(&definition)
            .filter(|entry| !entry.is_substance)
            .map(|entry| MaterialKind::new(entry.classifier))
    }

    /// The substance classifier of a definition, if cataloged as a substance.
    pub fn substance_kind(&self, definition: DefinitionId) -> Option<SubstanceKind> {
        self.entries
            .get(&definition)
            .filter(|entry| entry.is_substance)
            .map(|entry| SubstanceKind::new(entry.classifier))
    }

    /// A typed material property value, if the definition is a material with it.
    pub fn material_property(
        &self,
        definition: DefinitionId,
        key: MaterialProperty,
    ) -> Option<i64> {
        self.entries
            .get(&definition)
            .filter(|entry| !entry.is_substance)
            .and_then(|entry| entry.properties.get(&key.code()).copied())
    }

    /// A typed substance property value, if the definition is a substance with it.
    pub fn substance_property(
        &self,
        definition: DefinitionId,
        key: SubstanceProperty,
    ) -> Option<i64> {
        self.entries
            .get(&definition)
            .filter(|entry| entry.is_substance)
            .and_then(|entry| entry.properties.get(&key.code()).copied())
    }

    /// Whether a definition is cataloged (as either a material or substance).
    pub fn contains(&self, definition: DefinitionId) -> bool {
        self.entries.contains_key(&definition)
    }

    /// All cataloged definition ids, in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = DefinitionId> + '_ {
        self.entries.keys().copied()
    }

    /// The number of cataloged definitions.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(raw: u64) -> DefinitionId {
        DefinitionId::from_raw(raw)
    }

    #[test]
    fn new_and_default_are_empty() {
        assert!(MaterialCatalog::new().is_empty());
        assert_eq!(MaterialCatalog::new().len(), 0);
        assert!(MaterialCatalog::default().is_empty());
    }

    #[test]
    fn materials_and_substances_are_classified_separately() {
        let mut catalog = MaterialCatalog::new();
        assert!(catalog.register_material(
            d(1),
            MaterialKind::new(10),
            &[(MaterialProperty::new(1), 7)]
        ));
        assert!(catalog.register_substance(
            d(2),
            SubstanceKind::new(20),
            &[(SubstanceProperty::new(2), 9)]
        ));
        assert!(!catalog.register_material(d(1), MaterialKind::new(99), &[]));
        assert_eq!(catalog.len(), 2);

        assert_eq!(catalog.material_kind(d(1)), Some(MaterialKind::new(10)));
        assert_eq!(
            catalog.material_kind(d(2)),
            None,
            "a substance is not a material"
        );
        assert_eq!(catalog.substance_kind(d(2)), Some(SubstanceKind::new(20)));
        assert_eq!(catalog.substance_kind(d(1)), None);
        assert_eq!(
            catalog.material_property(d(1), MaterialProperty::new(1)),
            Some(7)
        );
        assert_eq!(
            catalog.material_property(d(1), MaterialProperty::new(99)),
            None
        );
        assert_eq!(
            catalog.substance_property(d(2), SubstanceProperty::new(2)),
            Some(9)
        );
        assert_eq!(
            catalog.substance_property(d(1), SubstanceProperty::new(2)),
            None
        );
        assert!(catalog.contains(d(1)));
        assert!(!catalog.contains(d(3)));
        let ids: Vec<u64> = catalog.iter().map(|id| id.raw()).collect();
        assert_eq!(ids, vec![1, 2]);
    }
}
