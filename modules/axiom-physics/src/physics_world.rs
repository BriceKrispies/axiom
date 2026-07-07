//! The deterministic physics world: owns all rigid-body and collider state.
//! `PhysicsWorld` is the heart of the module. It owns ordered (`Vec`-backed)
//! body and collider storage, the FIFO command queue, the event log, the step
//! counter, monotonic id allocators, the latest step record, and the most recent
//! step's contact manifolds. It mutates only its own state from explicit inputs —
//! it never reads a clock or randomness, and knows nothing of scenes, renderers,
//! assets, input, or gameplay.
//! ## Stepping (substepped, atomic)
//! A single fixed step is split into `max_substeps` deterministic substeps so a
//! fast body cannot tunnel through thin geometry in one large jump. Queued
//! commands are applied **once**, before substepping; each substep then runs the
//! full broad → narrow → integrate → solve → correct pipeline over a fraction of
//! the step. A `StepCompleted` event is emitted **once** per outer step.
//! ## Atomic finiteness (no poisoned state)
//! Validation screens input *finiteness*, but a finite-but-extreme force,
//! impulse, or gravity can still drive computed velocity/translation to
//! `NaN`/`±∞`. So a step **commits only if every resulting body is finite**; a
//! non-finite result rolls the world back to its exact pre-step state (bodies,
//! events, and the command queue all untouched) and returns a deterministic
//! `NonFiniteStepResult` error. A committed body — and therefore every snapshot —
//! can never carry a non-finite value.

use axiom_kernel::Meters;
use axiom_math::{Transform, Vec3};
use axiom_runtime::RuntimeStep;

use crate::broad_phase_pair;
use crate::contact_manifold::ContactManifold;
use crate::contact_report::ContactReport;
use crate::contact_pair;
use crate::contact_solver;
use crate::integrator;
use crate::physics_body::PhysicsBody;
use crate::physics_body_desc::PhysicsBodyDesc;
use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider::PhysicsCollider;
use crate::physics_collider_handle::PhysicsColliderHandle;
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_command::PhysicsCommand;
use crate::physics_config::PhysicsConfig;
use crate::physics_error::PhysicsError;
use crate::physics_event::PhysicsEvent;
use crate::physics_material::PhysicsMaterial;
use crate::physics_result::PhysicsResult;
use crate::physics_snapshot::{BodySnapshot, ColliderSnapshot, PhysicsSnapshot};
use crate::physics_step_record::PhysicsStepRecord;
use crate::physics_step_result::PhysicsStepResult;

/// The deterministic conversion factor from the explicit step's nanoseconds to
/// seconds — the single place a fixed step becomes a floating-point `dt`.
const NANOS_PER_SECOND: f32 = 1_000_000_000.0;

/// A command-apply function: mutate the target body and optionally emit an event.
type ApplyFn = fn(Vec3, PhysicsBodyHandle, &mut PhysicsBody) -> Option<PhysicsEvent>;

/// The command-apply table, indexed by `PhysicsCommandKind as usize`. Its order
/// is locked to the `PhysicsCommandKind` discriminants in `physics_command.rs`.
const APPLY_TABLE: [ApplyFn; 5] = [
    apply_force_cmd,
    apply_impulse_cmd,
    enable_cmd,
    disable_cmd,
    apply_torque_cmd,
];

fn apply_force_cmd(
    vector: Vec3,
    _body: PhysicsBodyHandle,
    target: &mut PhysicsBody,
) -> Option<PhysicsEvent> {
    target.forces_mut().apply_force(vector);
    None
}

fn apply_torque_cmd(
    vector: Vec3,
    _body: PhysicsBodyHandle,
    target: &mut PhysicsBody,
) -> Option<PhysicsEvent> {
    target.forces_mut().apply_torque(vector);
    None
}

fn apply_impulse_cmd(
    vector: Vec3,
    _body: PhysicsBodyHandle,
    target: &mut PhysicsBody,
) -> Option<PhysicsEvent> {
    target.forces_mut().apply_impulse(vector);
    None
}

fn enable_cmd(
    _vector: Vec3,
    body: PhysicsBodyHandle,
    target: &mut PhysicsBody,
) -> Option<PhysicsEvent> {
    target.set_enabled(true);
    Some(PhysicsEvent::BodyEnabled { body })
}

fn disable_cmd(
    _vector: Vec3,
    body: PhysicsBodyHandle,
    target: &mut PhysicsBody,
) -> Option<PhysicsEvent> {
    target.set_enabled(false);
    Some(PhysicsEvent::BodyDisabled { body })
}

/// `true` iff every component of `v` is finite.
fn vec3_is_finite(v: Vec3) -> bool {
    v.x.is_finite() & v.y.is_finite() & v.z.is_finite()
}

/// `true` iff every component of a transform (translation, rotation, scale) is
/// finite — the screen for an immediate teleport, so a non-finite target is
/// rejected deterministically rather than committed into a snapshot.
fn transform_is_finite(t: Transform) -> bool {
    vec3_is_finite(t.translation)
        & t.rotation.x.is_finite()
        & t.rotation.y.is_finite()
        & t.rotation.z.is_finite()
        & t.rotation.w.is_finite()
        & vec3_is_finite(t.scale)
}

/// Real per-step dynamics work, summed across the step's substeps.
#[derive(Debug, Clone, Copy)]
struct SubstepCounts {
    integration_count: u32,
    broad_phase_pair_count: u32,
    contact_pair_count: u32,
    solved_contact_count: u32,
    frictioned_contact_count: u32,
}

impl SubstepCounts {
    fn zero() -> Self {
        SubstepCounts {
            integration_count: 0,
            broad_phase_pair_count: 0,
            contact_pair_count: 0,
            solved_contact_count: 0,
            frictioned_contact_count: 0,
        }
    }

    fn add(self, other: SubstepCounts) -> Self {
        SubstepCounts {
            integration_count: self.integration_count + other.integration_count,
            broad_phase_pair_count: self.broad_phase_pair_count + other.broad_phase_pair_count,
            contact_pair_count: self.contact_pair_count + other.contact_pair_count,
            solved_contact_count: self.solved_contact_count + other.solved_contact_count,
            frictioned_contact_count: self.frictioned_contact_count
                + other.frictioned_contact_count,
        }
    }
}

/// The owned, deterministic state of a physics world.
#[derive(Debug)]
pub(crate) struct PhysicsWorld {
    config: PhysicsConfig,
    bodies: Vec<PhysicsBody>,
    colliders: Vec<PhysicsCollider>,
    commands: Vec<PhysicsCommand>,
    events: Vec<PhysicsEvent>,
    last_contacts: Vec<ContactManifold>,
    step_index: u64,
    next_body_id: u64,
    next_collider_id: u64,
    latest_record: PhysicsStepRecord,
}

impl PhysicsWorld {
    /// Create an empty world with the given configuration.
    pub(crate) fn new(config: PhysicsConfig) -> Self {
        PhysicsWorld {
            config,
            bodies: Vec::new(),
            colliders: Vec::new(),
            commands: Vec::new(),
            events: Vec::new(),
            last_contacts: Vec::new(),
            step_index: 0,
            next_body_id: 0,
            next_collider_id: 0,
            latest_record: PhysicsStepRecord::empty(),
        }
    }

    /// The bodies, in insertion order (used by queries).
    pub(crate) fn bodies(&self) -> &[PhysicsBody] {
        &self.bodies
    }

    /// The colliders, in insertion order (used by queries).
    pub(crate) fn colliders(&self) -> &[PhysicsCollider] {
        &self.colliders
    }

    /// The dense slice index of a body handle, or `None` for a `NULL`/stale
    /// handle. Handles are 1-based and allocated in creation order with no
    /// removal, so handle `h` always lives at index `h - 1` — an O(1) lookup, not
    /// a linear scan.
    fn body_index(&self, handle: PhysicsBodyHandle) -> Option<usize> {
        handle
            .raw()
            .checked_sub(1)
            .map(|i| i as usize)
            .filter(|i| *i < self.bodies.len())
    }

    fn body_at(&self, handle: PhysicsBodyHandle) -> Option<&PhysicsBody> {
        self.body_index(handle).map(|i| &self.bodies[i])
    }

    fn body_at_mut(&mut self, handle: PhysicsBodyHandle) -> Option<&mut PhysicsBody> {
        self.body_index(handle).map(|i| &mut self.bodies[i])
    }

    /// Create a body from a validated description, rejecting a full world.
    pub(crate) fn create_body(&mut self, desc: PhysicsBodyDesc) -> PhysicsResult<PhysicsBodyHandle> {
        ((self.bodies.len() as u32) < self.config.max_bodies())
            .then_some(())
            .ok_or(PhysicsError::body_capacity_exceeded(
                "body capacity exceeded",
            ))
            .map(|()| self.push_body(desc))
    }

    fn push_body(&mut self, desc: PhysicsBodyDesc) -> PhysicsBodyHandle {
        self.next_body_id += 1;
        let handle = PhysicsBodyHandle::from_raw(self.next_body_id);
        self.bodies.push(PhysicsBody::from_desc(handle, desc));
        self.events.push(PhysicsEvent::BodyCreated { body: handle });
        handle
    }

    /// Attach a collider to an existing body, rejecting an unknown body or a
    /// full collider store.
    pub(crate) fn attach_collider(
        &mut self,
        body: PhysicsBodyHandle,
        shape: PhysicsColliderShape,
        material: PhysicsMaterial,
        is_trigger: bool,
    ) -> PhysicsResult<PhysicsColliderHandle> {
        self.body_exists(body)
            .then_some(())
            .ok_or(PhysicsError::body_not_found(
                "collider target body not found",
            ))
            .and_then(|()| {
                ((self.colliders.len() as u32) < self.config.max_colliders())
                    .then_some(())
                    .ok_or(PhysicsError::collider_capacity_exceeded(
                        "collider capacity exceeded",
                    ))
            })
            .map(|()| self.push_collider(body, shape, material, is_trigger))
    }

    fn push_collider(
        &mut self,
        body: PhysicsBodyHandle,
        shape: PhysicsColliderShape,
        material: PhysicsMaterial,
        is_trigger: bool,
    ) -> PhysicsColliderHandle {
        self.next_collider_id += 1;
        let handle = PhysicsColliderHandle::from_raw(self.next_collider_id);
        self.colliders
            .push(PhysicsCollider::new(handle, body, shape, material, is_trigger));
        // Derive the body's inverse inertia from this collider's shape + mass, so
        // a torque produces the correct angular acceleration. An immovable body
        // (zero mass) derives zero inertia and is unaffected.
        self.body_at_mut(body).into_iter().for_each(|b| {
            let updated = b.mass_properties().with_inertia_for(shape);
            b.set_mass_properties(updated);
        });
        self.events.push(PhysicsEvent::ColliderAttached {
            collider: handle,
            body,
        });
        handle
    }

    /// Attach a heightfield collider (carrying its grid) to `body`, with the same
    /// body-exists + capacity validation as [`Self::attach_collider`].
    pub(crate) fn attach_heightfield_collider(
        &mut self,
        body: PhysicsBodyHandle,
        shape: PhysicsColliderShape,
        material: PhysicsMaterial,
        is_trigger: bool,
        heightfield: crate::physics_heightfield::Heightfield,
    ) -> PhysicsResult<PhysicsColliderHandle> {
        self.body_exists(body)
            .then_some(())
            .ok_or(PhysicsError::body_not_found("collider target body not found"))
            .and_then(|()| {
                ((self.colliders.len() as u32) < self.config.max_colliders())
                    .then_some(())
                    .ok_or(PhysicsError::collider_capacity_exceeded("collider capacity exceeded"))
            })
            .map(|()| {
                self.next_collider_id += 1;
                let handle = PhysicsColliderHandle::from_raw(self.next_collider_id);
                self.colliders.push(PhysicsCollider::new_heightfield(handle, body, shape, material, is_trigger, heightfield));
                self.body_at_mut(body).into_iter().for_each(|b| {
                    let updated = b.mass_properties().with_inertia_for(shape);
                    b.set_mass_properties(updated);
                });
                self.events.push(PhysicsEvent::ColliderAttached { collider: handle, body });
                handle
            })
    }

    /// Queue a force on a dynamic, enabled body (validated immediately).
    pub(crate) fn enqueue_apply_force(
        &mut self,
        body: PhysicsBodyHandle,
        force: Vec3,
    ) -> PhysicsResult<()> {
        self.validate_dynamic_target(
            body,
            force,
            PhysicsError::force_on_non_dynamic_body("force requires a dynamic body"),
        )
        .map(|()| self.commands.push(PhysicsCommand::apply_force(body, force)))
    }

    /// Queue an impulse on a dynamic, enabled body (validated immediately).
    pub(crate) fn enqueue_apply_impulse(
        &mut self,
        body: PhysicsBodyHandle,
        impulse: Vec3,
    ) -> PhysicsResult<()> {
        self.validate_dynamic_target(
            body,
            impulse,
            PhysicsError::impulse_on_non_dynamic_body("impulse requires a dynamic body"),
        )
        .map(|()| self.commands.push(PhysicsCommand::apply_impulse(body, impulse)))
    }

    /// Queue a torque on a dynamic, enabled body (validated immediately), rejected
    /// on a non-dynamic or disabled body exactly like a force.
    pub(crate) fn enqueue_apply_torque(
        &mut self,
        body: PhysicsBodyHandle,
        torque: Vec3,
    ) -> PhysicsResult<()> {
        self.validate_dynamic_target(
            body,
            torque,
            PhysicsError::force_on_non_dynamic_body("torque requires a dynamic body"),
        )
        .map(|()| self.commands.push(PhysicsCommand::apply_torque(body, torque)))
    }

    /// Validate a force/impulse target: the vector is finite, the body exists, it
    /// is dynamic, and it is currently enabled. A disabled body deterministically
    /// rejects the operation rather than silently dropping the force.
    fn validate_dynamic_target(
        &self,
        body: PhysicsBodyHandle,
        vector: Vec3,
        non_dynamic_error: PhysicsError,
    ) -> PhysicsResult<()> {
        vec3_is_finite(vector)
            .then_some(())
            .ok_or(PhysicsError::non_finite_input(
                "force / impulse vector must be finite",
            ))
            .and_then(|()| {
                self.body_at(body).ok_or(PhysicsError::body_not_found(
                    "force / impulse target body not found",
                ))
            })
            .and_then(|b| b.kind().is_dynamic().then_some(b).ok_or(non_dynamic_error))
            .and_then(|b| {
                b.enabled()
                    .then_some(())
                    .ok_or(PhysicsError::operation_on_disabled_body(
                        "force / impulse target body is disabled",
                    ))
            })
    }

    /// Queue enabling a body (validated immediately).
    pub(crate) fn enqueue_enable(&mut self, body: PhysicsBodyHandle) -> PhysicsResult<()> {
        self.require_body(body)
            .map(|()| self.commands.push(PhysicsCommand::enable_body(body)))
    }

    /// Queue disabling a body (validated immediately).
    pub(crate) fn enqueue_disable(&mut self, body: PhysicsBodyHandle) -> PhysicsResult<()> {
        self.require_body(body)
            .map(|()| self.commands.push(PhysicsCommand::disable_body(body)))
    }

    fn require_body(&self, body: PhysicsBodyHandle) -> PhysicsResult<()> {
        self.body_exists(body)
            .then_some(())
            .ok_or(PhysicsError::body_not_found("body not found"))
    }

    fn body_exists(&self, body: PhysicsBodyHandle) -> bool {
        self.body_at(body).is_some()
    }

    /// Immediately teleport a body to `transform` (no queue, no integration). The
    /// transform must be finite and the body must exist. Used to (re)position a
    /// body outside the normal integration — e.g. spawning or respawning a player
    /// at a fixed point.
    pub(crate) fn set_body_transform(
        &mut self,
        body: PhysicsBodyHandle,
        transform: Transform,
    ) -> PhysicsResult<()> {
        transform_is_finite(transform)
            .then_some(())
            .ok_or(PhysicsError::non_finite_input(
                "teleport transform must be finite",
            ))
            .and_then(|()| {
                self.body_at_mut(body)
                    .ok_or(PhysicsError::body_not_found("teleport target body not found"))
            })
            .map(|b| b.set_transform(transform))
    }

    /// Immediately set a body's linear and angular velocity (no queue). Both
    /// vectors must be finite and the body must exist. Used to bring a body to
    /// rest on respawn or to brake it. Velocity on a non-dynamic body is stored
    /// but never integrated (only enabled dynamic bodies move), so it is a
    /// harmless no-op there.
    pub(crate) fn set_body_velocity(
        &mut self,
        body: PhysicsBodyHandle,
        linear: Vec3,
        angular: Vec3,
    ) -> PhysicsResult<()> {
        (vec3_is_finite(linear) & vec3_is_finite(angular))
            .then_some(())
            .ok_or(PhysicsError::non_finite_input(
                "velocity vectors must be finite",
            ))
            .and_then(|()| {
                self.body_at_mut(body).ok_or(PhysicsError::body_not_found(
                    "velocity target body not found",
                ))
            })
            .map(|b| {
                b.set_linear_velocity(linear);
                b.set_angular_velocity(angular);
            })
    }

    /// Advance the world by one explicit fixed step, rejecting a zero step.
    pub(crate) fn step(&mut self, step: RuntimeStep) -> PhysicsResult<()> {
        let nanos = step.fixed_delta_nanos();
        (nanos != 0)
            .then_some(())
            .ok_or(PhysicsError::invalid_step(
                "fixed step must be greater than zero nanoseconds",
            ))
            .and_then(|()| self.step_inner(nanos))
    }

    /// Run one outer step: apply commands once, run the substeps, then commit
    /// atomically iff every resulting body is finite (else roll back and error).
    fn step_inner(&mut self, nanos: u64) -> PhysicsResult<()> {
        let substeps = self.config.max_substeps();
        let sub_nanos = nanos / substeps as u64;
        let remainder = nanos % substeps as u64;

        // Snapshot pre-step body state for an atomic rollback on a non-finite
        // result. Commands are applied to a clone-read so the queue is untouched
        // until commit.
        let before_bodies = self.bodies.clone();
        let pending = self.commands.clone();
        let command_count = pending.len() as u32;
        let staged_events = self.apply_commands(&pending);

        // Substeps: the first `remainder` substeps each take one extra nanosecond
        // so the substep durations sum exactly to `nanos` (deterministic).
        let (agg, last_manifolds) = (0..substeps).fold(
            (SubstepCounts::zero(), Vec::new()),
            |(counts, _prev), index| {
                let dt_nanos = sub_nanos + (((index as u64) < remainder) as u64);
                let dt = (dt_nanos as f32) / NANOS_PER_SECOND;
                let (sub, manifolds) = self.run_substep(dt);
                (counts.add(sub), manifolds)
            },
        );

        let finite = self.bodies.iter().all(PhysicsBody::is_finite_state);

        // Commit on success; otherwise restore pre-step bodies and leave the
        // command queue and event log untouched.
        let computed = core::mem::take(&mut self.bodies);
        self.bodies = before_bodies;
        finite.then(move || {
            self.commit_step(
                computed,
                command_count,
                staged_events,
                agg,
                last_manifolds,
                substeps,
            )
        });
        [
            Err(PhysicsError::non_finite_step_result(
                "step produced non-finite body state; world rolled back",
            )),
            Ok(()),
        ][finite as usize]
    }

    /// Run one substep's dynamics pipeline over `dt`, returning its work counts
    /// and its contact manifolds (the latter retained for the contact report).
    fn run_substep(&mut self, dt: f32) -> (SubstepCounts, Vec<ContactManifold>) {
        let pairs = broad_phase_pair::detect_pairs(&self.colliders, &self.bodies);
        let manifolds = contact_pair::generate_contacts(&pairs, &self.colliders, &self.bodies);
        let integration_count = integrator::integrate_velocities(
            &mut self.bodies,
            self.config.gravity(),
            dt,
            self.config.linear_damping(),
            self.config.angular_damping(),
        );
        let solved_contact_count = contact_solver::count_solved_contacts(&self.bodies, &manifolds);
        let frictioned_contact_count =
            contact_solver::count_frictioned_contacts(&self.bodies, &self.colliders, &manifolds);
        contact_solver::solve(
            &mut self.bodies,
            &self.colliders,
            &manifolds,
            self.config.solver_iterations(),
        );
        integrator::integrate_positions(&mut self.bodies, dt);
        contact_solver::correct_positions(&mut self.bodies, &manifolds);
        let counts = SubstepCounts {
            integration_count,
            broad_phase_pair_count: pairs.len() as u32,
            contact_pair_count: manifolds.len() as u32,
            solved_contact_count,
            frictioned_contact_count,
        };
        (counts, manifolds)
    }

    /// Commit a finite step: install the computed bodies and contacts, clear the
    /// command queue, emit the staged command events plus `StepCompleted`, bump
    /// the step index, and record honest diagnostics.
    fn commit_step(
        &mut self,
        computed: Vec<PhysicsBody>,
        command_count: u32,
        staged_events: Vec<PhysicsEvent>,
        agg: SubstepCounts,
        last_manifolds: Vec<ContactManifold>,
        substeps: u32,
    ) {
        self.bodies = computed;
        self.last_contacts = last_manifolds;
        self.commands.clear();
        let event_count = staged_events.len() as u32 + 1;
        staged_events.into_iter().for_each(|e| self.events.push(e));
        self.step_index += 1;
        self.events.push(PhysicsEvent::StepCompleted {
            step_index: self.step_index,
        });
        let dynamic_body_count = self
            .bodies
            .iter()
            .filter(|b| b.kind().is_dynamic())
            .count() as u32;
        let result = PhysicsStepResult::new(
            agg.integration_count,
            agg.broad_phase_pair_count,
            agg.contact_pair_count,
            agg.solved_contact_count,
            agg.frictioned_contact_count,
            self.config.solver_iterations(),
            substeps,
        );
        self.latest_record = PhysicsStepRecord::new(
            self.step_index,
            self.bodies.len() as u32,
            self.colliders.len() as u32,
            dynamic_body_count,
            command_count,
            event_count,
            &result,
        );
    }

    /// Apply a drained command list to the bodies, returning the events the
    /// commands emit (enable/disable), in order.
    fn apply_commands(&mut self, commands: &[PhysicsCommand]) -> Vec<PhysicsEvent> {
        commands
            .iter()
            .filter_map(|cmd| self.apply_command(cmd))
            .collect()
    }

    fn apply_command(&mut self, cmd: &PhysicsCommand) -> Option<PhysicsEvent> {
        let kind = cmd.kind();
        let body = cmd.body();
        let vector = cmd.vector();
        self.body_at_mut(body)
            .and_then(|target| APPLY_TABLE[kind as usize](vector, body, target))
    }

    /// A deterministic snapshot of the world, bodies and colliders in insertion
    /// order.
    pub(crate) fn snapshot(&self) -> PhysicsSnapshot {
        let bodies: Vec<BodySnapshot> = self
            .bodies
            .iter()
            .map(|b| {
                BodySnapshot::new(
                    b.handle(),
                    b.kind(),
                    b.transform(),
                    b.linear_velocity(),
                    b.angular_velocity(),
                    b.enabled(),
                )
            })
            .collect();
        let colliders: Vec<ColliderSnapshot> = self
            .colliders
            .iter()
            .map(|c| {
                ColliderSnapshot::new(
                    c.handle(),
                    c.body(),
                    c.shape(),
                    c.material(),
                    c.is_trigger(),
                    c.enabled(),
                )
            })
            .collect();
        PhysicsSnapshot::new(self.step_index, bodies, colliders)
    }

    /// The record of the most recent step (empty before the first step).
    pub(crate) fn latest_record(&self) -> PhysicsStepRecord {
        self.latest_record
    }

    /// The most recent step's contacts, as neutral read-only reports in
    /// deterministic (sorted) order. Empty before the first contact.
    pub(crate) fn latest_contacts(&self) -> Vec<ContactReport> {
        self.last_contacts
            .iter()
            .filter_map(|m| {
                Meters::new(m.depth()).ok().map(|depth| {
                    ContactReport::new(
                        m.body_a(),
                        m.body_b(),
                        m.collider_a(),
                        m.collider_b(),
                        m.normal(),
                        depth,
                        m.point(),
                    )
                })
            })
            .collect()
    }

    /// The event log, in emission order.
    pub(crate) fn events(&self) -> &[PhysicsEvent] {
        &self.events
    }

    /// Drain the event log, returning the events in emission order and clearing
    /// the queue so it cannot grow without bound.
    pub(crate) fn drain_events(&mut self) -> Vec<PhysicsEvent> {
        self.events.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_body_kind::PhysicsBodyKind;
    use axiom_math::Transform;

    fn world() -> PhysicsWorld {
        PhysicsWorld::new(PhysicsConfig::default_config())
    }

    #[test]
    fn body_index_lookup_returns_expected_body() {
        let mut w = world();
        let a = w
            .create_body(PhysicsBodyDesc::static_body(Transform::IDENTITY).unwrap())
            .unwrap();
        let b = w
            .create_body(PhysicsBodyDesc::kinematic_body(Transform::IDENTITY).unwrap())
            .unwrap();
        // Dense lookup maps each handle to the body that carries it.
        assert_eq!(w.body_at(a).unwrap().handle(), a);
        assert_eq!(w.body_at(b).unwrap().handle(), b);
        assert_eq!(w.body_at(b).unwrap().kind(), PhysicsBodyKind::Kinematic);
    }

    #[test]
    fn body_index_lookup_rejects_stale_or_missing_body() {
        let mut w = world();
        w.create_body(PhysicsBodyDesc::static_body(Transform::IDENTITY).unwrap())
            .unwrap();
        // A NULL handle (raw 0) and an out-of-range handle both resolve to None.
        assert!(w.body_at(PhysicsBodyHandle::NULL).is_none());
        assert!(w.body_at(PhysicsBodyHandle::from_raw(99)).is_none());
        assert!(w.body_index(PhysicsBodyHandle::from_raw(1)).is_some());
    }
}
