//! Perspective camera component.

use axiom_kernel::{FieldSchema, TypeSchema};
use axiom_math::{Mat4, MathApi};

use crate::scene_error::SceneError;
use crate::scene_result::SceneResult;

/// A perspective camera component, stored on the node entity it belongs to.
///
/// Stores the intrinsic projection parameters (`fovy`, `aspect`, `near`,
/// `far`). The node the camera follows is the entity this component is keyed
/// by — it is not a field. The projection matrix is derived on demand through
/// [`Camera::projection_matrix`], which delegates to
/// [`MathApi::mat4_perspective`] — the same finite-validation path every other
/// Layer-02 math user takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    fovy_radians: f32,
    aspect: f32,
    near: f32,
    far: f32,
}

impl Camera {
    /// The reflected shape of a camera component — what an agent reads to learn
    /// the camera's fields without a running engine.
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "Camera",
        &[
            FieldSchema::new("fovy_radians", "f32"),
            FieldSchema::new("aspect", "f32"),
            FieldSchema::new("near", "f32"),
            FieldSchema::new("far", "f32"),
        ],
    );

    /// Build and validate a perspective camera. Every intrinsic parameter is
    /// checked through the math layer; failures wrap a [`axiom_math::MathError`]
    /// inside a
    /// [`crate::scene_error_code::SceneErrorCode::InvalidCameraParameters`].
    pub fn perspective(
        math: &MathApi,
        fovy_radians: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> SceneResult<Self> {
        math.mat4_perspective(fovy_radians, aspect, near, far)
            .map_err(|cause| {
                SceneError::invalid_camera_parameters(
                    "camera perspective parameters were rejected by the math layer",
                    cause,
                )
            })?;
        Ok(Camera {
            fovy_radians,
            aspect,
            near,
            far,
        })
    }

    pub const fn fovy_radians(&self) -> f32 {
        self.fovy_radians
    }

    pub const fn aspect(&self) -> f32 {
        self.aspect
    }

    pub const fn near(&self) -> f32 {
        self.near
    }

    pub const fn far(&self) -> f32 {
        self.far
    }

    /// Project to a [`Mat4`] via [`MathApi::mat4_perspective`]. This is the only
    /// way the camera produces a projection matrix — there is no parallel
    /// implementation.
    pub fn projection_matrix(&self, math: &MathApi) -> SceneResult<Mat4> {
        math.mat4_perspective(self.fovy_radians, self.aspect, self.near, self.far)
            .map_err(|cause| {
                SceneError::invalid_camera_parameters(
                    "camera projection was rejected by the math layer",
                    cause,
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;

    fn math() -> MathApi {
        MathApi::new()
    }

    #[test]
    fn perspective_camera_is_built_with_valid_intrinsics() {
        let c = Camera::perspective(&math(), std::f32::consts::FRAC_PI_2, 16.0 / 9.0, 0.1, 1000.0)
            .unwrap();
        assert_eq!(c.aspect(), 16.0 / 9.0);
        assert_eq!(c.near(), 0.1);
        assert_eq!(c.far(), 1000.0);
    }

    #[test]
    fn fovy_radians_is_the_constructed_value() {
        let fov = std::f32::consts::FRAC_PI_3;
        let c = Camera::perspective(&math(), fov, 1.0, 0.1, 100.0).unwrap();
        assert_eq!(c.fovy_radians(), fov);
    }

    #[test]
    fn invalid_near_is_rejected() {
        let err = Camera::perspective(&math(), 1.0, 1.0, 0.0, 100.0).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
        assert!(err.math().is_some(), "math cause must be preserved");
    }

    #[test]
    fn far_less_than_near_is_rejected() {
        let err = Camera::perspective(&math(), 1.0, 1.0, 100.0, 1.0).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
    }

    #[test]
    fn invalid_aspect_is_rejected() {
        let err = Camera::perspective(&math(), 1.0, 0.0, 0.1, 100.0).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
    }

    #[test]
    fn invalid_fovy_is_rejected() {
        let err = Camera::perspective(&math(), 0.0, 1.0, 0.1, 100.0).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
        let err = Camera::perspective(&math(), f32::NAN, 1.0, 0.1, 100.0).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
    }

    #[test]
    fn projection_matrix_is_deterministic_for_identical_intrinsics() {
        let c = Camera::perspective(&math(), 1.5, 1.0, 0.1, 100.0).unwrap();
        let a = c.projection_matrix(&math()).unwrap();
        let b = c.projection_matrix(&math()).unwrap();
        assert_eq!(a.as_cols_array(), b.as_cols_array());
    }

    #[test]
    fn projection_matrix_failure_is_wrapped() {
        // A Camera holding intrinsics the math layer rejects exercises the
        // `map_err` arm of `projection_matrix`. Construction normally validates,
        // so the struct is built directly to reach this path.
        let bad = Camera {
            fovy_radians: 0.0,
            aspect: 0.0,
            near: 0.0,
            far: 0.0,
        };
        let err = bad.projection_matrix(&math()).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
        assert!(err.math().is_some());
    }

    #[test]
    fn schema_names_the_camera_fields() {
        assert_eq!(Camera::SCHEMA.name(), "Camera");
        assert_eq!(Camera::SCHEMA.fields().len(), 4);
        assert_eq!(Camera::SCHEMA.fields()[0].name(), "fovy_radians");
        assert_eq!(Camera::SCHEMA.fields()[3].name(), "far");
    }
}
