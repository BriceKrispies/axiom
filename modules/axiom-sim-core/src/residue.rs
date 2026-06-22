//! A generic residue: a quantity of a substance/material at a location.

use std::collections::BTreeMap;

use axiom_ecs::EntityHandle;

use crate::cause::CauseRef;
use crate::ids::{DefinitionId, ResidueId};
use crate::quantity::Quantity;

/// The placeholder handle stored in a symbol location's unused entity slot.
const NULL_ENTITY: EntityHandle = EntityHandle::new(axiom_kernel::EntityId::from_raw(0), 0);

/// Where a residue sits. Generic on purpose: an ECS entity, or an opaque coded
/// location the domain interprets later (entity part, cell, item, abstract
/// surface, relation endpoint — all encoded as symbol codes). A tagged value so
/// accessors read branchlessly; exactly one arm is meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResidueLocation {
    is_entity: bool,
    entity: EntityHandle,
    code: u64,
}

impl ResidueLocation {
    /// A location on an ECS entity.
    pub const fn entity(handle: EntityHandle) -> Self {
        ResidueLocation {
            is_entity: true,
            entity: handle,
            code: 0,
        }
    }

    /// A location named by an opaque symbol code (part/cell/item/surface/...).
    pub const fn symbol(code: u64) -> Self {
        ResidueLocation {
            is_entity: false,
            entity: NULL_ENTITY,
            code,
        }
    }

    /// The entity this location names, if it is an entity location.
    pub fn as_entity(self) -> Option<EntityHandle> {
        self.is_entity.then_some(self.entity)
    }

    /// The symbol code this location names, if it is a symbol location.
    pub fn as_symbol(self) -> Option<u64> {
        (!self.is_entity).then_some(self.code)
    }
}

/// An opaque, domain-defined residue state code (e.g. wet vs dried — later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResidueState(u32);

impl ResidueState {
    /// A residue state from a deterministic code.
    pub const fn new(code: u32) -> Self {
        ResidueState(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// A quantity of a substance/material definition located somewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Residue {
    id: ResidueId,
    definition: DefinitionId,
    quantity: Quantity,
    location: ResidueLocation,
    state: ResidueState,
    cause: Option<CauseRef>,
    tick: u64,
}

impl Residue {
    /// This residue's stable id (its deterministic ordering key).
    pub const fn id(&self) -> ResidueId {
        self.id
    }

    /// The substance/material definition this residue is made of.
    pub const fn definition(&self) -> DefinitionId {
        self.definition
    }

    /// The current quantity.
    pub const fn quantity(&self) -> Quantity {
        self.quantity
    }

    /// Where the residue sits.
    pub const fn location(&self) -> ResidueLocation {
        self.location
    }

    /// The residue state.
    pub const fn state(&self) -> ResidueState {
        self.state
    }

    /// What caused this residue, if recorded.
    pub const fn cause(&self) -> Option<CauseRef> {
        self.cause
    }

    /// The logical tick this residue was created/last changed at.
    pub const fn tick(&self) -> u64 {
        self.tick
    }
}

/// A deterministic store of residues, keyed and iterated by ascending id.
#[derive(Debug, Clone, Default)]
pub struct ResidueStore {
    residues: BTreeMap<ResidueId, Residue>,
    next: u64,
}

impl ResidueStore {
    /// Create an empty residue store. The first residue has id 1.
    pub fn new() -> Self {
        ResidueStore {
            residues: BTreeMap::new(),
            next: 1,
        }
    }

    /// Create a residue, minting and returning its deterministic id.
    pub fn create(
        &mut self,
        definition: DefinitionId,
        quantity: Quantity,
        location: ResidueLocation,
        state: ResidueState,
        cause: Option<CauseRef>,
        tick: u64,
    ) -> ResidueId {
        let id = ResidueId::from_raw(self.next);
        self.next += 1;
        self.residues.insert(
            id,
            Residue {
                id,
                definition,
                quantity,
                location,
                state,
                cause,
                tick,
            },
        );
        id
    }

    /// Borrow a residue by id, if present.
    pub fn get(&self, id: ResidueId) -> Option<&Residue> {
        self.residues.get(&id)
    }

    /// Remove a residue by id, returning it if present (clean `None` if absent).
    pub fn remove(&mut self, id: ResidueId) -> Option<Residue> {
        self.residues.remove(&id)
    }

    /// Set a residue's quantity at a logical tick. Returns whether it existed.
    pub fn set_quantity(&mut self, id: ResidueId, quantity: Quantity, tick: u64) -> bool {
        self.residues
            .get_mut(&id)
            .map(|residue| {
                residue.quantity = quantity;
                residue.tick = tick;
            })
            .is_some()
    }

    /// Residues at a given location, in ascending id order.
    pub fn by_location(&self, location: ResidueLocation) -> impl Iterator<Item = &Residue> {
        self.residues
            .values()
            .filter(move |residue| residue.location == location)
    }

    /// Residues of a given definition, in ascending id order.
    pub fn by_definition(&self, definition: DefinitionId) -> impl Iterator<Item = &Residue> {
        self.residues
            .values()
            .filter(move |residue| residue.definition == definition)
    }

    /// All residues, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &Residue> {
        self.residues.values()
    }

    /// The number of residues.
    pub fn len(&self) -> usize {
        self.residues.len()
    }

    /// Whether the store holds no residues.
    pub fn is_empty(&self) -> bool {
        self.residues.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantity::QuantityUnit;
    use axiom_ecs::EntityRegistry;

    fn qty(amount: i64) -> Quantity {
        Quantity::new(QuantityUnit::Volume, amount).unwrap()
    }

    #[test]
    fn location_accessors_are_branchless_and_exclusive() {
        let mut reg = EntityRegistry::new();
        let h = reg.spawn_handle();
        let on_entity = ResidueLocation::entity(h);
        assert_eq!(on_entity.as_entity(), Some(h));
        assert_eq!(on_entity.as_symbol(), None);
        let on_surface = ResidueLocation::symbol(42);
        assert_eq!(on_surface.as_symbol(), Some(42));
        assert_eq!(on_surface.as_entity(), None);
        assert_ne!(on_entity, on_surface);
    }

    #[test]
    fn create_get_set_remove() {
        let mut store = ResidueStore::new();
        assert!(store.is_empty());
        let loc = ResidueLocation::symbol(1);
        let id = store.create(
            DefinitionId::from_raw(9),
            qty(10),
            loc,
            ResidueState::new(0),
            None,
            0,
        );
        assert_eq!(id.raw(), 1);
        let residue = store.get(id).unwrap();
        assert_eq!(residue.definition(), DefinitionId::from_raw(9));
        assert_eq!(residue.quantity(), qty(10));
        assert_eq!(residue.location(), loc);
        assert_eq!(residue.state(), ResidueState::new(0));
        assert_eq!(residue.state().code(), 0);
        assert_eq!(residue.cause(), None);
        assert_eq!(residue.tick(), 0);
        assert!(store.set_quantity(id, qty(4), 2));
        assert_eq!(store.get(id).unwrap().quantity(), qty(4));
        assert_eq!(store.get(id).unwrap().tick(), 2);
        assert!(!store.set_quantity(ResidueId::from_raw(99), qty(1), 0));
        assert_eq!(store.remove(id).unwrap().id(), id);
        assert!(store.remove(id).is_none());
    }

    #[test]
    fn queries_by_location_and_definition_are_ascending() {
        let mut store = ResidueStore::new();
        let here = ResidueLocation::symbol(1);
        let there = ResidueLocation::symbol(2);
        let r1 = store.create(
            DefinitionId::from_raw(1),
            qty(1),
            here,
            ResidueState::new(0),
            None,
            0,
        );
        let _r2 = store.create(
            DefinitionId::from_raw(2),
            qty(1),
            there,
            ResidueState::new(0),
            None,
            0,
        );
        let r3 = store.create(
            DefinitionId::from_raw(1),
            qty(1),
            here,
            ResidueState::new(0),
            None,
            0,
        );
        let here_ids: Vec<ResidueId> = store.by_location(here).map(Residue::id).collect();
        assert_eq!(here_ids, vec![r1, r3]);
        let def1_ids: Vec<ResidueId> = store
            .by_definition(DefinitionId::from_raw(1))
            .map(Residue::id)
            .collect();
        assert_eq!(def1_ids, vec![r1, r3]);
        let all: Vec<u64> = store.iter().map(|r| r.id().raw()).collect();
        assert_eq!(all, vec![1, 2, 3]);
        assert_eq!(store.len(), 3);
    }
}
