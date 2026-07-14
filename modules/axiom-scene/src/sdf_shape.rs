//! Signed-distance-field shape component: a node renders as a raymarched
//! primitive (sphere / box / plane) instead of (or alongside) a triangle mesh.
//! This is the scene-authoring peer of [`crate::renderable::Renderable`]: a node
//! declares an `SdfShape` and the engine carries it — node world transform, kind,
//! dimensions, colour — into a deterministic [`crate::scene_snapshot::SceneSnapshot`].
//! An app or render module translates that snapshot into the backend-neutral SDF
//! contract the render backends march; the scene module itself marches nothing
//! and depends on no render code.
//! The component is pure data: a `kind` discriminant, the local `dims` it carries
//! (a sphere's radius, a box's half-extents, or nothing for a plane), and a linear
//! RGB surface colour. The shape's world placement is the node's transform, exactly
//! as a [`Renderable`](crate::renderable::Renderable) is placed by its node — there
//! is no transform stored on the component.

use axiom_kernel::{
    BinaryReader, BinaryWriter, FieldSchema, KernelResult, Meters, Reflect, TypeSchema,
};
use axiom_math::{MathApi, Vec3};

use crate::scene_error::SceneError;
use crate::scene_result::SceneResult;

/// An SDF shape component, stored on the node entity it belongs to.
/// `kind` selects the canonical local distance function ([`Self::SPHERE`],
/// [`Self::BOX`], [`Self::PLANE`]); `dims` carries the local dimensions the kind
/// needs (sphere: `(radius, radius, radius)`; box: the half-extents; plane: unused
/// [`Vec3::ZERO`]); `color` is the linear-RGB surface colour (opaque). These kind
/// discriminants intentionally match the backend SDF contract's primitive kinds
/// so the render translation is an identity on `kind`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SdfShape {
    kind: u32,
    dims: Vec3,
    color: Vec3,
}

impl SdfShape {
    /// `kind` discriminant for a sphere of radius `dims.x`.
    pub const SPHERE: u32 = 0;
    /// `kind` discriminant for an axis-aligned box of half-extents `dims`.
    pub const BOX: u32 = 1;
    /// `kind` discriminant for the local `y = 0` plane (`dims` unused).
    pub const PLANE: u32 = 2;

    /// The reflected shape of an SDF-shape component.
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "SdfShape",
        &[
            FieldSchema::new("kind", "u32"),
            FieldSchema::new("dims", "Vec3"),
            FieldSchema::new("color", "Vec3"),
        ],
    );

    /// A raymarched sphere of the given `radius` and linear-RGB `color`. `radius`
    /// is already finite (it is a [`Meters`]); it must additionally be positive,
    /// and each colour component must be finite and non-negative.
    pub fn sphere(math: &MathApi, radius: Meters, color: Vec3) -> SceneResult<Self> {
        Self::validate_color(math, color)
            .and_then(|()| {
                (radius.get() > 0.0).then_some(()).ok_or_else(|| {
                    SceneError::invalid_sdf_shape_parameters("sdf sphere radius must be positive")
                })
            })
            .map(|()| SdfShape {
                kind: Self::SPHERE,
                dims: Vec3::new(radius.get(), radius.get(), radius.get()),
                color,
            })
    }

    /// A raymarched axis-aligned box of the given `half_extents` and linear-RGB
    /// `color`. Every half-extent must be finite and positive, and each colour
    /// component must be finite and non-negative.
    pub fn cuboid(math: &MathApi, half_extents: Vec3, color: Vec3) -> SceneResult<Self> {
        Self::validate_color(math, color)
            .and_then(|()| {
                [half_extents.x, half_extents.y, half_extents.z]
                    .iter()
                    .all(|&c| math.validate_finite(c).is_ok() & (c > 0.0))
                    .then_some(())
                    .ok_or_else(|| {
                        SceneError::invalid_sdf_shape_parameters(
                            "sdf box half-extents must be finite and positive",
                        )
                    })
            })
            .map(|()| SdfShape {
                kind: Self::BOX,
                dims: half_extents,
                color,
            })
    }

    /// A raymarched ground plane (`y = 0` in local space) of linear-RGB `color`.
    /// A plane carries no dimensions; only the colour is validated.
    pub fn plane(math: &MathApi, color: Vec3) -> SceneResult<Self> {
        Self::validate_color(math, color).map(|()| SdfShape {
            kind: Self::PLANE,
            dims: Vec3::ZERO,
            color,
        })
    }

    /// Reject a non-finite or negative colour, routing finiteness through
    /// [`MathApi::validate_finite`] so the component inherits the engine's scalar
    /// discipline (the same path [`crate::light::Light`] uses).
    fn validate_color(math: &MathApi, color: Vec3) -> SceneResult<()> {
        [color.x, color.y, color.z]
            .iter()
            .all(|&c| math.validate_finite(c).is_ok())
            .then_some(())
            .ok_or_else(|| {
                SceneError::invalid_sdf_shape_parameters("sdf shape colour must be finite")
            })
            .and_then(|()| {
                ((color.x >= 0.0) & (color.y >= 0.0) & (color.z >= 0.0))
                    .then_some(())
                    .ok_or_else(|| {
                        SceneError::invalid_sdf_shape_parameters(
                            "sdf shape colour components must be non-negative",
                        )
                    })
            })
    }

    /// The kind discriminant.
    pub const fn kind(&self) -> u32 {
        self.kind
    }

    /// The local dimensions (sphere radius in `x`; box half-extents; plane unused).
    pub const fn dims(&self) -> Vec3 {
        self.dims
    }

    /// The linear-RGB surface colour.
    pub const fn color(&self) -> Vec3 {
        self.color
    }
}

impl Reflect for SdfShape {
    const SCHEMA: TypeSchema = SdfShape::SCHEMA;

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.kind.reflect_write(writer);
        self.dims.reflect_write(writer);
        self.color.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u32::reflect_read(reader).and_then(|kind| {
            Vec3::reflect_read(reader).and_then(|dims| {
                Vec3::reflect_read(reader).map(|color| SdfShape { kind, dims, color })
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

    fn m(x: f32) -> Meters {
        Meters::new(x).unwrap()
    }

    #[test]
    fn sphere_is_built_with_radius_in_each_dim() {
        let s = SdfShape::sphere(&math(), m(2.0), Vec3::new(1.0, 0.0, 0.0)).unwrap();
        assert_eq!(s.kind(), SdfShape::SPHERE);
        assert_eq!(s.dims(), Vec3::new(2.0, 2.0, 2.0));
        assert_eq!(s.color(), Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn cuboid_keeps_its_half_extents() {
        let s = SdfShape::cuboid(&math(), Vec3::new(1.0, 2.0, 3.0), Vec3::ONE).unwrap();
        assert_eq!(s.kind(), SdfShape::BOX);
        assert_eq!(s.dims(), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn plane_has_no_dimensions() {
        let s = SdfShape::plane(&math(), Vec3::new(0.5, 0.5, 0.5)).unwrap();
        assert_eq!(s.kind(), SdfShape::PLANE);
        assert_eq!(s.dims(), Vec3::ZERO);
        assert_eq!(s.color(), Vec3::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn kind_discriminants_are_distinct_and_ordered() {
        assert_eq!(SdfShape::SPHERE, 0);
        assert_eq!(SdfShape::BOX, 1);
        assert_eq!(SdfShape::PLANE, 2);
    }

    #[test]
    fn zero_or_negative_radius_is_rejected() {
        for bad in [0.0, -1.0] {
            let err = SdfShape::sphere(&math(), m(bad), Vec3::ONE).unwrap_err();
            assert_eq!(err.code(), SceneErrorCode::InvalidSdfShapeParameters);
        }
    }

    #[test]
    fn non_positive_box_extent_is_rejected_per_axis() {
        for bad in [
            Vec3::new(0.0, 1.0, 1.0),
            Vec3::new(1.0, -0.1, 1.0),
            Vec3::new(1.0, 1.0, 0.0),
        ] {
            let err = SdfShape::cuboid(&math(), bad, Vec3::ONE).unwrap_err();
            assert_eq!(err.code(), SceneErrorCode::InvalidSdfShapeParameters);
        }
    }

    #[test]
    fn non_finite_box_extent_is_rejected() {
        let err =
            SdfShape::cuboid(&math(), Vec3::new(f32::INFINITY, 1.0, 1.0), Vec3::ONE).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidSdfShapeParameters);
    }

    #[test]
    fn non_finite_colour_is_rejected() {
        let err = SdfShape::plane(&math(), Vec3::new(f32::NAN, 0.0, 0.0)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidSdfShapeParameters);
    }

    #[test]
    fn negative_colour_component_is_rejected_per_channel() {
        for bad in [
            Vec3::new(-0.1, 0.0, 0.0),
            Vec3::new(0.0, -0.1, 0.0),
            Vec3::new(0.0, 0.0, -0.1),
        ] {
            let err = SdfShape::sphere(&math(), m(1.0), bad).unwrap_err();
            assert_eq!(err.code(), SceneErrorCode::InvalidSdfShapeParameters);
        }
    }

    #[test]
    fn zero_colour_channels_are_allowed() {
        assert!(SdfShape::plane(&math(), Vec3::ZERO).is_ok());
    }

    #[test]
    fn schema_names_the_sdf_shape_fields() {
        assert_eq!(SdfShape::SCHEMA.name(), "SdfShape");
        assert_eq!(SdfShape::SCHEMA.fields().len(), 3);
        assert_eq!(SdfShape::SCHEMA.fields()[0].name(), "kind");
        assert_eq!(SdfShape::SCHEMA.fields()[1].name(), "dims");
    }

    #[test]
    fn reflect_round_trips_every_kind_and_rejects_truncation() {
        let shapes = [
            SdfShape::sphere(&math(), m(1.5), Vec3::new(0.2, 0.4, 0.6)).unwrap(),
            SdfShape::cuboid(&math(), Vec3::new(1.0, 2.0, 3.0), Vec3::ONE).unwrap(),
            SdfShape::plane(&math(), Vec3::new(0.1, 0.1, 0.1)).unwrap(),
        ];
        for shape in shapes {
            let mut w = BinaryWriter::new();
            shape.reflect_write(&mut w);
            assert_eq!(
                SdfShape::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap(),
                shape
            );
        }
        assert!(SdfShape::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }

    #[test]
    fn equal_shapes_compare_equal() {
        let a = SdfShape::sphere(&math(), m(1.0), Vec3::ONE).unwrap();
        let b = SdfShape::sphere(&math(), m(1.0), Vec3::ONE).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, SdfShape::plane(&math(), Vec3::ONE).unwrap());
        assert!(format!("{a:?}").contains("SdfShape"));
    }
}
