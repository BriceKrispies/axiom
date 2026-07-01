//! The crucible's scenario-action vocabulary.
//!
//! A scenario is a deterministic, ordered list of [`ScenarioAction`]s, each tagged
//! with the tick it runs on and a data-shaped [`ScenarioActionKind`]. The tick
//! loop runs the actions due at each tick through one generic executor — it
//! contains no per-tick special-casing. Every action holds the durable typed
//! handles it needs (from [`crate::scenario::ScenarioRefs`]); nothing is
//! re-queried at execution time.

use axiom_ecs::EntityHandle;
use axiom_sim_core::{BodySurfaceId, DefinitionId, FactId, ProcessId, ResidueId, TransferRuleId};

use crate::scenario::{self, ScenarioRefs};

/// Which cause stamps the causal events an action produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CauseSpec {
    Command,
    Process(ProcessId),
}

/// Where a surface transfer draws its source residue from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidueSource {
    /// A specific residue captured at setup (e.g. the tavern-cell source).
    Fixed(ResidueId),
    /// Whatever residue currently sits on a named surface (e.g. the paw). That
    /// residue is created at run time by an earlier transfer, so it has no
    /// setup-time handle and is addressed by its durable surface instead.
    OnSurface(BodySurfaceId),
}

/// Move substance along an interaction route onto a target surface: record a
/// surface interaction, then apply a transfer rule. Drives both the contact
/// (source → paw) and the grooming consequence (paw → mouth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceTransferAction {
    pub rule: TransferRuleId,
    pub route: u8,
    /// Where the moved residue comes from.
    pub source: ResidueSource,
    pub target_surface: BodySurfaceId,
    /// The primary entity of the interaction (the creature).
    pub primary: EntityHandle,
    pub material: DefinitionId,
    pub cause: CauseSpec,
    pub interaction_event_kind: u32,
    pub interaction_event_code: u64,
    pub transfer_event_kind: u32,
    pub transfer_event_code: u64,
}

/// Schedule a registered process to wake at a future tick (data-driven scheduling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessAction {
    pub process: ProcessId,
    pub wake_tick: u64,
}

/// Apply the generic material effect rules to the pending interaction (the one a
/// preceding surface transfer recorded), against a context fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectApplicationAction {
    pub context_fact: FactId,
    pub cause: CauseSpec,
}

/// One scenario action, tagged by what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioActionKind {
    SurfaceTransfer(SurfaceTransferAction),
    Process(ProcessAction),
    EffectApplication(EffectApplicationAction),
}

/// A scenario action scheduled at a deterministic tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioAction {
    pub tick: u64,
    pub kind: ScenarioActionKind,
}

/// The deterministic, ordered scenario schedule.
///
/// The tick loop runs the actions whose `tick` equals the current tick, in this
/// order. The grooming process is *registered* in [`scenario::build`]; here a
/// [`ProcessAction`] schedules its wake — the consequence chain (ingestion
/// transfer + effect) is expressed as further actions caused by that process,
/// never inlined into the driver.
pub fn schedule(refs: &ScenarioRefs) -> Vec<ScenarioAction> {
    vec![
        ScenarioAction {
            tick: 0,
            kind: ScenarioActionKind::Process(ProcessAction {
                process: refs.grooming,
                wake_tick: scenario::TICK_GROOM,
            }),
        },
        ScenarioAction {
            tick: scenario::TICK_CONTACT,
            kind: ScenarioActionKind::SurfaceTransfer(SurfaceTransferAction {
                rule: refs.touch_rule,
                route: scenario::ROUTE_TOUCH,
                source: ResidueSource::Fixed(refs.source_residue),
                target_surface: refs.extremity_surface,
                primary: refs.creature,
                material: refs.substance,
                cause: CauseSpec::Command,
                interaction_event_kind: scenario::KIND_CONTACT_INTERACTION,
                interaction_event_code: scenario::CODE_CONTACT_INTERACTION,
                transfer_event_kind: scenario::KIND_CONTACT_TRANSFER,
                transfer_event_code: scenario::CODE_CONTACT_TRANSFER,
            }),
        },
        ScenarioAction {
            tick: scenario::TICK_GROOM,
            kind: ScenarioActionKind::SurfaceTransfer(SurfaceTransferAction {
                rule: refs.ingestion_rule,
                route: scenario::ROUTE_INGESTION,
                source: ResidueSource::OnSurface(refs.extremity_surface),
                target_surface: refs.mouth_surface,
                primary: refs.creature,
                material: refs.substance,
                cause: CauseSpec::Process(refs.grooming),
                interaction_event_kind: scenario::KIND_INGESTION,
                interaction_event_code: scenario::CODE_INGESTION,
                transfer_event_kind: scenario::KIND_GROOM_TRANSFER,
                transfer_event_code: scenario::CODE_GROOM_TRANSFER,
            }),
        },
        ScenarioAction {
            tick: scenario::TICK_GROOM,
            kind: ScenarioActionKind::EffectApplication(EffectApplicationAction {
                context_fact: refs.intoxication_fact,
                cause: CauseSpec::Process(refs.grooming),
            }),
        },
    ]
}
