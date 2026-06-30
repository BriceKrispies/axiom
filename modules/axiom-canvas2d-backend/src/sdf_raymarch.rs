//! The CPU SDF raymarch pass for the software backend.
//!
//! This is the canvas2d peer of the GPU backend's WGSL raymarch shader: it
//! marches each pixel's ray through the frame's [`axiom_host::SdfScene`] and
//! composites the result into the software framebuffer, depth-tested against the
//! same depth buffer the triangle rasterizer wrote — so SDF shapes and meshes
//! occlude each other correctly in one frame. It is slow (per pixel × a fixed
//! step count × every primitive), but the software backend renders at a low
//! internal resolution, and "works even if slow" is exactly the property this
//! pass preserves: every render path has a canvas2d fallback.
//!
//! ## Branchless discipline
//! The module is spine code, so it carries no control-flow branches. The march
//! is a **fixed-iteration fold** that *freezes* its ray parameter once the ray
//! hits (no early `break`), mirroring `axiom-grid`'s bounded distance-field
//! relaxation; primitive dispatch is a table index by kind; and the per-pixel
//! colour/depth write is the rasterizer's `[old, new][pass as usize]` select.
//!
//! ## Depth convention
//! Identical to the triangle rasterizer (`projection` / `depth_buffer`): depth
//! is NDC z, **smaller = nearer**, the buffer clears to `f32::INFINITY`, and a
//! fragment overwrites colour + depth iff its NDC z is **strictly less** than
//! the stored depth. The marcher projects each world hit through the camera's
//! `view_proj` to get the same NDC z the rasterizer would.

use axiom_host::{FrameLight, SdfPrimitive, SdfScene};
use axiom_math::Vec3;

use crate::depth_buffer::DepthBuffer;
use crate::software_framebuffer::SoftwareFramebuffer;

/// The fixed number of march steps per pixel. A constant (not packet data) so
/// the per-pixel loop is a bounded, branchless fold.
const MARCH_STEPS: usize = 96;

/// Clip-space `w` at or below which a projected hit is treated as behind the
/// near plane and not composited (matches `projection::NEAR_W_EPS`).
const NEAR_W_EPS: f32 = 1e-6;

/// Constant ambient term so a surface facing away from every light is not pure
/// black.
const AMBIENT: f32 = 0.15;

/// The half-step used to estimate the surface normal by central differences.
const GRAD_H: f32 = 0.002;

/// March the `scene` for every pixel and composite hits into `framebuffer`,
/// depth-tested + depth-writing against `depth` using the scene's own
/// column-major `view_proj` (the self-contained contract carries it, so the
/// marcher no longer needs a separate `FrameCamera`). Returns the number of
/// pixels the pass wrote (a stat).
pub(crate) fn apply_sdf_raymarch(
    framebuffer: &mut SoftwareFramebuffer,
    depth: &mut DepthBuffer,
    scene: &SdfScene,
    lights: &[FrameLight],
) -> u64 {
    let view_proj = scene.view_proj();
    let w = framebuffer.width();
    let h = framebuffer.height();
    let rgba = framebuffer.rgba_mut();
    let dep = depth.slice_mut();
    (0..h).fold(0_u64, |acc, y| {
        (0..w).fold(acc, |acc, x| {
            acc + march_pixel(x, y, w, h, scene, &view_proj, lights, rgba, dep)
        })
    })
}

/// March one pixel; composite + depth-write on a nearer hit. Returns `1` when
/// it wrote the pixel, else `0`.
#[allow(clippy::too_many_arguments)]
fn march_pixel(
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    scene: &SdfScene,
    view_proj: &[f32; 16],
    lights: &[FrameLight],
    rgba: &mut [u8],
    dep: &mut [f32],
) -> u64 {
    let cam = scene.camera_world_pos();
    let origin = Vec3::new(cam[0], cam[1], cam[2]);
    // Pixel-centre NDC, y flipped (NDC up is +y; screen down is +y).
    let ndc_x = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
    let ndc_y = 1.0 - (y as f32 + 0.5) / h as f32 * 2.0;
    // A second point on the same ray (any NDC z works — all clip points with one
    // (ndc_x, ndc_y) lie on the camera ray); subtract the origin for direction.
    let on_ray = unproject(&scene.inv_view_proj(), ndc_x, ndc_y, 0.0);
    let dir = on_ray.subtract(origin).normalize().unwrap_or(Vec3::UNIT_Z);

    let max_dist = scene.march()[0];
    let eps = scene.march()[1];
    let (t, hit) = (0..MARCH_STEPS).fold((0.0_f32, false), |(t, hit), _| {
        let p = origin.add(dir.mul_scalar(t));
        let d = scene_distance(scene, p);
        let now_hit = hit | (d < eps);
        // Advance only while the ray has neither hit nor run past the far bound;
        // once either holds, `advance` is 0 and `t` freezes at the hit.
        let advance = (!now_hit) & (t < max_dist);
        (t + d * f32::from(advance), now_hit)
    });

    let p = origin.add(dir.mul_scalar(t));
    let surface = scene_color(scene, p);
    let normal = surface_normal(scene, p);
    let out = shade(surface, normal, p, lights);

    // Project the world hit to the rasterizer's NDC z for the depth test.
    let clip = clip_coords(view_proj, p);
    let cw = clip[3];
    let ndc_z = clip[2] / cw;
    let valid = hit & (cw > NEAR_W_EPS) & ndc_z.is_finite();

    let idx = y as usize * w as usize + x as usize;
    let pass = valid & (ndc_z < dep[idx]);
    let pi = usize::from(pass);
    let bytes = quantize(out);
    let off = idx * 4;
    rgba[off] = [rgba[off], bytes[0]][pi];
    rgba[off + 1] = [rgba[off + 1], bytes[1]][pi];
    rgba[off + 2] = [rgba[off + 2], bytes[2]][pi];
    rgba[off + 3] = [rgba[off + 3], bytes[3]][pi];
    dep[idx] = [dep[idx], ndc_z][pi];
    u64::from(pass)
}

/// The nearest signed distance from `p` (world space) to any primitive.
fn scene_distance(scene: &SdfScene, p: Vec3) -> f32 {
    scene
        .primitives()
        .iter()
        .fold(f32::INFINITY, |best, prim| primitive_distance(prim, p).min(best))
}

/// The linear RGBA colour of the primitive nearest to `p` (world space). Empty
/// scenes never reach a hit, so the default colour is unobservable.
fn scene_color(scene: &SdfScene, p: Vec3) -> [f32; 4] {
    scene
        .primitives()
        .iter()
        .fold((f32::INFINITY, [0.0_f32; 4]), |(best, col), prim| {
            let d = primitive_distance(prim, p);
            let closer = d < best;
            (d.min(best), [col, prim.color()][usize::from(closer)])
        })
        .1
}

/// The signed distance from world point `p` to one primitive: transform `p`
/// into the primitive's local frame, evaluate the canonical local SDF, and
/// rescale the local distance back to world units by the transform's uniform
/// scale.
fn primitive_distance(prim: &SdfPrimitive, p: Vec3) -> f32 {
    let local = transform_affine(&prim.inv_transform(), p);
    let params = prim.params();
    local_distance(prim.kind(), local, params) * params[3]
}

/// The canonical local-space SDF selected by `kind` (clamped into range so an
/// out-of-contract kind cannot index out of bounds).
fn local_distance(kind: u32, p: Vec3, params: [f32; 4]) -> f32 {
    let sphere = p.length() - params[0];
    let cuboid = box_distance(p, params);
    let plane = p.y;
    [sphere, cuboid, plane][(kind as usize).min(2)]
}

/// The exact signed distance to an origin-centred axis-aligned box with
/// half-extents `params[0..3]`.
fn box_distance(p: Vec3, params: [f32; 4]) -> f32 {
    let qx = p.x.abs() - params[0];
    let qy = p.y.abs() - params[1];
    let qz = p.z.abs() - params[2];
    let outside =
        Vec3::new(qx.max(0.0), qy.max(0.0), qz.max(0.0)).length();
    let inside = qx.max(qy.max(qz)).min(0.0);
    outside + inside
}

/// The surface normal at `p`, estimated by central differences of the scene SDF.
fn surface_normal(scene: &SdfScene, p: Vec3) -> Vec3 {
    let dx = scene_distance(scene, p.add(Vec3::new(GRAD_H, 0.0, 0.0)))
        - scene_distance(scene, p.subtract(Vec3::new(GRAD_H, 0.0, 0.0)));
    let dy = scene_distance(scene, p.add(Vec3::new(0.0, GRAD_H, 0.0)))
        - scene_distance(scene, p.subtract(Vec3::new(0.0, GRAD_H, 0.0)));
    let dz = scene_distance(scene, p.add(Vec3::new(0.0, 0.0, GRAD_H)))
        - scene_distance(scene, p.subtract(Vec3::new(0.0, 0.0, GRAD_H)));
    Vec3::new(dx, dy, dz).normalize().unwrap_or(Vec3::UNIT_Y)
}

/// Lambert shade `surface` at world point `hit` with normal `n` under `lights`,
/// plus a constant ambient term. Alpha is the surface alpha unchanged.
fn shade(surface: [f32; 4], n: Vec3, hit: Vec3, lights: &[FrameLight]) -> [f32; 4] {
    let lit = lights
        .iter()
        .fold(AMBIENT, |acc, l| acc + light_diffuse(l, n, hit))
        .min(1.0);
    [surface[0] * lit, surface[1] * lit, surface[2] * lit, surface[3]]
}

/// One light's diffuse contribution at `hit` with normal `n`. A directional
/// light (`kind == 0`) uses its to-light direction with unit attenuation; a
/// point light (`kind == 1`) uses the direction to its world position with
/// inverse-square attenuation. Selected branchlessly by kind.
fn light_diffuse(l: &FrameLight, n: Vec3, hit: Vec3) -> f32 {
    let ci = l.color_intensity();
    let intensity = ci[3];
    let v = l.vec();
    let vvec = Vec3::new(v[0], v[1], v[2]);
    let to_point = vvec.subtract(hit);
    let dist = to_point.length();
    let point_dir = to_point.normalize().unwrap_or(Vec3::UNIT_Y);
    let atten = 1.0 / (1.0 + dist * dist);
    let is_point = usize::from(l.kind() == 1);
    let dir = [vvec.normalize().unwrap_or(Vec3::UNIT_Y), point_dir][is_point];
    let a = [1.0, atten][is_point];
    n.dot(dir).max(0.0) * intensity * a
}

/// Transform an affine world point by a column-major world→local matrix
/// (assumes the matrix's last row is `[0,0,0,1]`, true for translate/rotate/
/// uniform-scale).
fn transform_affine(m: &[f32; 16], p: Vec3) -> Vec3 {
    Vec3::new(
        m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12],
        m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
        m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14],
    )
}

/// Transform a clip-space point `[ndc_x, ndc_y, ndc_z, 1]` by a column-major
/// matrix and perspective-divide — the unprojection that turns a pixel into a
/// world-space point on its camera ray.
fn unproject(m: &[f32; 16], ndc_x: f32, ndc_y: f32, ndc_z: f32) -> Vec3 {
    let x = m[0] * ndc_x + m[4] * ndc_y + m[8] * ndc_z + m[12];
    let y = m[1] * ndc_x + m[5] * ndc_y + m[9] * ndc_z + m[13];
    let z = m[2] * ndc_x + m[6] * ndc_y + m[10] * ndc_z + m[14];
    let w = m[3] * ndc_x + m[7] * ndc_y + m[11] * ndc_z + m[15];
    let inv = 1.0 / w;
    Vec3::new(x * inv, y * inv, z * inv)
}

/// Column-major `m · [p, 1]` into clip space `[cx, cy, cz, cw]`, without the
/// perspective divide (so the caller can read `cw` for the near test). Mirrors
/// `projection::clip_coords`.
fn clip_coords(m: &[f32; 16], p: Vec3) -> [f32; 4] {
    [
        m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12],
        m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
        m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14],
        m[3] * p.x + m[7] * p.y + m[11] * p.z + m[15],
    ]
}

/// Linear `0.0..=1.0` RGBA → clamped, rounded RGBA8 bytes (the framebuffer's own
/// quantization, replicated here for the inline indexed writes).
fn quantize(color: [f32; 4]) -> [u8; 4] {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    [byte(color[0]), byte(color[1]), byte(color[2]), byte(color[3])]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Mat4, Vec3};

    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    /// A camera at `(0,0,5)` looking at the origin, with its `view_proj`,
    /// `inv_view_proj`, and world position.
    fn camera() -> ([f32; 16], [f32; 16], [f32; 3]) {
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let view = Mat4::look_at(eye, Vec3::ZERO, Vec3::UNIT_Y).unwrap();
        let proj = Mat4::perspective(std::f32::consts::FRAC_PI_2, 1.0, 0.1, 100.0).unwrap();
        let view_proj = proj.multiply(view);
        let inv = view_proj.inverse().unwrap();
        (
            view_proj.as_cols_array(),
            inv.as_cols_array(),
            [eye.x, eye.y, eye.z],
        )
    }

    fn unit_sphere_scene(color: [f32; 4], lights_kind: u32) -> (SdfScene, Vec<FrameLight>) {
        let (vp, inv, cam) = camera();
        let prim = SdfPrimitive::new(SdfPrimitive::SPHERE, IDENTITY, [1.0, 0.0, 0.0, 1.0], color);
        let scene = SdfScene::new(vec![prim], vp, inv, cam, [100.0, 0.001, 0.0, 0.0]);
        let light = FrameLight::new(lights_kind, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
        (scene, vec![light])
    }

    fn pixel(fb: &SoftwareFramebuffer, w: u32, x: u32, y: u32) -> [u8; 4] {
        let bytes = fb.clone().into_rgba_bytes();
        let i = ((y * w + x) * 4) as usize;
        [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
    }

    #[test]
    fn center_ray_hits_the_sphere_and_corner_misses() {
        let (scene, lights) = unit_sphere_scene([1.0, 0.0, 0.0, 1.0], 0);
        let mut fb = SoftwareFramebuffer::new(16, 16);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        let mut depth = DepthBuffer::new(16, 16);
        depth.clear_far();

        let written = apply_sdf_raymarch(&mut fb, &mut depth, &scene, &lights);
        // Some pixels were written (the sphere covers the centre).
        assert!(written > 0);
        // The centre pixel is on the (lit, red) sphere; its red channel is high
        // and it is opaque.
        let c = pixel(&fb, 16, 8, 8);
        assert!(c[0] > 60, "centre is lit red, got {c:?}");
        assert_eq!(c[3], 255);
        // A corner ray misses the unit sphere — still the cleared black.
        assert_eq!(pixel(&fb, 16, 0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn center_hit_writes_a_nearer_depth() {
        let (scene, lights) = unit_sphere_scene([0.2, 0.8, 0.2, 1.0], 0);
        let mut fb = SoftwareFramebuffer::new(8, 8);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        let mut depth = DepthBuffer::new(8, 8);
        depth.clear_far();
        apply_sdf_raymarch(&mut fb, &mut depth, &scene, &lights);
        // The centre hit wrote a finite (nearer-than-far) depth; the corner did not.
        assert!(depth.depth_at(4, 4).is_finite());
        assert_eq!(depth.depth_at(0, 0), f32::INFINITY);
    }

    #[test]
    fn an_occluding_nearer_mesh_depth_rejects_the_sphere() {
        let (scene, lights) = unit_sphere_scene([1.0, 0.0, 0.0, 1.0], 0);
        let mut fb = SoftwareFramebuffer::new(8, 8);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        let mut depth = DepthBuffer::new(8, 8);
        depth.clear_far();
        // Pin every pixel's depth nearer than anything (−inf): the SDF must lose
        // the depth test everywhere and write nothing.
        depth.slice_mut().iter_mut().for_each(|d| *d = f32::NEG_INFINITY);
        let written = apply_sdf_raymarch(&mut fb, &mut depth, &scene, &lights);
        assert_eq!(written, 0);
        assert_eq!(pixel(&fb, 8, 4, 4), [0, 0, 0, 255]);
    }

    #[test]
    fn point_light_kind_is_shaded() {
        // Exercises the point-light arm (kind 1) of light_diffuse.
        let (scene, lights) = unit_sphere_scene([0.0, 0.0, 1.0, 1.0], 1);
        let mut fb = SoftwareFramebuffer::new(8, 8);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        let mut depth = DepthBuffer::new(8, 8);
        depth.clear_far();
        let written = apply_sdf_raymarch(&mut fb, &mut depth, &scene, &lights);
        assert!(written > 0);
        // The centre is at least ambient-lit blue.
        assert!(pixel(&fb, 8, 4, 4)[2] > 20);
    }

    #[test]
    fn box_and_plane_kinds_evaluate() {
        // A box filling the view and a ground plane both produce hits, covering
        // the box and plane arms of local_distance.
        let (vp, inv, cam) = camera();
        let cuboid = SdfPrimitive::new(SdfPrimitive::BOX, IDENTITY, [1.0, 1.0, 1.0, 1.0], [1.0, 1.0, 0.0, 1.0]);
        let scene = SdfScene::new(vec![cuboid], vp, inv, cam, [100.0, 0.001, 0.0, 0.0]);
        let lights = vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])];
        let mut fb = SoftwareFramebuffer::new(8, 8);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        let mut depth = DepthBuffer::new(8, 8);
        depth.clear_far();
        assert!(apply_sdf_raymarch(&mut fb, &mut depth, &scene, &lights) > 0);

        // The plane kind: a local y=0 plane evaluates (distance is the local y).
        let plane = SdfPrimitive::new(SdfPrimitive::PLANE, IDENTITY, [0.0, 0.0, 0.0, 1.0], [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(local_distance(SdfPrimitive::PLANE, Vec3::new(0.0, 2.0, 0.0), plane.params()), 2.0);
    }

    #[test]
    fn box_distance_is_signed_inside_and_outside() {
        // Outside on +x by 1 unit from a unit box → distance 1.
        assert!((box_distance(Vec3::new(2.0, 0.0, 0.0), [1.0, 1.0, 1.0, 1.0]) - 1.0).abs() < 1e-5);
        // Centre of a unit box → −1 (the inside arm: max(q)<0).
        assert!((box_distance(Vec3::ZERO, [1.0, 1.0, 1.0, 1.0]) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn out_of_range_kind_clamps_to_plane() {
        // A kind past the table clamps to the last entry (plane) rather than
        // panicking — the `.min(2)` guard.
        assert_eq!(local_distance(99, Vec3::new(0.0, 3.0, 0.0), [0.0; 4]), 3.0);
    }

    #[test]
    fn empty_scene_writes_nothing() {
        let (vp, inv, cam) = camera();
        let scene = SdfScene::new(Vec::new(), vp, inv, cam, [100.0, 0.001, 0.0, 0.0]);
        let mut fb = SoftwareFramebuffer::new(4, 4);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        let mut depth = DepthBuffer::new(4, 4);
        depth.clear_far();
        // No primitives → the ray never gets within eps → no writes, and the
        // far scene distance keeps the marcher from ever hitting.
        assert_eq!(apply_sdf_raymarch(&mut fb, &mut depth, &scene, &lights_none()), 0);
    }

    fn lights_none() -> Vec<FrameLight> {
        Vec::new()
    }

    #[test]
    fn degenerate_ray_falls_back_without_panicking() {
        // A zero inv_view_proj makes the unprojected point coincide with the
        // origin, so the ray direction normalize fails and falls back. The pass
        // must still complete (no panic) and simply write nothing meaningful.
        let (vp, _inv, cam) = camera();
        let prim = SdfPrimitive::new(SdfPrimitive::SPHERE, IDENTITY, [1.0, 0.0, 0.0, 1.0], [1.0; 4]);
        let scene = SdfScene::new(vec![prim], vp, [0.0; 16], cam, [100.0, 0.001, 0.0, 0.0]);
        let mut fb = SoftwareFramebuffer::new(4, 4);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        let mut depth = DepthBuffer::new(4, 4);
        depth.clear_far();
        // Just assert it runs to completion deterministically.
        let a = apply_sdf_raymarch(&mut fb.clone(), &mut depth.clone(), &scene, &lights_none());
        let b = apply_sdf_raymarch(&mut fb, &mut depth, &scene, &lights_none());
        assert_eq!(a, b);
    }

    #[test]
    fn sdf_pass_runs_when_a_packet_carries_a_scene_and_camera() {
        use axiom_host::{FrameCamera, FrameFeatureSet, FramePacket, FrameViewport};

        let (vp, inv, cam) = camera();
        let prim = SdfPrimitive::new(SdfPrimitive::SPHERE, IDENTITY, [1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]);
        let scene = SdfScene::new(vec![prim], vp, inv, cam, [100.0, 0.001, 0.0, 0.0]);
        let light = FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
        let with = FramePacket::new(
            2,
            120,
            FrameViewport::new(320, 180),
            [0.0, 0.0, 0.0, 1.0],
            Some(FrameCamera::new(IDENTITY, IDENTITY, vp)),
            Vec::new(),
            vec![light],
            IDENTITY,
            FrameFeatureSet::new(false, false, 0, 0),
        )
        .with_sdf(scene);

        let mut fb = SoftwareFramebuffer::new(16, 16);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        let mut depth = DepthBuffer::new(16, 16);
        depth.clear_far();
        // The `Some` arm: the camera + scene drive the marcher, compositing the
        // lit sphere at the centre.
        let written = crate::software_rasterizer::sdf_pass(&mut fb, &mut depth, &with);
        assert!(written > 0);
        assert!(pixel(&fb, 16, 8, 8)[0] > 60);

        // The `None` arm: a camera-only packet carries no SDF scene, so the pass
        // is a no-op and writes nothing.
        let without = FramePacket::new(
            2,
            120,
            FrameViewport::new(320, 180),
            [0.0, 0.0, 0.0, 1.0],
            Some(FrameCamera::new(IDENTITY, IDENTITY, vp)),
            Vec::new(),
            Vec::new(),
            IDENTITY,
            FrameFeatureSet::new(false, false, 0, 0),
        );
        let mut fb2 = SoftwareFramebuffer::new(16, 16);
        fb2.clear([0.0, 0.0, 0.0, 1.0]);
        let mut depth2 = DepthBuffer::new(16, 16);
        depth2.clear_far();
        assert_eq!(crate::software_rasterizer::sdf_pass(&mut fb2, &mut depth2, &without), 0);
    }
}
