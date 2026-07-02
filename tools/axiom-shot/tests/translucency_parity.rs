//! SPEC-11 §7 proof that `Material.opacity` actually blends a pixel on both
//! backends (previously it rode on `RenderMaterial` but the per-draw alpha came
//! only from `base_color`, never `opacity`, and Canvas 2D overwrote instead of
//! compositing).
//!
//! Builds an opaque red quad behind a translucent blue quad directly through
//! `axiom_render::RenderApi` (not the umbrella `App`/`Material` path) to exercise
//! the opacity fold and back-to-front sort straight in pixels. The projection is
//! pre-multiplied by the same GL->wgpu depth remap the render-pipeline bakes, so
//! the off-screen depth test behaves as in-app.
//!
//! Asserts (a) the opacity-0.5 centre pixel differs from the opacity-1.0 centre
//! pixel on each backend, and (b) GPU/canvas2d coarse region agreement (the same
//! metric `render_parity.rs` uses).
//!
//! Requires the native GPU adapter the sandbox provides (the off-screen arm).

mod common;

use std::collections::HashMap;

use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_gpu_backend::GpuBackendApi;
use axiom_host::FramePacket;
use axiom_kernel::Ratio;
use axiom_math::{Mat4, Vec2, Vec3, Vec4};
use axiom_render::RenderApi;

const W: u32 = 480;
const H: u32 = 270;

/// Column-major GL->wgpu clip-depth remap (`z' = (z + w) / 2`), matching the
/// remap the render-pipeline pre-multiplies into its view-projection.
const GL_TO_WGPU_DEPTH: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.5, 0.0, //
    0.0, 0.0, 0.5, 1.0, //
];

const IDENTITY16: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// A unit-ish quad in its local XY plane facing +z (toward the camera).
fn quad_positions() -> Vec<Vec3> {
    let s = 0.8;
    vec![
        Vec3::new(-s, -s, 0.0),
        Vec3::new(s, -s, 0.0),
        Vec3::new(s, s, 0.0),
        Vec3::new(-s, s, 0.0),
    ]
}

fn quad_normals() -> Vec<Vec3> {
    vec![Vec3::new(0.0, 0.0, 1.0); 4]
}

fn quad_uvs() -> Vec<Vec2> {
    vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ]
}

fn quad_indices() -> Vec<u32> {
    vec![0, 1, 2, 0, 2, 3]
}

/// The 12-float interleaved (pos3, normal3, uv2, colour4) geometry the backends
/// upload — opaque white vertex colour so the per-draw (instance) colour, which
/// carries the folded opacity, is what tints/fades the quad.
fn quad_geometry() -> Vec<f32> {
    let p = quad_positions();
    let uv = quad_uvs();
    let mut v = Vec::new();
    (0..4).for_each(|i| {
        v.extend_from_slice(&[p[i].x, p[i].y, p[i].z]);
        v.extend_from_slice(&[0.0, 0.0, 1.0]);
        v.extend_from_slice(&[uv[i].x, uv[i].y]);
        v.extend_from_slice(&[1.0, 1.0, 1.0, 1.0]);
    });
    v
}

/// A column-major translation matrix.
fn translate(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_cols_array([
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
    ])
}

/// Author the scene (opaque red quad behind, blue quad of `front_opacity` in
/// front) and compile its `FramePacket`. Also returns the backend mesh geometry
/// and the (untextured -> 1x1 white) material textures.
fn scene(
    front_opacity: f32,
) -> (
    FramePacket,
    Vec<(u64, Vec<f32>, Vec<u32>)>,
    Vec<(u64, u32, u32, Vec<u8>)>,
) {
    let api = RenderApi::new();
    let mut input = api.new_input(W, H);
    api.set_input_clear_color(&mut input, [0.02, 0.02, 0.05, 1.0]);

    let aspect = W as f32 / H as f32;
    let perspective = Mat4::perspective(0.96, aspect, 0.1, 100.0).expect("finite perspective");
    let projection = Mat4::from_cols_array(GL_TO_WGPU_DEPTH).multiply(perspective);
    let view = Mat4::look_at(
        Vec3::new(0.0, 0.0, 4.0),
        Vec3::ZERO,
        Vec3::new(0.0, 1.0, 0.0),
    )
    .expect("finite view");
    api.set_input_camera(&mut input, view, projection);

    let mesh = api.add_input_mesh(
        &mut input,
        1,
        quad_positions(),
        quad_normals(),
        quad_uvs(),
        quad_indices(),
    );
    let back = api.add_input_basic_lit_material(&mut input, 10, Vec4::new(0.9, 0.1, 0.1, 1.0));
    let front = api.add_input_lit_material(
        &mut input,
        20,
        Vec4::new(0.1, 0.2, 0.9, 1.0),
        Vec3::ZERO,
        Ratio::new(1.0).expect("finite"),
        Ratio::new(front_opacity).expect("finite"),
        0,
    );
    // Back farther (z=-1), front nearer (z=+1); the render layer sorts the
    // translucent front after the opaque back so alpha over-composites correctly.
    api.add_input_object(&mut input, 100, translate(0.0, 0.0, -1.0), mesh, back, true);
    api.add_input_object(&mut input, 200, translate(0.0, 0.0, 1.0), mesh, front, true);

    let packet = api.build_frame_packet(&input, 0, 0, IDENTITY16);
    let meshes = vec![(1u64, quad_geometry(), quad_indices())];
    let materials = vec![
        (10u64, 1u32, 1u32, vec![255, 255, 255, 255]),
        (20u64, 1u32, 1u32, vec![255, 255, 255, 255]),
    ];
    (packet, meshes, materials)
}

/// Group a packet's draws into the GPU backend's per-`(mesh, material)` instance
/// batches (mvp(16) | world(16) | colour(4) per instance), in first-appearance
/// order — the same packing `frame_packet_adapter` does, replicated here at the
/// tool tier (it is `pub(crate)` in the backend).
fn packet_to_batches(packet: &FramePacket) -> Vec<(u64, u64, Vec<f32>, u32)> {
    let mut order: Vec<(u64, u64)> = Vec::new();
    let mut packed: HashMap<(u64, u64), Vec<f32>> = HashMap::new();
    for d in packet.draws() {
        let key = (d.mesh_id(), d.material_id());
        let floats = packed.entry(key).or_insert_with(|| {
            order.push(key);
            Vec::new()
        });
        floats.extend_from_slice(&d.mvp());
        floats.extend_from_slice(&d.world());
        floats.extend_from_slice(&d.color());
    }
    order
        .into_iter()
        .map(|key| {
            let floats = packed.remove(&key).unwrap_or_default();
            let count = (floats.len() / 36) as u32;
            (key.0, key.1, floats, count)
        })
        .collect()
}

/// Render a packet through the native off-screen GPU path.
fn render_gpu(
    packet: &FramePacket,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
) -> Vec<u8> {
    let batches = packet_to_batches(packet);
    GpuBackendApi::render_offscreen_rgba(
        W,
        H,
        meshes,
        materials,
        &[],
        &[],
        packet.light_view_proj(),
        &batches,
        packet.clear_color(),
        packet.sdf(),
        axiom_host::FrameAmbient::default_hemisphere(),
    )
    .expect("a native GPU adapter is required to render a GPU screenshot")
}

/// Render a packet through the software Canvas 2D backend at quality tier 2.
fn render_canvas2d(
    packet: &FramePacket,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
) -> (Vec<u8>, u32, u32) {
    let mut backend = Canvas2dBackendApi::new(&common::present_request(W, H));
    backend.load_meshes(meshes);
    backend.set_quality_level(2);
    backend.render_offscreen_rgba(packet)
}

/// The centre RGBA pixel of an image (where the two quads overlap).
fn center_px(px: &[u8], w: u32, h: u32) -> [u8; 4] {
    let i = (((h / 2) * w + w / 2) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

#[test]
fn opacity_blends_on_both_backends() {
    let (p_trans, m_trans, mat_trans) = scene(0.5);
    let (p_opaque, m_opaque, mat_opaque) = scene(1.0);

    let gpu_trans = render_gpu(&p_trans, &m_trans, &mat_trans);
    let gpu_opaque = render_gpu(&p_opaque, &m_opaque, &mat_opaque);
    let gc_t = center_px(&gpu_trans, W, H);
    let gc_o = center_px(&gpu_opaque, W, H);
    assert_ne!(
        gc_t, gc_o,
        "GPU: opacity 0.5 must blend (centre {gc_t:?}) differently from opaque overwrite (centre {gc_o:?})"
    );
    assert!(
        gc_t[0] > gc_o[0],
        "GPU: translucent centre shows red from behind: trans {gc_t:?} vs opaque {gc_o:?}"
    );

    let (cv_trans, cw, chh) = render_canvas2d(&p_trans, &m_trans);
    let (cv_opaque, ow, ohh) = render_canvas2d(&p_opaque, &m_opaque);
    assert_eq!((cw, chh), (ow, ohh));
    let cc_t = center_px(&cv_trans, cw, chh);
    let cc_o = center_px(&cv_opaque, ow, ohh);
    assert_ne!(
        cc_t, cc_o,
        "canvas2d: opacity 0.5 must blend (centre {cc_t:?}) differently from opaque overwrite (centre {cc_o:?})"
    );
    assert!(
        cc_t[0] > cc_o[0],
        "canvas2d: translucent centre shows red from behind: trans {cc_t:?} vs opaque {cc_o:?}"
    );
}

#[test]
fn translucent_scene_agrees_across_backends() {
    let (packet, meshes, materials) = scene(0.5);
    let gpu = render_gpu(&packet, &meshes, &materials);
    let (sw, sw_w, sw_h) = render_canvas2d(&packet, &meshes);

    assert_eq!(gpu.len() as u32, W * H * 4);
    assert!(sw_w > 0 && sw_h > 0);
    assert_eq!(sw.len() as u32, sw_w * sw_h * 4);

    let (gcx, gcy, gcov) = common::region_stats(&gpu, W, H, 24);
    let (scx, scy, scov) = common::region_stats(&sw, sw_w, sw_h, 24);

    assert!(gcov > 0.02 && gcov < 0.9, "gpu coverage {gcov:.3} out of range");
    assert!(
        scov > 0.02 && scov < 0.9,
        "canvas2d coverage {scov:.3} out of range"
    );

    let dx = (gcx - scx).abs();
    let dy = (gcy - scy).abs();
    assert!(
        dx < 0.10 && dy < 0.10,
        "centroid disagreement gpu=({gcx:.3},{gcy:.3}) canvas2d=({scx:.3},{scy:.3})"
    );
}
