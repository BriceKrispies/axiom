//! Body instances: bodies instantiated from a plan, with parts and surfaces.

use std::collections::BTreeMap;

use axiom_ecs::EntityHandle;

use crate::body_plan::{BodyPlan, BodyPlanPartKind};
use crate::body_surface::{BodySurface, BodySurfaceKind};
use crate::cause::CauseRef;
use crate::ids::{BodyId, BodyPartId, BodyPlanId, BodySurfaceId};
use crate::tissue::TissueLayer;

/// An opaque, domain-defined body-part state code (e.g. intact vs severed later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyPartState(u32);

impl BodyPartState {
    /// A body-part state from a deterministic code.
    pub const fn new(code: u32) -> Self {
        BodyPartState(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// An instantiated body part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyPart {
    id: BodyPartId,
    body: BodyId,
    plan_index: u32,
    kind: BodyPlanPartKind,
    state: BodyPartState,
    tissue_layers: Vec<TissueLayer>,
    surfaces: Vec<BodySurfaceId>,
}

impl BodyPart {
    /// This part's id.
    pub const fn id(&self) -> BodyPartId {
        self.id
    }
    /// The body this part belongs to.
    pub const fn body(&self) -> BodyId {
        self.body
    }
    /// The plan-part index this was instantiated from.
    pub const fn plan_index(&self) -> u32 {
        self.plan_index
    }
    /// The part kind.
    pub const fn kind(&self) -> BodyPlanPartKind {
        self.kind
    }
    /// The part state.
    pub const fn state(&self) -> BodyPartState {
        self.state
    }
    /// The tissue layers, outermost first.
    pub fn tissue_layers(&self) -> &[TissueLayer] {
        &self.tissue_layers
    }
    /// The surfaces this part exposes.
    pub fn surfaces(&self) -> &[BodySurfaceId] {
        &self.surfaces
    }
}

/// A connection between two instantiated parts (`from` → `to`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyConnection {
    from: BodyPartId,
    to: BodyPartId,
}

impl BodyConnection {
    /// The source part.
    pub const fn from(self) -> BodyPartId {
        self.from
    }
    /// The destination part.
    pub const fn to(self) -> BodyPartId {
        self.to
    }
}

/// An instantiated body.
#[derive(Debug, Clone)]
pub struct Body {
    id: BodyId,
    owner: Option<EntityHandle>,
    plan: BodyPlanId,
    parts: Vec<BodyPartId>,
    connections: Vec<BodyConnection>,
    cause: Option<CauseRef>,
    tick: u64,
}

impl Body {
    /// This body's id.
    pub const fn id(&self) -> BodyId {
        self.id
    }
    /// The owning ECS entity, if assigned.
    pub const fn owner(&self) -> Option<EntityHandle> {
        self.owner
    }
    /// The plan this body was instantiated from.
    pub const fn plan(&self) -> BodyPlanId {
        self.plan
    }
    /// This body's part ids, in instantiation order.
    pub fn parts(&self) -> &[BodyPartId] {
        &self.parts
    }
    /// This body's connections.
    pub fn connections(&self) -> &[BodyConnection] {
        &self.connections
    }
    /// What caused this body, if recorded.
    pub const fn cause(&self) -> Option<CauseRef> {
        self.cause
    }
    /// The logical tick this body was instantiated at.
    pub const fn tick(&self) -> u64 {
        self.tick
    }
}

/// A deterministic store of bodies, their parts, and their surfaces.
#[derive(Debug, Clone, Default)]
pub struct BodyStore {
    bodies: BTreeMap<BodyId, Body>,
    parts: BTreeMap<BodyPartId, BodyPart>,
    surfaces: BTreeMap<BodySurfaceId, BodySurface>,
    by_owner: BTreeMap<EntityHandle, BodyId>,
    next_body: u64,
    next_part: u64,
    next_surface: u64,
}

impl BodyStore {
    /// Create an empty store. The first body/part/surface have id 1.
    pub fn new() -> Self {
        BodyStore {
            bodies: BTreeMap::new(),
            parts: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            by_owner: BTreeMap::new(),
            next_body: 1,
            next_part: 1,
            next_surface: 1,
        }
    }

    /// Instantiate a body from `plan`, minting parts and surfaces from its parts.
    /// `owner` (if any) is recorded for owner queries; liveness is the caller's to
    /// check before calling.
    pub fn instantiate(
        &mut self,
        plan: &BodyPlan,
        owner: Option<EntityHandle>,
        cause: Option<CauseRef>,
        tick: u64,
    ) -> BodyId {
        let body = BodyId::from_raw(self.next_body);
        self.next_body += 1;
        let part_ids: Vec<BodyPartId> = plan
            .parts()
            .iter()
            .map(|_| {
                let id = BodyPartId::from_raw(self.next_part);
                self.next_part += 1;
                id
            })
            .collect();
        let parts: Vec<BodyPart> = plan
            .parts()
            .iter()
            .zip(part_ids.iter())
            .map(|(plan_part, &part_id)| {
                let surfaces: Vec<BodySurface> = plan_part
                    .surfaces()
                    .iter()
                    .map(|spec| {
                        let surface_id = BodySurfaceId::from_raw(self.next_surface);
                        self.next_surface += 1;
                        BodySurface::new(surface_id, part_id, spec.kind(), spec.exposure())
                    })
                    .collect();
                let surface_ids: Vec<BodySurfaceId> =
                    surfaces.iter().map(BodySurface::id).collect();
                surfaces.into_iter().for_each(|surface| {
                    self.surfaces.insert(surface.id(), surface);
                });
                BodyPart {
                    id: part_id,
                    body,
                    plan_index: plan_part.index(),
                    kind: plan_part.kind(),
                    state: BodyPartState::new(0),
                    tissue_layers: plan_part.tissue_layers().to_vec(),
                    surfaces: surface_ids,
                }
            })
            .collect();
        let connections: Vec<BodyConnection> = plan
            .connections()
            .iter()
            .map(|connection| BodyConnection {
                from: part_ids[connection.from() as usize],
                to: part_ids[connection.to() as usize],
            })
            .collect();
        parts.into_iter().for_each(|part| {
            self.parts.insert(part.id(), part);
        });
        owner.map(|handle| self.by_owner.insert(handle, body));
        self.bodies.insert(
            body,
            Body {
                id: body,
                owner,
                plan: plan.id(),
                parts: part_ids,
                connections,
                cause,
                tick,
            },
        );
        body
    }

    /// Borrow a body by id.
    pub fn get(&self, id: BodyId) -> Option<&Body> {
        self.bodies.get(&id)
    }

    /// The body owned by an entity, if any.
    pub fn by_owner(&self, owner: EntityHandle) -> Option<BodyId> {
        self.by_owner.get(&owner).copied()
    }

    /// Borrow a part by id.
    pub fn part(&self, id: BodyPartId) -> Option<&BodyPart> {
        self.parts.get(&id)
    }

    /// Borrow a surface by id.
    pub fn surface(&self, id: BodySurfaceId) -> Option<&BodySurface> {
        self.surfaces.get(&id)
    }

    /// Set a body-part state. Returns whether the part existed.
    pub fn set_part_state(&mut self, id: BodyPartId, state: BodyPartState) -> bool {
        self.parts
            .get_mut(&id)
            .map(|part| part.state = state)
            .is_some()
    }

    /// Set a body-surface state. Returns whether the surface existed.
    pub fn set_surface_state(
        &mut self,
        id: BodySurfaceId,
        state: crate::body_surface::BodySurfaceState,
    ) -> bool {
        self.surfaces
            .get_mut(&id)
            .map(|surface| surface.set_state(state))
            .is_some()
    }

    /// A body's part ids, in instantiation order.
    pub fn parts_of(&self, body: BodyId) -> Vec<BodyPartId> {
        self.bodies
            .get(&body)
            .map(|b| b.parts.to_vec())
            .unwrap_or_default()
    }

    /// A body's parts of a given kind, in instantiation order.
    pub fn parts_by_kind(&self, body: BodyId, kind: BodyPlanPartKind) -> Vec<BodyPartId> {
        self.parts_of(body)
            .into_iter()
            .filter(|id| {
                self.parts
                    .get(id)
                    .map(|part| part.kind == kind)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Parts connected to `part` within `body` (either direction), ascending by id.
    pub fn connected_parts(&self, body: BodyId, part: BodyPartId) -> Vec<BodyPartId> {
        let connections = self
            .bodies
            .get(&body)
            .map(|b| b.connections.as_slice())
            .unwrap_or(&[]);
        let mut out: Vec<BodyPartId> = connections
            .iter()
            .filter_map(|connection| {
                (connection.from == part)
                    .then_some(connection.to)
                    .or((connection.to == part).then_some(connection.from))
            })
            .collect();
        out.sort_unstable();
        out
    }

    /// A body's surfaces, ascending by surface id.
    pub fn surfaces_of(&self, body: BodyId) -> Vec<BodySurfaceId> {
        let mut out: Vec<BodySurfaceId> = self
            .parts_of(body)
            .into_iter()
            .filter_map(|id| self.parts.get(&id))
            .flat_map(|part| part.surfaces.iter().copied())
            .collect();
        out.sort_unstable();
        out
    }

    /// A part's surfaces, ascending by surface id.
    pub fn surfaces_of_part(&self, part: BodyPartId) -> Vec<BodySurfaceId> {
        self.parts
            .get(&part)
            .map(|p| p.surfaces.to_vec())
            .unwrap_or_default()
    }

    /// A body's surfaces of a given kind, ascending by surface id.
    pub fn surfaces_by_kind(&self, body: BodyId, kind: BodySurfaceKind) -> Vec<BodySurfaceId> {
        self.surfaces_of(body)
            .into_iter()
            .filter(|id| {
                self.surfaces
                    .get(id)
                    .map(|s| s.kind() == kind)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// All body ids, ascending.
    pub fn iter(&self) -> impl Iterator<Item = BodyId> + '_ {
        self.bodies.keys().copied()
    }

    /// The number of bodies.
    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    /// Whether the store holds no bodies.
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_plan::{BodyPlanRegistry, BodyPlanSymmetry, PlanPartSpec, SurfaceSpec};
    use crate::body_surface::SurfaceExposure;
    use crate::ids::TissueId;
    use axiom_ecs::EntityRegistry;

    fn part_spec(name: &str, kind: BodyPlanPartKind, surfaces: Vec<SurfaceSpec>) -> PlanPartSpec {
        PlanPartSpec {
            name: name.to_string(),
            kind,
            symmetry: BodyPlanSymmetry::None,
            group: 0,
            capabilities: Vec::new(),
            tissue_layers: vec![TissueLayer::new(TissueId::from_raw(1), 0)],
            surfaces,
        }
    }

    /// A two-part plan: core (outer surface) + extremity (outer surface).
    fn two_part_plan(registry: &mut BodyPlanRegistry) -> BodyPlanId {
        let draft = registry.begin();
        registry.add_part(
            draft,
            part_spec(
                "test-core",
                BodyPlanPartKind::Core,
                vec![SurfaceSpec::new(
                    BodySurfaceKind::Outer,
                    SurfaceExposure::External,
                )],
            ),
        );
        registry.add_part(
            draft,
            part_spec(
                "test-extremity",
                BodyPlanPartKind::Extremity,
                vec![SurfaceSpec::new(
                    BodySurfaceKind::Outer,
                    SurfaceExposure::External,
                )],
            ),
        );
        registry.connect(draft, 0, 1);
        registry.finish(draft, "test-plan").unwrap()
    }

    #[test]
    fn instantiation_is_deterministic_and_queryable() {
        let mut registry = BodyPlanRegistry::new();
        let plan_id = two_part_plan(&mut registry);
        let plan = registry.get(plan_id).unwrap();
        let mut ereg = EntityRegistry::new();
        let owner = ereg.spawn_handle();

        let mut store = BodyStore::new();
        assert!(store.is_empty());
        let body = store.instantiate(plan, Some(owner), Some(CauseRef::Command), 0);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(body).unwrap().plan(), plan_id);
        assert_eq!(store.get(body).unwrap().owner(), Some(owner));
        assert_eq!(store.get(body).unwrap().cause(), Some(CauseRef::Command));
        assert_eq!(store.get(body).unwrap().tick(), 0);
        assert_eq!(store.by_owner(owner), Some(body));

        let parts = store.parts_of(body);
        assert_eq!(parts.len(), 2);
        assert_eq!(store.get(body).unwrap().parts(), parts.as_slice());
        let cores = store.parts_by_kind(body, BodyPlanPartKind::Core);
        assert_eq!(cores.len(), 1);
        let extremities = store.parts_by_kind(body, BodyPlanPartKind::Extremity);
        assert_eq!(extremities.len(), 1);
        assert_eq!(store.parts_by_kind(body, BodyPlanPartKind::Eye).len(), 0);
        assert_eq!(store.connected_parts(body, parts[0]), vec![parts[1]]);
        assert_eq!(store.connected_parts(body, parts[1]), vec![parts[0]]);
        let core = store.part(cores[0]).unwrap();
        assert_eq!(core.body(), body);
        assert_eq!(core.plan_index(), 0);
        assert_eq!(core.tissue_layers().len(), 1);
        assert_eq!(core.surfaces().len(), 1);
        let surfaces = store.surfaces_of(body);
        assert_eq!(surfaces.len(), 2);
        assert_eq!(store.surfaces_of_part(cores[0]).len(), 1);
        assert_eq!(
            store.surfaces_by_kind(body, BodySurfaceKind::Outer).len(),
            2
        );
        assert_eq!(
            store.surfaces_by_kind(body, BodySurfaceKind::Mouth).len(),
            0
        );
        let surface = store.surface(surfaces[0]).unwrap();
        assert_eq!(surface.kind(), BodySurfaceKind::Outer);
    }

    #[test]
    fn part_and_surface_state_can_be_updated() {
        use crate::body_surface::BodySurfaceState;
        let mut registry = BodyPlanRegistry::new();
        let plan_id = two_part_plan(&mut registry);
        let plan = registry.get(plan_id).unwrap();
        let mut store = BodyStore::new();
        let body = store.instantiate(plan, None, None, 0);
        let part = store.parts_of(body)[0];
        assert!(store.set_part_state(part, BodyPartState::new(3)));
        assert_eq!(store.part(part).unwrap().state(), BodyPartState::new(3));
        assert!(!store.set_part_state(BodyPartId::from_raw(9999), BodyPartState::new(1)));

        let surface = store.surfaces_of(body)[0];
        assert!(store.set_surface_state(surface, BodySurfaceState::new(2)));
        assert_eq!(
            store.surface(surface).unwrap().state(),
            BodySurfaceState::new(2)
        );
        assert!(!store.set_surface_state(BodySurfaceId::from_raw(9999), BodySurfaceState::new(1)));
    }

    #[test]
    fn missing_bodies_query_empty() {
        let store = BodyStore::new();
        let absent = BodyId::from_raw(99);
        assert!(store.get(absent).is_none());
        assert!(store.parts_of(absent).is_empty());
        assert!(store
            .parts_by_kind(absent, BodyPlanPartKind::Core)
            .is_empty());
        assert!(store
            .connected_parts(absent, BodyPartId::from_raw(1))
            .is_empty());
        assert!(store.surfaces_of(absent).is_empty());
        assert!(store.surfaces_of_part(BodyPartId::from_raw(1)).is_empty());
        assert!(store.iter().next().is_none());
    }

    #[test]
    fn two_runs_mint_identical_ids() {
        let build = || {
            let mut registry = BodyPlanRegistry::new();
            let plan_id = two_part_plan(&mut registry);
            let plan = registry.get(plan_id).unwrap();
            let mut store = BodyStore::new();
            let body = store.instantiate(plan, None, None, 0);
            (
                body.raw(),
                store
                    .parts_of(body)
                    .iter()
                    .map(|p| p.raw())
                    .collect::<Vec<_>>(),
                store
                    .surfaces_of(body)
                    .iter()
                    .map(|s| s.raw())
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(build(), build());
    }
}
