//! The deterministic linear integrator, split into a velocity pass and a
//! position pass so the contact solver can run **between** them.
//!
//! Semi-implicit (symplectic) Euler over an explicit fixed step:
//! 1. [`integrate_velocities`] applies gravity, accumulated force, and impulse to
//!    each enabled dynamic body's linear velocity, then clears its accumulators;
//! 2. the contact solver adjusts velocities to resolve contacts;
//! 3. [`integrate_positions`] advances each enabled dynamic body's translation by
//!    its (now solved) velocity.
//!
//! Static, kinematic, and disabled bodies have `active_factor == 0`, so both
//! passes collapse to "no change" for them with **no branches** — gating is
//! arithmetic, not control flow. Integration is **linear only**: orientation is
//! never integrated (a documented deferral — see `ROADMAP.md`). The integrator reads
//! no clock and no randomness; `dt` is derived solely from the explicit step.

use axiom_math::{Transform, Vec3};

use crate::physics_body::PhysicsBody;

/// `1.0` for an enabled dynamic body, `0.0` otherwise — the arithmetic gate that
/// zeroes every motion contribution for static, kinematic, and disabled bodies.
fn active_factor(body: &PhysicsBody) -> f32 {
    ((body.kind().is_dynamic() & body.enabled()) as u8) as f32
}

/// Apply gravity, accumulated force, and impulse to every body's linear velocity,
/// then clear the accumulators. Returns the number of bodies that actually
/// integrated (the enabled dynamic bodies).
pub(crate) fn integrate_velocities(bodies: &mut [PhysicsBody], gravity: Vec3, dt: f32) -> u32 {
    bodies
        .iter_mut()
        .map(|body| integrate_velocity(body, gravity, dt))
        .filter(|integrated| *integrated)
        .count() as u32
}

/// Integrate one body's velocity. Returns `true` iff it was an enabled dynamic
/// body (and therefore actually accelerated).
fn integrate_velocity(body: &mut PhysicsBody, gravity: Vec3, dt: f32) -> bool {
    let integrated = body.kind().is_dynamic() & body.enabled();
    let factor = (integrated as u8) as f32;
    let inverse_mass = body.mass_properties().inverse_mass().get();
    let force = body.forces().force();
    let impulse = body.forces().impulse();

    let acceleration = gravity
        .add(force.mul_scalar(inverse_mass))
        .mul_scalar(factor);
    let delta_velocity = impulse
        .mul_scalar(inverse_mass * factor)
        .add(acceleration.mul_scalar(dt));
    body.set_linear_velocity(body.linear_velocity().add(delta_velocity));
    body.forces_mut().clear();
    integrated
}

/// Advance every enabled dynamic body's translation by its current linear
/// velocity over `dt`. Static, kinematic, and disabled bodies do not move.
pub(crate) fn integrate_positions(bodies: &mut [PhysicsBody], dt: f32) {
    bodies.iter_mut().for_each(|body| integrate_position(body, dt));
}

/// Integrate one body's position. Orientation is never changed (linear only).
fn integrate_position(body: &mut PhysicsBody, dt: f32) {
    let factor = active_factor(body);
    let oriented = body.transform();
    let translation = oriented
        .translation
        .add(body.linear_velocity().mul_scalar(dt * factor));
    body.set_transform(Transform::new(translation, oriented.rotation, oriented.scale));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_body_desc::PhysicsBodyDesc;
    use crate::physics_body_handle::PhysicsBodyHandle;
    use axiom_kernel::Ratio;

    const GRAVITY: Vec3 = Vec3::new(0.0, -10.0, 0.0);
    const DT: f32 = 0.1;

    fn dynamic(raw: u64) -> PhysicsBody {
        let desc =
            PhysicsBodyDesc::dynamic_body(Transform::IDENTITY, Ratio::new(1.0).unwrap()).unwrap();
        PhysicsBody::from_desc(PhysicsBodyHandle::from_raw(raw), desc)
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
        let count = integrate_velocities(&mut bodies, GRAVITY, DT);
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
        integrate_velocities(&mut bodies, Vec3::ZERO, DT);
        // x: force 2 * inv_mass 1 * dt 0.1 = 0.2; z: impulse 5 * inv_mass 1 = 5.0
        assert_eq!(bodies[0].linear_velocity(), Vec3::new(0.2, 0.0, 5.0));
        assert_eq!(bodies[0].forces().force(), Vec3::ZERO);
        assert_eq!(bodies[0].forces().impulse(), Vec3::ZERO);
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
    fn position_pass_ignores_static_and_disabled_bodies() {
        // A disabled dynamic body keeps leftover velocity but must not move.
        let mut disabled = dynamic(1);
        disabled.set_linear_velocity(Vec3::new(5.0, 0.0, 0.0));
        disabled.set_enabled(false);
        let mut still = static_body(2);
        still.set_linear_velocity(Vec3::new(5.0, 0.0, 0.0));
        let mut bodies = [disabled, still];
        integrate_positions(&mut bodies, DT);
        bodies
            .iter()
            .for_each(|b| assert_eq!(b.transform().translation, Vec3::ZERO));
    }
}
