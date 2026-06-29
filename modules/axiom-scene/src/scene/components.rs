//! Component attachment on scene nodes — the camera, light, and renderable
//! facts keyed by node entity. A child module so it reaches `Scene`'s private
//! internals while keeping `scene.rs` within the per-file size budget, and so
//! neither `impl Scene` block exceeds the engine's impl-block size budget.

use axiom_kernel::Reflect;

use super::Scene;
use crate::camera::Camera;
use crate::light::Light;
use crate::renderable::Renderable;
use crate::scene_error::SceneError;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;
use crate::tag::Tag;

impl Scene {
    pub(crate) fn add_camera(&mut self, node: SceneNodeId, camera: Camera) -> SceneResult<()> {
        self.is_node(node)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("add_camera: node id not in scene"))
            .map(|()| {
                self.world
                    .storage_mut()
                    .cameras
                    .insert(Self::entity(node), camera);
            })
    }

    pub(crate) fn camera(&self, node: SceneNodeId) -> Option<&Camera> {
        self.world.storage().cameras.get(Self::entity(node))
    }

    pub(crate) fn remove_camera(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.world
            .storage_mut()
            .cameras
            .remove(Self::entity(node))
            .map(|_| ())
            .ok_or_else(|| SceneError::missing_camera("remove_camera: node has no camera"))
    }

    pub(crate) fn add_light(&mut self, node: SceneNodeId, light: Light) -> SceneResult<()> {
        self.is_node(node)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("add_light: node id not in scene"))
            .map(|()| {
                self.world
                    .storage_mut()
                    .lights
                    .insert(Self::entity(node), light);
            })
    }

    pub(crate) fn remove_light(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.world
            .storage_mut()
            .lights
            .remove(Self::entity(node))
            .map(|_| ())
            .ok_or_else(|| SceneError::missing_light("remove_light: node has no light"))
    }

    pub(crate) fn add_renderable(
        &mut self,
        node: SceneNodeId,
        renderable: Renderable,
    ) -> SceneResult<()> {
        self.is_node(node)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("add_renderable: node id not in scene"))
            .map(|()| {
                self.world
                    .storage_mut()
                    .renderables
                    .insert(Self::entity(node), renderable);
            })
    }

    pub(crate) fn remove_renderable(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.world
            .storage_mut()
            .renderables
            .remove(Self::entity(node))
            .map(|_| ())
            .ok_or_else(|| {
                SceneError::missing_renderable("remove_renderable: node has no renderable")
            })
    }

    pub(crate) fn set_renderable_visible(
        &mut self,
        node: SceneNodeId,
        visible: bool,
    ) -> SceneResult<()> {
        self.world
            .storage_mut()
            .renderables
            .get_mut(Self::entity(node))
            .map(|r| r.set_visible(visible))
            .ok_or_else(|| {
                SceneError::missing_renderable("set_renderable_visible: node has no renderable")
            })
    }

    /// Mark whether the renderable on `node` casts a contact shadow (a discrete
    /// dynamic object opts in; level geometry stays `false`).
    pub(crate) fn set_renderable_casts_contact_shadow(
        &mut self,
        node: SceneNodeId,
        casts: bool,
    ) -> SceneResult<()> {
        self.world
            .storage_mut()
            .renderables
            .get_mut(Self::entity(node))
            .map(|r| r.set_casts_contact_shadow(casts))
            .ok_or_else(|| {
                SceneError::missing_renderable(
                    "set_renderable_casts_contact_shadow: node has no renderable",
                )
            })
    }

    /// Attach (or replace) the coarse semantic kind on `node` — what the thing
    /// *is*, the classification a perceiving agent reads off a hit.
    pub(crate) fn add_tag(&mut self, node: SceneNodeId, kind_code: u32) -> SceneResult<()> {
        self.is_node(node)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("add_tag: node id not in scene"))
            .map(|()| {
                self.world
                    .storage_mut()
                    .tags
                    .insert(Self::entity(node), Tag::new(kind_code));
            })
    }

    /// The coarse kind code tagged on `node`, if any — the typed read behind a
    /// consumer's `get::<Tag>()`, letting it classify a raycast / overlap hit.
    pub(crate) fn tag_of(&self, node: SceneNodeId) -> Option<u32> {
        self.world
            .storage()
            .tags
            .get(Self::entity(node))
            .map(Tag::kind_code)
    }

    /// Every node tagged with `kind_code`, in ascending node-id order — the
    /// enumeration behind a consumer's "find all of kind K" query.
    pub(crate) fn tagged_nodes(&self, kind_code: u32) -> Vec<SceneNodeId> {
        self.world
            .storage()
            .tags
            .iter()
            .filter(|(_, tag)| tag.kind_code() == kind_code)
            .map(|(entity, _)| SceneNodeId::from_raw(entity.raw()))
            .collect()
    }

    /// Set (or replace) `node`'s app-defined component of type `T`, stored
    /// type-erased in the scene's dynamic arm. Returns whether `node` named a live
    /// node; a stale handle is a clean `false`, so a dynamic component is never
    /// attached to a dead/absent entity (which keeps every `query_dynamic` result
    /// live, since despawn clears the entity's dynamic components).
    pub(crate) fn set_dynamic<T: Reflect>(&mut self, node: SceneNodeId, value: T) -> bool {
        self.is_node(node)
            .then(|| {
                self.world
                    .storage_mut()
                    .dynamic
                    .insert(Self::entity(node), value);
            })
            .is_some()
    }

    /// Read `node`'s dynamic component of type `T`, deserialized to an owned value
    /// — `None` if absent, or if the stored bytes do not decode as `T` (a graceful
    /// miss, never UB).
    pub(crate) fn get_dynamic<T: Reflect>(&self, node: SceneNodeId) -> Option<T> {
        self.world
            .storage()
            .dynamic
            .get(Self::entity(node))
            .ok()
            .flatten()
    }

    /// Whether `node` carries a dynamic component of type `T`.
    pub(crate) fn has_dynamic<T: Reflect>(&self, node: SceneNodeId) -> bool {
        self.world
            .storage()
            .dynamic
            .contains::<T>(Self::entity(node))
    }

    /// Remove `node`'s dynamic component of type `T`, returning whether it existed.
    pub(crate) fn remove_dynamic<T: Reflect>(&mut self, node: SceneNodeId) -> bool {
        self.world
            .storage_mut()
            .dynamic
            .remove::<T>(Self::entity(node))
    }

    /// Every node carrying *all* the dynamic component kinds named in `kinds` (by
    /// `Reflect` schema name), in ascending node-id order — the dynamic mirror of a
    /// typed `query`, behind a retained world's `query(...kinds)`.
    pub(crate) fn query_dynamic(&self, kinds: &[&'static str]) -> Vec<SceneNodeId> {
        self.world
            .storage()
            .dynamic
            .entities_with_all(kinds)
            .into_iter()
            .map(|entity| SceneNodeId::from_raw(entity.raw()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_ref::MaterialRef;
    use crate::mesh_ref::MeshRef;
    use crate::scene_error_code::SceneErrorCode;
    use axiom_kernel::{Meters, Radians, Ratio};
    use axiom_math::{MathApi, Transform, Vec3};

    fn math() -> MathApi {
        MathApi::new()
    }

    fn node(s: &mut Scene) -> SceneNodeId {
        s.create_node(Transform::IDENTITY)
    }

    #[test]
    fn add_and_remove_camera() {
        let mut s = Scene::new();
        let n = node(&mut s);
        let cam = Camera::perspective(
            &math(),
            Radians::new(std::f32::consts::FRAC_PI_2).unwrap(),
            Ratio::new(1.0).unwrap(),
            Meters::new(0.1).unwrap(),
            Meters::new(100.0).unwrap(),
        )
        .unwrap();
        s.add_camera(n, cam).unwrap();
        assert_eq!(s.camera_count(), 1);
        // Missing node.
        assert_eq!(
            s.add_camera(SceneNodeId::from_raw(99), cam)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
        s.remove_camera(n).unwrap();
        assert_eq!(s.camera_count(), 0);
        // Removing absent camera fails.
        assert_eq!(
            s.remove_camera(n).unwrap_err().code(),
            SceneErrorCode::MissingCamera
        );
    }

    #[test]
    fn add_and_remove_light() {
        let mut s = Scene::new();
        let n = node(&mut s);
        let l = Light::directional(&math(), Vec3::ONE, Ratio::new(1.0).unwrap()).unwrap();
        s.add_light(n, l).unwrap();
        assert_eq!(s.light_count(), 1);
        assert_eq!(
            s.add_light(SceneNodeId::from_raw(99), l)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
        s.remove_light(n).unwrap();
        assert_eq!(
            s.remove_light(n).unwrap_err().code(),
            SceneErrorCode::MissingLight
        );
    }

    #[test]
    fn add_remove_and_toggle_renderable() {
        let mut s = Scene::new();
        let n = node(&mut s);
        let r = Renderable::new(MeshRef::from_raw(1), MaterialRef::from_raw(2)).unwrap();
        s.add_renderable(n, r).unwrap();
        assert_eq!(s.renderable_count(), 1);
        assert_eq!(
            s.add_renderable(SceneNodeId::from_raw(99), r)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
        // Toggle visibility + contact-shadow casting, present + missing. (The
        // caster value flowing through to a snapshot is asserted in the
        // render-pipeline tests.)
        s.set_renderable_visible(n, false).unwrap();
        s.set_renderable_casts_contact_shadow(n, true).unwrap();
        assert_eq!(
            s.set_renderable_visible(SceneNodeId::from_raw(99), true)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingRenderable
        );
        assert_eq!(
            s.set_renderable_casts_contact_shadow(SceneNodeId::from_raw(99), true)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingRenderable
        );
        s.remove_renderable(n).unwrap();
        assert_eq!(
            s.remove_renderable(n).unwrap_err().code(),
            SceneErrorCode::MissingRenderable
        );
    }

    #[test]
    fn add_tag_reads_back_and_enumerates_by_kind() {
        let mut s = Scene::new();
        let wall = node(&mut s);
        let enemy_a = node(&mut s);
        let enemy_b = node(&mut s);
        s.add_tag(wall, 1).unwrap();
        s.add_tag(enemy_a, 2).unwrap();
        s.add_tag(enemy_b, 2).unwrap();

        // Read back a single node's kind (present + untagged node).
        assert_eq!(s.tag_of(wall), Some(1));
        assert_eq!(s.tag_of(enemy_a), Some(2));
        let untagged = node(&mut s);
        assert_eq!(s.tag_of(untagged), None);

        // Enumerate by kind (deterministic ascending id order).
        assert_eq!(s.tagged_nodes(2), vec![enemy_a, enemy_b]);
        assert_eq!(s.tagged_nodes(1), vec![wall]);
        assert!(s.tagged_nodes(99).is_empty());

        // Re-tagging replaces the kind.
        s.add_tag(enemy_a, 1).unwrap();
        assert_eq!(s.tag_of(enemy_a), Some(1));

        // Tagging a missing node is a clean error.
        assert_eq!(
            s.add_tag(SceneNodeId::from_raw(9999), 5).unwrap_err().code(),
            SceneErrorCode::MissingNode
        );
    }
}
