//! Perspective camera component.

use axiom_kernel::{
    BinaryReader, BinaryWriter, FieldSchema, KernelResult, Meters, Radians, Ratio, Reflect,
    TypeSchema,
};
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
    fovy_radians: Radians,
    aspect: Ratio,
    near: Meters,
    far: Meters,
}

impl Camera {
    /// The reflected shape of a camera component — what an agent reads to learn
    /// the camera's fields without a running engine.
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "Camera",
        &[
            FieldSchema::new("fovy_radians", "Radians"),
            FieldSchema::new("aspect", "Ratio"),
            FieldSchema::new("near", "Meters"),
            FieldSchema::new("far", "Meters"),
        ],
    );

    /// Build and validate a perspective camera. Every intrinsic parameter is
    /// checked through the math layer; failures wrap a [`axiom_math::MathError`]
    /// inside a
    /// [`crate::scene_error_code::SceneErrorCode::InvalidCameraParameters`].
    pub fn perspective(
        math: &MathApi,
        fovy_radians: Radians,
        aspect: Ratio,
        near: Meters,
        far: Meters,
    ) -> SceneResult<Self> {
        math.mat4_perspective(fovy_radians.get(), aspect.get(), near.get(), far.get())
            .map_err(|cause| {
                SceneError::invalid_camera_parameters(
                    "camera perspective parameters were rejected by the math layer",
                    cause,
                )
            })
            .map(|_| Camera {
                fovy_radians,
                aspect,
                near,
                far,
            })
    }

    pub const fn fovy_radians(&self) -> Radians {
        self.fovy_radians
    }

    pub const fn aspect(&self) -> Ratio {
        self.aspect
    }

    pub const fn near(&self) -> Meters {
        self.near
    }

    pub const fn far(&self) -> Meters {
        self.far
    }

    /// Project to a [`Mat4`] via [`MathApi::mat4_perspective`]. This is the only
    /// way the camera produces a projection matrix — there is no parallel
    /// implementation.
    pub fn projection_matrix(&self, math: &MathApi) -> SceneResult<Mat4> {
        math.mat4_perspective(
            self.fovy_radians.get(),
            self.aspect.get(),
            self.near.get(),
            self.far.get(),
        )
        .map_err(|cause| {
            SceneError::invalid_camera_parameters(
                "camera projection was rejected by the math layer",
                cause,
            )
        })
    }
}

impl Reflect for Camera {
    const SCHEMA: TypeSchema = Camera::SCHEMA;

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.fovy_radians.reflect_write(writer);
        self.aspect.reflect_write(writer);
        self.near.reflect_write(writer);
        self.far.reflect_write(writer);
    }

    /// Reconstruct directly from the stored intrinsics. The quantity types
    /// re-validate finiteness on read; the perspective *relationship* was
    /// validated when the camera was first built.
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Radians::reflect_read(reader).and_then(|fovy_radians| {
            Ratio::reflect_read(reader).and_then(|aspect| {
                Meters::reflect_read(reader).and_then(|near| {
                    Meters::reflect_read(reader).map(|far| Camera {
                        fovy_radians,
                        aspect,
                        near,
                        far,
                    })
                })
            })
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

    fn rad(x: f32) -> Radians {
        Radians::new(x).unwrap()
    }
    fn rat(x: f32) -> Ratio {
        Ratio::new(x).unwrap()
    }
    fn m(x: f32) -> Meters {
        Meters::new(x).unwrap()
    }

    #[test]
    fn perspective_camera_is_built_with_valid_intrinsics() {
        let c = Camera::perspective(
            &math(),
            rad(std::f32::consts::FRAC_PI_2),
            rat(16.0 / 9.0),
            m(0.1),
            m(1000.0),
        )
        .unwrap();
        assert_eq!(c.aspect().get(), 16.0 / 9.0);
        assert_eq!(c.near().get(), 0.1);
        assert_eq!(c.far().get(), 1000.0);
    }

    #[test]
    fn fovy_radians_is_the_constructed_value() {
        let fov = std::f32::consts::FRAC_PI_3;
        let c = Camera::perspective(&math(), rad(fov), rat(1.0), m(0.1), m(100.0)).unwrap();
        assert_eq!(c.fovy_radians().get(), fov);
    }

    #[test]
    fn invalid_near_is_rejected() {
        let err = Camera::perspective(&math(), rad(1.0), rat(1.0), m(0.0), m(100.0)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
        assert!(err.math().is_some(), "math cause must be preserved");
    }

    #[test]
    fn far_less_than_near_is_rejected() {
        let err = Camera::perspective(&math(), rad(1.0), rat(1.0), m(100.0), m(1.0)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
    }

    #[test]
    fn invalid_aspect_is_rejected() {
        let err = Camera::perspective(&math(), rad(1.0), rat(0.0), m(0.1), m(100.0)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
    }

    #[test]
    fn invalid_fovy_is_rejected() {
        let err = Camera::perspective(&math(), rad(0.0), rat(1.0), m(0.1), m(100.0)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
    }

    #[test]
    fn projection_matrix_is_deterministic_for_identical_intrinsics() {
        let c = Camera::perspective(&math(), rad(1.5), rat(1.0), m(0.1), m(100.0)).unwrap();
        let a = c.projection_matrix(&math()).unwrap();
        let b = c.projection_matrix(&math()).unwrap();
        assert_eq!(a.as_cols_array(), b.as_cols_array());
    }

    #[test]
    fn projection_matrix_failure_is_wrapped() {
        // A Camera holding intrinsics the math layer rejects exercises the
        // `map_err` arm of `projection_matrix`. Construction normally validates,
        // so the struct is built directly to reach this path. The quantity types
        // accept these finite values; it is the perspective *relationship*
        // (zero fovy/aspect/extent) the math layer rejects.
        let bad = Camera {
            fovy_radians: rad(0.0),
            aspect: rat(0.0),
            near: m(0.0),
            far: m(0.0),
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

    #[test]
    fn reflect_round_trips_and_rejects_truncation() {
        let c = Camera::perspective(&math(), rad(1.2), rat(16.0 / 9.0), m(0.5), m(500.0)).unwrap();
        let mut w = BinaryWriter::new();
        c.reflect_write(&mut w);
        let got = Camera::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        assert_eq!(got, c);
        assert!(Camera::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }
}
