//! Deterministic mass / inverse-mass properties of a rigid body.

use axiom_kernel::Ratio;
use axiom_math::Vec3;

use crate::physics_error::PhysicsError;
use crate::physics_result::PhysicsResult;

/// The mass properties of a rigid body.
///
/// `inverse_mass` is the value the integrator actually uses: a static or
/// kinematic body has **zero** inverse mass (it never accelerates from a
/// force), and a dynamic body has `1 / mass`. `local_inverse_inertia` is a
/// placeholder (`Vec3::ZERO`) — angular dynamics are a documented deferral (see
/// `ROADMAP.md`), so it is stored but not yet integrated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MassProperties {
    mass: Ratio,
    inverse_mass: Ratio,
    local_inverse_inertia: Vec3,
}

impl MassProperties {
    /// The immovable mass properties shared by static and kinematic bodies:
    /// zero inverse mass, zero inverse inertia.
    fn immovable() -> Self {
        let zero = Ratio::new(0.0).expect("zero is a finite ratio");
        MassProperties {
            mass: zero,
            inverse_mass: zero,
            local_inverse_inertia: Vec3::ZERO,
        }
    }

    /// Mass properties for a static body (zero inverse mass).
    pub(crate) fn static_props() -> Self {
        MassProperties::immovable()
    }

    /// Mass properties for a kinematic body (zero inverse mass).
    pub(crate) fn kinematic_props() -> Self {
        MassProperties::immovable()
    }

    /// Mass properties for a dynamic body, rejecting a non-finite or
    /// non-positive mass. The clamp on the reciprocal keeps the computation
    /// total (never an infinity) even on the rejected path; the validity flag,
    /// not the arithmetic, decides whether the `Err` or `Ok` arm is returned.
    pub(crate) fn dynamic(mass: Ratio) -> PhysicsResult<Self> {
        let m = mass.get();
        let valid = m.is_finite() & (m > 0.0);
        let inverse_mass = Ratio::new(1.0 / m.max(f32::MIN_POSITIVE))
            .expect("reciprocal of a clamped positive mass is finite");
        [
            Err(PhysicsError::invalid_mass(
                "dynamic body mass must be finite and greater than zero",
            )),
            Ok(MassProperties {
                mass,
                inverse_mass,
                local_inverse_inertia: Vec3::ZERO,
            }),
        ][valid as usize]
    }

    /// The inverse mass — `0` for static/kinematic bodies, `1 / mass` for
    /// dynamic ones. This is the only mass quantity the linear integrator reads.
    pub(crate) fn inverse_mass(&self) -> Ratio {
        self.inverse_mass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_and_kinematic_have_zero_inverse_mass() {
        assert_eq!(MassProperties::static_props().inverse_mass().get(), 0.0);
        assert_eq!(MassProperties::kinematic_props().inverse_mass().get(), 0.0);
    }

    #[test]
    fn dynamic_has_reciprocal_inverse_mass() {
        let mp = MassProperties::dynamic(Ratio::new(4.0).unwrap()).unwrap();
        assert_eq!(mp.inverse_mass().get(), 0.25);
    }

    #[test]
    fn dynamic_rejects_zero_negative_and_non_finite_mass() {
        assert!(MassProperties::dynamic(Ratio::new(0.0).unwrap()).is_err());
        assert!(MassProperties::dynamic(Ratio::new(-2.0).unwrap()).is_err());
        let e = MassProperties::dynamic(Ratio::new(0.0).unwrap()).unwrap_err();
        assert_eq!(e.code(), crate::physics_error_code::PhysicsErrorCode::InvalidMass);
    }

    #[test]
    fn derives_are_exercised() {
        let mp = MassProperties::static_props();
        let c = mp;
        assert_eq!(mp, c);
        assert_ne!(mp, MassProperties::dynamic(Ratio::new(1.0).unwrap()).unwrap());
        assert!(format!("{mp:?}").contains("MassProperties"));
    }
}
