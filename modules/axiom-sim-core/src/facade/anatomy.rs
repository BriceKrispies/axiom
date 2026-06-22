//! The body/anatomy facade surface of `SimCoreApi`: tissue definitions, body
//! plans, body instantiation, surfaces, body routes, and wound records.
//!
//! A child module of `facade`, so it may use the private `world` field. Every
//! method routes through the same opaque-value discipline as the rest of the
//! facade: internal types are returned only as values or references, never named
//! by consumers.

use axiom_ecs::{EntityHandle, EntityRegistry};

use crate::body::{Body, BodyPart, BodyPartState};
use crate::body_plan::{
    BodyPlan, BodyPlanCapability, BodyPlanPart, BodyPlanPartKind, BodyPlanSymmetry, PlanPartSpec,
    SurfaceSpec,
};
use crate::body_route::{BodyRoute, BodyRouteKind, BodyRouteTarget};
use crate::body_surface::{BodySurface, BodySurfaceKind, BodySurfaceState, SurfaceExposure};
use crate::cause::CauseRef;
use crate::ids::{
    BodyId, BodyPartId, BodyPlanId, BodySurfaceId, DefinitionId, InteractionId, ResidueId,
    TissueId, WoundId,
};
use crate::interaction::{InteractionKind, InteractionParams, InteractionRoute};
use crate::quantity::Quantity;
use crate::relation::{Relation, RelationEndpoint, RelationKind};
use crate::residue::Residue;
use crate::tissue::{TissueDefinition, TissueKind, TissueLayer, TissueProperty};
use crate::wound::{DamageMode, DamageSeverity, TissueDamage, WoundRecord, WoundSpec, WoundState};

use super::SimCoreApi;

impl SimCoreApi {
    /// Tissue-kind code: covering.
    pub const TISSUE_COVERING: u8 = 0;
    /// Tissue-kind code: muscle.
    pub const TISSUE_MUSCLE: u8 = 1;
    /// Tissue-kind code: bone.
    pub const TISSUE_BONE: u8 = 2;
    /// Tissue-kind code: nerve.
    pub const TISSUE_NERVE: u8 = 3;
    /// Tissue-kind code: blood.
    pub const TISSUE_BLOOD: u8 = 4;
    /// Tissue-kind code: organ.
    pub const TISSUE_ORGAN: u8 = 5;
    /// Tissue-kind code: fluid.
    pub const TISSUE_FLUID: u8 = 6;
    /// Tissue-kind code: generic.
    pub const TISSUE_GENERIC: u8 = 7;

    /// Canonical tissue tag: can-hold-residue.
    pub const TTAG_CAN_HOLD_RESIDUE: &'static str = "can-hold-residue";
    /// Canonical tissue tag: absorbent.
    pub const TTAG_ABSORBENT: &'static str = "absorbent";
    /// Canonical tissue tag: protective.
    pub const TTAG_PROTECTIVE: &'static str = "protective";
    /// Canonical tissue tag: structural.
    pub const TTAG_STRUCTURAL: &'static str = "structural";
    /// Canonical tissue tag: pain-sensitive.
    pub const TTAG_PAIN_SENSITIVE: &'static str = "pain-sensitive";
    /// Canonical tissue tag: bleeds.
    pub const TTAG_BLEEDS: &'static str = "bleeds";
    /// Canonical tissue tag: vital.
    pub const TTAG_VITAL: &'static str = "vital";
    /// Canonical tissue tag: exposed.
    pub const TTAG_EXPOSED: &'static str = "exposed";
    /// Canonical tissue tag: internal.
    pub const TTAG_INTERNAL: &'static str = "internal";

    /// Register a tissue definition. `None` if the kind code is out of range or
    /// the durable name is already registered.
    pub fn register_tissue(
        &mut self,
        kind_code: u8,
        name: &str,
        tags: &[&str],
        properties: &[(u32, i64)],
    ) -> Option<TissueId> {
        let typed: Vec<(TissueProperty, i64)> = properties
            .iter()
            .map(|(key, value)| (TissueProperty::new(*key), *value))
            .collect();
        TissueKind::from_code(kind_code)
            .and_then(|kind| self.world.tissues_mut().register(kind, name, tags, &typed))
    }

    /// The id registered for a tissue durable name, if any.
    pub fn tissue_id(&self, name: &str) -> Option<TissueId> {
        self.world.tissues().id_of(name)
    }

    /// Borrow a tissue definition by id.
    pub fn tissue_definition(&self, id: TissueId) -> Option<&TissueDefinition> {
        self.world.tissues().get(id)
    }

    /// The kind code of a tissue definition, if present.
    pub fn tissue_kind_code(&self, id: TissueId) -> Option<u8> {
        self.world
            .tissues()
            .get(id)
            .map(|tissue| tissue.kind().code())
    }

    /// Whether a tissue carries `tag`.
    pub fn tissue_has_tag(&self, id: TissueId, tag: &str) -> bool {
        self.world
            .tissues()
            .get(id)
            .is_some_and(|tissue| tissue.has_tag(tag))
    }

    /// A typed tissue property value, if present.
    pub fn tissue_property(&self, id: TissueId, key: u32) -> Option<i64> {
        self.world
            .tissues()
            .get(id)
            .and_then(|tissue| tissue.property(TissueProperty::new(key)))
    }

    /// Tissue ids carrying `tag`, ascending.
    pub fn tissues_by_tag(&self, tag: &str) -> Vec<TissueId> {
        self.world
            .tissues()
            .by_tag(tag)
            .map(TissueDefinition::id)
            .collect()
    }

    /// The number of tissue definitions.
    pub fn tissue_count(&self) -> usize {
        self.world.tissues().len()
    }
}

impl SimCoreApi {
    /// Body-part-kind code: core.
    pub const PART_CORE: u8 = 0;
    /// Body-part-kind code: head.
    pub const PART_HEAD: u8 = 1;
    /// Body-part-kind code: limb.
    pub const PART_LIMB: u8 = 2;
    /// Body-part-kind code: extremity.
    pub const PART_EXTREMITY: u8 = 3;
    /// Body-part-kind code: organ.
    pub const PART_ORGAN: u8 = 4;
    /// Body-part-kind code: mouth.
    pub const PART_MOUTH: u8 = 5;
    /// Body-part-kind code: eye.
    pub const PART_EYE: u8 = 6;
    /// Body-part-kind code: generic.
    pub const PART_GENERIC: u8 = 7;

    /// Surface-kind code: outer.
    pub const SURFACE_OUTER: u8 = 0;
    /// Surface-kind code: inner.
    pub const SURFACE_INNER: u8 = 1;
    /// Surface-kind code: mouth.
    pub const SURFACE_MOUTH: u8 = 2;
    /// Surface-kind code: wound.
    pub const SURFACE_WOUND: u8 = 3;
    /// Surface-kind code: generic.
    pub const SURFACE_GENERIC: u8 = 4;

    /// Begin a new body-plan draft, returning its handle.
    pub fn begin_body_plan(&mut self) -> u32 {
        self.world.body_plans_mut().begin()
    }

    /// Add a part to a draft. `None` if the draft is unknown, the kind/symmetry/
    /// surface codes are out of range, or the part name duplicates an existing one.
    pub fn add_body_plan_part(
        &mut self,
        draft: u32,
        name: &str,
        kind_code: u8,
        symmetry_code: u8,
        group: u32,
        capabilities: &[u32],
        tissue_layers: &[(TissueId, u32)],
        surfaces: &[(u8, bool)],
    ) -> Option<u32> {
        let kind = BodyPlanPartKind::from_code(kind_code);
        let symmetry = BodyPlanSymmetry::from_code(symmetry_code);
        let surface_specs: Option<Vec<SurfaceSpec>> = surfaces
            .iter()
            .map(|(code, external)| {
                BodySurfaceKind::from_code(*code).map(|kind| {
                    let exposure =
                        [SurfaceExposure::Internal, SurfaceExposure::External][*external as usize];
                    SurfaceSpec::new(kind, exposure)
                })
            })
            .collect();
        let layers: Vec<TissueLayer> = tissue_layers
            .iter()
            .map(|(tissue, depth)| TissueLayer::new(*tissue, *depth))
            .collect();
        kind.zip(symmetry)
            .zip(surface_specs)
            .and_then(|((kind, symmetry), surfaces)| {
                self.world.body_plans_mut().add_part(
                    draft,
                    PlanPartSpec {
                        name: name.to_string(),
                        kind,
                        symmetry,
                        group,
                        capabilities: capabilities.to_vec(),
                        tissue_layers: layers,
                        surfaces,
                    },
                )
            })
    }

    /// Connect two parts in a draft. Returns whether the connection was valid.
    pub fn connect_body_plan_parts(&mut self, draft: u32, from: u32, to: u32) -> bool {
        self.world.body_plans_mut().connect(draft, from, to)
    }

    /// Finish a draft into a named plan. `None` if the draft is unknown or the
    /// plan name is already registered.
    pub fn finish_body_plan(&mut self, draft: u32, name: &str) -> Option<BodyPlanId> {
        self.world.body_plans_mut().finish(draft, name)
    }

    /// The id of a registered body plan by name, if any.
    pub fn body_plan_id(&self, name: &str) -> Option<BodyPlanId> {
        self.world.body_plans().by_name(name).map(BodyPlan::id)
    }

    /// Borrow a body plan by id.
    pub fn body_plan(&self, id: BodyPlanId) -> Option<&BodyPlan> {
        self.world.body_plans().get(id)
    }

    /// The number of parts in a plan, if present.
    pub fn body_plan_part_count(&self, id: BodyPlanId) -> Option<usize> {
        self.world
            .body_plans()
            .get(id)
            .map(|plan| plan.parts().len())
    }

    /// Plan part indices of a given kind, in authored order (empty for an unknown
    /// plan or kind code).
    pub fn body_plan_parts_by_kind(&self, id: BodyPlanId, kind_code: u8) -> Vec<u32> {
        BodyPlanPartKind::from_code(kind_code)
            .zip(self.world.body_plans().get(id))
            .map(|(kind, plan)| plan.parts_by_kind(kind).map(BodyPlanPart::index).collect())
            .unwrap_or_default()
    }

    /// Plan part indices providing a capability, in authored order.
    pub fn body_plan_parts_by_capability(&self, id: BodyPlanId, capability: u32) -> Vec<u32> {
        self.world
            .body_plans()
            .get(id)
            .map(|plan| {
                plan.parts_by_capability(BodyPlanCapability::new(capability))
                    .map(BodyPlanPart::index)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The number of registered body plans.
    pub fn body_plan_count(&self) -> usize {
        self.world.body_plans().len()
    }
}

impl SimCoreApi {
    /// Instantiate a body from a plan for an optional owner entity. `None` if the
    /// plan id is unknown or the owner handle is stale/dead.
    pub fn instantiate_body(
        &mut self,
        plan: BodyPlanId,
        owner: Option<EntityHandle>,
        registry: &EntityRegistry,
        cause: Option<CauseRef>,
        tick: u64,
    ) -> Option<BodyId> {
        self.world
            .instantiate_body(plan, owner, registry, cause, tick)
    }

    /// Borrow a body by id.
    pub fn body(&self, id: BodyId) -> Option<&Body> {
        self.world.bodies().get(id)
    }

    /// The owning entity of a body, if assigned.
    pub fn body_owner(&self, id: BodyId) -> Option<EntityHandle> {
        self.world.bodies().get(id).and_then(Body::owner)
    }

    /// The body owned by an entity, if any.
    pub fn body_by_owner(&self, owner: EntityHandle) -> Option<BodyId> {
        self.world.bodies().by_owner(owner)
    }

    /// The plan a body was instantiated from, if present.
    pub fn body_plan_of(&self, body: BodyId) -> Option<BodyPlanId> {
        self.world.bodies().get(body).map(Body::plan)
    }

    /// A body's part ids, in instantiation order.
    pub fn body_parts(&self, body: BodyId) -> Vec<BodyPartId> {
        self.world.bodies().parts_of(body)
    }

    /// A body's parts of a given kind (empty for unknown body/kind code).
    pub fn body_parts_by_kind(&self, body: BodyId, kind_code: u8) -> Vec<BodyPartId> {
        BodyPlanPartKind::from_code(kind_code)
            .map(|kind| self.world.bodies().parts_by_kind(body, kind))
            .unwrap_or_default()
    }

    /// Parts connected to `part` within `body`, ascending.
    pub fn connected_parts(&self, body: BodyId, part: BodyPartId) -> Vec<BodyPartId> {
        self.world.bodies().connected_parts(body, part)
    }

    /// Borrow a body part by id.
    pub fn body_part(&self, id: BodyPartId) -> Option<&BodyPart> {
        self.world.bodies().part(id)
    }

    /// The kind code of a body part, if present.
    pub fn body_part_kind_code(&self, id: BodyPartId) -> Option<u8> {
        self.world.bodies().part(id).map(|part| part.kind().code())
    }

    /// The state code of a body part, if present.
    pub fn body_part_state(&self, id: BodyPartId) -> Option<u32> {
        self.world.bodies().part(id).map(|part| part.state().code())
    }

    /// The number of bodies.
    pub fn body_count(&self) -> usize {
        self.world.bodies().len()
    }

    /// All body ids, ascending.
    pub fn all_body_ids(&self) -> Vec<BodyId> {
        self.world.bodies().iter().collect()
    }
}

impl SimCoreApi {
    /// A body's surfaces, ascending by surface id.
    pub fn body_surfaces(&self, body: BodyId) -> Vec<BodySurfaceId> {
        self.world.bodies().surfaces_of(body)
    }

    /// A part's surfaces, ascending by surface id.
    pub fn part_surfaces(&self, part: BodyPartId) -> Vec<BodySurfaceId> {
        self.world.bodies().surfaces_of_part(part)
    }

    /// A body's surfaces of a given kind (empty for unknown body/kind code).
    pub fn body_surfaces_by_kind(&self, body: BodyId, kind_code: u8) -> Vec<BodySurfaceId> {
        BodySurfaceKind::from_code(kind_code)
            .map(|kind| self.world.bodies().surfaces_by_kind(body, kind))
            .unwrap_or_default()
    }

    /// Borrow a body surface by id.
    pub fn body_surface(&self, id: BodySurfaceId) -> Option<&BodySurface> {
        self.world.bodies().surface(id)
    }

    /// The kind code of a surface, if present.
    pub fn surface_kind_code(&self, id: BodySurfaceId) -> Option<u8> {
        self.world
            .bodies()
            .surface(id)
            .map(|surface| surface.kind().code())
    }

    /// The body part a surface belongs to, if present.
    pub fn surface_part(&self, id: BodySurfaceId) -> Option<BodyPartId> {
        self.world.bodies().surface(id).map(BodySurface::part)
    }

    /// The Phase-3 residue location that addresses a body surface (so residues
    /// can sit on it via the normal residue/transfer machinery).
    pub fn residue_location_for_surface(
        &self,
        surface: BodySurfaceId,
    ) -> crate::residue::ResidueLocation {
        crate::residue::ResidueLocation::symbol(surface.raw())
    }

    /// Residue ids on a body surface, ascending.
    pub fn residues_on_surface(&self, surface: BodySurfaceId) -> Vec<ResidueId> {
        let location = self.residue_location_for_surface(surface);
        self.world
            .residues()
            .by_location(location)
            .map(Residue::id)
            .collect()
    }

    /// Residue ids on any surface of a part, ascending.
    pub fn residues_on_part(&self, part: BodyPartId) -> Vec<ResidueId> {
        let mut out: Vec<ResidueId> = self
            .world
            .bodies()
            .surfaces_of_part(part)
            .into_iter()
            .flat_map(|surface| self.residues_on_surface(surface))
            .collect();
        out.sort_unstable();
        out
    }

    /// Residue ids on any surface of a body, ascending.
    pub fn residues_on_body(&self, body: BodyId) -> Vec<ResidueId> {
        let mut out: Vec<ResidueId> = self
            .world
            .bodies()
            .surfaces_of(body)
            .into_iter()
            .flat_map(|surface| self.residues_on_surface(surface))
            .collect();
        out.sort_unstable();
        out
    }
}

impl SimCoreApi {
    /// Body-route-kind code: surface-contact.
    pub const BODY_ROUTE_SURFACE_CONTACT: u8 = 0;
    /// Body-route-kind code: mouth-contact.
    pub const BODY_ROUTE_MOUTH_CONTACT: u8 = 1;
    /// Body-route-kind code: ingestion-entry.
    pub const BODY_ROUTE_INGESTION_ENTRY: u8 = 2;
    /// Body-route-kind code: inhalation-entry.
    pub const BODY_ROUTE_INHALATION_ENTRY: u8 = 3;
    /// Body-route-kind code: wound-entry.
    pub const BODY_ROUTE_WOUND_ENTRY: u8 = 4;
    /// Body-route-kind code: embedded-entry.
    pub const BODY_ROUTE_EMBEDDED_ENTRY: u8 = 5;
    /// Body-route-kind code: internal-contact.
    pub const BODY_ROUTE_INTERNAL_CONTACT: u8 = 6;
    /// Body-route-kind code: generic.
    pub const BODY_ROUTE_GENERIC: u8 = 7;

    /// The body-route code a Phase-3 interaction route maps to. `None` if the
    /// interaction route code is out of range.
    pub fn body_route_from_interaction(&self, route_code: u8) -> Option<u8> {
        InteractionRoute::from_code(route_code)
            .map(|route| BodyRoute::from_interaction(route).kind().code())
    }

    /// Whether a body route may target a surface kind. `None` if either code is
    /// out of range.
    pub fn body_route_can_target(
        &self,
        body_route_code: u8,
        surface_kind_code: u8,
    ) -> Option<bool> {
        BodyRouteKind::from_code(body_route_code)
            .zip(BodySurfaceKind::from_code(surface_kind_code))
            .map(|(route, surface)| BodyRoute::new(route).can_target(surface))
    }

    /// Record an interaction targeting a body surface, validating that the route
    /// (mapped to a body route) may reach the surface's kind, and emitting a
    /// causal event. The interaction's location is the surface's residue location.
    /// `None` if the route code is invalid, the surface is unknown, or the route
    /// cannot target the surface kind.
    pub fn record_surface_interaction(
        &mut self,
        kind_code: u32,
        route_code: u8,
        primary: EntityHandle,
        surface: BodySurfaceId,
        material: Option<DefinitionId>,
        residue: Option<ResidueId>,
        quantity: Option<Quantity>,
        event_kind: u32,
        event_code: u64,
        tick: u64,
        cause: Option<CauseRef>,
    ) -> Option<InteractionId> {
        let route = InteractionRoute::from_code(route_code);
        let surface_kind = self.world.bodies().surface(surface).map(BodySurface::kind);
        route.zip(surface_kind).and_then(|(route, kind)| {
            // The (body-route, surface) pair this interaction targets. Validating
            // and locating both flow through the target so the route refinement is
            // explicit, not implied.
            let target = BodyRouteTarget::new(BodyRoute::from_interaction(route).kind(), surface);
            BodyRoute::new(target.route()).can_target(kind).then(|| {
                let location = self.residue_location_for_surface(target.surface());
                let id = self.world.interactions_mut().create(InteractionParams {
                    kind: InteractionKind::new(kind_code),
                    route,
                    primary,
                    secondary: None,
                    material,
                    residue,
                    quantity,
                    location: Some(location),
                    tick,
                    cause,
                });
                self.world.journal_mut().append(
                    crate::causal::CausalEventKind::new(event_kind),
                    tick,
                    Some(primary),
                    None,
                    cause,
                    event_code,
                    None,
                );
                id
            })
        })
    }
}

impl SimCoreApi {
    /// Damage-mode code: blunt.
    pub const DAMAGE_BLUNT: u8 = 0;
    /// Damage-mode code: cut.
    pub const DAMAGE_CUT: u8 = 1;
    /// Damage-mode code: pierce.
    pub const DAMAGE_PIERCE: u8 = 2;
    /// Damage-mode code: burn.
    pub const DAMAGE_BURN: u8 = 3;
    /// Damage-mode code: crush.
    pub const DAMAGE_CRUSH: u8 = 4;
    /// Damage-mode code: tear.
    pub const DAMAGE_TEAR: u8 = 5;
    /// Damage-mode code: chemical.
    pub const DAMAGE_CHEMICAL: u8 = 6;
    /// Damage-mode code: generic.
    pub const DAMAGE_GENERIC: u8 = 7;

    /// Create a wound on a body part, validating the body/part/tissue references
    /// and emitting a causal event. `None` if the damage-mode code is out of range
    /// or any reference is invalid.
    pub fn create_wound(
        &mut self,
        body: BodyId,
        part: BodyPartId,
        tissue: Option<TissueId>,
        mode_code: u8,
        severity: u32,
        material: Option<DefinitionId>,
        residue: Option<ResidueId>,
        tissue_damage: &[(TissueId, u32)],
        event_kind: u32,
        event_code: u64,
        tick: u64,
        cause: Option<CauseRef>,
    ) -> Option<WoundId> {
        let damage: Vec<TissueDamage> = tissue_damage
            .iter()
            .map(|(t, s)| TissueDamage::new(*t, DamageSeverity::new(*s)))
            .collect();
        DamageMode::from_code(mode_code).and_then(|mode| {
            self.world.create_wound(
                WoundSpec {
                    body,
                    part,
                    tissue,
                    mode,
                    severity: DamageSeverity::new(severity),
                    material,
                    residue,
                    tissue_damage: damage,
                    cause,
                    tick,
                },
                event_kind,
                event_code,
            )
        })
    }

    /// Borrow a wound record by id.
    pub fn wound(&self, id: WoundId) -> Option<&WoundRecord> {
        self.world.wounds().get(id)
    }

    /// Wound ids on a body, ascending.
    pub fn wounds_by_body(&self, body: BodyId) -> Vec<WoundId> {
        self.world
            .wounds()
            .by_body(body)
            .map(WoundRecord::id)
            .collect()
    }

    /// Wound ids on a part, ascending.
    pub fn wounds_by_part(&self, part: BodyPartId) -> Vec<WoundId> {
        self.world
            .wounds()
            .by_part(part)
            .map(WoundRecord::id)
            .collect()
    }

    /// Wound ids of a damage mode, ascending (empty for an invalid code).
    pub fn wounds_by_mode(&self, mode_code: u8) -> Vec<WoundId> {
        DamageMode::from_code(mode_code)
            .map(|mode| {
                self.world
                    .wounds()
                    .by_mode(mode)
                    .map(WoundRecord::id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Wound ids of an exact severity, ascending.
    pub fn wounds_by_severity(&self, severity: u32) -> Vec<WoundId> {
        self.world
            .wounds()
            .by_severity(DamageSeverity::new(severity))
            .map(WoundRecord::id)
            .collect()
    }

    /// Set a wound's state. Returns whether it existed.
    pub fn set_wound_state(&mut self, id: WoundId, state_code: u32) -> bool {
        self.world
            .wounds_mut()
            .set_state(id, WoundState::new(state_code))
    }

    /// The number of wound records.
    pub fn wound_count(&self) -> usize {
        self.world.wounds().len()
    }

    /// Record a body's part connections as Phase-2 relations (each endpoint a
    /// symbol of the part id), returning how many were recorded. The relations are
    /// then queryable via [`Self::relations_by_endpoint`] with a symbol endpoint of
    /// a part id.
    pub fn record_body_connections_as_relations(
        &mut self,
        body: BodyId,
        relation_kind: u32,
    ) -> usize {
        let connections: Vec<(u64, u64)> = self
            .world
            .bodies()
            .get(body)
            .map(|b| {
                b.connections()
                    .iter()
                    .map(|c| (c.from().raw(), c.to().raw()))
                    .collect()
            })
            .unwrap_or_default();
        connections
            .iter()
            .map(|(from, to)| {
                self.world.relations_mut().insert(
                    RelationKind::new(relation_kind),
                    vec![
                        RelationEndpoint::symbol(*from),
                        RelationEndpoint::symbol(*to),
                    ],
                    None,
                    None,
                )
            })
            .count()
    }

    /// Relation ids whose endpoint is the symbol of a body part id, ascending —
    /// the connection relations recorded by
    /// [`Self::record_body_connections_as_relations`].
    pub fn connection_relations_for_part(&self, part: BodyPartId) -> Vec<crate::ids::RelationId> {
        self.world
            .relations()
            .by_endpoint(RelationEndpoint::symbol(part.raw()))
            .map(Relation::id)
            .collect()
    }

    /// Place a residue of `definition` directly on a body surface. A convenience
    /// over [`Self::create_residue`] that targets the surface's residue location.
    pub fn create_residue_on_surface(
        &mut self,
        definition: DefinitionId,
        quantity: Quantity,
        surface: BodySurfaceId,
        state_code: u32,
        cause: Option<CauseRef>,
        tick: u64,
    ) -> Option<ResidueId> {
        self.world
            .bodies()
            .surface(surface)
            .map(BodySurface::id)
            .map(|surface_id| {
                let location = self.residue_location_for_surface(surface_id);
                self.world.residues_mut().create(
                    definition,
                    quantity,
                    location,
                    crate::residue::ResidueState::new(state_code),
                    cause,
                    tick,
                )
            })
    }
}

impl SimCoreApi {
    /// Set a body-part state code. Returns whether the part existed.
    pub fn set_body_part_state(&mut self, part: BodyPartId, state_code: u32) -> bool {
        self.world
            .bodies_mut()
            .set_part_state(part, BodyPartState::new(state_code))
    }

    /// Set a body-surface state code. Returns whether the surface existed.
    pub fn set_surface_state(&mut self, surface: BodySurfaceId, state_code: u32) -> bool {
        self.world
            .bodies_mut()
            .set_surface_state(surface, BodySurfaceState::new(state_code))
    }

    /// The state code of a surface, if present.
    pub fn surface_state(&self, surface: BodySurfaceId) -> Option<u32> {
        self.world
            .bodies()
            .surface(surface)
            .map(|s| s.state().code())
    }

    /// Borrow a tissue definition by durable name.
    pub fn tissue_definition_by_name(&self, name: &str) -> Option<&TissueDefinition> {
        self.world.tissues().by_name(name)
    }

    /// Tissue ids whose typed property equals `value`, ascending.
    pub fn tissues_by_property(&self, key: u32, value: i64) -> Vec<TissueId> {
        self.world
            .tissues()
            .by_property(TissueProperty::new(key), value)
            .map(TissueDefinition::id)
            .collect()
    }

    /// All tissue ids, ascending.
    pub fn all_tissue_ids(&self) -> Vec<TissueId> {
        self.world
            .tissues()
            .iter()
            .map(TissueDefinition::id)
            .collect()
    }

    /// All body-plan ids, ascending.
    pub fn all_body_plan_ids(&self) -> Vec<BodyPlanId> {
        self.world.body_plans().iter().map(BodyPlan::id).collect()
    }

    /// All wound ids, ascending.
    pub fn all_wound_ids(&self) -> Vec<WoundId> {
        self.world.wounds().iter().map(WoundRecord::id).collect()
    }
}

#[cfg(test)]
mod tests;
