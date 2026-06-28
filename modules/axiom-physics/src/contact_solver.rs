//! The deterministic sequential-impulse contact solver.
//!
//! Given the narrow phase's [`ContactManifold`]s, the solver resolves
//! interpenetration in two deterministic stages:
//!
//! 1. **Velocity solve** ([`solve`]) — a fixed number of sequential-impulse
//!    iterations (from `PhysicsConfig::solver_iterations`). Each iteration walks
//!    the manifolds in their stable sorted order and applies a normal impulse that
//!    removes the approaching relative velocity, scaled by `1 + restitution` so a
//!    bouncy material rebounds. Impulses are distributed by inverse mass, so a
//!    static or kinematic body (zero inverse mass) is never moved.
//! 2. **Position correction** ([`correct_positions`]) — a Baumgarte-style push
//!    that removes residual penetration beyond a small slop, again split by
//!    inverse mass, so resting stacks do not sink.
//!
//! Friction is **not** resolved: material friction is validated and stored but
//! applies no tangential impulse yet (a documented deferral — see `ROADMAP.md`).
//! Every operation is branchless: contact gating is arithmetic
//! (`approaching.then(..)`, `(depth - slop).max(0.0)`), never control flow.

use axiom_math::{Transform, Vec3};

use crate::contact_manifold::ContactManifold;
use crate::physics_body::PhysicsBody;
use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider::PhysicsCollider;
use crate::physics_collider_handle::PhysicsColliderHandle;

/// Penetration below this depth (metres) is left uncorrected, preventing jitter
/// on resting contacts.
const PENETRATION_SLOP: f32 = 0.01;

/// The fraction of remaining penetration removed per step by position correction.
const CORRECTION_BETA: f32 = 0.2;

/// The dense slice index of a body handle. Handles are 1-based and allocated in
/// creation order with no removal, so handle `h` always lives at slice index
/// `h - 1`; this is an O(1) lookup, not a linear scan. A `NULL` (raw `0`) or
/// out-of-range handle yields `None`.
fn body_index(handle: PhysicsBodyHandle) -> Option<usize> {
    handle.raw().checked_sub(1).map(|i| i as usize)
}

/// The dense slice index of a collider handle (same 1-based, creation-ordered,
/// never-removed invariant as bodies).
fn collider_index(handle: PhysicsColliderHandle) -> Option<usize> {
    handle.raw().checked_sub(1).map(|i| i as usize)
}

/// A body's `(linear_velocity, inverse_mass)`, or `(ZERO, 0)` if the handle is
/// somehow absent (unreachable: manifold bodies always exist).
fn body_state(bodies: &[PhysicsBody], handle: PhysicsBodyHandle) -> (Vec3, f32) {
    body_index(handle)
        .and_then(|i| bodies.get(i))
        .map_or((Vec3::ZERO, 0.0), |b| {
            (b.linear_velocity(), b.mass_properties().inverse_mass().get())
        })
}

/// Add `delta` to a body's linear velocity (no-op for an absent handle).
fn add_velocity(bodies: &mut [PhysicsBody], handle: PhysicsBodyHandle, delta: Vec3) {
    body_index(handle)
        .and_then(|i| bodies.get_mut(i))
        .into_iter()
        .for_each(|b| b.set_linear_velocity(b.linear_velocity().add(delta)));
}

/// Translate a body by `delta` (orientation/scale preserved; no-op for an absent
/// handle).
fn translate(bodies: &mut [PhysicsBody], handle: PhysicsBodyHandle, delta: Vec3) {
    body_index(handle)
        .and_then(|i| bodies.get_mut(i))
        .into_iter()
        .for_each(|b| {
            let t = b.transform();
            b.set_transform(Transform::new(t.translation.add(delta), t.rotation, t.scale));
        });
}

/// The restitution stored on a collider (`0` if the handle is absent).
fn restitution_of(colliders: &[PhysicsCollider], handle: PhysicsColliderHandle) -> f32 {
    collider_index(handle)
        .and_then(|i| colliders.get(i))
        .map_or(0.0, |c| c.material().restitution().get())
}

/// `true` iff the two bodies of `manifold` are approaching along the contact
/// normal (the condition under which the solver applies a normal impulse).
fn is_approaching(bodies: &[PhysicsBody], manifold: &ContactManifold) -> bool {
    let (va, _) = body_state(bodies, manifold.body_a());
    let (vb, _) = body_state(bodies, manifold.body_b());
    vb.subtract(va).dot(manifold.normal()) < 0.0
}

/// The number of contacts the solver will actually resolve — the manifolds whose
/// bodies are approaching at solve entry (a separating contact receives no
/// impulse and is not counted). Measured once, before the impulse passes, so it
/// reports genuine solver work for the step record's `solved_contact_count`.
pub(crate) fn count_solved_contacts(bodies: &[PhysicsBody], manifolds: &[ContactManifold]) -> u32 {
    manifolds
        .iter()
        .filter(|manifold| is_approaching(bodies, manifold))
        .count() as u32
}

/// The combined restitution of a contact — the larger of the two colliders'
/// restitutions, so a bouncy body rebounds off any surface.
fn combined_restitution(colliders: &[PhysicsCollider], manifold: &ContactManifold) -> f32 {
    restitution_of(colliders, manifold.collider_a())
        .max(restitution_of(colliders, manifold.collider_b()))
}

/// Apply one normal-impulse pass for a single manifold.
fn solve_contact(bodies: &mut [PhysicsBody], manifold: &ContactManifold, restitution: f32) {
    let (va, inv_a) = body_state(bodies, manifold.body_a());
    let (vb, inv_b) = body_state(bodies, manifold.body_b());
    let normal = manifold.normal();
    let relative = vb.subtract(va).dot(normal);
    let inverse_sum = inv_a + inv_b;
    let approaching = relative < 0.0;
    let magnitude = approaching
        .then(|| -(1.0 + restitution) * relative / inverse_sum.max(f32::MIN_POSITIVE))
        .unwrap_or(0.0);
    add_velocity(bodies, manifold.body_a(), normal.mul_scalar(-magnitude * inv_a));
    add_velocity(bodies, manifold.body_b(), normal.mul_scalar(magnitude * inv_b));
}

/// Resolve contact velocities over `iterations` sequential-impulse passes,
/// returning the iteration count (recorded in the step record). Manifolds are
/// processed in their stable sorted order every iteration, so the result is a
/// deterministic function of the inputs.
pub(crate) fn solve(
    bodies: &mut [PhysicsBody],
    colliders: &[PhysicsCollider],
    manifolds: &[ContactManifold],
    iterations: u32,
) -> u32 {
    (0..iterations).for_each(|_| {
        manifolds.iter().for_each(|manifold| {
            let restitution = combined_restitution(colliders, manifold);
            solve_contact(bodies, manifold, restitution);
        });
    });
    iterations
}

/// Push interpenetrating bodies apart by the portion of their penetration that
/// exceeds the slop, split by inverse mass. Applied after position integration.
pub(crate) fn correct_positions(bodies: &mut [PhysicsBody], manifolds: &[ContactManifold]) {
    manifolds.iter().for_each(|manifold| {
        let inv_a = body_state(bodies, manifold.body_a()).1;
        let inv_b = body_state(bodies, manifold.body_b()).1;
        let inverse_sum = inv_a + inv_b;
        let correction =
            (manifold.depth() - PENETRATION_SLOP).max(0.0) * CORRECTION_BETA
                / inverse_sum.max(f32::MIN_POSITIVE);
        let normal = manifold.normal();
        translate(bodies, manifold.body_a(), normal.mul_scalar(-correction * inv_a));
        translate(bodies, manifold.body_b(), normal.mul_scalar(correction * inv_b));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_body_desc::PhysicsBodyDesc;
    use crate::physics_collider_shape::PhysicsColliderShape;
    use crate::physics_material::PhysicsMaterial;
    use axiom_kernel::{Meters, Ratio};

    fn dynamic(raw: u64, velocity: Vec3) -> PhysicsBody {
        dynamic_mass(raw, velocity, 1.0)
    }

    fn dynamic_mass(raw: u64, velocity: Vec3, mass: f32) -> PhysicsBody {
        let desc =
            PhysicsBodyDesc::dynamic_body(Transform::IDENTITY, Ratio::new(mass).unwrap()).unwrap();
        let mut b = PhysicsBody::from_desc(PhysicsBodyHandle::from_raw(raw), desc);
        b.set_linear_velocity(velocity);
        b
    }

    // Two dynamic bodies A (handle 1) and B (handle 2); normal A->B points down.
    fn dynamic_pair_manifold() -> ContactManifold {
        ContactManifold::new(
            PhysicsColliderHandle::from_raw(1),
            PhysicsColliderHandle::from_raw(2),
            PhysicsBodyHandle::from_raw(1),
            PhysicsBodyHandle::from_raw(2),
            Vec3::new(0.0, -1.0, 0.0),
            0.1,
            Vec3::ZERO,
        )
    }

    fn static_body(raw: u64) -> PhysicsBody {
        let desc = PhysicsBodyDesc::static_body(Transform::IDENTITY).unwrap();
        PhysicsBody::from_desc(PhysicsBodyHandle::from_raw(raw), desc)
    }

    fn collider(collider_raw: u64, body_raw: u64, restitution: f32) -> PhysicsCollider {
        let material = PhysicsMaterial::new(
            Ratio::new(0.0).unwrap(),
            Ratio::new(restitution).unwrap(),
            Ratio::new(1.0).unwrap(),
        )
        .unwrap();
        PhysicsCollider::new(
            PhysicsColliderHandle::from_raw(collider_raw),
            PhysicsBodyHandle::from_raw(body_raw),
            PhysicsColliderShape::sphere(Meters::new(1.0).unwrap()).unwrap(),
            material,
            false,
        )
    }

    // Dynamic body A (handle 1) above static body B (handle 2); normal A->B points
    // down, so a downward-moving A is approaching. Handles are dense (collider/body
    // `h` lives at slice index `h - 1`), matching the world's real invariant.
    fn manifold(depth: f32) -> ContactManifold {
        ContactManifold::new(
            PhysicsColliderHandle::from_raw(1),
            PhysicsColliderHandle::from_raw(2),
            PhysicsBodyHandle::from_raw(1),
            PhysicsBodyHandle::from_raw(2),
            Vec3::new(0.0, -1.0, 0.0),
            depth,
            Vec3::ZERO,
        )
    }

    #[test]
    fn zero_restitution_removes_approaching_velocity() {
        let mut bodies = [dynamic(1, Vec3::new(0.0, -2.0, 0.0)), static_body(2)];
        let colliders = [collider(1, 1, 0.0), collider(2, 2, 0.0)];
        let iters = solve(&mut bodies, &colliders, &[manifold(0.1)], 4);
        assert_eq!(iters, 4);
        // The downward velocity is cancelled; the body neither sinks nor bounces.
        assert!(bodies[0].linear_velocity().y.abs() < 1.0e-5);
        // The static body is never moved.
        assert_eq!(bodies[1].linear_velocity(), Vec3::ZERO);
    }

    #[test]
    fn full_restitution_reverses_velocity() {
        let mut bodies = [dynamic(1, Vec3::new(0.0, -2.0, 0.0)), static_body(2)];
        let colliders = [collider(1, 1, 1.0), collider(2, 2, 1.0)];
        solve(&mut bodies, &colliders, &[manifold(0.1)], 1);
        // e == 1 rebounds the approach velocity upward.
        assert!((bodies[0].linear_velocity().y - 2.0).abs() < 1.0e-5);
    }

    #[test]
    fn separating_contact_applies_no_impulse() {
        // Body already moving away from the contact (upward) is left untouched.
        let mut bodies = [dynamic(1, Vec3::new(0.0, 3.0, 0.0)), static_body(2)];
        let colliders = [collider(1, 1, 0.5), collider(2, 2, 0.5)];
        solve(&mut bodies, &colliders, &[manifold(0.1)], 1);
        assert_eq!(bodies[0].linear_velocity(), Vec3::new(0.0, 3.0, 0.0));
    }

    #[test]
    fn zero_iterations_does_nothing() {
        let mut bodies = [dynamic(1, Vec3::new(0.0, -2.0, 0.0)), static_body(2)];
        let colliders = [collider(1, 1, 0.0), collider(2, 2, 0.0)];
        assert_eq!(solve(&mut bodies, &colliders, &[manifold(0.1)], 0), 0);
        assert_eq!(bodies[0].linear_velocity(), Vec3::new(0.0, -2.0, 0.0));
    }

    #[test]
    fn position_correction_pushes_a_penetrating_dynamic_body_out() {
        let mut bodies = [dynamic(1, Vec3::ZERO), static_body(2)];
        // depth 0.5, slop 0.01 -> correction (0.49 * 0.2) along -normal for A.
        correct_positions(&mut bodies, &[manifold(0.5)]);
        // normal A->B is (0,-1,0); A moves -normal = +Y (out of the surface).
        assert!(bodies[0].transform().translation.y > 0.0);
        // The static body stays put.
        assert_eq!(bodies[1].transform().translation, Vec3::ZERO);
    }

    #[test]
    fn position_correction_ignores_penetration_within_slop() {
        let mut bodies = [dynamic(1, Vec3::ZERO), static_body(2)];
        correct_positions(&mut bodies, &[manifold(0.005)]);
        assert_eq!(bodies[0].transform().translation, Vec3::ZERO);
    }

    #[test]
    fn count_solved_contacts_counts_only_approaching() {
        // A approaching the static surface -> one contact to solve.
        let approaching = [dynamic(1, Vec3::new(0.0, -2.0, 0.0)), static_body(2)];
        assert_eq!(count_solved_contacts(&approaching, &[manifold(0.1)]), 1);
        // A separating (moving away) -> nothing to solve, even with a manifold.
        let separating = [dynamic(1, Vec3::new(0.0, 3.0, 0.0)), static_body(2)];
        assert_eq!(count_solved_contacts(&separating, &[manifold(0.1)]), 0);
    }

    #[test]
    fn solve_splits_impulse_between_two_dynamic_bodies_by_inverse_mass() {
        // Equal masses approaching head-on; both approach velocities are removed
        // and total linear momentum (zero here) is conserved.
        let mut bodies = [
            dynamic_mass(1, Vec3::new(0.0, -2.0, 0.0), 1.0),
            dynamic_mass(2, Vec3::new(0.0, 2.0, 0.0), 1.0),
        ];
        let colliders = [collider(1, 1, 0.0), collider(2, 2, 0.0)];
        assert_eq!(count_solved_contacts(&bodies, &[dynamic_pair_manifold()]), 1);
        solve(&mut bodies, &colliders, &[dynamic_pair_manifold()], 4);
        assert!(bodies[0].linear_velocity().y.abs() < 1.0e-5);
        assert!(bodies[1].linear_velocity().y.abs() < 1.0e-5);
    }

    #[test]
    fn unequal_mass_pair_splits_impulse_by_inverse_mass() {
        // A (mass 1) drives into B (mass 3) at rest; the lighter body's velocity
        // changes more, and momentum is conserved.
        let mut bodies = [
            dynamic_mass(1, Vec3::new(0.0, -4.0, 0.0), 1.0),
            dynamic_mass(2, Vec3::ZERO, 3.0),
        ];
        let colliders = [collider(1, 1, 0.0), collider(2, 2, 0.0)];
        solve(&mut bodies, &colliders, &[dynamic_pair_manifold()], 1);
        let delta_a = (bodies[0].linear_velocity().y - (-4.0)).abs();
        let delta_b = (bodies[1].linear_velocity().y - 0.0).abs();
        assert!(delta_a > delta_b, "lighter body changes velocity more");
        // Inelastic (e=0): both reach a common velocity of -1.
        assert!((bodies[0].linear_velocity().y - (-1.0)).abs() < 1.0e-5);
        assert!((bodies[1].linear_velocity().y - (-1.0)).abs() < 1.0e-5);
        // Momentum conserved: 1*(-4) == 1*(-1) + 3*(-1).
        let p = bodies[0].linear_velocity().y + 3.0 * bodies[1].linear_velocity().y;
        assert!((p - (-4.0)).abs() < 1.0e-5);
    }
}
