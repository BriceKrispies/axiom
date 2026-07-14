//! The world facade: the per-frame streaming + culling + LOD plan.

use axiom_kernel::Meters;
use axiom_math::{Aabb, Mat4, Vec3};
use axiom_streaming::{ChunkCoord, Residency};
use axiom_visibility::VisibilityApi;

use crate::ids::{VisibleChunk, WorldConfig, WorldFramePlan};

/// A streaming world's per-frame planner.
///
/// Stateful only in its residency ring; every frame's plan is otherwise a pure
/// function of the camera and the [`WorldConfig`], so a replayed camera path
/// produces byte-identical plans.
#[derive(Debug)]
pub struct WorldApi {
    config: WorldConfig,
    residency: Residency,
}

impl WorldApi {
    /// A new world with nothing loaded yet.
    pub fn new(config: WorldConfig) -> Self {
        Self {
            config,
            residency: Residency::new(),
        }
    }

    /// The chunk whose square contains ground position `(x, z)` metres.
    pub fn focus_chunk(&self, x: Meters, z: Meters) -> ChunkCoord {
        let size = self.config.chunk_size.get();
        ChunkCoord::new(
            (x.get() / size).floor() as i32,
            (z.get() / size).floor() as i32,
        )
    }

    /// Plan one frame.
    ///
    /// Advances the residency ring to the camera's focus chunk (yielding the
    /// `load` / `unload` delta), then frustum-culls + LOD-rates every resident
    /// chunk against the camera to yield the `visible` set. `chunk_aabb` gives
    /// each chunk's world-space bounding box — the caller owns terrain, so it
    /// decides the box's vertical extent (e.g. ground range + tallest content).
    pub fn frame_plan(
        &mut self,
        camera: Vec3,
        view_proj: Mat4,
        chunk_aabb: impl Fn(ChunkCoord) -> Aabb,
    ) -> WorldFramePlan {
        let focus = self.focus_chunk(
            Meters::finite_or_zero(camera.x),
            Meters::finite_or_zero(camera.z),
        );
        let delta =
            self.residency
                .apply(focus, self.config.load_radius, self.config.margin, |_| {
                    false
                });
        let resident = self.residency.resident_coords();
        let boxes: Vec<Aabb> = resident.iter().map(|c| chunk_aabb(*c)).collect();
        let mask = VisibilityApi::visible_mask(view_proj, &boxes);
        let lods = VisibilityApi::lod_levels(camera, &boxes, &self.config.lod_bands);
        let visible: Vec<VisibleChunk> = resident
            .iter()
            .zip(mask)
            .zip(lods)
            .filter(|((_, keep), _)| *keep)
            .map(|((coord, _), lod)| VisibleChunk { coord: *coord, lod })
            .collect();
        WorldFramePlan {
            load: delta.load,
            unload: delta.unload,
            visible,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(chunk_size: f32, load_radius: i32, margin: i32, bands: &[f32]) -> WorldConfig {
        WorldConfig {
            chunk_size: Meters::finite_or_zero(chunk_size),
            load_radius,
            margin,
            lod_bands: bands.iter().map(|b| Meters::finite_or_zero(*b)).collect(),
        }
    }

    /// A short box centred on each chunk's ground square (unwrap is fine in tests).
    fn boxer(size: f32) -> impl Fn(ChunkCoord) -> Aabb {
        move |c| {
            let cx = (c.x as f32 + 0.5) * size;
            let cz = (c.z as f32 + 0.5) * size;
            Aabb::from_center_extents(
                Vec3::new(cx, 0.0, cz),
                Vec3::new(size * 0.5, 2.0, size * 0.5),
            )
            .unwrap()
        }
    }

    /// A perspective camera at `eye` looking toward `target`, as clip-from-world.
    fn view_proj(eye: Vec3, target: Vec3) -> Mat4 {
        let proj = Mat4::perspective(std::f32::consts::FRAC_PI_3, 1.0, 0.1, 1000.0).unwrap();
        let view = Mat4::look_at(eye, target, Vec3::UNIT_Y).unwrap();
        proj.multiply(view)
    }

    #[test]
    fn focus_chunk_maps_ground_position_to_its_cell() {
        let w = WorldApi::new(config(10.0, 1, 1, &[]));
        assert_eq!(
            w.focus_chunk(Meters::finite_or_zero(5.0), Meters::finite_or_zero(25.0)),
            ChunkCoord::new(0, 2)
        );
        // Negative coordinates floor toward −∞ (cell −1, not 0).
        assert_eq!(
            w.focus_chunk(Meters::finite_or_zero(-3.0), Meters::finite_or_zero(-11.0)),
            ChunkCoord::new(-1, -2)
        );
    }

    #[test]
    fn first_frame_loads_the_ring_and_sees_chunks_ahead() {
        let size = 10.0;
        let mut w = WorldApi::new(config(size, 1, 1, &[]));
        // Camera near the origin looking down −Z into the forest.
        let plan = w.frame_plan(
            Vec3::new(5.0, 2.0, 5.0),
            view_proj(Vec3::new(5.0, 2.0, 5.0), Vec3::new(5.0, 2.0, -40.0)),
            boxer(size),
        );
        // Focus chunk (0,0); radius-1 ring = 9 chunks loaded, nothing unloaded.
        assert_eq!(plan.load.len(), 9);
        assert!(plan.unload.is_empty());
        // Some chunks are in front (visible) and some behind (culled): a strict,
        // non-empty subset of the 9 resident chunks.
        assert!(!plan.visible.is_empty());
        assert!(plan.visible.len() < 9);
    }

    #[test]
    fn moving_forward_loads_ahead_and_unloads_behind() {
        let size = 10.0;
        let mut w = WorldApi::new(config(size, 1, 0, &[])); // margin 0 → eager eviction
        let vp = view_proj(Vec3::new(5.0, 2.0, 5.0), Vec3::new(5.0, 2.0, -40.0));
        w.frame_plan(Vec3::new(5.0, 2.0, 5.0), vp, boxer(size)); // focus (0,0)
                                                                 // Move one chunk east (+X): focus (1,0). New east column loads, west unloads.
        let plan = w.frame_plan(Vec3::new(15.0, 2.0, 5.0), vp, boxer(size));
        assert_eq!(plan.load.len(), 3); // new column x=2
        assert_eq!(plan.unload.len(), 3); // old column x=-1 exits the keep square
    }

    #[test]
    fn lod_rises_with_distance_for_visible_chunks() {
        let size = 10.0;
        // Bands at 15 m and 35 m; a wide radius so distant chunks are still loaded.
        let mut w = WorldApi::new(config(size, 3, 1, &[15.0, 35.0]));
        let eye = Vec3::new(5.0, 2.0, 5.0);
        let plan = w.frame_plan(eye, view_proj(eye, Vec3::new(5.0, 2.0, -60.0)), boxer(size));
        // At least one visible chunk sits in a farther LOD band than the nearest.
        let max_lod = plan.visible.iter().map(|v| v.lod).max().unwrap_or(0);
        assert!(max_lod >= 1);
    }

    #[test]
    fn world_is_debuggable() {
        let text = format!("{:?}", WorldApi::new(config(10.0, 1, 1, &[])));
        assert!(text.contains("WorldApi"));
    }
}
