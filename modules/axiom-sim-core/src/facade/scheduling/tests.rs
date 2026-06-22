//! Scheduler facade tests (child of `facade::scheduling`, sees its private items).

use crate::SimCoreApi;
use axiom_ecs::{EntityHandle, EntityRegistry};

const FACT_KIND: u32 = 7;

/// A fresh API + registry + a live subject entity + a fact `f = 0` on it.
fn built() -> (SimCoreApi, EntityRegistry, EntityHandle, crate::ids::FactId) {
    let mut reg = EntityRegistry::new();
    let subject = reg.spawn_handle();
    let mut api = SimCoreApi::new();
    let f = api
        .add_fact(&reg, FACT_KIND, subject, api.value_unsigned(0), None, 0)
        .unwrap();
    (api, reg, subject, f)
}

#[test]
fn empty_scheduler() {
    let api = SimCoreApi::new();
    assert_eq!(api.scheduler_process_count(), 0);
    assert_eq!(api.pending_wake_count(), 0);
    assert!(!api.is_dirty());
    assert_eq!(api.dirty_len(), 0);
    assert!(api.execution_records().is_empty());
}

#[test]
fn checked_tick_math_through_the_facade() {
    let api = SimCoreApi::new();
    assert_eq!(api.tick_add(10, 5), Some(15));
    assert_eq!(api.tick_add(u64::MAX, 1), None);
}

#[test]
fn register_schedule_step_boundary_updates_a_fact() {
    let (mut api, reg, subject, f) = built();
    let p = api.register_process_updating_fact(1, subject, f, api.value_unsigned(5), 1, 0);
    assert_eq!(api.scheduler_process_count(), 1);
    assert_eq!(api.scheduler_process_kind(p), Some(1));
    assert_eq!(
        api.process_status_code(p),
        Some(SimCoreApi::STATUS_SCHEDULED)
    );

    api.schedule_process_wake(p, 1);
    assert_eq!(
        api.process_status_code(p),
        Some(SimCoreApi::STATUS_SLEEPING)
    );
    assert_eq!(api.process_pending_wake(p), Some(1));
    assert_eq!(api.pending_wake_count(), 1);
    // Inspect due (non-consuming): not due at 0, due at 1.
    assert!(api.due_process_ids(0).is_empty());
    assert_eq!(api.due_process_ids(1), vec![p]);

    // Step: handler runs, but effects are NOT applied yet.
    assert_eq!(api.step_scheduler(1), vec![p]);
    assert_eq!(api.process_status_code(p), Some(SimCoreApi::STATUS_RUNNING));
    assert_eq!(
        api.fact_value(f),
        Some(api.value_unsigned(0)),
        "effects deferred to the boundary"
    );

    // Boundary: effects apply, fact updates, process completes.
    let (batches, effects, failed) = api.apply_scheduler_boundary(1, &reg);
    assert_eq!((batches, effects, failed), (1, 1, 0));
    assert_eq!(api.fact_value(f), Some(api.value_unsigned(5)));
    assert_eq!(
        api.process_status_code(p),
        Some(SimCoreApi::STATUS_COMPLETED)
    );

    // Execution record + causal events.
    let records = api.execution_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, p);
    assert_eq!(records[0].2, 1, "one effect produced");
    assert_eq!(records[0].3, SimCoreApi::STATUS_RUNNING, "from running");
    assert_eq!(records[0].4, SimCoreApi::STATUS_COMPLETED, "to completed");
    let events = api.scheduler_events_for_process(p);
    assert!(
        events.len() >= 5,
        "scheduled, woke, started, produced, applied, completed"
    );
}

#[test]
fn dirty_dependency_wakes_a_subscribed_process() {
    let (mut api, reg, subject, f) = built();
    let g = api
        .add_fact(&reg, 8, subject, api.value_unsigned(0), None, 0)
        .unwrap();
    let p = api.register_process_updating_fact(1, subject, g, api.value_unsigned(9), 2, 0);
    // Subscribe p to changes of fact-kind FACT_KIND (the kind of f).
    assert!(api.subscribe_process(p, SimCoreApi::DEP_FACT_KIND, u64::from(FACT_KIND)));
    assert_eq!(
        api.process_dependencies(p),
        vec![(SimCoreApi::DEP_FACT_KIND, u64::from(FACT_KIND))]
    );
    assert_eq!(api.process_subscriptions(p).len(), 1);
    assert_eq!(api.subscription_count(), 1);

    // Update f through an effect batch -> marks f dirty.
    let mut batch = api.new_effect_batch();
    batch.update_fact(f, api.value_unsigned(1), 2);
    api.apply_effects(batch, &reg);
    assert!(api.is_dirty());
    assert_eq!(api.dirty_fact_ids(), vec![f]);
    let details = api.dirty_fact_details();
    assert_eq!(details, vec![(f, FACT_KIND, SimCoreApi::DIRTY_UPDATED)]);

    // Dirty invalidation wakes p (a subscriber), and clears the dirty set.
    let woken = api.apply_dirty_invalidations(2, Some(api.cause_command()));
    assert_eq!(woken, 1);
    assert!(!api.is_dirty(), "invalidation clears the dirty set");
    assert_eq!(
        api.process_status_code(p),
        Some(SimCoreApi::STATUS_SLEEPING)
    );

    // Step + boundary at tick 2 -> g updated.
    assert_eq!(api.step_scheduler(2), vec![p]);
    api.apply_scheduler_boundary(2, &reg);
    assert_eq!(api.fact_value(g), Some(api.value_unsigned(9)));
}

#[test]
fn manual_dirty_marking_and_inspection() {
    let (mut api, mut reg, subject, f) = built();
    let other = reg.spawn_handle();
    let r = api
        .add_relation(&reg, 3, vec![api.endpoint_entity(subject)], None, None)
        .unwrap();
    assert!(api.mark_dirty_fact(f, FACT_KIND, SimCoreApi::DIRTY_UPDATED));
    assert!(api.mark_dirty_relation(r, 3, SimCoreApi::DIRTY_ADDED));
    assert!(api.mark_dirty_subject(other, SimCoreApi::DIRTY_TOUCHED));
    // Invalid dirty code rejected.
    assert!(!api.mark_dirty_fact(f, FACT_KIND, 250));
    assert_eq!(api.dirty_fact_ids(), vec![f]);
    assert_eq!(api.dirty_relation_ids(), vec![r]);
    assert_eq!(api.dirty_subject_count(), 1);
    assert_eq!(
        api.dirty_relation_details(),
        vec![(r, 3, SimCoreApi::DIRTY_ADDED)]
    );
    assert_eq!(
        api.dirty_subject_details(),
        vec![(other, SimCoreApi::DIRTY_TOUCHED, false)]
    );
    assert_eq!(api.dirty_len(), 3);
}

#[test]
fn failing_and_canceling_processes() {
    let (mut api, reg, subject, _f) = built();
    // Failing handler -> status Failed at boundary.
    let fail = api.register_failing_process(1, subject, 0);
    api.schedule_process_wake(fail, 0);
    api.step_scheduler(0);
    api.apply_scheduler_boundary(0, &reg);
    assert_eq!(
        api.process_status_code(fail),
        Some(SimCoreApi::STATUS_FAILED)
    );

    // Cancel a scheduled process.
    let cancelable = api.register_process(1, subject, 0);
    api.schedule_process_wake(cancelable, 5);
    assert!(api.cancel_scheduler_process(cancelable, 1));
    assert_eq!(
        api.process_status_code(cancelable),
        Some(SimCoreApi::STATUS_CANCELED)
    );
    assert!(
        !api.cancel_scheduler_process(cancelable, 2),
        "already terminal"
    );
    // Canceled process is not due.
    assert!(api.due_process_ids(5).is_empty());

    // A process whose own handler cancels it -> Canceled at its boundary.
    let self_cancel = api.register_canceling_process(1, subject, 7);
    api.schedule_process_wake(self_cancel, 7);
    api.step_scheduler(7);
    api.apply_scheduler_boundary(7, &reg);
    assert_eq!(
        api.process_status_code(self_cancel),
        Some(SimCoreApi::STATUS_CANCELED)
    );
}

#[test]
fn dirty_relation_invalidation_wakes_a_subscribed_process() {
    let (mut api, reg, subject, _f) = built();
    let r = api
        .add_relation(&reg, 3, vec![api.endpoint_entity(subject)], None, None)
        .unwrap();
    let p = api.register_process(1, subject, 0);
    // Subscribe p to changes of relation-kind 3 (the kind of r).
    assert!(api.subscribe_process(p, SimCoreApi::DEP_RELATION_KIND, 3));

    // Mark the relation dirty, then invalidate: p (a subscriber) wakes and the
    // dirty set clears.
    assert!(api.mark_dirty_relation(r, 3, SimCoreApi::DIRTY_ADDED));
    assert_eq!(api.dirty_relation_ids(), vec![r]);
    let woken = api.apply_dirty_invalidations(2, Some(api.cause_command()));
    assert_eq!(woken, 1);
    assert!(!api.is_dirty(), "invalidation clears the dirty set");
    assert_eq!(
        api.process_status_code(p),
        Some(SimCoreApi::STATUS_SLEEPING)
    );
}

#[test]
fn rescheduling_handler_sleeps_and_re_arms() {
    let (mut api, reg, subject, _f) = built();
    let p = api.register_process_rescheduling(1, subject, 3, 0);
    api.schedule_process_wake(p, 0);
    api.step_scheduler(0);
    api.apply_scheduler_boundary(0, &reg);
    // Reschedule disposition -> Sleeping, re-armed 3 ticks later.
    assert_eq!(
        api.process_status_code(p),
        Some(SimCoreApi::STATUS_SLEEPING)
    );
    assert_eq!(api.process_pending_wake(p), Some(3));
    // Reschedule manually too.
    assert!(api.reschedule_process_wake(p, 9));
    assert_eq!(api.process_pending_wake(p), Some(9));
}

#[test]
fn add_fact_handler_and_with_reason_wake() {
    let (mut api, reg, subject, _f) = built();
    let p = api.register_process_adding_fact(1, subject, 99, api.value_unsigned(1), 4, 0);
    // Schedule with an explicit reason code; invalid reason rejected.
    assert!(api.schedule_process_wake_with_reason(p, 4, SimCoreApi::WAKE_SCHEDULED));
    assert!(!api.schedule_process_wake_with_reason(p, 4, 250));
    api.step_scheduler(4);
    api.apply_scheduler_boundary(4, &reg);
    // The handler added a fact of kind 99 on the subject.
    assert_eq!(api.facts_by_kind(99).len(), 1);
}

#[test]
fn invalid_subscription_codes_and_unknown_process() {
    let (mut api, reg, subject, _f) = built();
    let p = api.register_process(1, subject, 0);
    assert!(api.subscribe_process(p, SimCoreApi::DEP_SUBJECT, subject.id().raw()));
    assert!(
        !api.subscribe_process(p, SimCoreApi::DEP_SUBJECT, subject.id().raw()),
        "dedup"
    );
    assert!(!api.subscribe_process(p, 250, 0), "invalid dependency code");
    assert!(
        !api.subscribe_process(
            crate::ids::ProcessId::from_raw(9999),
            SimCoreApi::DEP_GENERIC,
            0
        ),
        "unknown process"
    );
    // Unknown process status / kind are None.
    assert!(api
        .process_status_code(crate::ids::ProcessId::from_raw(9999))
        .is_none());
    assert!(api
        .scheduler_process_kind(crate::ids::ProcessId::from_raw(9999))
        .is_none());
    let _ = reg;
}

#[test]
fn scheduler_chain_is_deterministic() {
    let run = || {
        let (mut api, reg, subject, f) = built();
        let p = api.register_process_updating_fact(1, subject, f, api.value_unsigned(5), 1, 0);
        api.subscribe_process(p, SimCoreApi::DEP_FACT_KIND, u64::from(FACT_KIND));
        api.schedule_process_wake(p, 1);
        api.step_scheduler(1);
        api.apply_scheduler_boundary(1, &reg);
        (
            api.fact_value(f) == Some(api.value_unsigned(5)),
            api.process_status_code(p),
            api.scheduler_events_for_process(p).len(),
        )
    };
    assert_eq!(run(), run());
}
