//! The deterministic rigid-body integrator, split into a velocity pass and a
//! position pass so the contact solver can run **between** them.
//!
//! Semi-implicit (symplectic) Euler over an explicit fixed step:
//! 1. [`integrate_velocities`] applies gravity, accumulated force, and impulse to
//!    each enabled dynamic body's linear velocity, applies accumulated torque to
//!    its angular velocity, decays both by the configured damping, then clears its
//!    accumulators;
//! 2. the contact solver adjusts velocities to resolve contacts;
//! 3. [`integrate_positions`] advances each enabled dynamic body's translation by
//!    its (now solved) linear velocity and its orientation by its angular velocity.
//!
//! Static, kinematic, and disabled bodies have `active == 0`, so both passes
//! collapse to "no change" for them with **no branches** — gating is arithmetic
//! and table-index selection, not control flow. The integrator reads no clock and
//! no randomness; `dt` and the damping fractions are derived solely from the
//! explicit step and the validated world config.
//!
//! ## Orientation integration (deterministic, NaN-safe)
//! Orientation advances by `q' = normalize(q + 0.5·dt·(ω_quat ⊗ q))` using
//! [`axiom_math::Quat::multiply`] in a fixed factor order, where `ω_quat` is the
//! pure quaternion `(ω, 0)`. The normalize divides by a length clamped to
//! [`f32::MIN_POSITIVE`], so a degenerate quaternion can never yield a `NaN`. An
//! inactive body keeps its exact stored orientation (the candidate is selected
//! away by table index), so a static body's authored rotation is never perturbed.

use axiom_math::{Quat, Transform, Vec3};

use crate::physics_body::PhysicsBody;

/// `1` for an enabled dynamic body, `0` otherwise — the table index that selects
/// "integrated" vs "unchanged" for static, kinematic, and disabled bodies.
fn active_index(body: &PhysicsBody) -> usize {
    (body.kind().is_dynamic() & body.enabled()) as usize
}

/// Apply gravity, force, impulse, and torque to every body's velocities, decay
/// them by the configured per-step damping, then clear the accumulators. Returns
/// the number of bodies that actually integrated (the enabled dynamic bodies).
pub(crate) fn integrate_velocities(
    bodies: &mut [PhysicsBody],
    gravity: Vec3,
    dt: f32,
    linear_damping: f32,
    angular_damping: f32,
) -> u32 {
    bodies
        .iter_mut()
        .map(|body| integrate_velocity(body, gravity, dt, linear_damping, angular_damping))
        .filter(|integrated| *integrated)
        .count() as u32
}

/// Integrate one body's linear and angular velocity. Returns `true` iff it was an
/// enabled dynamic body (and therefore actually accelerated).
fn integrate_velocity(
    body: &mut PhysicsBody,
    gravity: Vec3,
    dt: f32,
    linear_damping: f32,
    angular_damping: f32,
) -> bool {
    let integrated = body.kind().is_dynamic() & body.enabled();
    let factor = (integrated as u8) as f32;
    let inverse_mass = body.mass_properties().inverse_mass().get();
    let inverse_inertia = body.mass_properties().inverse_inertia();
    let force = body.forces().force();
    let impulse = body.forces().impulse();
    let torque = body.forces().torque();

    // Linear: gravity + force/mass over dt, plus the instantaneous impulse.
    let acceleration = gravity
        .add(force.mul_scalar(inverse_mass))
        .mul_scalar(factor);
    let delta_velocity = impulse
        .mul_scalar(inverse_mass * factor)
        .add(acceleration.mul_scalar(dt));
    let new_linear = body.linear_velocity().add(delta_velocity);
    body.set_linear_velocity(new_linear.mul_scalar(1.0 - linear_damping * factor));

    // Angular: torque ⊙ inverse_inertia over dt (diagonal inertia -> per-axis).
    let angular_acceleration = Vec3::new(
        inverse_inertia.x * torque.x,
        inverse_inertia.y * torque.y,
        inverse_inertia.z * torque.z,
    )
    .mul_scalar(factor);
    let new_angular = body
        .angular_velocity()
        .add(angular_acceleration.mul_scalar(dt));
    body.set_angular_velocity(new_angular.mul_scalar(1.0 - angular_damping * factor));

    body.forces_mut().clear();
    integrated
}

/// Advance every enabled dynamic body's translation by its current linear
/// velocity and its orientation by its angular velocity, over `dt`. Static,
/// kinematic, and disabled bodies do not move or rotate.
pub(crate) fn integrate_positions(bodies: &mut [PhysicsBody], dt: f32) {
    bodies.iter_mut().for_each(|body| integrate_position(body, dt));
}

/// Integrate one body's position and orientation.
fn integrate_position(body: &mut PhysicsBody, dt: f32) {
    let active = active_index(body);
    let factor = active as f32;
    let oriented = body.transform();
    let translation = oriented
        .translation
        .add(body.linear_velocity().mul_scalar(dt * factor));
    let rotation = integrate_orientation(oriented.rotation, body.angular_velocity(), dt, active);
    body.set_transform(Transform::new(translation, rotation, oriented.scale));
}

/// Integrate orientation by `q' = normalize(q + 0.5·dt·(ω_quat ⊗ q))`. The active
/// flag selects the integrated-and-normalized candidate (index `1`) or the
/// untouched original orientation (index `0`), so an inactive body's orientation
/// is byte-preserved.
fn integrate_orientation(q: Quat, angular_velocity: Vec3, dt: f32, active: usize) -> Quat {
    let omega = Quat::new(angular_velocity.x, angular_velocity.y, angular_velocity.z, 0.0);
    let spin = omega.multiply(q);
    let half = 0.5 * dt;
    let candidate = Quat::new(
        q.x + spin.x * half,
        q.y + spin.y * half,
        q.z + spin.z * half,
        q.w + spin.w * half,
    );
    [q, normalize_clamped(candidate)][active]
}

/// Unit-normalize `q`, dividing by a length clamped to [`f32::MIN_POSITIVE`] so a
/// degenerate (near-zero) quaternion yields a finite result rather than a `NaN`.
fn normalize_clamped(q: Quat) -> Quat {
    let inverse_length = 1.0 / q.length().max(f32::MIN_POSITIVE);
    Quat::new(
        q.x * inverse_length,
        q.y * inverse_length,
        q.z * inverse_length,
        q.w * inverse_length,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_body_desc::PhysicsBodyDesc;
    use crate::physics_body_handle::PhysicsBodyHandle;
    use crate::physics_collider_shape::PhysicsColliderShape;
    use axiom_kernel::{Meters, Ratio};

    const GRAVITY: Vec3 = Vec3::new(0.0, -10.0, 0.0);
    const DT: f32 = 0.1;

    fn dynamic(raw: u64) -> PhysicsBody {
        let desc =
            PhysicsBodyDesc::dynamic_body(Transform::IDENTITY, Ratio::new(1.0).unwrap()).unwrap();
        PhysicsBody::from_desc(PhysicsBodyHandle::from_raw(raw), desc)
    }

    /// A dynamic body whose inverse inertia is derived from a unit sphere, so it
    /// can actually acquire angular velocity from a torque.
    fn spinning(raw: u64) -> PhysicsBody {
        let mut b = dynamic(raw);
        let sphere = PhysicsColliderShape::sphere(Meters::new(1.0).unwrap()).unwrap();
        let mp = b.mass_properties().with_inertia_for(sphere);
        b.set_mass_properties(mp);
        b
    }

    fn static_body(raw: u64) -> PhysicsBody {
        let desc = PhysicsBodyDesc::static_body(Transform::IDENTITY).unwrap();
        PhysicsBody::from_desc(PhysicsBodyHandle::from_raw(raw), desc)
    }

    #[test]
    fn velocity_pass_accelerates_only_active_dynamic_bodies() {
        let mut disabled = dynamic(2);
        disabled.set_enabled(false);
        let mut bodies = [dynamic(1), static_body(3), disabled];
        let count = integrate_velocities(&mut bodies, GRAVITY, DT, 0.0, 0.0);
        assert_eq!(count, 1);
        assert_eq!(bodies[0].linear_velocity(), Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(bodies[1].linear_velocity(), Vec3::ZERO);
        assert_eq!(bodies[2].linear_velocity(), Vec3::ZERO);
    }

    #[test]
    fn velocity_pass_consumes_force_and_impulse_then_clears() {
        let mut body = dynamic(1);
        body.forces_mut().apply_force(Vec3::new(2.0, 0.0, 0.0));
        body.forces_mut().apply_impulse(Vec3::new(0.0, 0.0, 5.0));
        let mut bodies = [body];
        integrate_velocities(&mut bodies, Vec3::ZERO, DT, 0.0, 0.0);
        // x: force 2 * inv_mass 1 * dt 0.1 = 0.2; z: impulse 5 * inv_mass 1 = 5.0
        assert_eq!(bodies[0].linear_velocity(), Vec3::new(0.2, 0.0, 5.0));
        assert_eq!(bodies[0].forces().force(), Vec3::ZERO);
        assert_eq!(bodies[0].forces().impulse(), Vec3::ZERO);
        assert_eq!(bodies[0].forces().torque(), Vec3::ZERO);
    }

    #[test]
    fn torque_spins_only_active_dynamic_bodies_scaled_by_inverse_inertia() {
        let mut disabled = spinning(2);
        disabled.set_enabled(false);
        disabled.forces_mut().apply_torque(Vec3::new(0.0, 1.0, 0.0));
        let mut active = spinning(1);
        active.forces_mut().apply_torque(Vec3::new(0.0, 1.0, 0.0));
        let mut bodies = [active, static_body(3), disabled];
        integrate_velocities(&mut bodies, Vec3::ZERO, DT, 0.0, 0.0);
        // inverse inertia of a unit sphere mass 1: 1/(0.4) = 2.5; w = 2.5*1*0.1.
        assert!((bodies[0].angular_velocity().y - 0.25).abs() < 1.0e-6);
        // Static and disabled bodies never spin.
        assert_eq!(bodies[1].angular_velocity(), Vec3::ZERO);
        assert_eq!(bodies[2].angular_velocity(), Vec3::ZERO);
    }

    #[test]
    fn linear_and_angular_damping_decay_velocity_each_step() {
        let mut body = spinning(1);
        body.set_linear_velocity(Vec3::new(10.0, 0.0, 0.0));
        body.set_angular_velocity(Vec3::new(0.0, 8.0, 0.0));
        let mut bodies = [body];
        // 25% linear, 50% angular decay; no gravity/forces.
        integrate_velocities(&mut bodies, Vec3::ZERO, DT, 0.25, 0.5);
        assert_eq!(bodies[0].linear_velocity(), Vec3::new(7.5, 0.0, 0.0));
        assert_eq!(bodies[0].angular_velocity(), Vec3::new(0.0, 4.0, 0.0));
    }

    #[test]
    fn zero_damping_reproduces_undamped_velocity_exactly() {
        let mut damped = spinning(1);
        damped.set_linear_velocity(Vec3::new(3.0, 0.0, 0.0));
        damped.set_angular_velocity(Vec3::new(0.0, 0.0, 7.0));
        let mut a = [damped.clone()];
        let mut b = [damped];
        integrate_velocities(&mut a, GRAVITY, DT, 0.0, 0.0);
        // A disabled body is left frozen even with damping configured.
        b[0].set_enabled(false);
        integrate_velocities(&mut b, GRAVITY, DT, 0.9, 0.9);
        assert_eq!(b[0].linear_velocity(), Vec3::new(3.0, 0.0, 0.0));
        assert_eq!(b[0].angular_velocity(), Vec3::new(0.0, 0.0, 7.0));
        // And the active, zero-damped body keeps its full velocity + gravity.
        assert_eq!(a[0].linear_velocity(), Vec3::new(3.0, -1.0, 0.0));
        assert_eq!(a[0].angular_velocity(), Vec3::new(0.0, 0.0, 7.0));
    }

    #[test]
    fn position_pass_moves_active_bodies_by_their_velocity() {
        let mut body = dynamic(1);
        body.set_linear_velocity(Vec3::new(0.0, -1.0, 0.0));
        let mut bodies = [body];
        integrate_positions(&mut bodies, DT);
        assert_eq!(bodies[0].transform().translation, Vec3::new(0.0, -0.1, 0.0));
    }

    #[test]
    fn position_pass_advances_orientation_of_a_spinning_body() {
        let mut body = spinning(1);
        body.set_angular_velocity(Vec3::new(0.0, 2.0, 0.0));
        let mut bodies = [body];
        integrate_positions(&mut bodies, DT);
        let r = bodies[0].transform().rotation;
        // A +Y spin tilts the identity quaternion toward +Y; it stays unit.
        assert!(r.y > 0.0, "orientation should advance about +Y, got {r:?}");
        assert!((r.length() - 1.0).abs() < 1.0e-6, "stays unit");
    }

    #[test]
    fn position_pass_ignores_static_and_disabled_bodies() {
        // A disabled dynamic body keeps leftover velocity but must not move or spin.
        let mut disabled = spinning(1);
        disabled.set_linear_velocity(Vec3::new(5.0, 0.0, 0.0));
        disabled.set_angular_velocity(Vec3::new(0.0, 5.0, 0.0));
        disabled.set_enabled(false);
        let mut still = static_body(2);
        still.set_linear_velocity(Vec3::new(5.0, 0.0, 0.0));
        still.set_angular_velocity(Vec3::new(0.0, 5.0, 0.0));
        let mut bodies = [disabled, still];
        integrate_positions(&mut bodies, DT);
        bodies.iter().for_each(|b| {
            assert_eq!(b.transform().translation, Vec3::ZERO);
            // Inactive bodies keep their exact identity orientation.
            assert_eq!(b.transform().rotation, Quat::IDENTITY);
        });
    }

    #[test]
    fn orientation_integrate_is_nan_safe_for_a_degenerate_quaternion() {
        // A zero quaternion has zero length; the clamped normalize must keep it
        // finite rather than producing a NaN.
        let n = normalize_clamped(Quat::new(0.0, 0.0, 0.0, 0.0));
        assert!(n.x.is_finite() & n.y.is_finite() & n.z.is_finite() & n.w.is_finite());
    }
}
