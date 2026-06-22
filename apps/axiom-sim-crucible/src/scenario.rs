//! Deterministic construction of the crucible scenario.
//!
//! Everything here is generic substrate composed with concrete *scenario data*
//! (a `cat`, a `beer` substance, a `paw`, a `mouth`, a `tavern-cell`). Those
//! domain names live only in this app; the substrate sees opaque codes, names,
//! tags, and routes. The rule that fires the final effect matches the generic
//! `intoxicant` tag + the ingestion route — never the names `cat` or `beer`.
//!
//! `build` runs once at tick 0 and returns a [`ScenarioRefs`] of durable typed
//! handles — captured at setup and reused for the rest of the run, never
//! re-queried. sim-core publishes its identity vocabulary (`ResidueId`,
//! `BodySurfaceId`, `ProcessId`, …) precisely so a composition root can hold
//! these nouns; see `ARCHITECTURE.md`.

use axiom_ecs::{EntityHandle, EntityRegistry};
use axiom_sim_core::{
    BodyId, BodyPartId, BodySurfaceId, DefinitionId, FactId, ProcessId, ResidueId, SimCoreApi,
    TransferRuleId,
};

/// The scenario's display name.
pub const SCENARIO_NAME: &str = "cat-in-tavern";

// ---- logical ticks (fixed and deterministic; documented in tests) ----

/// Tick at which the creature contacts the source residue.
pub const TICK_CONTACT: u64 = 2;
/// Tick at which the grooming process wakes and its consequences resolve.
pub const TICK_GROOM: u64 = 5;
/// Final tick; the report is produced after this tick runs.
pub const TICK_FINAL: u64 = 8;

// ---- sim-core InteractionRoute codes (Touch = 0, Ingestion = 1) ----

/// Interaction route code: touch (surface contact).
pub const ROUTE_TOUCH: u8 = 0;
/// Interaction route code: ingestion (mouth entry).
pub const ROUTE_INGESTION: u8 = 1;

// ---- opaque domain codes (meaningless to the substrate) ----

const SUBSTANCE_KIND: u32 = 1;
const GROOMING_PROCESS_KIND: u32 = 10;
const INTERACTION_KIND: u32 = 200;

/// Fact kind: the creature's intoxication marker (updated by the effect rule).
pub(crate) const INTOX_FACT_KIND: u32 = 100;
/// Fact kind: the "groomed" marker the grooming process adds via its effect.
pub(crate) const GROOMED_FACT_KIND: u32 = 101;

// ---- causal-event kinds the scenario stamps (used by the report) ----

/// Causal kind: the contact interaction record.
pub const KIND_CONTACT_INTERACTION: u32 = 7000;
/// Causal kind: the source → extremity-surface transfer.
pub const KIND_CONTACT_TRANSFER: u32 = 7001;
/// Causal kind: the extremity-surface → mouth-surface transfer.
pub const KIND_GROOM_TRANSFER: u32 = 7002;
/// Causal kind: the ingestion-entry interaction record.
pub const KIND_INGESTION: u32 = 7003;
/// Causal kind: the generic material effect updating the creature fact.
pub const KIND_INTOX_EFFECT: u32 = 7004;

// ---- causal-event symbol codes ----

pub(crate) const CODE_CONTACT_INTERACTION: u64 = 0xC0;
pub(crate) const CODE_CONTACT_TRANSFER: u64 = 0xC1;
pub(crate) const CODE_GROOM_TRANSFER: u64 = 0xC2;
pub(crate) const CODE_INGESTION: u64 = 0xC3;
pub(crate) const CODE_INTOX_EFFECT: u64 = 0xC4;

// ---- scenario data ----

/// Non-body source location (the tavern cell), as an opaque residue-location code.
pub(crate) const TAVERN_CELL: u64 = 9001;
/// Substance name (app-only).
pub(crate) const SUBSTANCE_NAME: &str = "beer";
/// How much substance starts in the source residue.
pub(crate) const SOURCE_AMOUNT: i64 = 10;
/// How much each contact moves onto the extremity surface.
pub(crate) const CONTACT_MOVE: i64 = 4;
/// How much grooming moves from the extremity surface to the mouth.
pub(crate) const GROOM_MOVE: i64 = 3;
/// The intoxication fact value the effect rule sets.
pub(crate) const INTOX_VALUE: u64 = 1;

/// Durable typed handles into the scenario, captured once at setup.
///
/// Every field is a value-type id from the ECS layer or sim-core's published
/// identity vocabulary. The driver holds these and reuses them each tick — it
/// never re-queries the world for "the paw" or "the grooming process".
#[derive(Debug, Clone, Copy)]
pub struct ScenarioRefs {
    /// The creature entity.
    pub creature: EntityHandle,
    /// The creature's instantiated body.
    pub body: BodyId,
    /// The extremity (paw) body part.
    pub extremity_part: BodyPartId,
    /// The outer surface of the extremity (paw) — beer lands here on contact.
    pub extremity_surface: BodySurfaceId,
    /// The mouth surface — beer enters here on ingestion.
    pub mouth_surface: BodySurfaceId,
    /// The source residue sitting in the tavern cell.
    pub source_residue: ResidueId,
    /// The fixed transfer rule on the touch route (source → paw).
    pub touch_rule: TransferRuleId,
    /// The fixed transfer rule on the ingestion route (paw → mouth).
    pub ingestion_rule: TransferRuleId,
    /// The creature's intoxication fact (updated by the generic effect rule).
    pub intoxication_fact: FactId,
    /// The beer substance definition.
    pub substance: DefinitionId,
    /// The grooming process (registered here, scheduled by a scenario action).
    pub grooming: ProcessId,
}

/// Build the full scenario into `api`/`reg`, returning durable [`ScenarioRefs`].
///
/// Deterministic and side-effect-ordered: a creature, a `core + paw + head`
/// body plan, a transferable+intoxicant `beer` substance, a source residue in the
/// tavern cell, two transfer rules (touch / ingestion), two generic material
/// effect rules (update-fact + emit-causal-event, matched by tag + route), an
/// intoxication fact starting at 0, and a grooming process that adds a "groomed"
/// fact through the effect boundary when it wakes. The process is *registered*
/// here but *scheduled* by a scenario action (see [`crate::action`]).
pub fn build(api: &mut SimCoreApi, reg: &mut EntityRegistry) -> ScenarioRefs {
    let creature = reg.spawn_handle();

    // Tissue: a covering that can hold residue.
    let fur = api
        .register_tissue(
            SimCoreApi::TISSUE_COVERING,
            "fur",
            &[SimCoreApi::TTAG_CAN_HOLD_RESIDUE],
            &[],
        )
        .expect("tissue registers");

    // Body plan: core (outer surface) + paw extremity (outer surface) + head (mouth surface).
    let draft = api.begin_body_plan();
    api.add_body_plan_part(
        draft,
        "core",
        SimCoreApi::PART_CORE,
        0,
        0,
        &[],
        &[(fur, 0)],
        &[(SimCoreApi::SURFACE_OUTER, true)],
    )
    .expect("core part");
    api.add_body_plan_part(
        draft,
        "paw",
        SimCoreApi::PART_EXTREMITY,
        0,
        0,
        &[],
        &[(fur, 0)],
        &[(SimCoreApi::SURFACE_OUTER, true)],
    )
    .expect("paw part");
    api.add_body_plan_part(
        draft,
        "head",
        SimCoreApi::PART_MOUTH,
        0,
        0,
        &[],
        &[],
        &[(SimCoreApi::SURFACE_MOUTH, true)],
    )
    .expect("head part");
    api.connect_body_plan_parts(draft, 0, 1);
    api.connect_body_plan_parts(draft, 0, 2);
    let plan = api
        .finish_body_plan(draft, "cat-body")
        .expect("body plan finishes");

    // Instantiate the body, owned by the creature, then capture its parts/surfaces.
    let body = api
        .instantiate_body(plan, Some(creature), reg, Some(api.cause_command()), 0)
        .expect("body instantiates");
    let extremity_part = *api
        .body_parts_by_kind(body, SimCoreApi::PART_EXTREMITY)
        .first()
        .expect("the body has exactly one extremity part");
    let extremity_surface = *api
        .part_surfaces(extremity_part)
        .first()
        .expect("the extremity has an outer surface");
    let mouth_surface = *api
        .body_surfaces_by_kind(body, SimCoreApi::SURFACE_MOUTH)
        .first()
        .expect("the body has a mouth surface");

    // Substance: beer — tagged transferable + intoxicant (generic tags the rule matches).
    let substance = api
        .register_substance(
            SUBSTANCE_NAME,
            SUBSTANCE_KIND,
            &[
                SimCoreApi::TAG_CONTACT_TRANSFERABLE,
                SimCoreApi::TAG_INTOXICANT,
            ],
            &[],
        )
        .expect("substance registers");

    // Source residue in the tavern cell (a non-body location).
    let source_location = api.residue_location_symbol(TAVERN_CELL);
    let source_amount = api
        .quantity(SimCoreApi::UNIT_VOLUME, SOURCE_AMOUNT)
        .expect("valid quantity");
    let source_residue = api.create_residue(substance, source_amount, source_location, 0, None, 0);

    // Transfer rules: fixed amounts on the touch and ingestion routes.
    let touch_rule = api
        .register_transfer_fixed(CONTACT_MOVE, ROUTE_TOUCH, false)
        .expect("contact rule");
    let ingestion_rule = api
        .register_transfer_fixed(GROOM_MOVE, ROUTE_INGESTION, false)
        .expect("grooming rule");

    // The creature's intoxication fact starts at 0.
    let intoxication_fact = api
        .add_fact(
            reg,
            INTOX_FACT_KIND,
            creature,
            api.value_unsigned(0),
            None,
            0,
        )
        .expect("intox fact");

    // Generic material effect rules: any intoxicant entering via ingestion sets the
    // intoxication fact and emits a causal event. Matched by tag + route, not names.
    api.register_material_effect_rule(
        Some(SimCoreApi::TAG_INTOXICANT),
        ROUTE_INGESTION,
        SimCoreApi::EFFECT_UPDATE_FACT,
        0,
        Some(api.value_unsigned(INTOX_VALUE)),
        0,
        0,
        0,
        0,
    )
    .expect("update-fact effect rule");
    api.register_material_effect_rule(
        Some(SimCoreApi::TAG_INTOXICANT),
        ROUTE_INGESTION,
        SimCoreApi::EFFECT_EMIT_CAUSAL_EVENT,
        KIND_INTOX_EFFECT,
        Some(api.value_unsigned(INTOX_VALUE)),
        0,
        CODE_INTOX_EFFECT,
        0,
        0,
    )
    .expect("emit-event effect rule");

    // Grooming process: when it wakes it adds a "groomed" fact via the effect
    // boundary (it never mutates stores directly). A scenario action schedules its
    // wake for TICK_GROOM.
    let grooming = api.register_process_adding_fact(
        GROOMING_PROCESS_KIND,
        creature,
        GROOMED_FACT_KIND,
        api.value_unsigned(1),
        TICK_GROOM,
        0,
    );

    ScenarioRefs {
        creature,
        body,
        extremity_part,
        extremity_surface,
        mouth_surface,
        source_residue,
        touch_rule,
        ingestion_rule,
        intoxication_fact,
        substance,
        grooming,
    }
}

/// The interaction kind code stamped on recorded interactions.
pub(crate) const fn interaction_kind() -> u32 {
    INTERACTION_KIND
}
