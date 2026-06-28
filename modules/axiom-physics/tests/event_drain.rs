//! Proofs for the deterministic event log: ordering, draining, and bounded growth.
//!
//! `events()` is a read-only view of pending events in emission order;
//! `drain_events()` returns them in that order and clears the queue so the log —
//! which gains a `StepCompleted` every step — cannot grow without bound. Driven
//! only through the public [`PhysicsApi`] facade. The event type is sealed, so
//! kinds are inspected through their `Debug` rendering, never by naming a variant.

use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::Transform;
use axiom_physics::PhysicsApi;
use axiom_runtime::RuntimeStep;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn meters(v: f32) -> Meters {
    Meters::new(v).unwrap()
}

fn tenth_second() -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(0), Tick::new(0), 100_000_000, 0)
}

/// A world with two bodies, one collider, and one completed step — so the log
/// holds, in order: BodyCreated, BodyCreated, ColliderAttached, StepCompleted.
fn seeded_world() -> PhysicsApi {
    let mut api = PhysicsApi::new();
    let a = api.create_static_body(Transform::IDENTITY).unwrap();
    let _b = api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    api.attach_sphere_collider(a, meters(1.0), material, false).unwrap();
    api.step(tenth_second()).unwrap();
    api
}

#[test]
fn drain_events_returns_events_in_order() {
    let mut api = seeded_world();
    let view: Vec<String> = api.events().iter().map(|e| format!("{e:?}")).collect();
    let drained = api.drain_events();
    let drained_text: Vec<String> = drained.iter().map(|e| format!("{e:?}")).collect();

    assert_eq!(view, drained_text, "drain returns the same events the view showed");
    assert_eq!(drained.len(), 4);
    assert!(drained_text[0].contains("BodyCreated"), "first event is a creation");
    assert!(drained_text[1].contains("BodyCreated"));
    assert!(drained_text[2].contains("ColliderAttached"));
    assert!(drained_text[3].contains("StepCompleted"), "last event is the step completion");
}

#[test]
fn drain_events_clears_internal_event_queue() {
    let mut api = seeded_world();
    assert!(!api.events().is_empty(), "events accumulate before a drain");
    let drained = api.drain_events();
    assert_eq!(drained.len(), 4);
    assert!(api.events().is_empty(), "the queue is empty immediately after a drain");
}

#[test]
fn events_view_matches_pending_events_before_drain() {
    let mut api = seeded_world();
    let view: Vec<_> = api.events().to_vec();
    let drained = api.drain_events();
    assert_eq!(view, drained, "the view is exactly what a drain yields");
}

#[test]
fn step_completed_does_not_grow_without_bound_when_drained_each_step() {
    let mut api = PhysicsApi::new();
    api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    api.drain_events(); // clear the initial BodyCreated event

    for _ in 0..1000 {
        api.step(tenth_second()).unwrap();
        // Each step adds exactly one StepCompleted; draining keeps the log at zero.
        assert_eq!(api.events().len(), 1, "only this step's event is pending");
        let drained = api.drain_events();
        assert_eq!(drained.len(), 1);
        assert!(api.events().is_empty(), "drained each step, the log never grows");
    }
}

#[test]
fn draining_empty_events_is_deterministic() {
    let mut api = PhysicsApi::new();
    let first = api.drain_events();
    let second = api.drain_events();
    assert!(first.is_empty(), "a fresh world has no events");
    assert_eq!(first, second, "draining an empty log is deterministic and empty");
}

#[test]
fn events_after_drain_only_include_new_events() {
    let mut api = seeded_world();
    let _old = api.drain_events();
    assert!(api.events().is_empty());

    // A fresh operation produces only its own event, not the drained history.
    api.create_static_body(Transform::IDENTITY).unwrap();
    let now: Vec<String> = api.events().iter().map(|e| format!("{e:?}")).collect();
    assert_eq!(now.len(), 1, "only the new BodyCreated is pending");
    assert!(now[0].contains("BodyCreated"));
}
