//! Vertex projection: model-space position → device-pixel screen point + depth.
//!
//! A column-major `mvp` (`projection * view * world`, the backend-neutral form
//! the frame packet carries) transforms a vertex to clip space; a perspective
//! divide yields NDC; an NDC→pixel map (with the y axis flipped, since NDC up is
//! screen down) yields the device-pixel point. A vertex at or behind the near
//! plane (clip `w <= eps`) returns `None` so the rasterizer culls its triangle.

/// Clip-space `w` at or below which a vertex is treated as behind the near plane
/// and culled.
const NEAR_W_EPS: f32 = 1e-6;

/// Project `position` through column-major `mvp` into the `width`×`height`
/// viewport, returning `[screen_x, screen_y, ndc_depth]` in device pixels (depth
/// is the NDC z, larger = farther), or `None` when the vertex is at/behind the
/// near plane.
pub(crate) fn project_vertex(
    mvp: &[f32; 16],
    position: [f32; 3],
    width: u32,
    height: u32,
) -> Option<[f32; 3]> {
    let [x, y, z] = position;
    let clip_x = mvp[0] * x + mvp[4] * y + mvp[8] * z + mvp[12];
    let clip_y = mvp[1] * x + mvp[5] * y + mvp[9] * z + mvp[13];
    let clip_z = mvp[2] * x + mvp[6] * y + mvp[10] * z + mvp[14];
    let clip_w = mvp[3] * x + mvp[7] * y + mvp[11] * z + mvp[15];
    (clip_w > NEAR_W_EPS).then(|| {
        let inv = 1.0 / clip_w;
        let ndc_x = clip_x * inv;
        let ndc_y = clip_y * inv;
        let ndc_z = clip_z * inv;
        let screen_x = (ndc_x * 0.5 + 0.5) * width as f32;
        // NDC up is +y; screen down is +y — flip.
        let screen_y = (ndc_y * -0.5 + 0.5) * height as f32;
        [screen_x, screen_y, ndc_z]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Column-major identity matrix.
    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    #[test]
    fn identity_projects_origin_to_screen_centre() {
        // Identity → clip = (0,0,0,1) → ndc (0,0,0) → centre of an 800x600 target.
        let p = project_vertex(&IDENTITY, [0.0, 0.0, 0.0], 800, 600).expect("in front");
        assert_eq!(p, [400.0, 300.0, 0.0]);
    }

    #[test]
    fn ndc_corners_map_to_pixel_corners_with_y_flipped() {
        // (+1,+1) NDC → top-right pixel (x=width, y=0).
        let tr = project_vertex(&IDENTITY, [1.0, 1.0, 0.0], 800, 600).expect("in front");
        assert_eq!(tr, [800.0, 0.0, 0.0]);
        // (-1,-1) NDC → bottom-left pixel (x=0, y=height).
        let bl = project_vertex(&IDENTITY, [-1.0, -1.0, 0.0], 800, 600).expect("in front");
        assert_eq!(bl, [0.0, 600.0, 0.0]);
    }

    #[test]
    fn depth_is_the_ndc_z() {
        // A column-major matrix that scales z by 0.5 leaves w=1, so ndc_z = 0.5z.
        let mut m = IDENTITY;
        m[10] = 0.5;
        let p = project_vertex(&m, [0.0, 0.0, 1.0], 100, 100).expect("in front");
        assert_eq!(p[2], 0.5);
    }

    #[test]
    fn vertex_behind_the_near_plane_is_culled() {
        // An all-zero matrix yields clip_w = 0 (<= eps) → None.
        let zero = [0.0_f32; 16];
        assert!(project_vertex(&zero, [1.0, 2.0, 3.0], 100, 100).is_none());
    }
}
