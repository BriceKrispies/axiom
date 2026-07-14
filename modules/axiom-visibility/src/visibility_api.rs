//! The visibility facade: frustum culling + distance-banded LOD.

use axiom_kernel::Meters;
use axiom_math::{Aabb, Frustum, Mat4, Vec3};

/// Per-frame view-visibility queries over world-space bounding boxes.
///
/// Stateless: every query is a pure function of its arguments, so a frame's
/// visibility is deterministic and independently testable on native.
#[derive(Debug)]
pub struct VisibilityApi;

impl VisibilityApi {
    /// Frustum-cull `boxes` against the camera's clip-from-world matrix
    /// (`projection · view`). Returns one flag per box in the same order —
    /// `true` = keep (the box intersects the frustum), `false` = cull.
    ///
    /// A degenerate matrix from which no frustum can be extracted is treated
    /// **conservatively**: every box is kept, so culling can never wrongly hide
    /// geometry (it only ever removes provably-offscreen boxes).
    pub fn visible_mask(view_proj: Mat4, boxes: &[Aabb]) -> Vec<bool> {
        Frustum::from_view_projection(view_proj)
            .map(|frustum| boxes.iter().map(|b| frustum.intersects_aabb(b)).collect())
            .unwrap_or_else(|_| boxes.iter().map(|_| true).collect())
    }

    /// Level of detail for each box: the number of ascending distance `bands`
    /// (metres) that the camera→box-centre distance exceeds. `0` is the nearest /
    /// highest-detail level; each band the distance crosses steps the level up by
    /// one. With no bands every box is level `0`.
    pub fn lod_levels(camera: Vec3, boxes: &[Aabb], bands: &[Meters]) -> Vec<u8> {
        boxes
            .iter()
            .map(|b| {
                let dist = b.center().subtract(camera).length();
                bands.iter().filter(|band| dist > band.get()).count() as u8
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A perspective camera at `eye` looking at the origin, as a clip-from-world
    /// matrix. Looks down −Z when `eye` is on +Z.
    fn view_proj(eye: Vec3) -> Mat4 {
        let proj = Mat4::perspective(std::f32::consts::FRAC_PI_3, 1.0, 0.1, 1000.0).unwrap();
        let view = Mat4::look_at(eye, Vec3::ZERO, Vec3::UNIT_Y).unwrap();
        proj.multiply(view)
    }

    fn box_at(x: f32, y: f32, z: f32) -> Aabb {
        Aabb::from_center_extents(Vec3::new(x, y, z), Vec3::new(0.5, 0.5, 0.5)).unwrap()
    }

    #[test]
    fn keeps_a_box_in_front_and_culls_one_behind() {
        // Camera at +Z=5 looking toward the origin (down −Z). The origin box is in
        // view; a box far behind the camera (+Z) is outside the frustum.
        let vp = view_proj(Vec3::new(0.0, 0.0, 5.0));
        let mask =
            VisibilityApi::visible_mask(vp, &[box_at(0.0, 0.0, 0.0), box_at(0.0, 0.0, 60.0)]);
        assert_eq!(mask, vec![true, false]);
    }

    #[test]
    fn culls_a_box_off_to_the_side() {
        // A box far to the right of a narrow forward view is outside the frustum.
        let vp = view_proj(Vec3::new(0.0, 0.0, 5.0));
        let mask = VisibilityApi::visible_mask(vp, &[box_at(500.0, 0.0, 0.0)]);
        assert_eq!(mask, vec![false]);
    }

    #[test]
    fn degenerate_matrix_keeps_everything_conservatively() {
        // A zero matrix yields no extractable frustum → conservative all-visible,
        // preserving the box count.
        let zero = Mat4::from_cols_array([0.0; 16]);
        let mask =
            VisibilityApi::visible_mask(zero, &[box_at(0.0, 0.0, 0.0), box_at(9.0, 9.0, 9.0)]);
        assert_eq!(mask, vec![true, true]);
    }

    #[test]
    fn empty_boxes_yield_empty_outputs() {
        let vp = view_proj(Vec3::new(0.0, 0.0, 5.0));
        assert!(VisibilityApi::visible_mask(vp, &[]).is_empty());
        assert!(
            VisibilityApi::lod_levels(Vec3::ZERO, &[], &[Meters::finite_or_zero(10.0)]).is_empty()
        );
    }

    #[test]
    fn lod_steps_up_across_each_distance_band() {
        // Bands at 10 m and 20 m. Boxes at 5 m, 15 m, 30 m from the camera → levels
        // 0, 1, 2 (crossing 0, 1, and 2 bands respectively).
        let camera = Vec3::ZERO;
        let boxes = [
            box_at(0.0, 0.0, 5.0),
            box_at(0.0, 0.0, 15.0),
            box_at(0.0, 0.0, 30.0),
        ];
        let bands = [Meters::finite_or_zero(10.0), Meters::finite_or_zero(20.0)];
        assert_eq!(
            VisibilityApi::lod_levels(camera, &boxes, &bands),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn no_bands_means_every_box_is_level_zero() {
        let levels = VisibilityApi::lod_levels(Vec3::ZERO, &[box_at(0.0, 0.0, 500.0)], &[]);
        assert_eq!(levels, vec![0]);
    }
}
