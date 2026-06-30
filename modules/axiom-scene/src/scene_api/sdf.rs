//! The signed-distance-field authoring arm of the [`SceneApi`] facade: attaching
//! raymarched SDF primitives (sphere / box / plane) to nodes. A child module so
//! neither `impl SceneApi` block exceeds the engine's impl-block size budget.

use axiom_kernel::Meters;
use axiom_math::{MathApi, Vec3};

use super::SceneApi;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;
use crate::sdf_shape::SdfShape;

impl SceneApi {
    /// Attach a raymarched SDF **sphere** of `radius` and linear-RGB `color` to
    /// `node`. The sphere is placed by the node's world transform, like a
    /// renderable. `radius` must be positive and `color` finite + non-negative.
    pub fn add_sdf_sphere(
        &mut self,
        math: &MathApi,
        node: SceneNodeId,
        radius: Meters,
        color: Vec3,
    ) -> SceneResult<()> {
        SdfShape::sphere(math, radius, color).and_then(|shape| self.scene.add_sdf_shape(node, shape))
    }

    /// Attach a raymarched axis-aligned SDF **box** of `half_extents` and
    /// linear-RGB `color` to `node`. Every half-extent must be positive and
    /// finite, and `color` finite + non-negative.
    pub fn add_sdf_box(
        &mut self,
        math: &MathApi,
        node: SceneNodeId,
        half_extents: Vec3,
        color: Vec3,
    ) -> SceneResult<()> {
        SdfShape::cuboid(math, half_extents, color)
            .and_then(|shape| self.scene.add_sdf_shape(node, shape))
    }

    /// Attach a raymarched SDF ground **plane** (`y = 0` in the node's local
    /// space) of linear-RGB `color` to `node`. `color` must be finite +
    /// non-negative.
    pub fn add_sdf_plane(
        &mut self,
        math: &MathApi,
        node: SceneNodeId,
        color: Vec3,
    ) -> SceneResult<()> {
        SdfShape::plane(math, color).and_then(|shape| self.scene.add_sdf_shape(node, shape))
    }

    /// Remove the SDF shape on `node`.
    pub fn remove_sdf_shape(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.scene.remove_sdf_shape(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;
    use axiom_math::Transform;

    fn math() -> MathApi {
        MathApi::new()
    }

    #[test]
    fn add_each_sdf_kind_then_remove() {
        let mut api = SceneApi::new();
        let n = api.create_node_with_transform(Transform::IDENTITY);
        api.add_sdf_sphere(&math(), n, Meters::new(1.0).unwrap(), Vec3::ONE)
            .unwrap();
        // The shape reaches the snapshot as a sphere keyed by its node.
        assert_eq!(api.snapshot().sdf_shapes().len(), 1);
        assert_eq!(api.snapshot().sdf_shapes()[0].kind(), SdfShape::SPHERE);
        // Re-authoring the same node replaces the shape (the column overwrites).
        api.add_sdf_box(&math(), n, Vec3::new(1.0, 2.0, 3.0), Vec3::ONE)
            .unwrap();
        assert_eq!(api.snapshot().sdf_shapes()[0].kind(), SdfShape::BOX);
        api.remove_sdf_shape(n).unwrap();
        assert!(api.snapshot().sdf_shapes().is_empty());

        let p = api.create_node_with_transform(Transform::IDENTITY);
        api.add_sdf_plane(&math(), p, Vec3::ZERO).unwrap();
        assert_eq!(api.snapshot().sdf_shapes()[0].kind(), SdfShape::PLANE);
    }

    #[test]
    fn invalid_parameters_propagate_through_each_constructor() {
        let mut api = SceneApi::new();
        let n = api.create_node_with_transform(Transform::IDENTITY);
        assert_eq!(
            api.add_sdf_sphere(&math(), n, Meters::new(0.0).unwrap(), Vec3::ONE)
                .unwrap_err()
                .code(),
            SceneErrorCode::InvalidSdfShapeParameters
        );
        assert_eq!(
            api.add_sdf_box(&math(), n, Vec3::new(-1.0, 1.0, 1.0), Vec3::ONE)
                .unwrap_err()
                .code(),
            SceneErrorCode::InvalidSdfShapeParameters
        );
        assert_eq!(
            api.add_sdf_plane(&math(), n, Vec3::new(f32::NAN, 0.0, 0.0))
                .unwrap_err()
                .code(),
            SceneErrorCode::InvalidSdfShapeParameters
        );
    }

    #[test]
    fn missing_node_and_absent_removal_are_rejected() {
        let mut api = SceneApi::new();
        let absent = SceneNodeId::from_raw(999);
        assert_eq!(
            api.add_sdf_sphere(&math(), absent, Meters::new(1.0).unwrap(), Vec3::ONE)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
        assert_eq!(
            api.remove_sdf_shape(absent).unwrap_err().code(),
            SceneErrorCode::MissingSdfShape
        );
    }
}
