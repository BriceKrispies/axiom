//! Deterministic, bounded world configuration.

use axiom_math::Vec3;

use crate::physics_error::PhysicsError;
use crate::physics_result::PhysicsResult;

/// The fixed, deterministic configuration of a [`crate::PhysicsApi`] world.
///
/// Every value is explicit and bounded — there is no hidden global state and no
/// wall-clock or random input. The defaults ([`PhysicsConfig::default_config`])
/// are deterministic constants; [`PhysicsConfig::new`] rejects any invalid value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PhysicsConfig {
    gravity: Vec3,
    solver_iterations: u32,
    max_bodies: u32,
    max_colliders: u32,
    max_substeps: u32,
    sleeping_disabled: bool,
}

impl PhysicsConfig {
    /// The deterministic default configuration:
    /// gravity `(0, -9.8, 0)`, 8 solver iterations, capacity 4096 bodies and
    /// 4096 colliders, a single substep, and sleeping disabled.
    pub(crate) fn default_config() -> Self {
        PhysicsConfig {
            gravity: Vec3::new(0.0, -9.8, 0.0),
            solver_iterations: 8,
            max_bodies: 4096,
            max_colliders: 4096,
            max_substeps: 1,
            sleeping_disabled: true,
        }
    }

    /// Construct a configuration, rejecting non-finite gravity or any zero
    /// capacity / iteration / substep count.
    pub(crate) fn new(
        gravity: Vec3,
        solver_iterations: u32,
        max_bodies: u32,
        max_colliders: u32,
        max_substeps: u32,
        sleeping_disabled: bool,
    ) -> PhysicsResult<Self> {
        let finite = gravity.x.is_finite() & gravity.y.is_finite() & gravity.z.is_finite();
        let positive = (solver_iterations != 0)
            & (max_bodies != 0)
            & (max_colliders != 0)
            & (max_substeps != 0);
        [
            Err(PhysicsError::invalid_config(
                "physics config requires finite gravity and non-zero capacities, iterations, and substeps",
            )),
            Ok(PhysicsConfig {
                gravity,
                solver_iterations,
                max_bodies,
                max_colliders,
                max_substeps,
                sleeping_disabled,
            }),
        ][(finite & positive) as usize]
    }

    pub(crate) fn gravity(&self) -> Vec3 {
        self.gravity
    }

    pub(crate) fn solver_iterations(&self) -> u32 {
        self.solver_iterations
    }

    pub(crate) fn max_bodies(&self) -> u32 {
        self.max_bodies
    }

    pub(crate) fn max_colliders(&self) -> u32 {
        self.max_colliders
    }

    /// The number of deterministic substeps a single fixed step is split into
    /// (always `>= 1`). Substepping shortens each integration interval so a fast
    /// body cannot tunnel through thin geometry in one large jump.
    pub(crate) fn max_substeps(&self) -> u32 {
        self.max_substeps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_the_documented_constants() {
        let c = PhysicsConfig::default_config();
        assert_eq!(c.gravity(), Vec3::new(0.0, -9.8, 0.0));
        assert_eq!(c.solver_iterations(), 8);
        assert_eq!(c.max_bodies(), 4096);
        assert_eq!(c.max_colliders(), 4096);
    }

    #[test]
    fn new_accepts_valid_values() {
        let c = PhysicsConfig::new(Vec3::new(0.0, -1.0, 0.0), 4, 16, 16, 2, false).unwrap();
        assert_eq!(c.solver_iterations(), 4);
        assert_eq!(c.max_bodies(), 16);
        assert_eq!(c.max_colliders(), 16);
        assert_eq!(c.max_substeps(), 2);
        // The sleeping field is stored (read here via Debug).
        assert!(format!("{c:?}").contains("sleeping_disabled"));
    }

    #[test]
    fn new_rejects_non_finite_gravity() {
        let e = PhysicsConfig::new(Vec3::new(f32::NAN, 0.0, 0.0), 1, 1, 1, 1, true).unwrap_err();
        assert_eq!(e.code(), crate::physics_error_code::PhysicsErrorCode::InvalidConfig);
        assert!(PhysicsConfig::new(Vec3::new(0.0, f32::INFINITY, 0.0), 1, 1, 1, 1, true).is_err());
    }

    #[test]
    fn new_rejects_zero_counts() {
        assert!(PhysicsConfig::new(Vec3::ZERO, 0, 1, 1, 1, true).is_err());
        assert!(PhysicsConfig::new(Vec3::ZERO, 1, 0, 1, 1, true).is_err());
        assert!(PhysicsConfig::new(Vec3::ZERO, 1, 1, 0, 1, true).is_err());
        assert!(PhysicsConfig::new(Vec3::ZERO, 1, 1, 1, 0, true).is_err());
    }

    #[test]
    fn derives_are_exercised() {
        let c = PhysicsConfig::default_config();
        let d = c;
        assert_eq!(c, d);
        assert_ne!(c, PhysicsConfig::new(Vec3::ZERO, 1, 1, 1, 1, true).unwrap());
    }
}
