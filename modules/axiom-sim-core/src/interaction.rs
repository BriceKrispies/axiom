//! A generic contact/interaction route model.

use std::collections::BTreeMap;

use axiom_ecs::EntityHandle;

use crate::cause::CauseRef;
use crate::ids::{DefinitionId, InteractionId, ResidueId};
use crate::quantity::Quantity;
use crate::residue::ResidueLocation;

/// How two subjects came into contact. Routes carry no behavior — sim-core does
/// not implement touching, eating, breathing, or collision; it only records that
/// the route applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InteractionRoute {
    /// Surface contact.
    Touch,
    /// Taken in by mouth.
    Ingestion,
    /// Taken in by breathing.
    Inhalation,
    /// Contact through an open wound.
    WoundContact,
    /// Lodged inside.
    Embedded,
    /// Held within a container.
    Contained,
    /// Next to, without contact.
    Adjacent,
    /// An unclassified route.
    Generic,
}

const ROUTES: [InteractionRoute; 8] = [
    InteractionRoute::Touch,
    InteractionRoute::Ingestion,
    InteractionRoute::Inhalation,
    InteractionRoute::WoundContact,
    InteractionRoute::Embedded,
    InteractionRoute::Contained,
    InteractionRoute::Adjacent,
    InteractionRoute::Generic,
];

impl InteractionRoute {
    /// Validate and construct a route from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<InteractionRoute> {
        ROUTES.get(code as usize).copied()
    }

    /// The route's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// The domain-defined *kind* of an interaction, as an opaque deterministic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionKind(u32);

impl InteractionKind {
    /// An interaction kind from a deterministic code.
    pub const fn new(code: u32) -> Self {
        InteractionKind(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// A record that an interaction happened, with everything later phases need to
/// reason about it — but no behavior of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractionRecord {
    id: InteractionId,
    kind: InteractionKind,
    route: InteractionRoute,
    primary: EntityHandle,
    secondary: Option<EntityHandle>,
    material: Option<DefinitionId>,
    residue: Option<ResidueId>,
    quantity: Option<Quantity>,
    location: Option<ResidueLocation>,
    tick: u64,
    cause: Option<CauseRef>,
}

impl InteractionRecord {
    /// This record's stable id (its deterministic ordering key).
    pub const fn id(&self) -> InteractionId {
        self.id
    }
    /// The interaction kind.
    pub const fn kind(&self) -> InteractionKind {
        self.kind
    }
    /// The interaction route.
    pub const fn route(&self) -> InteractionRoute {
        self.route
    }
    /// The primary subject.
    pub const fn primary(&self) -> EntityHandle {
        self.primary
    }
    /// The secondary subject/target, if any.
    pub const fn secondary(&self) -> Option<EntityHandle> {
        self.secondary
    }
    /// The material/substance definition involved, if any.
    pub const fn material(&self) -> Option<DefinitionId> {
        self.material
    }
    /// The residue involved, if any.
    pub const fn residue(&self) -> Option<ResidueId> {
        self.residue
    }
    /// The quantity involved, if any.
    pub const fn quantity(&self) -> Option<Quantity> {
        self.quantity
    }
    /// The location involved, if any.
    pub const fn location(&self) -> Option<ResidueLocation> {
        self.location
    }
    /// The logical tick.
    pub const fn tick(&self) -> u64 {
        self.tick
    }
    /// The cause, if recorded.
    pub const fn cause(&self) -> Option<CauseRef> {
        self.cause
    }
}

/// Parameters for recording an interaction (grouped to keep the call boring).
#[derive(Debug, Clone, Copy)]
pub struct InteractionParams {
    /// The interaction kind.
    pub kind: InteractionKind,
    /// The route.
    pub route: InteractionRoute,
    /// The primary subject.
    pub primary: EntityHandle,
    /// The secondary subject/target, if any.
    pub secondary: Option<EntityHandle>,
    /// The material/substance definition involved, if any.
    pub material: Option<DefinitionId>,
    /// The residue involved, if any.
    pub residue: Option<ResidueId>,
    /// The quantity involved, if any.
    pub quantity: Option<Quantity>,
    /// The location involved, if any.
    pub location: Option<ResidueLocation>,
    /// The logical tick.
    pub tick: u64,
    /// The cause, if any.
    pub cause: Option<CauseRef>,
}

/// A deterministic store of interaction records, keyed and iterated by ascending
/// id.
#[derive(Debug, Clone, Default)]
pub struct InteractionStore {
    records: BTreeMap<InteractionId, InteractionRecord>,
    next: u64,
}

impl InteractionStore {
    /// Create an empty store. The first record has id 1.
    pub fn new() -> Self {
        InteractionStore {
            records: BTreeMap::new(),
            next: 1,
        }
    }

    /// Record an interaction, minting and returning its deterministic id.
    pub fn create(&mut self, params: InteractionParams) -> InteractionId {
        let id = InteractionId::from_raw(self.next);
        self.next += 1;
        self.records.insert(
            id,
            InteractionRecord {
                id,
                kind: params.kind,
                route: params.route,
                primary: params.primary,
                secondary: params.secondary,
                material: params.material,
                residue: params.residue,
                quantity: params.quantity,
                location: params.location,
                tick: params.tick,
                cause: params.cause,
            },
        );
        id
    }

    /// Borrow an interaction record by id, if present.
    pub fn get(&self, id: InteractionId) -> Option<&InteractionRecord> {
        self.records.get(&id)
    }

    /// Records whose primary subject is `subject`, in ascending id order.
    pub fn by_subject(&self, subject: EntityHandle) -> impl Iterator<Item = &InteractionRecord> {
        self.records
            .values()
            .filter(move |record| record.primary == subject)
    }

    /// Records on a given route, in ascending id order.
    pub fn by_route(&self, route: InteractionRoute) -> impl Iterator<Item = &InteractionRecord> {
        self.records
            .values()
            .filter(move |record| record.route == route)
    }

    /// All records, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &InteractionRecord> {
        self.records.values()
    }

    /// The number of records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the store holds no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_ecs::EntityRegistry;

    fn params(route: InteractionRoute, primary: EntityHandle) -> InteractionParams {
        InteractionParams {
            kind: InteractionKind::new(1),
            route,
            primary,
            secondary: None,
            material: None,
            residue: None,
            quantity: None,
            location: None,
            tick: 0,
            cause: None,
        }
    }

    #[test]
    fn route_codes_validate_and_round_trip() {
        assert_eq!(
            InteractionRoute::from_code(0),
            Some(InteractionRoute::Touch)
        );
        assert_eq!(
            InteractionRoute::from_code(7),
            Some(InteractionRoute::Generic)
        );
        assert_eq!(
            InteractionRoute::from_code(8),
            None,
            "out-of-range route fails cleanly"
        );
        assert_eq!(InteractionRoute::Ingestion.code(), 1);
        assert_eq!(
            InteractionRoute::from_code(InteractionRoute::WoundContact.code()),
            Some(InteractionRoute::WoundContact)
        );
    }

    #[test]
    fn create_and_get_round_trip_fields() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let b = reg.spawn_handle();
        let mut store = InteractionStore::new();
        let mut p = params(InteractionRoute::Touch, a);
        p.secondary = Some(b);
        p.material = Some(DefinitionId::from_raw(3));
        p.residue = Some(ResidueId::from_raw(4));
        p.cause = Some(CauseRef::Command);
        let id = store.create(p);
        assert_eq!(id.raw(), 1);
        let record = store.get(id).unwrap();
        assert_eq!(record.kind(), InteractionKind::new(1));
        assert_eq!(record.kind().code(), 1);
        assert_eq!(record.route(), InteractionRoute::Touch);
        assert_eq!(record.primary(), a);
        assert_eq!(record.secondary(), Some(b));
        assert_eq!(record.material(), Some(DefinitionId::from_raw(3)));
        assert_eq!(record.residue(), Some(ResidueId::from_raw(4)));
        assert_eq!(record.quantity(), None);
        assert_eq!(record.location(), None);
        assert_eq!(record.tick(), 0);
        assert_eq!(record.cause(), Some(CauseRef::Command));
    }

    #[test]
    fn queries_by_subject_and_route_are_ascending() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let b = reg.spawn_handle();
        let mut store = InteractionStore::new();
        let i1 = store.create(params(InteractionRoute::Touch, a));
        let _i2 = store.create(params(InteractionRoute::Ingestion, b));
        let i3 = store.create(params(InteractionRoute::Touch, a));
        let by_subject: Vec<InteractionId> =
            store.by_subject(a).map(InteractionRecord::id).collect();
        assert_eq!(by_subject, vec![i1, i3]);
        let by_route: Vec<InteractionId> = store
            .by_route(InteractionRoute::Touch)
            .map(InteractionRecord::id)
            .collect();
        assert_eq!(by_route, vec![i1, i3]);
        assert_eq!(store.by_route(InteractionRoute::Adjacent).count(), 0);
        let all: Vec<u64> = store.iter().map(|r| r.id().raw()).collect();
        assert_eq!(all, vec![1, 2, 3]);
        assert!(!store.is_empty());
    }
}
