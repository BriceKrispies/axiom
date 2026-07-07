//! Deterministic mass / inverse-mass / inverse-inertia properties of a rigid body.

use axiom_kernel::Ratio;
use axiom_math::Vec3;

use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_error::PhysicsError;
use crate::physics_result::PhysicsResult;

/// The mass properties of a rigid body.
/// `inverse_mass` is the value the linear integrator uses: a static or kinematic
/// body has **zero** inverse mass (it never accelerates from a force), and a
/// dynamic body has `1 / mass`. `local_inverse_inertia` is the per-axis inverse
/// of the body's diagonal moment of inertia — the value the angular integrator
/// uses to turn a torque into an angular acceleration. It is derived from the
/// body's mass and the shape of the collider attached to it
/// ([`MassProperties::with_inertia_for`]); before any collider is attached, or
/// for an immovable body, it is `Vec3::ZERO` (the body never spins from a torque).
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
    /// `local_inverse_inertia` starts at `Vec3::ZERO` and is filled in per shape
    /// by [`MassProperties::with_inertia_for`] when a collider is attached.
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

    /// This body's mass properties with `local_inverse_inertia` derived from
    /// `shape` and the body's mass. An immovable body (mass `0`) yields a zero
    /// moment and therefore zero inverse inertia — it never spins, exactly as it
    /// never translates. The derivation is a per-shape solid-body diagonal moment
    /// (sphere/box/capsule), with an infinite plane contributing no rotational
    /// resistance (zero inverse inertia, like an immovable body).
    pub(crate) fn with_inertia_for(self, shape: PhysicsColliderShape) -> Self {
        let moment = diagonal_moment(self.mass.get(), shape);
        MassProperties {
            local_inverse_inertia: invert_diagonal(moment),
            ..self
        }
    }

    /// The inverse mass — `0` for static/kinematic bodies, `1 / mass` for
    /// dynamic ones. This is the only mass quantity the linear integrator reads.
    pub(crate) fn inverse_mass(&self) -> Ratio {
        self.inverse_mass
    }

    /// The per-axis inverse moment of inertia (`Vec3::ZERO` for an immovable body
    /// or a body with no rotational extent). The angular integrator multiplies a
    /// torque by this, componentwise, to get an angular acceleration.
    pub(crate) fn inverse_inertia(&self) -> Vec3 {
        self.local_inverse_inertia
    }
}

/// A per-shape diagonal moment of inertia for a solid body of `mass`, indexed by
/// the shape's kind so the dispatch is a function table, not a `match`.
fn diagonal_moment(mass: f32, shape: PhysicsColliderShape) -> Vec3 {
    const TABLE: [fn(f32, PhysicsColliderShape) -> Vec3; 5] =
        [sphere_moment, box_moment, capsule_moment, plane_moment, heightfield_moment];
    TABLE[shape.kind().index()](mass, shape)
}

/// Solid sphere: `I = (2/5) m r²` about every axis.
fn sphere_moment(mass: f32, shape: PhysicsColliderShape) -> Vec3 {
    let r = shape.radius();
    let i = 0.4 * mass * r * r;
    Vec3::new(i, i, i)
}

/// Solid box (full extents `2·half_extents`): `I_x = (1/3) m (h_y² + h_z²)`, and
/// cyclically for the other axes.
fn box_moment(mass: f32, shape: PhysicsColliderShape) -> Vec3 {
    let h = shape.half_extents();
    let k = mass / 3.0;
    Vec3::new(
        k * (h.y * h.y + h.z * h.z),
        k * (h.x * h.x + h.z * h.z),
        k * (h.x * h.x + h.y * h.y),
    )
}

/// Capsule: approximated by the solid box of its local AABB half-extents — a
/// deterministic diagonal moment sufficient for the 2D-dominant catalog (capsule
/// contacts themselves are still a documented narrow-phase deferral).
fn capsule_moment(mass: f32, shape: PhysicsColliderShape) -> Vec3 {
    box_moment(mass, shape)
}

/// Infinite plane: no finite extent, so no rotational resistance is defined —
/// zero moment (and therefore zero inverse inertia, like an immovable body).
fn plane_moment(_mass: f32, _shape: PhysicsColliderShape) -> Vec3 {
    Vec3::ZERO
}

/// Static heightfield: a track surface is always attached to an immovable (static)
/// body, so its rotational resistance is irrelevant — zero moment (zero inverse
/// inertia), exactly like a plane.
fn heightfield_moment(_mass: f32, _shape: PhysicsColliderShape) -> Vec3 {
    Vec3::ZERO
}

/// Invert each diagonal moment component, mapping a zero (or non-positive) moment
/// to zero inverse inertia rather than an infinity.
fn invert_diagonal(moment: Vec3) -> Vec3 {
    Vec3::new(
        invert_component(moment.x),
        invert_component(moment.y),
        invert_component(moment.z),
    )
}

/// `1 / i` for a positive moment, `0` otherwise. The positivity flag (not a
/// branch) selects between the reciprocal and zero, and the divisor is clamped so
/// the reciprocal is always finite even on the zero path.
fn invert_component(i: f32) -> f32 {
    let positive = (i > 0.0) as u8 as f32;
    positive / i.max(f32::MIN_POSITIVE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;

    fn sphere(r: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::sphere(Meters::new(r).unwrap()).unwrap()
    }

    fn box_shape(x: f32, y: f32, z: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::box_shape(Vec3::new(x, y, z)).unwrap()
    }

    fn capsule(r: f32, hh: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::capsule(Meters::new(r).unwrap(), Meters::new(hh).unwrap()).unwrap()
    }

    fn plane() -> PhysicsColliderShape {
        PhysicsColliderShape::plane(Vec3::UNIT_Y, Meters::new(0.0).unwrap()).unwrap()
    }

    #[test]
    fn static_and_kinematic_have_zero_inverse_mass() {
        assert_eq!(MassProperties::static_props().inverse_mass().get(), 0.0);
        assert_eq!(MassProperties::kinematic_props().inverse_mass().get(), 0.0);
    }

    #[test]
    fn dynamic_has_reciprocal_inverse_mass_and_starts_inertia_free() {
        let mp = MassProperties::dynamic(Ratio::new(4.0).unwrap()).unwrap();
        assert_eq!(mp.inverse_mass().get(), 0.25);
        assert_eq!(mp.inverse_inertia(), Vec3::ZERO);
    }

    #[test]
    fn dynamic_rejects_zero_negative_and_non_finite_mass() {
        assert!(MassProperties::dynamic(Ratio::new(0.0).unwrap()).is_err());
        assert!(MassProperties::dynamic(Ratio::new(-2.0).unwrap()).is_err());
        let e = MassProperties::dynamic(Ratio::new(0.0).unwrap()).unwrap_err();
        assert_eq!(e.code(), crate::physics_error_code::PhysicsErrorCode::InvalidMass);
    }

    #[test]
    fn sphere_inertia_is_isotropic_two_fifths_m_r_squared() {
        // mass 5, radius 2: I = 0.4 * 5 * 4 = 8 on every axis; inverse = 1/8.
        let mp = MassProperties::dynamic(Ratio::new(5.0).unwrap())
            .unwrap()
            .with_inertia_for(sphere(2.0));
        let inv = mp.inverse_inertia();
        assert_eq!(inv, Vec3::new(0.125, 0.125, 0.125));
    }

    #[test]
    fn box_inertia_is_anisotropic_per_axis() {
        // mass 3, half-extents (1, 2, 3): k = 1.
        // Ix = 1*(4+9) = 13; Iy = 1*(1+9) = 10; Iz = 1*(1+4) = 5.
        let mp = MassProperties::dynamic(Ratio::new(3.0).unwrap())
            .unwrap()
            .with_inertia_for(box_shape(1.0, 2.0, 3.0));
        let inv = mp.inverse_inertia();
        assert!((inv.x - 1.0 / 13.0).abs() < 1.0e-6);
        assert!((inv.y - 1.0 / 10.0).abs() < 1.0e-6);
        assert!((inv.z - 1.0 / 5.0).abs() < 1.0e-6);
    }

    #[test]
    fn capsule_inertia_matches_its_aabb_box() {
        let mass = Ratio::new(2.0).unwrap();
        // capsule (r=1, hh=2) packs local half-extents (1, 3, 1).
        let cap = MassProperties::dynamic(mass).unwrap().with_inertia_for(capsule(1.0, 2.0));
        let boxed = MassProperties::dynamic(mass)
            .unwrap()
            .with_inertia_for(box_shape(1.0, 3.0, 1.0));
        assert_eq!(cap.inverse_inertia(), boxed.inverse_inertia());
    }

    #[test]
    fn heightfield_contributes_no_inverse_inertia() {
        // A heightfield is static-surface-only, so (like a plane) it yields zero
        // moment and therefore zero inverse inertia.
        let hf = PhysicsColliderShape::heightfield_shape(Vec3::new(4.0, 1.0, 6.0)).unwrap();
        let mp = MassProperties::dynamic(Ratio::new(7.0).unwrap()).unwrap().with_inertia_for(hf);
        assert_eq!(mp.inverse_inertia(), Vec3::ZERO);
    }

    #[test]
    fn plane_contributes_no_inverse_inertia() {
        let mp = MassProperties::dynamic(Ratio::new(7.0).unwrap())
            .unwrap()
            .with_inertia_for(plane());
        assert_eq!(mp.inverse_inertia(), Vec3::ZERO);
    }

    #[test]
    fn immovable_body_with_a_shape_still_has_zero_inverse_inertia() {
        let mp = MassProperties::static_props().with_inertia_for(sphere(2.0));
        assert_eq!(mp.inverse_inertia(), Vec3::ZERO);
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
