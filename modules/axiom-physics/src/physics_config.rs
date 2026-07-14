//! Deterministic, bounded world configuration.

use axiom_kernel::Ratio;
use axiom_math::Vec3;

use crate::physics_error::PhysicsError;
use crate::physics_result::PhysicsResult;

/// The fixed, deterministic configuration of a [`crate::PhysicsApi`] world.
/// Every value is explicit and bounded — there is no hidden global state and no
/// wall-clock or random input. The defaults ([`PhysicsConfig::default_config`])
/// are deterministic constants; [`PhysicsConfig::new`] rejects any invalid value.
/// `linear_damping` / `angular_damping` are per-step velocity-decay fractions in
/// `[0, 1]`: each step a body's linear (resp. angular) velocity is scaled by
/// `1 - damping`, so `0` is no decay (a free body coasts forever, today's
/// behaviour) and `1` brings it to rest in one step. They enter as kernel
/// [`Ratio`] values (finite) and are additionally range-checked here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PhysicsConfig {
    gravity: Vec3,
    solver_iterations: u32,
    max_bodies: u32,
    max_colliders: u32,
    max_substeps: u32,
    sleeping_disabled: bool,
    linear_damping: f32,
    angular_damping: f32,
}

impl PhysicsConfig {
    /// The deterministic default configuration:
    /// gravity `(0, -9.8, 0)`, 8 solver iterations, capacity 4096 bodies and
    /// 4096 colliders, a single substep, sleeping disabled, and **no damping**
    /// (linear and angular damping both `0`).
    pub(crate) fn default_config() -> Self {
        PhysicsConfig {
            gravity: Vec3::new(0.0, -9.8, 0.0),
            solver_iterations: 8,
            max_bodies: 4096,
            max_colliders: 4096,
            max_substeps: 1,
            sleeping_disabled: true,
            linear_damping: 0.0,
            angular_damping: 0.0,
        }
    }

    /// Construct a configuration, rejecting non-finite gravity, any zero
    /// capacity / iteration / substep count, or a damping fraction outside
    /// `[0, 1]`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        gravity: Vec3,
        solver_iterations: u32,
        max_bodies: u32,
        max_colliders: u32,
        max_substeps: u32,
        sleeping_disabled: bool,
        linear_damping: Ratio,
        angular_damping: Ratio,
    ) -> PhysicsResult<Self> {
        let finite = gravity.x.is_finite() & gravity.y.is_finite() & gravity.z.is_finite();
        let positive = (solver_iterations != 0)
            & (max_bodies != 0)
            & (max_colliders != 0)
            & (max_substeps != 0);
        let ld = linear_damping.get();
        let ad = angular_damping.get();
        let damping_ok = (0.0..=1.0).contains(&ld) & (0.0..=1.0).contains(&ad);
        [
            Err(PhysicsError::invalid_config(
                "physics config requires finite gravity, non-zero capacities, iterations, and substeps, and damping in [0, 1]",
            )),
            Ok(PhysicsConfig {
                gravity,
                solver_iterations,
                max_bodies,
                max_colliders,
                max_substeps,
                sleeping_disabled,
                linear_damping: ld,
                angular_damping: ad,
            }),
        ][(finite & positive & damping_ok) as usize]
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

    /// The per-step linear-velocity decay fraction (in `[0, 1]`; `0` = no decay).
    pub(crate) fn linear_damping(&self) -> f32 {
        self.linear_damping
    }

    /// The per-step angular-velocity decay fraction (in `[0, 1]`; `0` = no decay).
    pub(crate) fn angular_damping(&self) -> f32 {
        self.angular_damping
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn valid(
        gravity: Vec3,
        iters: u32,
        bodies: u32,
        colliders: u32,
        substeps: u32,
    ) -> PhysicsResult<PhysicsConfig> {
        PhysicsConfig::new(
            gravity,
            iters,
            bodies,
            colliders,
            substeps,
            true,
            r(0.0),
            r(0.0),
        )
    }

    #[test]
    fn default_config_is_the_documented_constants() {
        let c = PhysicsConfig::default_config();
        assert_eq!(c.gravity(), Vec3::new(0.0, -9.8, 0.0));
        assert_eq!(c.solver_iterations(), 8);
        assert_eq!(c.max_bodies(), 4096);
        assert_eq!(c.max_colliders(), 4096);
        assert_eq!(c.linear_damping(), 0.0);
        assert_eq!(c.angular_damping(), 0.0);
    }

    #[test]
    fn new_accepts_valid_values_and_stores_damping() {
        let c = PhysicsConfig::new(
            Vec3::new(0.0, -1.0, 0.0),
            4,
            16,
            16,
            2,
            false,
            r(0.1),
            r(0.25),
        )
        .unwrap();
        assert_eq!(c.solver_iterations(), 4);
        assert_eq!(c.max_bodies(), 16);
        assert_eq!(c.max_colliders(), 16);
        assert_eq!(c.max_substeps(), 2);
        assert_eq!(c.linear_damping(), 0.1);
        assert_eq!(c.angular_damping(), 0.25);
        // The sleeping field is stored (read here via Debug).
        assert!(format!("{c:?}").contains("sleeping_disabled"));
    }

    #[test]
    fn damping_boundaries_zero_and_one_are_accepted() {
        assert!(PhysicsConfig::new(Vec3::ZERO, 1, 1, 1, 1, true, r(0.0), r(1.0)).is_ok());
        assert!(PhysicsConfig::new(Vec3::ZERO, 1, 1, 1, 1, true, r(1.0), r(0.0)).is_ok());
    }

    #[test]
    fn damping_outside_unit_range_is_rejected() {
        assert!(PhysicsConfig::new(Vec3::ZERO, 1, 1, 1, 1, true, r(-0.1), r(0.0)).is_err());
        assert!(PhysicsConfig::new(Vec3::ZERO, 1, 1, 1, 1, true, r(1.1), r(0.0)).is_err());
        assert!(PhysicsConfig::new(Vec3::ZERO, 1, 1, 1, 1, true, r(0.0), r(-0.1)).is_err());
        let e = PhysicsConfig::new(Vec3::ZERO, 1, 1, 1, 1, true, r(0.0), r(2.0)).unwrap_err();
        assert_eq!(
            e.code(),
            crate::physics_error_code::PhysicsErrorCode::InvalidConfig
        );
    }

    #[test]
    fn new_rejects_non_finite_gravity() {
        let e = valid(Vec3::new(f32::NAN, 0.0, 0.0), 1, 1, 1, 1).unwrap_err();
        assert_eq!(
            e.code(),
            crate::physics_error_code::PhysicsErrorCode::InvalidConfig
        );
        assert!(valid(Vec3::new(0.0, f32::INFINITY, 0.0), 1, 1, 1, 1).is_err());
    }

    #[test]
    fn new_rejects_zero_counts() {
        assert!(valid(Vec3::ZERO, 0, 1, 1, 1).is_err());
        assert!(valid(Vec3::ZERO, 1, 0, 1, 1).is_err());
        assert!(valid(Vec3::ZERO, 1, 1, 0, 1).is_err());
        assert!(valid(Vec3::ZERO, 1, 1, 1, 0).is_err());
    }

    #[test]
    fn derives_are_exercised() {
        let c = PhysicsConfig::default_config();
        let d = c;
        assert_eq!(c, d);
        assert_ne!(c, valid(Vec3::ZERO, 1, 1, 1, 1).unwrap());
    }
}
