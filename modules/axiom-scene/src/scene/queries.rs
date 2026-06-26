//! Bounding-volume spatial queries — raycast and box overlap — over the scene.
//!
//! Category 2 of the game vocabulary (`docs/game-vocabulary.md`): the engine
//! answers "what is where" so an app never reimplements geometry. These are
//! spatial *queries* (picking, line-of-sight, overlap) over scene bounding
//! volumes — not physics. Split out of `scene.rs` to keep that file within the
//! engine's per-file size budget; as a child module it still reaches `Scene`'s
//! private internals.

use axiom_ecs::Query;
use axiom_kernel::EntityId;
use axiom_math::{Aabb, Ray, Transform, Vec3};

use super::Scene;
use crate::bounds::Bounds;
use crate::scene_error::SceneError;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;

impl Scene {
    /// Attach an axis-aligned bounding volume to `node`.
    pub(crate) fn add_bounds(&mut self, node: SceneNodeId, bounds: Bounds) -> SceneResult<()> {
        self.is_node(node)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("add_bounds: node id not in scene"))
            .map(|()| {
                self.world
                    .storage_mut()
                    .bounds
                    .insert(Self::entity(node), bounds);
            })
    }

    /// The half-extents of `node`'s bounding volume, if it has one — the typed
    /// read behind `get::<Bounds>()`.
    pub(crate) fn bounds_half_extents(&self, node: SceneNodeId) -> Option<Vec3> {
        self.world
            .storage()
            .bounds
            .get(Self::entity(node))
            .map(Bounds::half_extents)
    }

    /// Every bounded node's `(id, half-extents)`, in ascending node-id order — the
    /// enumeration behind `query::<Bounds>()`.
    pub(crate) fn bounded_nodes(&self) -> Vec<(SceneNodeId, Vec3)> {
        self.world
            .storage()
            .bounds
            .iter()
            .map(|(entity, bounds)| (SceneNodeId::from_raw(entity.raw()), bounds.half_extents()))
            .collect()
    }

    /// Remove the bounding volume on `node`.
    pub(crate) fn remove_bounds(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.world
            .storage_mut()
            .bounds
            .remove(Self::entity(node))
            .map(|_| ())
            .ok_or_else(|| SceneError::missing_bounds("remove_bounds: node has no bounds"))
    }

    /// The world-space box for `bounds` placed by world transform `world`, or
    /// `None` when the world scale yields a degenerate (negative/non-finite) box.
    /// Centered at the world translation; half-extents scaled by the world scale
    /// (rotation is not modeled in v1). The world transform is supplied by the
    /// caller's [`Query::two`] join of the `bounds` and `worlds` columns, so a node
    /// without a propagated world transform never reaches here.
    fn world_aabb(world: &Transform, bounds: &Bounds) -> Option<Aabb> {
        let he = bounds.half_extents();
        let extents = Vec3::new(
            world.scale.x * he.x,
            world.scale.y * he.y,
            world.scale.z * he.z,
        );
        Aabb::from_center_extents(world.translation, extents).ok()
    }

    /// The nearest bounded node whose box the ray enters within `max_distance`,
    /// or `None`. A `fold` over every bounded node that keeps the closest entry
    /// distance — the branchless nearest-of-many pattern. A zero/non-finite
    /// `direction` yields `None` (no ray).
    pub(crate) fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
    ) -> Option<SceneNodeId> {
        let storage = self.world.storage();
        Ray::new(origin, direction).ok().and_then(|ray| {
            // The live `bounds` × `worlds` join: only nodes with both a bounding
            // volume and a propagated world transform are candidates.
            Query::two(self.world.entities(), &storage.bounds, &storage.worlds)
                .fold(
                    None,
                    |best: Option<(EntityId, f32)>, (entity, bounds, world)| {
                        let hit = Self::world_aabb(world, bounds)
                            .and_then(|aabb| ray.intersect_aabb_entry(&aabb))
                            .filter(|&t| t <= max_distance)
                            .map(|t| (entity, t));
                        // Keep the nearer of `best` and `hit` with no branch: when
                        // both are present, index a 2-element table by the compare.
                        best.map_or(hit, |b| {
                            Some(hit.map_or(b, |h| [b, h][usize::from(h.1 < b.1)]))
                        })
                    },
                )
                .map(|(entity, _)| SceneNodeId::from_raw(entity.raw()))
        })
    }

    /// The player index marked on `node`, if any. Lets a caller classify a
    /// raycast / overlap hit as a player-marked actor (e.g. an enemy) versus
    /// plain geometry, without the un-nameable node id crossing the facade.
    pub(crate) fn player_index(&self, node: SceneNodeId) -> Option<u32> {
        self.world
            .storage()
            .players
            .get(&Self::entity(node))
            .copied()
    }

    /// Every bounded node whose world box overlaps the query box (centered at
    /// `center`, of `half_extents`), in ascending node-id order. A degenerate
    /// query box yields an empty result.
    pub(crate) fn overlap_box(&self, center: Vec3, half_extents: Vec3) -> Vec<SceneNodeId> {
        let storage = self.world.storage();
        Aabb::from_center_extents(center, half_extents)
            .ok()
            .map(|query| {
                Query::two(self.world.entities(), &storage.bounds, &storage.worlds)
                    .filter_map(|(entity, bounds, world)| {
                        Self::world_aabb(world, bounds)
                            .filter(|aabb| aabb.overlaps(&query))
                            .map(|_| SceneNodeId::from_raw(entity.raw()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;
    use axiom_kernel::{BinaryReader, BinaryWriter};
    use axiom_math::Transform;

    /// A unit-cube bounds (half-extent 0.5 on each axis -> a 1×1×1 box).
    fn unit() -> Bounds {
        Bounds::new(Vec3::new(0.5, 0.5, 0.5))
    }

    /// A node at `(x, y, z)` with the identity scale.
    fn at(s: &mut Scene, x: f32, y: f32, z: f32) -> SceneNodeId {
        s.create_node(Transform::from_translation(Vec3::new(x, y, z)))
    }

    #[test]
    fn add_bounds_present_and_missing_node() {
        let mut s = Scene::new();
        let n = at(&mut s, 0.0, 0.0, 0.0);
        s.add_bounds(n, unit()).unwrap();
        assert_eq!(
            s.add_bounds(SceneNodeId::from_raw(99), unit())
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn remove_bounds_present_and_missing() {
        let mut s = Scene::new();
        let n = at(&mut s, 0.0, 0.0, 0.0);
        s.add_bounds(n, unit()).unwrap();
        s.remove_bounds(n).unwrap();
        // Removing again is a missing-bounds error.
        assert_eq!(
            s.remove_bounds(n).unwrap_err().code(),
            SceneErrorCode::MissingBounds
        );
    }

    #[test]
    fn raycast_requires_propagation_then_hits_the_nearest() {
        let mut s = Scene::new();
        // Two boxes on +X at x=3 and x=6; `near` is created first (lower id).
        let near = at(&mut s, 3.0, 0.0, 0.0);
        let far = at(&mut s, 6.0, 0.0, 0.0);
        s.add_bounds(near, unit()).unwrap();
        s.add_bounds(far, unit()).unwrap();
        let dir = Vec3::new(1.0, 0.0, 0.0);
        // Before propagation no node has a world transform -> nothing queryable.
        assert_eq!(s.raycast(Vec3::ZERO, dir, 100.0), None);
        s.update_world_transforms();
        // The nearer box is returned (far is kept as best then never beats it).
        assert_eq!(s.raycast(Vec3::ZERO, dir, 100.0), Some(near));
    }

    #[test]
    fn raycast_picks_the_closer_box_even_when_iterated_later() {
        let mut s = Scene::new();
        // `far` is created first (iterated first); `near` is closer but later.
        let far = at(&mut s, 6.0, 0.0, 0.0);
        let near = at(&mut s, 3.0, 0.0, 0.0);
        s.add_bounds(far, unit()).unwrap();
        s.add_bounds(near, unit()).unwrap();
        s.update_world_transforms();
        // Exercises the "closer hit replaces the running best" table index.
        assert_eq!(
            s.raycast(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 100.0),
            Some(near)
        );
    }

    #[test]
    fn raycast_zero_direction_out_of_range_and_miss_are_none() {
        let mut s = Scene::new();
        let n = at(&mut s, 3.0, 0.0, 0.0);
        s.add_bounds(n, unit()).unwrap();
        s.update_world_transforms();
        // Zero direction -> no ray.
        assert_eq!(s.raycast(Vec3::ZERO, Vec3::ZERO, 100.0), None);
        // Box enters at t=2.5 but max_distance 1.0 filters it out.
        assert_eq!(s.raycast(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 1.0), None);
        // Aimed away from the box.
        assert_eq!(s.raycast(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), 100.0), None);
    }

    #[test]
    fn degenerate_bounds_are_skipped_by_both_queries() {
        let mut s = Scene::new();
        let n = at(&mut s, 0.0, 0.0, 0.0);
        // Negative half-extents -> no valid world box -> skipped.
        s.add_bounds(n, Bounds::new(Vec3::new(-1.0, -1.0, -1.0)))
            .unwrap();
        s.update_world_transforms();
        assert_eq!(s.raycast(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 100.0), None);
        assert!(s
            .overlap_box(Vec3::ZERO, Vec3::new(0.5, 0.5, 0.5))
            .is_empty());
    }

    #[test]
    fn overlap_box_includes_overlapping_excludes_distant_and_rejects_degenerate_query() {
        let mut s = Scene::new();
        let near = at(&mut s, 0.0, 0.0, 0.0);
        let far = at(&mut s, 10.0, 0.0, 0.0);
        s.add_bounds(near, unit()).unwrap();
        s.add_bounds(far, unit()).unwrap();
        s.update_world_transforms();
        // A query box around the origin overlaps `near` only.
        assert_eq!(
            s.overlap_box(Vec3::ZERO, Vec3::new(0.6, 0.6, 0.6)),
            vec![near]
        );
        // A degenerate query box (negative half-extent) yields nothing.
        assert!(s
            .overlap_box(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0))
            .is_empty());
    }

    #[test]
    fn bounds_survive_a_state_round_trip() {
        let mut s = Scene::new();
        let n = at(&mut s, 1.0, 2.0, 3.0);
        s.add_bounds(n, Bounds::new(Vec3::new(0.5, 0.75, 1.0)))
            .unwrap();
        let mut w = BinaryWriter::new();
        s.write_state(&mut w);
        let bytes = w.into_bytes();
        let mut restored = Scene::new();
        restored.read_state(&mut BinaryReader::new(&bytes)).unwrap();
        restored.update_world_transforms();
        // The restored bounds answer the same overlap query.
        assert_eq!(
            restored.overlap_box(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.1, 0.1, 0.1)),
            vec![n]
        );
    }
}
