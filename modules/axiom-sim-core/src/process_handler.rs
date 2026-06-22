//! The process handler seam: how a woken process produces effects.
//!
//! A handler is a pure transform from a read-only [`ProcessContext`] to a
//! [`ProcessOutput`] (an [`EffectBatch`] plus a requested [`ProcessDisposition`]).
//! It never mutates stores directly — the scheduler applies the returned effects
//! at an explicit boundary. The generic [`ProcessHandler`] trait is the seam;
//! [`HandlerSpec`] is the deterministic, `Clone` implementation the facade drives
//! (the arbitrary boxed-handler production shape is deferred — see
//! `PHASE_5_DEFERRED.md`).

use axiom_ecs::EntityHandle;

use crate::effect::EffectBatch;
use crate::fact::FactValue;
use crate::ids::FactId;
use crate::process_lifecycle::ProcessStatus;
use crate::sim_tick::{SimTick, TickDelta};

// Disposition tags — index into `DISPOSITION_STATUS`.
const DISP_COMPLETE: u8 = 0;
const DISP_RESCHEDULE: u8 = 1;
const DISP_FAIL: u8 = 2;
const DISP_CANCEL: u8 = 3;

// The terminal/sleeping status each disposition resolves a running process to.
const DISPOSITION_STATUS: [ProcessStatus; 4] = [
    ProcessStatus::Completed,
    ProcessStatus::Sleeping,
    ProcessStatus::Failed,
    ProcessStatus::Canceled,
];

/// What a handler asks the scheduler to do with the process after it runs. A
/// tagged value (not an enum) so the scheduler resolves it branchlessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessDisposition {
    tag: u8,
    tick: SimTick,
}

impl ProcessDisposition {
    /// The process is finished.
    pub const fn complete() -> Self {
        ProcessDisposition {
            tag: DISP_COMPLETE,
            tick: SimTick::new(0),
        }
    }
    /// Re-sleep until `tick` (still subscribed/alive).
    pub const fn reschedule(tick: SimTick) -> Self {
        ProcessDisposition {
            tag: DISP_RESCHEDULE,
            tick,
        }
    }
    /// The process errored.
    pub const fn fail() -> Self {
        ProcessDisposition {
            tag: DISP_FAIL,
            tick: SimTick::new(0),
        }
    }
    /// The process is canceled.
    pub const fn cancel() -> Self {
        ProcessDisposition {
            tag: DISP_CANCEL,
            tick: SimTick::new(0),
        }
    }

    /// The running-process status this disposition resolves to.
    pub fn target_status(self) -> ProcessStatus {
        DISPOSITION_STATUS[self.tag as usize]
    }

    /// The reschedule tick, if this is a reschedule.
    pub fn as_reschedule(self) -> Option<SimTick> {
        (self.tag == DISP_RESCHEDULE).then_some(self.tick)
    }
}

/// The read-only context a handler runs against: its subject and the current
/// tick. (Richer read access — the process id and sim-state queries — is the
/// deferred production context, see `PHASE_5_DEFERRED.md`.)
#[derive(Debug, Clone, Copy)]
pub struct ProcessContext {
    subject: EntityHandle,
    tick: SimTick,
}

impl ProcessContext {
    /// Build a context (scheduler-internal).
    pub(crate) const fn new(subject: EntityHandle, tick: SimTick) -> Self {
        ProcessContext { subject, tick }
    }
    /// The process's subject entity.
    pub const fn subject(&self) -> EntityHandle {
        self.subject
    }
    /// The current tick.
    pub const fn tick(&self) -> SimTick {
        self.tick
    }
}

/// A handler's output: the effects it proposes and what to do with the process.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    effects: EffectBatch,
    disposition: ProcessDisposition,
}

impl ProcessOutput {
    /// Build an output.
    pub fn new(effects: EffectBatch, disposition: ProcessDisposition) -> Self {
        ProcessOutput {
            effects,
            disposition,
        }
    }
    /// The proposed effects (consumed at the boundary).
    pub(crate) fn into_effects(self) -> EffectBatch {
        self.effects
    }
    /// The requested disposition.
    pub const fn disposition(&self) -> ProcessDisposition {
        self.disposition
    }
}

/// The seam: a process handler turns a context into an output.
pub trait ProcessHandler {
    /// Run the handler. Pure — no store mutation.
    fn run(&self, context: &ProcessContext) -> ProcessOutput;
}

// HandlerSpec tags — index into `RUN`.
const COMPLETE: u8 = 0;
const UPDATE_FACT: u8 = 1;
const ADD_FACT: u8 = 2;
const RESCHEDULE: u8 = 3;
const FAIL: u8 = 4;
const CANCEL: u8 = 5;

/// A deterministic, `Clone` handler the facade can register without naming the
/// `ProcessHandler` trait. A tagged value so [`ProcessHandler::run`] dispatches by
/// table, not by pattern match.
#[derive(Debug, Clone, Copy)]
pub struct HandlerSpec {
    tag: u8,
    fact: Option<FactId>,
    kind: u32,
    value: Option<FactValue>,
    tick: u64,
    delta: TickDelta,
}

impl HandlerSpec {
    fn empty(tag: u8) -> Self {
        HandlerSpec {
            tag,
            fact: None,
            kind: 0,
            value: None,
            tick: 0,
            delta: TickDelta::new(0),
        }
    }

    /// Produce no effects and complete.
    pub fn complete() -> Self {
        HandlerSpec::empty(COMPLETE)
    }

    /// Produce an update-fact effect, then complete.
    pub fn update_fact_then_complete(fact: FactId, value: FactValue, tick: u64) -> Self {
        HandlerSpec {
            fact: Some(fact),
            value: Some(value),
            tick,
            ..HandlerSpec::empty(UPDATE_FACT)
        }
    }

    /// Produce an add-fact effect on the context subject, then complete.
    pub fn add_fact_then_complete(kind: u32, value: FactValue, tick: u64) -> Self {
        HandlerSpec {
            kind,
            value: Some(value),
            tick,
            ..HandlerSpec::empty(ADD_FACT)
        }
    }

    /// Produce no effects and request a reschedule `delta` ticks later.
    pub fn reschedule_after(delta: TickDelta) -> Self {
        HandlerSpec {
            delta,
            ..HandlerSpec::empty(RESCHEDULE)
        }
    }

    /// Produce no effects and fail.
    pub fn fail() -> Self {
        HandlerSpec::empty(FAIL)
    }

    /// Produce no effects and cancel.
    pub fn cancel() -> Self {
        HandlerSpec::empty(CANCEL)
    }
}

// One run fn per tag.
const RUN: [fn(&HandlerSpec, &ProcessContext) -> ProcessOutput; 6] = [
    run_complete,
    run_update_fact,
    run_add_fact,
    run_reschedule,
    run_fail,
    run_cancel,
];

fn run_complete(_spec: &HandlerSpec, _ctx: &ProcessContext) -> ProcessOutput {
    ProcessOutput::new(EffectBatch::new(), ProcessDisposition::complete())
}

fn run_update_fact(spec: &HandlerSpec, _ctx: &ProcessContext) -> ProcessOutput {
    let mut effects = EffectBatch::new();
    spec.fact
        .zip(spec.value)
        .map(|(fact, value)| effects.update_fact(fact, value, spec.tick));
    ProcessOutput::new(effects, ProcessDisposition::complete())
}

fn run_add_fact(spec: &HandlerSpec, ctx: &ProcessContext) -> ProcessOutput {
    let mut effects = EffectBatch::new();
    spec.value
        .map(|value| effects.add_fact(spec.kind, ctx.subject(), value, None, spec.tick));
    ProcessOutput::new(effects, ProcessDisposition::complete())
}

fn run_reschedule(spec: &HandlerSpec, ctx: &ProcessContext) -> ProcessOutput {
    let disposition = ctx
        .tick()
        .checked_add(spec.delta)
        .map(ProcessDisposition::reschedule)
        .unwrap_or(ProcessDisposition::fail());
    ProcessOutput::new(EffectBatch::new(), disposition)
}

fn run_fail(_spec: &HandlerSpec, _ctx: &ProcessContext) -> ProcessOutput {
    ProcessOutput::new(EffectBatch::new(), ProcessDisposition::fail())
}

fn run_cancel(_spec: &HandlerSpec, _ctx: &ProcessContext) -> ProcessOutput {
    ProcessOutput::new(EffectBatch::new(), ProcessDisposition::cancel())
}

impl ProcessHandler for HandlerSpec {
    fn run(&self, context: &ProcessContext) -> ProcessOutput {
        RUN[self.tag as usize](self, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(tick: u64) -> ProcessContext {
        ProcessContext::new(
            EntityHandle::new(axiom_kernel::EntityId::from_raw(1), 0),
            SimTick::new(tick),
        )
    }

    #[test]
    fn complete_produces_no_effects() {
        let out = HandlerSpec::complete().run(&ctx(0));
        assert_eq!(out.disposition(), ProcessDisposition::complete());
        assert!(out.into_effects().is_empty());
    }

    #[test]
    fn update_and_add_fact_produce_one_effect() {
        let update =
            HandlerSpec::update_fact_then_complete(FactId::from_raw(2), FactValue::Unsigned(7), 1)
                .run(&ctx(0));
        assert_eq!(update.disposition(), ProcessDisposition::complete());
        assert_eq!(update.into_effects().len(), 1);
        let add = HandlerSpec::add_fact_then_complete(5, FactValue::Bool(true), 1).run(&ctx(0));
        assert_eq!(add.into_effects().len(), 1);
    }

    #[test]
    fn reschedule_computes_next_tick_and_overflows_to_fail() {
        let ok = HandlerSpec::reschedule_after(TickDelta::new(5)).run(&ctx(10));
        assert_eq!(
            ok.disposition(),
            ProcessDisposition::reschedule(SimTick::new(15))
        );
        let overflow = HandlerSpec::reschedule_after(TickDelta::new(1)).run(&ctx(u64::MAX));
        assert_eq!(overflow.disposition(), ProcessDisposition::fail());
    }

    #[test]
    fn fail_and_cancel_dispositions() {
        assert_eq!(
            HandlerSpec::fail().run(&ctx(0)).disposition(),
            ProcessDisposition::fail()
        );
        assert_eq!(
            HandlerSpec::cancel().run(&ctx(0)).disposition(),
            ProcessDisposition::cancel()
        );
    }

    #[test]
    fn context_exposes_subject_and_tick() {
        let c = ctx(4);
        assert_eq!(c.tick(), SimTick::new(4));
        assert_eq!(c.subject().id().raw(), 1);
    }

    /// A bespoke handler proves the trait seam is genuinely generic.
    struct AlwaysComplete;
    impl ProcessHandler for AlwaysComplete {
        fn run(&self, _ctx: &ProcessContext) -> ProcessOutput {
            ProcessOutput::new(EffectBatch::new(), ProcessDisposition::complete())
        }
    }

    #[test]
    fn custom_handler_implements_the_seam() {
        let out = AlwaysComplete.run(&ctx(0));
        assert_eq!(out.disposition(), ProcessDisposition::complete());
        // into_effects consumes the output (covers the boundary accessor).
        assert!(out.into_effects().is_empty());
    }
}
