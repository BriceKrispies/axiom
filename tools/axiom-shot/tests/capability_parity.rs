//! Per-capability cross-backend parity (audit M4 / §9.6).
//!
//! For each feature class the backend capability system gates — **textures**,
//! **alpha cutout**, **shadows**, **SDF**, and **retro 32-bit** — this renders the
//! *same* semantic frame two ways and asserts the capability is load-bearing:
//!
//! * On the **GPU** off-screen backend, toggling the capability in the
//!   [`BackendCapabilityProfile`] visibly changes the rendered pixels — so the GPU is
//!   no longer unconditionally full; it consults the profile (audit M3).
//! * On the **Canvas 2D** software backend, the capability is either applied (SDF,
//!   retro — both gate the same way the GPU does) or **degraded per the declared
//!   policy** ([`RenderCapability::degradation`]): textures are a reported *drop*
//!   (flat colour), the directional shadow is a *substitute* (planar contact
//!   shadow). The test asserts the declared degradation actually happened — the drop
//!   is reported, the substitute is drawn — not that pixels match.
//!
//! The GPU arm needs the native off-screen wgpu adapter (the `offscreen` feature,
//! which `axiom-shot` always enables); every scene is hand-built at the backend
//! boundary (meshes / materials / instance batches / `FramePacket`) so the proof
//! exercises exactly the capability plumbing, not the high-level authoring API.
#![cfg(feature = "offscreen")]

use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_gpu_backend::GpuBackendApi;
use axiom_host::{
    BackendCapabilityProfile, CapabilityDegradation, FrameAmbient, FrameCamera, FrameDrawItem,
    FrameFeature, FrameFeatureSet, FrameLight, FramePacket, FrameRetro32BitProfile, FrameViewport,
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest, RenderCapability, SdfPrimitive, SdfScene,
};
use axiom_kernel::{KernelApi, Ratio};
use axiom_math::{Mat4, Vec3};

const W: u32 = 96;
const H: u32 = 72;

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// The render look every scene here binds with: the bright ambient, no fog, no
/// sky, no bloom. These proofs are about capability plumbing, and an unauthored
/// look part is that part's exact no-op — so the look never colours the result.
fn plain_look() -> axiom_host::FrameRenderLook {
    axiom_host::FrameRenderLook::lit_by(bright_ambient())
}

/// A validated host presentation request a backend is sized from.
fn request(w: u32, h: u32) -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(w, h, Ratio::new(1.0).expect("finite scale"))
        .expect("valid viewport");
    let target = host
        .presentation_target(&kernel, 1, "axiom-capability-parity")
        .expect("valid target");
    let surface = host.surface_handle(&kernel, 2).expect("valid surface");
    let descriptor = host.surface_descriptor(
        viewport,
        HostPresentMode::Fifo,
        HostAlphaMode::Opaque,
        HostColorFormat::Bgra8UnormSrgb,
    );
    let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
    let device = host.device_request(true, HostDeviceProfile::Baseline);
    host.presentation_request(target, surface, descriptor, adapter, device)
        .expect("valid presentation request")
}

/// One interleaved 12-float vertex: position(3) + normal(3) + uv(2) + colour(4).
fn vertex(pos: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> [f32; 12] {
    [
        pos[0], pos[1], pos[2], normal[0], normal[1], normal[2], uv[0], uv[1], 1.0, 1.0, 1.0, 1.0,
    ]
}

/// A screen-filling quad in clip space (z=0, facing the camera), UVs over [0,1] —
/// with an IDENTITY mvp it covers the whole framebuffer, so a texture's variation is
/// visible across the frame.
fn fullscreen_quad(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
    let n = [0.0, 0.0, 1.0];
    let mut v = Vec::new();
    v.extend_from_slice(&vertex([-1.0, -1.0, 0.0], n, [0.0, 0.0]));
    v.extend_from_slice(&vertex([1.0, -1.0, 0.0], n, [1.0, 0.0]));
    v.extend_from_slice(&vertex([1.0, 1.0, 0.0], n, [1.0, 1.0]));
    v.extend_from_slice(&vertex([-1.0, 1.0, 0.0], n, [0.0, 1.0]));
    (id, v, vec![0, 1, 2, 0, 2, 3])
}

/// One 40-float instance: mvp(16) + world(16) + colour(4) + emissive(3)+pad(1).
fn instance(mvp: [f32; 16], world: [f32; 16], color: [f32; 4]) -> Vec<f32> {
    let mut f = Vec::with_capacity(40);
    f.extend_from_slice(&mvp);
    f.extend_from_slice(&world);
    f.extend_from_slice(&color);
    // Emissive + specular: nothing in this parity scene self-illuminates or
    // catches a highlight.
    f.extend_from_slice(&[0.0, 0.0, 0.0, 0.0]);
    f
}

/// One instance whose fourth emissive lane carries a specular strength — the
/// lane `instance` above leaves at zero, and the one the highlight is gated on.
fn glossy_instance(mvp: [f32; 16], world: [f32; 16], color: [f32; 4], specular: f32) -> Vec<f32> {
    let mut f = instance(mvp, world, color);
    let last = f.len() - 1;
    f[last] = specular;
    f
}

/// The fraction of pixels differing from the background (top-left corner) on any
/// channel by more than `threshold` — a resolution-independent "how much rendered".
fn coverage(px: &[u8], threshold: u8) -> f64 {
    let bg = [px[0], px[1], px[2]];
    let count = px
        .chunks_exact(4)
        .filter(|p| {
            p[0].abs_diff(bg[0])
                .max(p[1].abs_diff(bg[1]))
                .max(p[2].abs_diff(bg[2]))
                > threshold
        })
        .count();
    count as f64 / (px.len() / 4) as f64
}

/// Whether two equal-length RGBA8 buffers differ on any channel by more than 1
/// (a real rendered change, not sub-LSB rounding noise).
fn differs(a: &[u8], b: &[u8]) -> bool {
    a.iter().zip(b).any(|(x, y)| x.abs_diff(*y) > 1)
}

/// White ambient so a flat white material still lights up (isolates the capability
/// under test from ambient darkness).
fn bright_ambient() -> FrameAmbient {
    FrameAmbient::new([0.9, 0.9, 0.9], [0.9, 0.9, 0.9])
}

/// A single directional light pointing at the quad.
fn front_light() -> Vec<(u32, [f32; 3], [f32; 3], f32)> {
    vec![(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0], 1.0)]
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn gpu_look(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    normals: &[(u64, u32, u32, Vec<u8>)],
    lights: &[(u32, [f32; 3], [f32; 3], f32)],
    light_view_proj: [f32; 16],
    batches: &[(u64, u64, Vec<f32>, u32)],
    clear: [f32; 4],
    sdf: Option<&SdfScene>,
    retro: Option<FrameRetro32BitProfile>,
    profile: BackendCapabilityProfile,
    // The render look the frame binds with, and the camera its sky pass reads.
    // Most scenes here author neither and pass the plain bright ambient.
    look: axiom_host::FrameRenderLook,
    camera: [f32; 16],
) -> Vec<u8> {
    GpuBackendApi::render_offscreen_rgba(
        W,
        H,
        meshes,
        materials,
        normals,
        lights,
        light_view_proj,
        // The camera the sky pass reads; a scene authoring a sky passes its own.
        camera,
        batches,
        // These scenes submit no skinned bodies.
        &[],
        &[],
        clear,
        sdf,
        look,
        retro,
        profile,
        None,
        None,
    )
    .expect("a native GPU adapter is required for the capability parity proof")
}

/// The ten scenes that author no sky and no bloom, and whose camera is unread.
#[allow(clippy::too_many_arguments)]
fn gpu(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    normals: &[(u64, u32, u32, Vec<u8>)],
    lights: &[(u32, [f32; 3], [f32; 3], f32)],
    light_view_proj: [f32; 16],
    batches: &[(u64, u64, Vec<f32>, u32)],
    clear: [f32; 4],
    sdf: Option<&SdfScene>,
    retro: Option<FrameRetro32BitProfile>,
    profile: BackendCapabilityProfile,
    look: axiom_host::FrameRenderLook,
    camera: [f32; 16],
) -> Vec<u8> {
    gpu_look(
        meshes,
        materials,
        normals,
        lights,
        light_view_proj,
        batches,
        clear,
        sdf,
        retro,
        profile,
        look,
        camera,
    )
}

// ---------------------------------------------------------------------------
// Textures — GPU samples the albedo; Canvas 2D drops it to a flat colour (reported).
// ---------------------------------------------------------------------------

/// A 2×2 opaque checker of four distinct colours.
fn checker_texture(id: u64) -> (u64, u32, u32, Vec<u8>) {
    let px = vec![
        255, 40, 40, 255, 40, 255, 40, 255, 40, 40, 255, 255, 240, 240, 40, 255,
    ];
    (id, 2, 2, px)
}

#[test]
fn textures_gpu_samples_albedo_canvas2d_reports_the_drop() {
    let meshes = [fullscreen_quad(1)];
    let materials = [checker_texture(7)];
    let batches = [(1_u64, 7_u64, instance(IDENTITY, IDENTITY, [1.0; 4]), 1_u32)];
    let clear = [0.0, 0.0, 0.0, 1.0];

    // GPU with Textures on samples the four-colour checker (a varied frame); with
    // Textures dropped it renders flat white — so the profile is load-bearing.
    let textured = gpu(
        &meshes,
        &materials,
        &[],
        &front_light(),
        IDENTITY,
        &batches,
        clear,
        None,
        None,
        BackendCapabilityProfile::all(),
        plain_look(),
        IDENTITY,
    );
    let flat = gpu(
        &meshes,
        &materials,
        &[],
        &front_light(),
        IDENTITY,
        &batches,
        clear,
        None,
        None,
        BackendCapabilityProfile::all().without(RenderCapability::Textures),
        plain_look(),
        IDENTITY,
    );
    assert!(
        differs(&textured, &flat),
        "GPU must consult the Textures capability"
    );
    assert!(
        coverage(&textured, 24) > 0.5,
        "textured quad should cover the frame"
    );

    // Canvas 2D declares Textures a reported drop; a frame that uses textures reports
    // AlbedoSampling degraded and a degraded material count — the declared policy.
    assert_eq!(
        RenderCapability::Textures.degradation(),
        CapabilityDegradation::Drop
    );
    let mut backend = Canvas2dBackendApi::new(&request(W, H));
    backend.load_meshes(&meshes);
    let draws = vec![FrameDrawItem::new(
        0, 1, 7, IDENTITY, IDENTITY, [1.0; 4], false,
    )];
    let packet = FramePacket::new(
        1,
        1,
        FrameViewport::new(W, H),
        clear,
        Some(FrameCamera::new(IDENTITY, IDENTITY, IDENTITY)),
        draws,
        Vec::new(),
        IDENTITY,
        FrameFeatureSet::new(true, false, 0, 0),
    );
    let report = backend.present_packet(&packet);
    assert!(
        report
            .degraded_features()
            .contains(&FrameFeature::AlbedoSampling),
        "Canvas 2D must report the albedo-sampling drop"
    );
    assert_eq!(
        report.degraded_materials(),
        1,
        "the textured material is degraded to flat"
    );
}

// ---------------------------------------------------------------------------
// Alpha cutout — GPU discards transparent texels; Canvas 2D can't cutout (drop).
// ---------------------------------------------------------------------------

/// A 2×2 texture with two low-alpha (0.3) texels. Below the 0.5 cutout threshold, a
/// cutout pass *discards* them (they read as the clear colour), whereas without cutout
/// they alpha-blend (0.3) to a visible dimmed colour — so the two renders differ. (A
/// fully-transparent alpha-0 texel would blend to nothing either way, hiding the gate.)
fn cutout_texture(id: u64) -> (u64, u32, u32, Vec<u8>) {
    let px = vec![
        255, 40, 40, 255, 40, 255, 40, 77, 40, 40, 255, 77, 240, 240, 40, 255,
    ];
    (id, 2, 2, px)
}

#[test]
fn alpha_cutout_is_gated_on_the_gpu_and_dropped_on_canvas2d() {
    let meshes = [fullscreen_quad(1)];
    let materials = [cutout_texture(7)];
    let batches = [(1_u64, 7_u64, instance(IDENTITY, IDENTITY, [1.0; 4]), 1_u32)];
    let clear = [0.0, 0.0, 0.0, 1.0];

    // With both Textures + AlphaMask on, the transparent texels are discarded, so the
    // quad no longer fully covers the frame; dropping AlphaMask keeps the quad opaque.
    let cutout = gpu(
        &meshes,
        &materials,
        &[],
        &front_light(),
        IDENTITY,
        &batches,
        clear,
        None,
        None,
        BackendCapabilityProfile::all(),
        plain_look(),
        IDENTITY,
    );
    let opaque = gpu(
        &meshes,
        &materials,
        &[],
        &front_light(),
        IDENTITY,
        &batches,
        clear,
        None,
        None,
        BackendCapabilityProfile::all().without(RenderCapability::AlphaMask),
        plain_look(),
        IDENTITY,
    );
    assert!(
        differs(&cutout, &opaque),
        "GPU must consult the AlphaMask capability"
    );
    assert!(
        coverage(&cutout, 24) < coverage(&opaque, 24),
        "the cutout must punch holes the opaque render fills"
    );
    // The Canvas 2D flat rasterizer cannot cutout: the declared degradation is a drop.
    assert_eq!(
        RenderCapability::AlphaMask.degradation(),
        CapabilityDegradation::Drop
    );
    assert!(!BackendCapabilityProfile::canvas2d().contains(RenderCapability::AlphaMask));
}

// ---------------------------------------------------------------------------
// SDF — both backends march the SAME scene; both gate on the Sdf capability.
// ---------------------------------------------------------------------------

/// A camera at (0,0,5) looking at the origin.
fn sdf_camera() -> ([f32; 16], [f32; 16], [f32; 3]) {
    let eye = Vec3::new(0.0, 0.0, 5.0);
    let view = Mat4::look_at(eye, Vec3::ZERO, Vec3::UNIT_Y).expect("view");
    let proj = Mat4::perspective(std::f32::consts::FRAC_PI_2, W as f32 / H as f32, 0.1, 100.0)
        .expect("proj");
    let vp = proj.multiply(view);
    (
        vp.as_cols_array(),
        vp.inverse().expect("inv").as_cols_array(),
        [eye.x, eye.y, eye.z],
    )
}

fn unit_sphere_scene() -> SdfScene {
    let (vp, inv, cam) = sdf_camera();
    let prim = SdfPrimitive::new(
        SdfPrimitive::SPHERE,
        IDENTITY,
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 0.2, 0.2, 1.0],
    );
    SdfScene::new(vec![prim], vp, inv, cam, [100.0, 0.001, 0.0, 0.0])
}

#[test]
fn sdf_renders_on_both_backends_and_both_gate_it() {
    let scene = unit_sphere_scene();
    let clear = [0.0, 0.0, 0.0, 1.0];

    // GPU: the raymarch pass draws the sphere; dropping Sdf renders meshes only (none
    // here → empty), so the capability is load-bearing.
    let with = gpu(
        &[],
        &[],
        &[],
        &front_light(),
        IDENTITY,
        &[],
        clear,
        Some(&scene),
        None,
        BackendCapabilityProfile::all(),
        plain_look(),
        IDENTITY,
    );
    let without = gpu(
        &[],
        &[],
        &[],
        &front_light(),
        IDENTITY,
        &[],
        clear,
        Some(&scene),
        None,
        BackendCapabilityProfile::all().without(RenderCapability::Sdf),
        plain_look(),
        IDENTITY,
    );
    assert!(
        coverage(&with, 24) > 0.02,
        "GPU SDF pass should draw the sphere"
    );
    assert!(
        coverage(&without, 24) < 0.005,
        "dropping Sdf must skip the raymarch"
    );

    // Canvas 2D marches the SAME scene on the CPU (its profile keeps Sdf), and drops it
    // when Sdf is removed — parity up to the flat CPU march degrade.
    let packet = FramePacket::new(
        1,
        1,
        FrameViewport::new(W, H),
        clear,
        Some(FrameCamera::new(IDENTITY, IDENTITY, IDENTITY)),
        Vec::new(),
        vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])],
        IDENTITY,
        FrameFeatureSet::new(false, false, 0, 0),
    )
    .with_sdf(scene);
    let mut on = Canvas2dBackendApi::new(&request(W, H));
    let (cpu_on, _, _) = on.render_offscreen_rgba(&packet);
    assert!(
        coverage(&cpu_on, 24) > 0.02,
        "Canvas 2D CPU march should draw the sphere"
    );
    let mut off = Canvas2dBackendApi::new(&request(W, H));
    off.set_capability_profile(BackendCapabilityProfile::canvas2d().without(RenderCapability::Sdf));
    let (cpu_off, _, _) = off.render_offscreen_rgba(&packet);
    assert!(
        coverage(&cpu_off, 24) < 0.005,
        "Canvas 2D must gate the SDF march too"
    );
}

// ---------------------------------------------------------------------------
// Retro 32-bit — both backends apply the quantize+dither; both gate on the profile.
// ---------------------------------------------------------------------------

#[test]
fn retro_32bit_is_gated_on_both_backends() {
    let meshes = [fullscreen_quad(1)];
    let materials = [(9_u64, 1_u32, 1_u32, vec![200, 140, 80, 255])];
    let batches = [(1_u64, 9_u64, instance(IDENTITY, IDENTITY, [1.0; 4]), 1_u32)];
    let clear = [0.15, 0.1, 0.05, 1.0];
    let retro = FrameRetro32BitProfile::retro_32bit();

    // GPU: the retro colour-depth quantize + dither reshapes the readback; dropping
    // Retro32Bit leaves it full-fidelity.
    let quantized = gpu(
        &meshes,
        &materials,
        &[],
        &front_light(),
        IDENTITY,
        &batches,
        clear,
        None,
        Some(retro),
        BackendCapabilityProfile::all(),
        plain_look(),
        IDENTITY,
    );
    let full = gpu(
        &meshes,
        &materials,
        &[],
        &front_light(),
        IDENTITY,
        &batches,
        clear,
        None,
        Some(retro),
        BackendCapabilityProfile::all().without(RenderCapability::Retro32Bit),
        plain_look(),
        IDENTITY,
    );
    assert!(
        differs(&quantized, &full),
        "GPU must consult the Retro32Bit capability"
    );

    // Canvas 2D applies the SAME neutral retro post (its profile keeps Retro32Bit) and
    // skips it when the capability is dropped.
    let packet = FramePacket::new(
        1,
        1,
        FrameViewport::new(W, H),
        clear,
        Some(FrameCamera::new(IDENTITY, IDENTITY, IDENTITY)),
        vec![FrameDrawItem::new(
            0, 1, 9, IDENTITY, IDENTITY, [1.0; 4], false,
        )],
        vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])],
        IDENTITY,
        FrameFeatureSet::new(false, false, 0, 0),
    )
    .with_retro_32bit_profile(retro);
    let mut on = Canvas2dBackendApi::new(&request(W, H));
    on.load_meshes(&meshes);
    let (cpu_retro, _, _) = on.render_offscreen_rgba(&packet);
    let mut off = Canvas2dBackendApi::new(&request(W, H));
    off.load_meshes(&meshes);
    off.set_capability_profile(
        BackendCapabilityProfile::canvas2d().without(RenderCapability::Retro32Bit),
    );
    let (cpu_plain, _, _) = off.render_offscreen_rgba(&packet);
    assert!(
        differs(&cpu_retro, &cpu_plain),
        "Canvas 2D must gate the retro post pass too"
    );
}

// ---------------------------------------------------------------------------
// Shadows — GPU casts a PCF shadow; Canvas 2D substitutes a planar contact shadow.
// ---------------------------------------------------------------------------

/// A horizontal quad in the y=`y` plane spanning [-half, half] in x and z, facing up.
fn ground_quad(id: u64, y: f32, half: f32) -> (u64, Vec<f32>, Vec<u32>) {
    let n = [0.0, 1.0, 0.0];
    let mut v = Vec::new();
    v.extend_from_slice(&vertex([-half, y, -half], n, [0.0, 0.0]));
    v.extend_from_slice(&vertex([half, y, -half], n, [1.0, 0.0]));
    v.extend_from_slice(&vertex([half, y, half], n, [1.0, 1.0]));
    v.extend_from_slice(&vertex([-half, y, half], n, [0.0, 1.0]));
    (id, v, vec![0, 1, 2, 0, 2, 3])
}

/// The shared shadow scene: a ground plane, a smaller caster hovering above it, one
/// directional light, and the camera + light view-projections. Returns
/// `(meshes, materials, batches, lights, light_vp, camera_vp)`.
#[allow(clippy::type_complexity)]
fn shadow_scene() -> (
    Vec<(u64, Vec<f32>, Vec<u32>)>,
    Vec<(u64, u32, u32, Vec<u8>)>,
    Vec<(u64, u64, Vec<f32>, u32)>,
    Vec<(u32, [f32; 3], [f32; 3], f32)>,
    [f32; 16],
    [f32; 16],
) {
    let cam_view =
        Mat4::look_at(Vec3::new(0.0, 3.5, 5.5), Vec3::ZERO, Vec3::UNIT_Y).expect("cam view");
    let cam_proj = Mat4::perspective(std::f32::consts::FRAC_PI_3, W as f32 / H as f32, 0.1, 100.0)
        .expect("cam proj");
    let cam_vp = cam_proj.multiply(cam_view).as_cols_array();

    // A directional light from up and to the side, so the caster's shadow lands on the
    // *visible* part of the ground (not hidden behind the caster).
    let light_view =
        Mat4::look_at(Vec3::new(3.0, 6.0, 1.5), Vec3::ZERO, Vec3::UNIT_Y).expect("light view");
    let light_proj =
        Mat4::perspective(std::f32::consts::FRAC_PI_2, 1.0, 0.5, 40.0).expect("light proj");
    let light_vp = light_proj.multiply(light_view).as_cols_array();

    let meshes = vec![ground_quad(1, 0.0, 4.0), ground_quad(2, 1.6, 0.9)];
    let materials = vec![(5_u64, 1_u32, 1_u32, vec![255, 255, 255, 255])];
    let batches = vec![
        (
            1_u64,
            5_u64,
            instance(cam_vp, IDENTITY, [0.65, 0.65, 0.65, 1.0]),
            1_u32,
        ),
        (
            2_u64,
            5_u64,
            instance(cam_vp, IDENTITY, [0.85, 0.25, 0.25, 1.0]),
            1_u32,
        ),
    ];
    // Directional light: to-light direction points up toward the light.
    let lights = vec![(0_u32, [0.4, 1.0, 0.2], [1.0, 1.0, 1.0], 1.0_f32)];
    (meshes, materials, batches, lights, light_vp, cam_vp)
}

#[test]
fn shadows_gpu_casts_pcf_canvas2d_substitutes_planar_contact() {
    let (meshes, materials, batches, lights, light_vp, cam_vp) = shadow_scene();
    let clear = [0.05, 0.06, 0.09, 1.0];

    // GPU with Shadows on darkens the ground under the caster (the PCF lookup); dropping
    // Shadows leaves the directional light fully lit — the capability is load-bearing.
    let shadowed = gpu(
        &meshes,
        &materials,
        &[],
        &lights,
        light_vp,
        &batches,
        clear,
        None,
        None,
        BackendCapabilityProfile::all(),
        plain_look(),
        IDENTITY,
    );
    let unshadowed = gpu(
        &meshes,
        &materials,
        &[],
        &lights,
        light_vp,
        &batches,
        clear,
        None,
        None,
        BackendCapabilityProfile::all().without(RenderCapability::Shadows),
        plain_look(),
        IDENTITY,
    );
    assert!(
        differs(&shadowed, &unshadowed),
        "GPU must consult the Shadows capability (PCF)"
    );

    // Canvas 2D declares the shadow a SUBSTITUTE (planar contact shadow). Rendering the
    // same caster reports the shadow degraded AND draws the substitute — the declared
    // degradation applied, not a silent drop.
    assert_eq!(
        RenderCapability::Shadows.degradation(),
        CapabilityDegradation::Substitute
    );
    let mut backend = Canvas2dBackendApi::new(&request(W, H));
    backend.load_meshes(&meshes);
    let draws = vec![
        FrameDrawItem::new(0, 1, 5, cam_vp, IDENTITY, [0.65, 0.65, 0.65, 1.0], false),
        // The caster is marked a contact-shadow caster, so the planar substitute grounds it.
        FrameDrawItem::new(1, 2, 5, cam_vp, IDENTITY, [0.85, 0.25, 0.25, 1.0], true),
    ];
    let packet = FramePacket::new(
        1,
        1,
        FrameViewport::new(W, H),
        clear,
        Some(FrameCamera::new(IDENTITY, IDENTITY, cam_vp)),
        draws,
        vec![FrameLight::new(0, [0.4, 1.0, 0.2], [1.0, 1.0, 1.0, 1.0])],
        light_vp,
        FrameFeatureSet::new(false, true, 1, 0),
    );
    let report = backend.present_packet(&packet);
    assert!(
        report.degraded_features().contains(&FrameFeature::Shadows),
        "Canvas 2D must report the PCF shadow as degraded"
    );
    assert!(
        report.raster().depth_cues.contact_shadows_drawn > 0,
        "Canvas 2D must draw the planar contact-shadow substitute"
    );
}

// ---------------------------------------------------------------------------
// Sky, specular and bloom — the GPU realizes all three; Canvas 2D reports the
// drops. This is what "skip it for the Canvas 2D version" means: the frame
// carries its full intent and the backend declares what it could not honour.
// ---------------------------------------------------------------------------

/// A moonlit look: a gradient sky with a bright body in it, plus bloom.
fn moonlit_look() -> axiom_host::FrameRenderLook {
    plain_look()
        .with_sky(
            axiom_host::FrameSky::gradient([0.02, 0.03, 0.06], [0.06, 0.08, 0.13]).with_body(
                // Toward -Z: `sdf_camera` looks that way, so the moon is IN
                // frame. Behind the camera the sky is just a dim gradient, well
                // under the bloom threshold, and the bloom proof is vacuous.
                [0.0, 0.35, -1.0],
                axiom_kernel::Radians::finite_or_zero(0.14),
                [1.6, 1.7, 1.9],
                Ratio::finite_or_zero(400.0),
                Ratio::finite_or_zero(0.3),
            ),
        )
        .with_bloom(axiom_host::FrameBloom::moonlit())
}

#[test]
fn sky_specular_and_bloom_render_on_the_gpu_and_are_reported_dropped_on_canvas2d() {
    let meshes = [fullscreen_quad(1)];
    let materials = [checker_texture(7)];
    let clear = [0.0, 0.0, 0.0, 1.0];
    let all = BackendCapabilityProfile::all();

    // A sky is load-bearing on the GPU: the same frame with the Sky capability
    // dropped clears to the flat colour instead of evaluating the gradient+moon.
    // (No draws at all, so the ONLY thing in frame is the sky.)
    // A REAL camera: with the identity, the inverse view-projection maps every
    // pixel to the same direction, so the sky renders one flat colour and proves
    // nothing about the gradient or the disc.
    let (camera, _, _) = sdf_camera();
    let with_sky = gpu_look(
        &meshes, &materials, &[], &[], IDENTITY, &[], clear, None, None, all, moonlit_look(),
        camera,
    );
    let without_sky = gpu_look(
        &meshes,
        &materials,
        &[],
        &[],
        IDENTITY,
        &[],
        clear,
        None,
        None,
        all.without(RenderCapability::Sky),
        moonlit_look(),
        camera,
    );
    assert!(
        differs(&with_sky, &without_sky),
        "the GPU must consult the Sky capability"
    );
    assert!(
        coverage(&with_sky, 4) > 0.2,
        "the sky is a GRADIENT with a disc in it, so it varies across the frame"
    );
    assert_eq!(
        coverage(&without_sky, 0),
        0.0,
        "a dropped Sky leaves the flat clear colour, which varies nowhere"
    );

    // Bloom is load-bearing too: it spreads the moon's light into the sky
    // around it, so the same frame with Bloom dropped is a different image.
    let unbloomed = gpu_look(
        &meshes,
        &materials,
        &[],
        &[],
        IDENTITY,
        &[],
        clear,
        None,
        None,
        all.without(RenderCapability::Bloom),
        moonlit_look(),
        camera,
    );
    assert!(
        differs(&with_sky, &unbloomed),
        "the GPU must consult the Bloom capability"
    );

    // Specular likewise, on a lit surface that authors a highlight strength.
    let lit_batch = [(
        1_u64,
        7_u64,
        glossy_instance(IDENTITY, IDENTITY, [1.0; 4], 0.9),
        1_u32,
    )];
    let shiny = gpu_look(
        &meshes,
        &materials,
        &[],
        &front_light(),
        IDENTITY,
        &lit_batch,
        clear,
        None,
        None,
        all,
        plain_look(),
        camera,
    );
    let matte = gpu_look(
        &meshes,
        &materials,
        &[],
        &front_light(),
        IDENTITY,
        &lit_batch,
        clear,
        None,
        None,
        all.without(RenderCapability::Specular),
        plain_look(),
        camera,
    );
    assert!(
        differs(&shiny, &matte),
        "the GPU must consult the Specular capability"
    );

    // All three degrade to a REPORTED drop, never a silent no-op...
    [
        RenderCapability::Sky,
        RenderCapability::Specular,
        RenderCapability::Bloom,
    ]
    .iter()
    .for_each(|c| assert_eq!(c.degradation(), CapabilityDegradation::Drop));

    // ...and the Canvas 2D backend, whose profile drops all three, says so.
    let mut backend = Canvas2dBackendApi::new(&request(W, H));
    backend.load_meshes(&meshes);
    let packet = FramePacket::new(
        1,
        1,
        FrameViewport::new(W, H),
        clear,
        Some(FrameCamera::new(IDENTITY, IDENTITY, IDENTITY)),
        vec![
            FrameDrawItem::new(0, 1, 7, IDENTITY, IDENTITY, [1.0; 4], false)
                .with_specular(Ratio::finite_or_zero(0.8)),
        ],
        Vec::new(),
        IDENTITY,
        FrameFeatureSet::new(false, false, 0, 0),
    )
    .with_sky(moonlit_look().sky().expect("the moonlit look carries a sky"))
    .with_bloom(
        moonlit_look()
            .bloom()
            .expect("the moonlit look carries bloom"),
    );
    let degraded = backend.present_packet(&packet).degraded_features().to_vec();
    assert!(degraded.contains(&FrameFeature::Sky), "{degraded:?}");
    assert!(
        degraded.contains(&FrameFeature::SpecularHighlight),
        "{degraded:?}"
    );
    assert!(degraded.contains(&FrameFeature::Bloom), "{degraded:?}");
    // But NOT the colour grade, which this backend genuinely performs — the
    // whole reason Bloom is its own capability bit and not part of PostProcess.
    assert!(
        !degraded.contains(&FrameFeature::PostProcessing),
        "{degraded:?}"
    );
}
