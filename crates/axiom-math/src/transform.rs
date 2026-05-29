//! A translation/rotation/scale composition.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::mat4::Mat4;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::quat::Quat;
use crate::vec3::Vec3;

/// A rigid-plus-scale transform: translation `T`, rotation `R`, scale `S`.
///
/// The applied order is `T * R * S` — points pass through scale, then
/// rotation, then translation. `Transform` is the compact authoring form;
/// [`Transform::to_matrix`] expands it to a [`Mat4`] when the engine needs the
/// homogeneous representation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    /// The identity transform: zero translation, identity rotation, unit scale.
    pub const IDENTITY: Transform = Transform {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    /// Translation-only transform.
    pub const fn from_translation(t: Vec3) -> Transform {
        Transform {
            translation: t,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// Rotation-only transform.
    pub const fn from_rotation(r: Quat) -> Transform {
        Transform {
            translation: Vec3::ZERO,
            rotation: r,
            scale: Vec3::ONE,
        }
    }

    /// Scale-only transform.
    pub const fn from_scale(s: Vec3) -> Transform {
        Transform {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: s,
        }
    }

    /// Raw TRS constructor.
    pub const fn new(translation: Vec3, rotation: Quat, scale: Vec3) -> Transform {
        Transform {
            translation,
            rotation,
            scale,
        }
    }

    /// Apply the transform to a point: `T(R(S * p))`.
    pub fn transform_point(self, p: Vec3) -> Vec3 {
        let scaled = Vec3::new(p.x * self.scale.x, p.y * self.scale.y, p.z * self.scale.z);
        self.translation.add(self.rotation.rotate(scaled))
    }

    /// Apply the transform to a direction: rotation and scale apply,
    /// translation does not.
    pub fn transform_vector(self, d: Vec3) -> Vec3 {
        let scaled = Vec3::new(d.x * self.scale.x, d.y * self.scale.y, d.z * self.scale.z);
        self.rotation.rotate(scaled)
    }

    /// Compose `parent` with `child`: the result transforms a point exactly
    /// the same as `parent.transform_point(child.transform_point(p))`.
    pub fn combine(parent: Transform, child: Transform) -> Transform {
        let scale = Vec3::new(
            parent.scale.x * child.scale.x,
            parent.scale.y * child.scale.y,
            parent.scale.z * child.scale.z,
        );
        let rotation = parent.rotation.multiply(child.rotation);
        let scaled_child_t = Vec3::new(
            parent.scale.x * child.translation.x,
            parent.scale.y * child.translation.y,
            parent.scale.z * child.translation.z,
        );
        let translation = parent
            .translation
            .add(parent.rotation.rotate(scaled_child_t));
        Transform {
            translation,
            rotation,
            scale,
        }
    }

    /// Inverse transform.
    ///
    /// The TRS structure is closed under inverse only for **uniform** scale.
    /// For a non-uniform scale, the inverse `S^-1 R^-1 T^-1` is not itself a
    /// TRS transform (rotation and non-uniform scale do not commute), so this
    /// method returns
    /// [`crate::math_error_code::MathErrorCode::InvalidMatrixOperation`].
    ///
    /// Fails additionally when the scale is zero or non-finite, or when the
    /// rotation cannot be inverted.
    pub fn inverse(self) -> MathResult<Transform> {
        for component in [self.scale.x, self.scale.y, self.scale.z] {
            if !component.is_finite() {
                return Err(MathError::non_finite_scalar(
                    "transform scale must be finite to invert",
                ));
            }
            if component == 0.0 {
                return Err(MathError::divide_by_zero(
                    "transform scale must be non-zero to invert",
                ));
            }
        }
        if self.scale.x != self.scale.y || self.scale.y != self.scale.z {
            return Err(MathError::invalid_matrix_operation(
                "Transform::inverse requires uniform scale; expand to Mat4 first for non-uniform scale",
            ));
        }
        let inv_rot = self.rotation.inverse()?;
        let s_inv = 1.0 / self.scale.x;
        let neg_t = self.translation.mul_scalar(-1.0);
        let inv_translation = inv_rot.rotate(neg_t).mul_scalar(s_inv);
        Ok(Transform {
            translation: inv_translation,
            rotation: inv_rot,
            scale: Vec3::new(s_inv, s_inv, s_inv),
        })
    }

    /// Expand to a homogeneous `T * R * S` matrix.
    pub fn to_matrix(self) -> Mat4 {
        let t = Mat4::translation(self.translation);
        let r = Mat4::from_quaternion(self.rotation);
        let s = Mat4::scale(self.scale);
        t.multiply(r).multiply(s)
    }

    /// Append translation, rotation, and scale in declaration order.
    pub fn write_to(self, writer: &mut BinaryWriter) {
        self.translation.write_to(writer);
        self.rotation.write_to(writer);
        self.scale.write_to(writer);
    }

    /// Read translation, rotation, and scale in declaration order.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Transform> {
        let translation = Vec3::read_from(reader)?;
        let rotation = Quat::read_from(reader)?;
        let scale = Vec3::read_from(reader)?;
        Ok(Transform {
            translation,
            rotation,
            scale,
        })
    }
}

impl ApproxEq for Transform {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.translation.approx_eq(&other.translation, epsilon)
            && self.rotation.approx_eq(&other.rotation, epsilon)
            && self.scale.approx_eq(&other.scale, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;
    use axiom_kernel::KernelApi;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-4).unwrap()
    }

    #[test]
    fn identity_is_neutral_on_points_and_vectors() {
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert!(Transform::IDENTITY.transform_point(p).approx_eq(&p, eps()));
        assert!(Transform::IDENTITY.transform_vector(p).approx_eq(&p, eps()));
    }

    #[test]
    fn from_translation_only_moves_points() {
        let t = Transform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let p = t.transform_point(Vec3::ZERO);
        assert!(p.approx_eq(&Vec3::new(1.0, 2.0, 3.0), eps()));
        // Vectors are unaffected by translation.
        let d = t.transform_vector(Vec3::UNIT_X);
        assert!(d.approx_eq(&Vec3::UNIT_X, eps()));
    }

    #[test]
    fn from_rotation_only_rotates_points_and_vectors() {
        let q = Quat::from_axis_angle(Vec3::UNIT_Z, std::f32::consts::FRAC_PI_2).unwrap();
        let t = Transform::from_rotation(q);
        assert!(t.transform_point(Vec3::UNIT_X).approx_eq(&Vec3::UNIT_Y, eps()));
        assert!(t.transform_vector(Vec3::UNIT_X).approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn from_scale_only_scales_points_and_vectors() {
        let t = Transform::from_scale(Vec3::new(2.0, 3.0, 4.0));
        assert!(t
            .transform_point(Vec3::new(1.0, 1.0, 1.0))
            .approx_eq(&Vec3::new(2.0, 3.0, 4.0), eps()));
        assert!(t
            .transform_vector(Vec3::new(1.0, 1.0, 1.0))
            .approx_eq(&Vec3::new(2.0, 3.0, 4.0), eps()));
    }

    #[test]
    fn new_composes_translation_rotation_and_scale() {
        let t = Transform::new(
            Vec3::new(10.0, 0.0, 0.0),
            Quat::from_axis_angle(Vec3::UNIT_Z, std::f32::consts::FRAC_PI_2).unwrap(),
            Vec3::new(2.0, 2.0, 2.0),
        );
        // Apply: scale x=1 -> 2, rotate (2,0,0) -> (0,2,0), translate -> (10,2,0).
        assert!(t
            .transform_point(Vec3::UNIT_X)
            .approx_eq(&Vec3::new(10.0, 2.0, 0.0), eps()));
    }

    #[test]
    fn combine_matches_sequential_application() {
        let parent = Transform::new(
            Vec3::new(1.0, 0.0, 0.0),
            Quat::from_axis_angle(Vec3::UNIT_Z, std::f32::consts::FRAC_PI_2).unwrap(),
            Vec3::new(2.0, 2.0, 2.0),
        );
        let child = Transform::new(
            Vec3::new(0.0, 1.0, 0.0),
            Quat::IDENTITY,
            Vec3::new(0.5, 0.5, 0.5),
        );
        let combined = Transform::combine(parent, child);
        let p = Vec3::new(1.0, 2.0, 3.0);
        let manual = parent.transform_point(child.transform_point(p));
        assert!(combined.transform_point(p).approx_eq(&manual, eps()));
    }

    #[test]
    fn to_matrix_matches_direct_transform_for_points() {
        let t = Transform::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_axis_angle(Vec3::UNIT_Y, 0.7).unwrap(),
            Vec3::new(1.5, 1.5, 1.5),
        );
        let m = t.to_matrix();
        let p = Vec3::new(0.5, -1.0, 2.0);
        assert!(m.transform_point(p).approx_eq(&t.transform_point(p), eps()));
        let d = Vec3::new(1.0, 0.0, 0.0);
        assert!(m.transform_vector(d).approx_eq(&t.transform_vector(d), eps()));
    }

    #[test]
    fn inverse_undoes_uniform_scale_transform() {
        let t = Transform::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_axis_angle(Vec3::new(1.0, 1.0, 0.0), 0.9).unwrap(),
            Vec3::new(2.0, 2.0, 2.0),
        );
        let inv = t.inverse().unwrap();
        let p = Vec3::new(0.7, -0.4, 1.2);
        let round = inv.transform_point(t.transform_point(p));
        assert!(round.approx_eq(&p, eps()));
    }

    #[test]
    fn inverse_rejects_non_uniform_scale() {
        let t = Transform::from_scale(Vec3::new(2.0, 0.5, 1.25));
        assert_eq!(
            t.inverse().unwrap_err().code(),
            MathErrorCode::InvalidMatrixOperation
        );
    }

    #[test]
    fn inverse_zero_scale_fails() {
        let t = Transform::from_scale(Vec3::new(0.0, 1.0, 1.0));
        assert_eq!(
            t.inverse().unwrap_err().code(),
            MathErrorCode::DivideByZero
        );
    }

    #[test]
    fn inverse_nan_scale_fails() {
        let t = Transform::from_scale(Vec3::new(f32::NAN, 1.0, 1.0));
        assert_eq!(
            t.inverse().unwrap_err().code(),
            MathErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn binary_round_trip_preserves_components() {
        let api = KernelApi::new();
        let t = Transform::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_axis_angle(Vec3::UNIT_Z, 0.5).unwrap(),
            Vec3::new(1.0, 2.0, 3.0),
        );
        let mut writer = api.binary_writer();
        t.write_to(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = api.binary_reader(&bytes);
        let back = Transform::read_from(&mut reader).unwrap();
        assert!(back.approx_eq(&t, eps()));
    }
}
