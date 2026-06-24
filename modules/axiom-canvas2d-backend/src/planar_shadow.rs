//! Planar projected contact shadows — the depth-correct replacement for the old
//! screen-space "contact-shadow blob" post-pass.
//!
//! A screen-space ellipse under every important object's bounding box was wrong
//! twice over: it was applied to whatever the coverage heuristic called an
//! "object" (so mid-distance *walls* got ground-shadow ovals), and it never read
//! the depth buffer (so the oval painted up wall faces and over occluders). This
//! computes a *real* shadow instead, from the data the frame already carries:
//!
//! * only draws the scene explicitly marked as contact-shadow casters
//!   ([`axiom_host::FrameDrawItem::casts_contact_shadow`]) cast — level geometry
//!   never does;
//! * each caster's mesh geometry is projected, **along the directional light**,
//!   onto the ground plane (the lowest world-Y of the casters' own geometry —
//!   their contact level), in world space;
//! * the projected polygon is rasterized through the camera's `view_proj` and
//!   **depth-tested** against the finished scene depth buffer, so a wall in front
//!   occludes the shadow and the shadow lands on the floor, never on a wall.
//!
//! It runs after the main raster (the depth buffer is complete) and blends the
//! covered pixels toward black; it never writes depth. Pure, branchless,
//! deterministic — no browser types.

use axiom_host::FramePacket;

use crate::canvas_depth_cue::to_byte;
use crate::canvas_post_pass::clamp_axis;
use crate::depth_buffer::DepthBuffer;
use crate::mesh_cache::MeshCache;
use crate::projection::project_vertex;
use crate::software_framebuffer::SoftwareFramebuffer;

/// Signed screen area below which a projected shadow triangle is degenerate.
const AREA_EPS: f32 = 1e-6;
/// Minimum vertical component of the light's travel direction for a *ground*
/// shadow to exist; a (near-)horizontal light casts no shadow onto a floor.
const LIGHT_Y_EPS: f32 = 1e-3;

/// Project every contact-shadow caster in `packet` onto the ground plane along
/// the directional light, rasterize each depth-tested against `depth`, and blend
/// the covered framebuffer pixels toward black by `alpha`. `bias` (NDC) lets a
/// floor-coplanar shadow win the depth test against the floor. Returns
/// `(casters drawn, pixels darkened)`. A frame without a camera, without a
/// directional light, with a (near-)horizontal light, or with no casters draws
/// nothing.
pub(crate) fn apply_planar_shadows(
    fb: &mut SoftwareFramebuffer,
    depth: &DepthBuffer,
    packet: &FramePacket,
    cache: &MeshCache,
    alpha: f32,
    bias: f32,
) -> (u32, u64) {
    let (w, h) = (fb.width(), fb.height());
    let alpha = alpha.clamp(0.0, 1.0);
    // A ground shadow needs the camera view-projection and a directional light
    // with a usable vertical component; `and_then` collapses all three absent
    // cases (no camera / no directional light / horizontal light) to "no setup".
    let setup = packet.camera().map(|c| c.view_proj()).and_then(|view_proj| {
        packet
            .lights()
            .iter()
            .find(|l| l.kind() == 0)
            .map(|l| l.vec())
            .and_then(|to_light| {
                // The light *travels* opposite the to-light direction.
                let travel = [-to_light[0], -to_light[1], -to_light[2]];
                (travel[1].abs() > LIGHT_Y_EPS).then_some((view_proj, travel))
            })
    });
    setup
        .map(|(view_proj, travel)| {
            // Casters whose geometry the backend actually holds.
            let casters: Vec<_> = packet
                .draws()
                .iter()
                .filter(|d| d.casts_contact_shadow())
                .filter(|d| cache.get(d.mesh_id()).is_some())
                .collect();
            // Ground plane = the lowest world-Y of all caster geometry (their
            // contact level). `INFINITY` with no casters → the caster loop below
            // is empty anyway, so the value is never used.
            let ground = casters.iter().fold(f32::INFINITY, |acc, d| {
                let world = d.world();
                let geo = cache.get(d.mesh_id()).expect("casters filtered to present meshes");
                geo.indices().iter().fold(acc, |acc, &idx| {
                    acc.min(world_point(&world, geo.position(idx))[1])
                })
            });
            let rgba = fb.rgba_mut();
            casters.iter().fold((0_u32, 0_u64), |(count, pixels), d| {
                let world = d.world();
                let geo = cache.get(d.mesh_id()).expect("casters filtered to present meshes");
                let drawn = geo.indices().chunks_exact(3).fold(0_u64, |acc, tri| {
                    // Project each vertex: model → world → ground plane → screen.
                    let vertex = |k: usize| {
                        let world_pos = world_point(&world, geo.position(tri[k]));
                        let on_ground = project_to_ground(world_pos, travel, ground);
                        project_vertex(&view_proj, on_ground, w, h)
                    };
                    let drawn = vertex(0)
                        .zip(vertex(1))
                        .zip(vertex(2))
                        .map(|((a, b), c)| {
                            rasterize_shadow_triangle(rgba, depth, w, h, [a, b, c], alpha, bias)
                        })
                        .unwrap_or(0);
                    acc + drawn
                });
                (count + 1, pixels + drawn)
            })
        })
        .unwrap_or((0, 0))
}

/// Transform a model-space point by a column-major affine `world` matrix
/// (translation + rotation + scale), returning the world-space point. No
/// perspective — the `w` row is `[0,0,0,1]` for an affine transform.
fn world_point(world: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = p;
    [
        world[0] * x + world[4] * y + world[8] * z + world[12],
        world[1] * x + world[5] * y + world[9] * z + world[13],
        world[2] * x + world[6] * y + world[10] * z + world[14],
    ]
}

/// Project a world point onto the horizontal plane `y = ground` along the light's
/// `travel` direction: find `s` with `(p + s·travel).y == ground` and return
/// `p + s·travel`. Caller guarantees `travel.y` is non-zero (the `LIGHT_Y_EPS`
/// gate), so the divide is finite.
fn project_to_ground(p: [f32; 3], travel: [f32; 3], ground: f32) -> [f32; 3] {
    let s = (ground - p[1]) / travel[1];
    [p[0] + s * travel[0], ground, p[2] + s * travel[2]]
}

/// The edge function: twice the signed area of triangle `(a, b, p)`.
fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

/// Rasterize one projected shadow triangle (screen `[x, y, ndc_depth]` vertices)
/// into `rgba`, blending each covered, depth-passing pixel toward black by
/// `alpha`. Depth-tested against `depth` (a fragment passes iff it is no farther
/// than the stored surface plus `bias`) but never writes depth. Branchless: a
/// rejected pixel blends by `0` (a no-op). Returns the pixels darkened.
fn rasterize_shadow_triangle(
    rgba: &mut [u8],
    depth: &DepthBuffer,
    w: u32,
    h: u32,
    v: [[f32; 3]; 3],
    alpha: f32,
    bias: f32,
) -> u64 {
    let area = edge(v[0][0], v[0][1], v[1][0], v[1][1], v[2][0], v[2][1]);
    let valid = area.abs() > AREA_EPS;
    // Signed inverse area makes the barycentric inside-test winding-independent;
    // `0` for a degenerate triangle forces every `l_i` to 0 → no coverage.
    let inv_area = valid.then(|| 1.0 / area).unwrap_or(0.0);
    let xs = [v[0][0], v[1][0], v[2][0]];
    let ys = [v[0][1], v[1][1], v[2][1]];
    let minx = clamp_axis(xs.iter().copied().fold(f32::INFINITY, f32::min).floor(), w);
    let maxx = clamp_axis(xs.iter().copied().fold(f32::NEG_INFINITY, f32::max).ceil(), w);
    let miny = clamp_axis(ys.iter().copied().fold(f32::INFINITY, f32::min).floor(), h);
    let maxy = clamp_axis(ys.iter().copied().fold(f32::NEG_INFINITY, f32::max).ceil(), h);
    (miny..maxy + 1).fold(0_u64, |acc, py| {
        (minx..maxx + 1).fold(acc, |acc, px| {
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            let l0 = edge(v[1][0], v[1][1], v[2][0], v[2][1], fx, fy) * inv_area;
            let l1 = edge(v[2][0], v[2][1], v[0][0], v[0][1], fx, fy) * inv_area;
            let l2 = edge(v[0][0], v[0][1], v[1][0], v[1][1], fx, fy) * inv_area;
            let inside = valid & (l0 >= 0.0) & (l1 >= 0.0) & (l2 >= 0.0);
            let dep = l0 * v[0][2] + l1 * v[1][2] + l2 * v[2][2];
            let pass = inside & (dep <= depth.depth_at(px, py) + bias);
            let t = alpha * f32::from(u8::from(pass));
            blend_toward_black(rgba, (py as usize * w as usize + px as usize) * 4, t);
            acc + u64::from(pass)
        })
    })
}

/// Blend the RGB at byte offset `off` toward black by `t` (0 = keep, 1 = black),
/// preserving alpha. `t == 0` re-writes the same quantized value (a no-op). An
/// out-of-range offset is ignored.
fn blend_toward_black(rgba: &mut [u8], off: usize, t: f32) {
    rgba.get_mut(off..off + 3).into_iter().for_each(|p| {
        p[0] = to_byte(p[0] as f32 / 255.0 * (1.0 - t));
        p[1] = to_byte(p[1] as f32 / 255.0 * (1.0 - t));
        p[2] = to_byte(p[2] as f32 / 255.0 * (1.0 - t));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{
        FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FrameViewport,
    };

    const ID16: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    /// A "top-down" view-projection: screen uses world `(x, z)`, depth = world
    /// `y`. So a triangle flattened onto a constant-`y` ground plane still has
    /// screen area (a real perspective camera would too; this is just easy to
    /// reason about).
    const TOPDOWN: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    fn vtx(p: [f32; 3]) -> [f32; 12] {
        [p[0], p[1], p[2], 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]
    }

    /// A flat triangle at world y=0.5 spanning x,z in [-0.3, 0.3].
    fn caster_mesh(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
        let mut v = Vec::new();
        v.extend_from_slice(&vtx([-0.3, 0.5, -0.3]));
        v.extend_from_slice(&vtx([0.3, 0.5, -0.3]));
        v.extend_from_slice(&vtx([0.0, 0.5, 0.3]));
        (id, v, vec![0, 1, 2])
    }

    fn caster_draw(mesh_id: u64, casts: bool) -> FrameDrawItem {
        FrameDrawItem::new(1, mesh_id, 0, ID16, ID16, [1.0; 4], casts)
    }

    fn packet(
        draws: Vec<FrameDrawItem>,
        camera: Option<FrameCamera>,
        lights: Vec<FrameLight>,
    ) -> FramePacket {
        FramePacket::new(
            0,
            0,
            FrameViewport::new(16, 16),
            [0.0; 4],
            camera,
            draws,
            lights,
            ID16,
            FrameFeatureSet::new(false, false, 0, 0),
        )
    }

    fn topdown_cam() -> Option<FrameCamera> {
        Some(FrameCamera::new(ID16, ID16, TOPDOWN))
    }

    /// A straight-down directional light (`to-light` is +Y, so it travels -Y).
    fn down_light() -> Vec<FrameLight> {
        vec![FrameLight::new(0, [0.0, 1.0, 0.0], [1.0, 1.0, 1.0, 1.0])]
    }

    fn white_fb() -> SoftwareFramebuffer {
        let mut fb = SoftwareFramebuffer::new(16, 16);
        fb.clear([1.0, 1.0, 1.0, 1.0]);
        fb
    }

    #[test]
    fn marked_caster_casts_a_depth_tested_ground_shadow() {
        let cache = MeshCache::load(&[caster_mesh(7)]);
        let mut fb = white_fb();
        let depth = DepthBuffer::new(16, 16); // all far
        let p = packet(vec![caster_draw(7, true)], topdown_cam(), down_light());
        let (count, pixels) = apply_planar_shadows(&mut fb, &depth, &p, &cache, 0.5, 0.002);
        assert_eq!(count, 1);
        assert!(pixels > 0, "the shadow darkened ground pixels");
        // Some pixel really changed away from the white background.
        assert!(fb.into_rgba_bytes().chunks_exact(4).any(|c| c[0] < 255));
    }

    #[test]
    fn a_nearer_surface_occludes_the_shadow() {
        let cache = MeshCache::load(&[caster_mesh(7)]);
        let mut fb = white_fb();
        let mut depth = DepthBuffer::new(16, 16);
        // Everything in front of the shadow's ground depth (0.5): the wall wins.
        depth.slice_mut().iter_mut().for_each(|d| *d = -1.0);
        let p = packet(vec![caster_draw(7, true)], topdown_cam(), down_light());
        let (count, pixels) = apply_planar_shadows(&mut fb, &depth, &p, &cache, 0.5, 0.002);
        assert_eq!(count, 1, "the caster was processed");
        assert_eq!(pixels, 0, "but every shadow pixel is occluded");
        assert!(fb.into_rgba_bytes().iter().all(|&b| b == 255), "nothing darkened");
    }

    #[test]
    fn an_unmarked_draw_casts_no_shadow() {
        let cache = MeshCache::load(&[caster_mesh(7)]);
        let mut fb = white_fb();
        let depth = DepthBuffer::new(16, 16);
        let p = packet(vec![caster_draw(7, false)], topdown_cam(), down_light());
        assert_eq!(apply_planar_shadows(&mut fb, &depth, &p, &cache, 0.5, 0.002), (0, 0));
    }

    #[test]
    fn a_caster_with_no_uploaded_mesh_is_skipped() {
        let cache = MeshCache::default();
        let mut fb = white_fb();
        let depth = DepthBuffer::new(16, 16);
        let p = packet(vec![caster_draw(404, true)], topdown_cam(), down_light());
        assert_eq!(apply_planar_shadows(&mut fb, &depth, &p, &cache, 0.5, 0.002), (0, 0));
    }

    #[test]
    fn without_a_camera_there_is_no_shadow() {
        let cache = MeshCache::load(&[caster_mesh(7)]);
        let mut fb = white_fb();
        let depth = DepthBuffer::new(16, 16);
        let p = packet(vec![caster_draw(7, true)], None, down_light());
        assert_eq!(apply_planar_shadows(&mut fb, &depth, &p, &cache, 0.5, 0.002), (0, 0));
    }

    #[test]
    fn without_a_directional_light_there_is_no_shadow() {
        let cache = MeshCache::load(&[caster_mesh(7)]);
        let mut fb = white_fb();
        let depth = DepthBuffer::new(16, 16);
        // Only a point light (kind 1) — no sun to cast from.
        let lights = vec![FrameLight::new(1, [0.0, 5.0, 0.0], [1.0, 1.0, 1.0, 1.0])];
        let p = packet(vec![caster_draw(7, true)], topdown_cam(), lights);
        assert_eq!(apply_planar_shadows(&mut fb, &depth, &p, &cache, 0.5, 0.002), (0, 0));
    }

    #[test]
    fn a_horizontal_light_casts_no_ground_shadow() {
        let cache = MeshCache::load(&[caster_mesh(7)]);
        let mut fb = white_fb();
        let depth = DepthBuffer::new(16, 16);
        // `to-light` purely horizontal → travel horizontal → no vertical component.
        let lights = vec![FrameLight::new(0, [1.0, 0.0, 0.0], [1.0, 1.0, 1.0, 1.0])];
        let p = packet(vec![caster_draw(7, true)], topdown_cam(), lights);
        assert_eq!(apply_planar_shadows(&mut fb, &depth, &p, &cache, 0.5, 0.002), (0, 0));
    }

    #[test]
    fn alpha_clamps_and_full_alpha_blackens() {
        let cache = MeshCache::load(&[caster_mesh(7)]);
        let mut fb = white_fb();
        let depth = DepthBuffer::new(16, 16);
        let p = packet(vec![caster_draw(7, true)], topdown_cam(), down_light());
        // Over-unity alpha is clamped to 1.0 → covered pixels go fully black.
        let (_, pixels) = apply_planar_shadows(&mut fb, &depth, &p, &cache, 5.0, 0.002);
        assert!(pixels > 0);
        assert!(fb.into_rgba_bytes().chunks_exact(4).any(|c| c[0] == 0));
    }

    #[test]
    fn degenerate_shadow_triangle_draws_no_pixels() {
        let mut rgba = vec![255_u8; 16 * 16 * 4];
        let depth = DepthBuffer::new(16, 16);
        // Collinear (zero-area) triangle → no coverage.
        let v = [[2.0, 2.0, 0.5], [6.0, 2.0, 0.5], [4.0, 2.0, 0.5]];
        assert_eq!(rasterize_shadow_triangle(&mut rgba, &depth, 16, 16, v, 0.5, 0.002), 0);
        assert!(rgba.iter().all(|&b| b == 255), "nothing darkened");
    }

    #[test]
    fn helpers_are_exact() {
        // `world_point` applies translation + the linear part.
        let mut world = ID16;
        world[12] = 1.0; // +x translation
        world[13] = 2.0; // +y translation
        assert_eq!(world_point(&world, [0.0, 0.0, 0.0]), [1.0, 2.0, 0.0]);
        // `project_to_ground` lands a point on the plane along the travel dir.
        let on = project_to_ground([0.0, 1.0, 0.0], [0.0, -1.0, 0.0], 0.25);
        assert_eq!(on, [0.0, 0.25, 0.0]);
        // `blend_toward_black`: out-of-range offset is a no-op; full blend blackens
        // RGB and preserves alpha.
        let mut buf = vec![255_u8; 8];
        blend_toward_black(&mut buf, 100, 0.5);
        assert!(buf.iter().all(|&b| b == 255));
        blend_toward_black(&mut buf, 0, 1.0);
        assert_eq!(&buf[0..3], &[0, 0, 0]);
        assert_eq!(buf[3], 255);
    }
}
