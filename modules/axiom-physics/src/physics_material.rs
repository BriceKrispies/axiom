//! Deterministic surface material of a collider.

use axiom_kernel::Ratio;

use crate::physics_error::PhysicsError;
use crate::physics_result::PhysicsResult;

/// The surface response parameters of a [`crate::PhysicsApi`] collider.
///
/// `restitution` is resolved live by the contact solver — a bouncy material
/// rebounds off any surface. `friction` is validated and stored but not yet
/// dynamically solved (no tangential impulse is applied; a documented deferral —
/// see `ROADMAP.md`). Each value is carried by a kernel [`Ratio`] (a finite
/// scalar); `PhysicsMaterial` additionally enforces the physical ranges below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsMaterial {
    friction: Ratio,
    restitution: Ratio,
    density: Ratio,
}

impl PhysicsMaterial {
    /// Construct a material, rejecting out-of-range values: `friction >= 0`,
    /// `0 <= restitution <= 1`, and `density > 0`.
    pub(crate) fn new(friction: Ratio, restitution: Ratio, density: Ratio) -> PhysicsResult<Self> {
        let f = friction.get();
        let r = restitution.get();
        let d = density.get();
        let valid = (f >= 0.0) & (0.0..=1.0).contains(&r) & (d > 0.0);
        [
            Err(PhysicsError::invalid_material(
                "material requires friction >= 0, restitution in [0, 1], and density > 0",
            )),
            Ok(PhysicsMaterial {
                friction,
                restitution,
                density,
            }),
        ][valid as usize]
    }

    /// The coefficient of friction (`>= 0`).
    pub fn friction(&self) -> Ratio {
        self.friction
    }

    /// The coefficient of restitution / bounciness (in `[0, 1]`).
    pub fn restitution(&self) -> Ratio {
        self.restitution
    }

    /// The material density (`> 0`).
    pub fn density(&self) -> Ratio {
        self.density
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_error_code::PhysicsErrorCode;

    fn r(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    #[test]
    fn new_accepts_valid_ranges_and_exposes_them() {
        let m = PhysicsMaterial::new(r(0.5), r(0.25), r(2.0)).unwrap();
        assert_eq!(m.friction().get(), 0.5);
        assert_eq!(m.restitution().get(), 0.25);
        assert_eq!(m.density().get(), 2.0);
    }

    #[test]
    fn new_rejects_out_of_range_values() {
        assert!(PhysicsMaterial::new(r(-0.1), r(0.0), r(1.0)).is_err());
        assert!(PhysicsMaterial::new(r(0.0), r(-0.1), r(1.0)).is_err());
        assert!(PhysicsMaterial::new(r(0.0), r(1.1), r(1.0)).is_err());
        assert!(PhysicsMaterial::new(r(0.0), r(0.0), r(0.0)).is_err());
        let e = PhysicsMaterial::new(r(0.0), r(2.0), r(1.0)).unwrap_err();
        assert_eq!(e.code(), PhysicsErrorCode::InvalidMaterial);
    }

    #[test]
    fn boundary_values_are_accepted() {
        assert!(PhysicsMaterial::new(r(0.0), r(0.0), r(0.001)).is_ok());
        assert!(PhysicsMaterial::new(r(0.0), r(1.0), r(1.0)).is_ok());
    }

    #[test]
    fn derives_are_exercised() {
        let m = PhysicsMaterial::new(r(0.5), r(0.5), r(1.0)).unwrap();
        let c = m;
        assert_eq!(m, c);
        assert_ne!(m, PhysicsMaterial::new(r(0.1), r(0.5), r(1.0)).unwrap());
        assert!(format!("{m:?}").contains("PhysicsMaterial"));
    }
}
