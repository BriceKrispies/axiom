//! The deterministic sequential-impulse contact solver.
//!
//! Given the narrow phase's [`ContactManifold`]s, the solver resolves
//! interpenetration in two deterministic stages:
//!
//! 1. **Velocity solve** ([`solve`]) — a fixed number of sequential-impulse
//!    iterations (from `PhysicsConfig::solver_iterations`). Each iteration walks
//!    the manifolds in their stable sorted order and applies a normal impulse that
//!    removes the approaching relative velocity, scaled by `1 + restitution` so a
//!    bouncy material rebounds, then a **tangential friction impulse** along a
//!    deterministic tangent basis, bounded by the Coulomb cone `|j_t| <= μ·j_n`.
//!    Impulses are distributed by inverse mass, so a static or kinematic body
//!    (zero inverse mass) is never moved.
//! 2. **Position correction** ([`correct_positions`]) — a Baumgarte-style push
//!    that removes residual penetration beyond a small slop, again split by
//!    inverse mass, so resting stacks do not sink.
//!
//! ## Friction (deterministic, branchless)
//! The friction impulse needs a tangent direction. It is derived **only from the
//! contact normal** — never from discovery order or an iterative search that could
//! pick a different basis per run — by crossing the normal with the world axis it
//! is *least* aligned with (the smallest absolute component), then completing an
//! orthonormal pair. The combined coefficient is the geometric mean
//! `sqrt(μ_a·μ_b)` of the two colliders' frictions, and the Coulomb clamp
//! `|j_t| <= μ·j_n` is applied with `clamp` (min/max), never a branch. The
//! friction pass walks the same stable handle-sorted manifold order as the normal
//! pass, so the result stays a pure function of world state.
//!
//! Every operation is branchless: contact gating is arithmetic
//! (`approaching.then(..)`, `(depth - slop).max(0.0)`, the Coulomb `clamp`),
//! never control flow.

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

/// The world axes, indexed `x = 0, y = 1, z = 2`. The tangent-basis construction
/// crosses the normal with the axis it is least aligned with.
const AXES: [Vec3; 3] = [Vec3::UNIT_X, Vec3::UNIT_Y, Vec3::UNIT_Z];

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

/// The friction stored on a collider (`0` if the handle is absent).
fn friction_of(colliders: &[PhysicsCollider], handle: PhysicsColliderHandle) -> f32 {
    collider_index(handle)
        .and_then(|i| colliders.get(i))
        .map_or(0.0, |c| c.material().friction().get())
}

/// `true` iff the two bodies of `manifold` are approaching along the contact
/// normal (the condition under which the solver applies a normal impulse).
fn is_approaching(bodies: &[PhysicsBody], manifold: &ContactManifold) -> bool {
    let (va, _) = body_state(bodies, manifold.body_a());
    let (vb, _) = body_state(bodies, manifold.body_b());
    vb.subtract(va).dot(manifold.normal()) < 0.0
}

/// The squared tangential relative speed at a contact (the part of `vb - va`
/// perpendicular to the normal). Friction only does work when this is nonzero.
fn tangential_speed_squared(bodies: &[PhysicsBody], manifold: &ContactManifold) -> f32 {
    let (va, _) = body_state(bodies, manifold.body_a());
    let (vb, _) = body_state(bodies, manifold.body_b());
    let n = manifold.normal();
    let relative = vb.subtract(va);
    let normal_part = n.mul_scalar(relative.dot(n));
    relative.subtract(normal_part).length_squared()
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

/// The number of contacts that will receive a genuine tangential friction impulse
/// — those that are approaching (so a normal impulse, and thus a Coulomb cone,
/// exists), have a positive combined friction, and have nonzero tangential
/// relative speed. Measured once before the passes, mirroring
/// [`count_solved_contacts`], so it is an honest report for the step record's
/// `frictioned_contact_count`.
pub(crate) fn count_frictioned_contacts(
    bodies: &[PhysicsBody],
    colliders: &[PhysicsCollider],
    manifolds: &[ContactManifold],
) -> u32 {
    manifolds
        .iter()
        .filter(|manifold| {
            is_approaching(bodies, manifold)
                & (combined_friction(colliders, manifold) > 0.0)
                & (tangential_speed_squared(bodies, manifold) > 0.0)
        })
        .count() as u32
}

/// The combined restitution of a contact — the larger of the two colliders'
/// restitutions, so a bouncy body rebounds off any surface.
fn combined_restitution(colliders: &[PhysicsCollider], manifold: &ContactManifold) -> f32 {
    restitution_of(colliders, manifold.collider_a())
        .max(restitution_of(colliders, manifold.collider_b()))
}

/// The combined friction of a contact — the geometric mean `sqrt(μ_a·μ_b)` of the
/// two colliders' friction coefficients (the standard rule: a zero on either side
/// gives a frictionless contact).
fn combined_friction(colliders: &[PhysicsCollider], manifold: &ContactManifold) -> f32 {
    (friction_of(colliders, manifold.collider_a()) * friction_of(colliders, manifold.collider_b()))
        .sqrt()
}

/// A deterministic orthonormal tangent basis `(t1, t2)` for a unit contact
/// `normal`. `t1 = normalize(reference × normal)` where `reference` is the world
/// axis the normal is *least* aligned with (smallest absolute component) — so the
/// cross is well-conditioned and never near-degenerate — and `t2 = normal × t1`.
fn tangent_basis(normal: Vec3) -> (Vec3, Vec3) {
    let ax = normal.x.abs();
    let ay = normal.y.abs();
    let az = normal.z.abs();
    let pick_x = (ax <= ay) & (ax <= az);
    let pick_y = (!pick_x) & (ay <= az);
    // Arithmetic index select (x -> 0, y -> 1, z -> 2): when `pick_x`, the leading
    // factor is 0; otherwise it is 1 scaled by `1 + (not pick_y)` to land on 1 or 2.
    let index = (!pick_x as usize) * (1 + (!pick_y as usize));
    let reference = AXES[index];
    let t1 = normalize_or_zero(reference.cross(normal));
    let t2 = normal.cross(t1);
    (t1, t2)
}

/// Unit-normalize `v`, dividing by a length clamped to [`f32::MIN_POSITIVE`] so a
/// near-degenerate vector yields a finite (effectively zero) tangent rather than a
/// `NaN`.
fn normalize_or_zero(v: Vec3) -> Vec3 {
    v.mul_scalar(1.0 / v.length().max(f32::MIN_POSITIVE))
}

/// Apply one friction impulse along tangent `axis`, clamped to the Coulomb cone
/// `[-bound, bound]`. Reads the (post-normal-impulse) velocities so it resists the
/// residual tangential motion.
fn apply_friction_axis(
    bodies: &mut [PhysicsBody],
    manifold: &ContactManifold,
    axis: Vec3,
    inverse_sum: f32,
    inv_a: f32,
    inv_b: f32,
    bound: f32,
) {
    let (va, _) = body_state(bodies, manifold.body_a());
    let (vb, _) = body_state(bodies, manifold.body_b());
    let relative = vb.subtract(va).dot(axis);
    // Coulomb clamp to `[-bound, bound]` via `max`/`min` (never `f32::clamp`,
    // which panics on a NaN bound that a finite-but-extreme input can produce).
    // A non-finite result here is caught and rolled back by the world's atomic
    // finiteness check, so it can never poison committed state.
    let magnitude = (-relative / inverse_sum).max(-bound).min(bound);
    add_velocity(bodies, manifold.body_a(), axis.mul_scalar(-magnitude * inv_a));
    add_velocity(bodies, manifold.body_b(), axis.mul_scalar(magnitude * inv_b));
}

/// Apply one normal-plus-friction impulse pass for a single manifold.
fn solve_contact(
    bodies: &mut [PhysicsBody],
    manifold: &ContactManifold,
    restitution: f32,
    friction: f32,
) {
    let (va, inv_a) = body_state(bodies, manifold.body_a());
    let (vb, inv_b) = body_state(bodies, manifold.body_b());
    let normal = manifold.normal();
    let relative = vb.subtract(va).dot(normal);
    let inverse_sum = (inv_a + inv_b).max(f32::MIN_POSITIVE);
    // Arithmetic gate: a separating contact (`relative >= 0`) zeroes the impulse,
    // so only an approaching contact pushes apart. The flag multiply is exact
    // (`* 1.0` / `* 0.0`), matching the prior `then(..).unwrap_or(0.0)` form.
    let approaching = (relative < 0.0) as u8 as f32;
    let magnitude = -(1.0 + restitution) * relative / inverse_sum * approaching;
    add_velocity(bodies, manifold.body_a(), normal.mul_scalar(-magnitude * inv_a));
    add_velocity(bodies, manifold.body_b(), normal.mul_scalar(magnitude * inv_b));

    // Friction: tangential impulses bounded by the Coulomb cone μ·j_n. The bound
    // is zero when friction is zero or the contact is separating (magnitude 0),
    // so the friction pass is then an exact no-op.
    let (t1, t2) = tangent_basis(normal);
    let bound = friction * magnitude;
    apply_friction_axis(bodies, manifold, t1, inverse_sum, inv_a, inv_b, bound);
    apply_friction_axis(bodies, manifold, t2, inverse_sum, inv_a, inv_b, bound);
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
            let friction = combined_friction(colliders, manifold);
            solve_contact(bodies, manifold, restitution, friction);
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
        collider_with(collider_raw, body_raw, restitution, 0.0)
    }

    fn collider_with(
        collider_raw: u64,
        body_raw: u64,
        restitution: f32,
        friction: f32,
    ) -> PhysicsCollider {
        let material = PhysicsMaterial::new(
            Ratio::new(friction).unwrap(),
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
    fn friction_removes_tangential_velocity_within_the_cone() {
        // A approaches the static floor (-Y, normal impulse magnitude 2) while
        // sliding sideways (+X) at 1.5 < cone bound μ·j_n = 2. The slide is fully
        // absorbed; the normal velocity is removed as before.
        let mut bodies = [dynamic(1, Vec3::new(1.5, -2.0, 0.0)), static_body(2)];
        let colliders = [collider_with(1, 1, 0.0, 1.0), collider_with(2, 2, 0.0, 1.0)];
        solve(&mut bodies, &colliders, &[manifold(0.1)], 8);
        assert!(bodies[0].linear_velocity().y.abs() < 1.0e-5, "normal cancelled");
        assert!(
            bodies[0].linear_velocity().x.abs() < 1.0e-4,
            "within-cone friction kills the slide"
        );
        assert_eq!(bodies[1].linear_velocity(), Vec3::ZERO, "static body fixed");
    }

    #[test]
    fn zero_friction_leaves_tangential_velocity_untouched() {
        let mut bodies = [dynamic(1, Vec3::new(3.0, -2.0, 0.0)), static_body(2)];
        let colliders = [collider_with(1, 1, 0.0, 0.0), collider_with(2, 2, 0.0, 0.0)];
        solve(&mut bodies, &colliders, &[manifold(0.1)], 8);
        assert!(bodies[0].linear_velocity().y.abs() < 1.0e-5, "normal cancelled");
        // The frictionless slide is exactly preserved.
        assert_eq!(bodies[0].linear_velocity().x, 3.0);
    }

    #[test]
    fn friction_is_capped_by_the_coulomb_cone() {
        // A fast slide with low friction: the tangential velocity is reduced but
        // not eliminated (the cone μ·j_n cannot remove the whole slide in one step).
        let mut bodies = [dynamic(1, Vec3::new(10.0, -1.0, 0.0)), static_body(2)];
        let colliders = [collider_with(1, 1, 0.0, 0.2), collider_with(2, 2, 0.0, 0.2)];
        solve(&mut bodies, &colliders, &[manifold(0.1)], 1);
        let vx = bodies[0].linear_velocity().x;
        assert!(vx < 10.0 && vx > 0.0, "slide reduced but not removed, got {vx}");
    }

    #[test]
    fn tangent_basis_is_orthonormal_for_each_dominant_normal_axis() {
        for n in [Vec3::UNIT_X, Vec3::UNIT_Y, Vec3::UNIT_Z, Vec3::new(0.0, -1.0, 0.0)] {
            let (t1, t2) = tangent_basis(n);
            assert!((t1.length() - 1.0).abs() < 1.0e-6, "t1 unit for {n:?}");
            assert!((t2.length() - 1.0).abs() < 1.0e-6, "t2 unit for {n:?}");
            assert!(t1.dot(n).abs() < 1.0e-6, "t1 ⟂ n for {n:?}");
            assert!(t2.dot(n).abs() < 1.0e-6, "t2 ⟂ n for {n:?}");
            assert!(t1.dot(t2).abs() < 1.0e-6, "t1 ⟂ t2 for {n:?}");
        }
    }

    #[test]
    fn count_frictioned_contacts_counts_only_pressed_sliding_frictional_contacts() {
        let colliders = [collider_with(1, 1, 0.0, 0.5), collider_with(2, 2, 0.0, 0.5)];
        // Approaching + sliding + friction -> counted.
        let sliding = [dynamic(1, Vec3::new(2.0, -2.0, 0.0)), static_body(2)];
        assert_eq!(count_frictioned_contacts(&sliding, &colliders, &[manifold(0.1)]), 1);
        // Approaching but no tangential motion -> not counted.
        let straight = [dynamic(1, Vec3::new(0.0, -2.0, 0.0)), static_body(2)];
        assert_eq!(count_frictioned_contacts(&straight, &colliders, &[manifold(0.1)]), 0);
        // Separating -> not counted even while sliding.
        let separating = [dynamic(1, Vec3::new(2.0, 3.0, 0.0)), static_body(2)];
        assert_eq!(count_frictioned_contacts(&separating, &colliders, &[manifold(0.1)]), 0);
        // Frictionless material -> not counted even while pressed and sliding.
        let frictionless = [collider_with(1, 1, 0.0, 0.0), collider_with(2, 2, 0.0, 0.0)];
        assert_eq!(count_frictioned_contacts(&sliding, &frictionless, &[manifold(0.1)]), 0);
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
