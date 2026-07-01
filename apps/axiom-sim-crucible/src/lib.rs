//! # Axiom Simulation Crucible — proof app
//!
//! A tiny deterministic, headless app that proves the ECS + sim-core substrate
//! can express a Dwarf-Fortress-like emergent causal chain from *generic*
//! mechanisms — body surfaces, residues, interaction routes, transfer rules, the
//! process scheduler, effect batches, and the causal journal — with no
//! special-case domain logic.
//!
//! The scenario: a `cat` walks through a `tavern-cell` holding a transferable
//! `beer` residue; the beer transfers onto its `paw` surface; a generic grooming
//! process later moves the beer to a `mouth` route via an ingestion-entry
//! interaction; a generic material effect rule (matched by the `intoxicant` tag +
//! the ingestion route, never by the names `cat`/`beer`) updates a fact on the
//! creature. The causal journal explains every step; replay is byte-stable.
//!
//! ## Shape
//! [`scenario::build`] runs once and returns durable typed [`scenario::ScenarioRefs`]
//! (sim-core publishes its id vocabulary, so the driver can hold them).
//! [`action::schedule`] turns those refs into a deterministic, ordered list of
//! [`action::ScenarioAction`]s. The tick loop is boring: step the scheduler, run
//! the actions due this tick through one generic executor, apply the boundary,
//! record. No per-tick consequence is inlined into the driver.

use std::collections::BTreeMap;

use axiom_ecs::EntityRegistry;
use axiom_sim_core::{BodySurfaceId, InteractionId, ResidueId, SimCoreApi};

use crate::action::{
    CauseSpec, EffectApplicationAction, ResidueSource, ScenarioAction, ScenarioActionKind,
    SurfaceTransferAction,
};

pub mod action;
pub mod replay;
pub mod report;
pub mod scenario;

pub use report::{CausalRow, ParentRef};
pub use scenario::{ScenarioRefs, SCENARIO_NAME};

/// The running crucible: the sim world, the ECS registry, the captured scenario
/// references, the action schedule, and the parent attribution for the report.
#[derive(Debug)]
pub struct Crucible {
    api: SimCoreApi,
    reg: EntityRegistry,
    refs: ScenarioRefs,
    schedule: Vec<ScenarioAction>,
    parents: BTreeMap<u64, ParentRef>,
    grooming_woke_tick: Option<u64>,
    pending_interaction: Option<InteractionId>,
    next_tick: u64,
}

impl Crucible {
    /// Build the scenario at tick 0 and derive its action schedule.
    pub fn new() -> Self {
        let mut api = SimCoreApi::new();
        let mut reg = EntityRegistry::new();
        let refs = scenario::build(&mut api, &mut reg);
        let schedule = action::schedule(&refs);
        Crucible {
            api,
            reg,
            refs,
            schedule,
            parents: BTreeMap::new(),
            grooming_woke_tick: None,
            pending_interaction: None,
            next_tick: 0,
        }
    }

    /// Run every tick up to and including `TICK_FINAL`.
    pub fn run(&mut self) {
        self.run_to(scenario::TICK_FINAL);
    }

    /// Advance through every not-yet-run tick up to and including `last`, then
    /// recapture parent attribution. Cumulative and idempotent, so tests can step
    /// the run in stages and inspect between them.
    pub fn run_to(&mut self, last: u64) {
        while self.next_tick <= last {
            let tick = self.next_tick;
            self.tick(tick);
            self.next_tick += 1;
        }
        self.recapture_parents();
    }

    /// One logical tick: step the scheduler (running due process handlers), note a
    /// grooming wake, apply the effect boundary, then run the scenario actions due
    /// this tick. Entirely data-driven — no tick is special-cased here.
    fn tick(&mut self, tick: u64) {
        let woken = self.api.step_scheduler(tick);
        if woken.contains(&self.refs.grooming) {
            self.grooming_woke_tick = Some(tick);
        }
        self.api.apply_scheduler_boundary(tick, &self.reg);
        let due: Vec<ScenarioAction> = self
            .schedule
            .iter()
            .copied()
            .filter(|action| action.tick == tick)
            .collect();
        for action in due {
            self.execute(tick, action);
        }
    }

    /// Generic executor: dispatch one scenario action by its kind.
    fn execute(&mut self, tick: u64, action: ScenarioAction) {
        match action.kind {
            ScenarioActionKind::SurfaceTransfer(transfer) => {
                self.run_surface_transfer(tick, transfer)
            }
            ScenarioActionKind::Process(process) => {
                self.api
                    .schedule_process_wake(process.process, process.wake_tick);
            }
            ScenarioActionKind::EffectApplication(effect) => self.run_effect_application(effect),
        }
    }

    /// Record a surface interaction along a route, then apply the transfer rule that
    /// moves residue onto the target surface.
    fn run_surface_transfer(&mut self, tick: u64, action: SurfaceTransferAction) {
        let source = self.resolve_source(action.source);
        let cause = match action.cause {
            CauseSpec::Command => self.api.cause_command(),
            CauseSpec::Process(process) => self.api.cause_process(process),
        };
        let interaction = self
            .api
            .record_surface_interaction(
                (scenario::interaction_kind(), action.route),
                (action.primary, action.target_surface),
                (Some(action.material), Some(source), None),
                (
                    action.interaction_event_kind,
                    action.interaction_event_code,
                    tick,
                    Some(cause),
                ),
            )
            .expect("surface interaction records");
        self.pending_interaction = Some(interaction);
        let target = self.api.residue_location_for_surface(action.target_surface);
        self.api.apply_transfer(
            action.rule,
            interaction,
            target,
            action.transfer_event_kind,
            action.transfer_event_code,
            tick,
        );
    }

    /// Resolve a transfer's source residue: a fixed setup-captured residue, or the
    /// residue currently on a named surface (created at run time by an earlier
    /// transfer).
    fn resolve_source(&self, source: ResidueSource) -> ResidueId {
        match source {
            ResidueSource::Fixed(residue) => residue,
            ResidueSource::OnSurface(surface) => *self
                .api
                .residues_on_surface(surface)
                .first()
                .expect("the surface holds a residue to transfer"),
        }
    }

    /// Apply the generic material effect rules to the pending (ingestion) interaction.
    fn run_effect_application(&mut self, action: EffectApplicationAction) {
        let cause = match action.cause {
            CauseSpec::Command => self.api.cause_command(),
            CauseSpec::Process(process) => self.api.cause_process(process),
        };
        let interaction = self
            .pending_interaction
            .expect("a surface interaction precedes effect application");
        self.api.apply_material_effects(
            interaction,
            Some(action.context_fact),
            Some(cause),
            &self.reg,
        );
    }

    /// Recapture parent attribution from the durable command and grooming-process
    /// causes. Idempotent: rebuilt from scratch each call.
    fn recapture_parents(&mut self) {
        self.parents.clear();
        let command = self.api.cause_command();
        for event in self.api.events_by_parent(command) {
            self.parents.insert(event.raw(), ParentRef::Command);
        }
        let groom_cause = self.api.cause_process(self.refs.grooming);
        let groom_raw = self.refs.grooming.raw();
        for event in self.api.events_by_parent(groom_cause) {
            self.parents
                .insert(event.raw(), ParentRef::Process(groom_raw));
        }
    }

    /// Total beer residue remaining at the source (tavern-cell) residue.
    pub fn source_amount(&self) -> i64 {
        self.api
            .residue(self.refs.source_residue)
            .map(|residue| residue.quantity().amount())
            .unwrap_or(0)
    }

    /// Total beer residue currently on the creature's extremity (paw) surface.
    pub fn paw_amount(&self) -> i64 {
        self.surface_amount(self.refs.extremity_surface)
    }

    /// Total beer residue currently on the creature's mouth surface.
    pub fn mouth_amount(&self) -> i64 {
        self.surface_amount(self.refs.mouth_surface)
    }

    /// Total residue amount currently on a captured body surface.
    fn surface_amount(&self, surface: BodySurfaceId) -> i64 {
        self.api
            .residues_on_surface(surface)
            .into_iter()
            .filter_map(|residue| self.api.residue(residue).map(|r| r.quantity().amount()))
            .sum()
    }

    /// Whether the creature's intoxication fact has been set by the effect rule.
    pub fn intox_active(&self) -> bool {
        self.api.fact_value(self.refs.intoxication_fact)
            == Some(self.api.value_unsigned(scenario::INTOX_VALUE))
    }

    /// Whether the grooming process added its "groomed" fact (via the effect boundary).
    pub fn groomed(&self) -> bool {
        !self
            .api
            .facts_by_kind(scenario::GROOMED_FACT_KIND)
            .is_empty()
    }

    /// The tick the grooming process woke, if it has.
    pub fn grooming_woke_tick(&self) -> Option<u64> {
        self.grooming_woke_tick
    }

    /// The scenario's deterministic action schedule.
    pub fn actions(&self) -> &[ScenarioAction] {
        &self.schedule
    }

    /// The ordered causal-chain rows.
    pub fn rows(&self) -> Vec<CausalRow> {
        report::build_rows(&self.api, &self.parents)
    }

    /// A structural digest over the final outcome plus the causal chain.
    pub fn digest(&self) -> u64 {
        report::state_digest(
            &self.rows(),
            self.paw_amount(),
            self.mouth_amount(),
            self.intox_active(),
            self.groomed(),
        )
    }

    /// A digest over just the causal-event chain.
    pub fn causal_digest(&self) -> u64 {
        report::causal_digest(&self.rows())
    }
}

impl Default for Crucible {
    fn default() -> Self {
        Crucible::new()
    }
}

/// Run the crucible to completion and render the full structured report.
pub fn run_report() -> String {
    let mut crucible = Crucible::new();
    crucible.run();
    let rows = crucible.rows();
    let replay = replay::verify();
    let mut out = String::new();
    out.push_str(&format!("scenario:   {SCENARIO_NAME}\n"));
    out.push_str(&format!("ticks:      0..={}\n", scenario::TICK_FINAL));
    out.push_str(&format!(
        "outcome:    paw={} mouth={} intoxicated={} groomed={}\n",
        crucible.paw_amount(),
        crucible.mouth_amount(),
        crucible.intox_active(),
        crucible.groomed(),
    ));
    out.push_str(&format!("causal chain ({} events):\n", rows.len()));
    out.push_str(&report::render(&rows));
    out.push_str(&format!("replay:     {}\n", replay.summary()));
    out.push_str(&format!("digest:     {:#018x}\n", crucible.digest()));
    out
}
