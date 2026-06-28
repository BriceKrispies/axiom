//! A validated description used to create a rigid body.

use axiom_kernel::Ratio;
use axiom_math::Transform;

use crate::mass_properties::MassProperties;
use crate::physics_body_kind::PhysicsBodyKind;
use crate::physics_error::PhysicsError;
use crate::physics_result::PhysicsResult;

/// `true` iff every component of a transform (translation, rotation, scale) is
/// finite. The math `Transform` carries raw `f32` components that are not
/// validated on construction, so physics screens them here before a body is
/// created from the description.
fn transform_is_finite(t: Transform) -> bool {
    let tr = t.translation;
    let s = t.scale;
    let r = t.rotation;
    tr.x.is_finite()
        & tr.y.is_finite()
        & tr.z.is_finite()
        & s.x.is_finite()
        & s.y.is_finite()
        & s.z.is_finite()
        & r.x.is_finite()
        & r.y.is_finite()
        & r.z.is_finite()
        & r.w.is_finite()
}

/// The validated inputs for creating a rigid body.
///
/// Built by the explicit constructors below — never by an app directly (the
/// facade owns construction). A description is well-formed by construction: its
/// transform is finite and, for a dynamic body, its mass is finite and positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PhysicsBodyDesc {
    kind: PhysicsBodyKind,
    transform: Transform,
    mass_properties: MassProperties,
}

impl PhysicsBodyDesc {
    /// A static body at `transform`. Static bodies have zero inverse mass.
    pub(crate) fn static_body(transform: Transform) -> PhysicsResult<Self> {
        [
            Err(PhysicsError::non_finite_input(
                "static body transform must be finite",
            )),
            Ok(PhysicsBodyDesc {
                kind: PhysicsBodyKind::Static,
                transform,
                mass_properties: MassProperties::static_props(),
            }),
        ][transform_is_finite(transform) as usize]
    }

    /// A kinematic body at `transform`. Kinematic bodies have zero inverse mass.
    pub(crate) fn kinematic_body(transform: Transform) -> PhysicsResult<Self> {
        [
            Err(PhysicsError::non_finite_input(
                "kinematic body transform must be finite",
            )),
            Ok(PhysicsBodyDesc {
                kind: PhysicsBodyKind::Kinematic,
                transform,
                mass_properties: MassProperties::kinematic_props(),
            }),
        ][transform_is_finite(transform) as usize]
    }

    /// A dynamic body at `transform` with the given mass (finite, `> 0`).
    pub(crate) fn dynamic_body(transform: Transform, mass: Ratio) -> PhysicsResult<Self> {
        transform_is_finite(transform)
            .then_some(())
            .ok_or(PhysicsError::non_finite_input(
                "dynamic body transform must be finite",
            ))
            .and_then(|()| MassProperties::dynamic(mass))
            .map(|mass_properties| PhysicsBodyDesc {
                kind: PhysicsBodyKind::Dynamic,
                transform,
                mass_properties,
            })
    }

    pub(crate) fn kind(&self) -> PhysicsBodyKind {
        self.kind
    }

    pub(crate) fn transform(&self) -> Transform {
        self.transform
    }

    pub(crate) fn mass_properties(&self) -> MassProperties {
        self.mass_properties
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_error_code::PhysicsErrorCode;
    use axiom_math::Vec3;

    fn nan_transform() -> Transform {
        Transform::from_translation(Vec3::new(f32::NAN, 0.0, 0.0))
    }

    #[test]
    fn static_body_is_static_with_finite_transform() {
        let d = PhysicsBodyDesc::static_body(Transform::IDENTITY).unwrap();
        assert_eq!(d.kind(), PhysicsBodyKind::Static);
        assert_eq!(d.transform(), Transform::IDENTITY);
        assert_eq!(d.mass_properties().inverse_mass().get(), 0.0);
    }

    #[test]
    fn kinematic_body_is_kinematic_with_zero_inverse_mass() {
        let d = PhysicsBodyDesc::kinematic_body(Transform::IDENTITY).unwrap();
        assert_eq!(d.kind(), PhysicsBodyKind::Kinematic);
        assert_eq!(d.mass_properties().inverse_mass().get(), 0.0);
    }

    #[test]
    fn dynamic_body_carries_reciprocal_mass() {
        let d = PhysicsBodyDesc::dynamic_body(Transform::IDENTITY, Ratio::new(2.0).unwrap()).unwrap();
        assert_eq!(d.kind(), PhysicsBodyKind::Dynamic);
        assert_eq!(d.mass_properties().inverse_mass().get(), 0.5);
    }

    #[test]
    fn non_finite_transform_is_rejected_for_every_kind() {
        let s = PhysicsBodyDesc::static_body(nan_transform()).unwrap_err();
        assert_eq!(s.code(), PhysicsErrorCode::NonFiniteInput);
        assert!(PhysicsBodyDesc::kinematic_body(nan_transform()).is_err());
        assert!(PhysicsBodyDesc::dynamic_body(nan_transform(), Ratio::new(1.0).unwrap()).is_err());
    }

    #[test]
    fn dynamic_body_rejects_invalid_mass() {
        let e = PhysicsBodyDesc::dynamic_body(Transform::IDENTITY, Ratio::new(0.0).unwrap())
            .unwrap_err();
        assert_eq!(e.code(), PhysicsErrorCode::InvalidMass);
    }

    #[test]
    fn derives_are_exercised() {
        let d = PhysicsBodyDesc::static_body(Transform::IDENTITY).unwrap();
        let c = d;
        assert_eq!(d, c);
        assert_ne!(d, PhysicsBodyDesc::kinematic_body(Transform::IDENTITY).unwrap());
        assert!(format!("{d:?}").contains("PhysicsBodyDesc"));
    }
}
