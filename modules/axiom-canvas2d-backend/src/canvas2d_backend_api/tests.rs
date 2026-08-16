//! Tests for the Canvas 2D backend facade.
//!
//! Split out of `canvas2d_backend_api.rs` so the facade file stays inside the
//! engine's per-file budget — the same `foo.rs` + `foo/tests.rs` shape
//! `software_rasterizer` and `draw2d_raster` already use.

use super::*;
use axiom_host::{
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode,
};
use axiom_kernel::{KernelApi, Ratio};

/// Build a validated presentation request the way windowing does.
pub(super) fn request(width: u32, height: u32) -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(width, height, Ratio::new(1.0).expect("finite"))
        .expect("valid viewport");
    let target = host
        .presentation_target(&kernel, 1, "axiom-canvas2d-test")
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
        .expect("valid request")
}

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn vertex(pos: [f32; 3], color: [f32; 4]) -> [f32; 12] {
    [
        pos[0], pos[1], pos[2], 0.0, 1.0, 0.0, 0.0, 0.0, color[0], color[1], color[2], color[3],
    ]
}

fn ground(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
    let c = [0.2, 0.6, 0.3, 1.0];
    let mut v = Vec::new();
    v.extend_from_slice(&vertex([-1.0, -1.0, 0.0], c));
    v.extend_from_slice(&vertex([1.0, -1.0, 0.0], c));
    v.extend_from_slice(&vertex([1.0, 1.0, 0.0], c));
    v.extend_from_slice(&vertex([-1.0, 1.0, 0.0], c));
    (id, v, vec![0, 1, 2, 0, 2, 3])
}

/// The `ground` quad at an explicit NDC depth, for the cues that only bite
/// away from the near plane (fog).
fn ground_at_depth(id: u64, z: f32) -> (u64, Vec<f32>, Vec<u32>) {
    let c = [0.2, 0.6, 0.3, 1.0];
    let mut v = Vec::new();
    v.extend_from_slice(&vertex([-1.0, -1.0, z], c));
    v.extend_from_slice(&vertex([1.0, -1.0, z], c));
    v.extend_from_slice(&vertex([1.0, 1.0, z], c));
    v.extend_from_slice(&vertex([-1.0, 1.0, z], c));
    (id, v, vec![0, 1, 2, 0, 2, 3])
}

fn packet(
    draws: Vec<axiom_host::FrameDrawItem>,
    features: axiom_host::FrameFeatureSet,
) -> FramePacket {
    use axiom_host::{FrameCamera, FrameViewport};
    FramePacket::new(
        2,
        120,
        FrameViewport::new(800, 600),
        [0.4, 0.6, 0.9, 1.0],
        Some(FrameCamera::new(IDENTITY, IDENTITY, IDENTITY)),
        draws,
        Vec::new(),
        IDENTITY,
        features,
    )
}

#[test]
fn new_reads_surface_size_from_the_request() {
    let backend = Canvas2dBackendApi::new(&request(800, 600));
    assert_eq!(backend.width(), 800);
    assert_eq!(backend.height(), 600);
    assert!(format!("{backend:?}").starts_with("Canvas2dBackendApi"));
}

#[test]
fn presents_a_packet_to_a_canvas2d_report_with_raster_stats() {
    use axiom_host::{FrameDrawItem, FrameFeatureSet};
    let mut backend = Canvas2dBackendApi::new(&request(800, 600));
    backend.load_meshes(&[ground(7)]);
    let draws = vec![FrameDrawItem::new(
        1,
        7,
        9,
        IDENTITY,
        IDENTITY,
        [1.0, 0.0, 0.0, 1.0],
        false,
    )];
    let report =
        backend.present_packet(&packet(draws, FrameFeatureSet::new(false, false, 0, 0)));

    assert_eq!(report.backend(), BackendKind::Canvas2d);
    assert_eq!(report.frame_index(), 2);
    assert_eq!(report.tick(), 120);
    assert_eq!(report.submitted_draws(), 1);
    assert_eq!(report.skipped_draws(), 0);
    assert_eq!(report.critical_coverage_skipped(), 0);
    // The framebuffer is the low internal resolution, aspect-matched to the
    // 800×600 (4:3) surface — Low tier's 240 long-edge budget → 240×180, not a
    // distorting fixed 16:9, and not the full 800×600 canvas.
    assert_eq!(report.raster().framebuffer_width, 240);
    assert_eq!(report.raster().framebuffer_height, 180);
    assert_eq!(report.raster().rasterized_triangles, 2);
    assert!(report.raster().depth_written_pixels > 0);
    assert_eq!(report.raster().terrain_draws_preserved, 1);
    assert!(report.raster().candidate_pixels > 0);
    assert!(!report.raster().budget_exhausted);
}

/// An authored [`axiom_host::FrameDepthFog`] must reach the software
/// rasterizer's cues — *including its colour*, which is the one fog field
/// that is not a scalar and so needs its own arm.
///
/// The distinction that matters: with no authored fog this backend recedes
/// toward the frame's **clear colour**; with authored fog it must recede
/// toward the **fog's own colour** instead. Asserting merely "some fog was
/// applied" would pass either way, so this drives a fog colour deliberately
/// unlike the clear (the packet clears blue `[0.4, 0.6, 0.9]`; the fog is
/// red) and checks the image moves toward red specifically.
///
/// The quad sits at NDC z=0.6 rather than the shared helper's z=0.
/// `fog_mix` is zero at the near plane by definition, and an authored
/// `FrameDepthFog` carries `Ratio` near/far — which cannot go negative — so
/// a frame whose geometry sits at depth 0 can never show authored fog at
/// all. Depth is what makes this arm observable.
#[test]
fn an_authored_fog_colour_reaches_the_raster_and_differs_from_the_clear_colour() {
    use axiom_host::{FrameDepthFog, FrameDrawItem, FrameFeatureSet};
    use axiom_kernel::Ratio;

    let draws = || {
        vec![FrameDrawItem::new(
            1,
            7,
            9,
            IDENTITY,
            IDENTITY,
            [1.0, 1.0, 1.0, 1.0],
            false,
        )]
    };
    let features = || FrameFeatureSet::new(false, false, 0, 0);

    let fog = FrameDepthFog::new(
        Ratio::new(0.0).expect("finite"),
        Ratio::new(1.0).expect("finite"),
        Ratio::new(1.0).expect("finite"),
        [1.0, 0.0, 0.0],
    );

    let mut fogged_backend = Canvas2dBackendApi::new(&request(800, 600));
    fogged_backend.load_meshes(&[ground_at_depth(7, 0.6)]);
    let fogged = fogged_backend
        .render_offscreen_rgba(&packet(draws(), features()).with_depth_fog(fog));

    let mut plain_backend = Canvas2dBackendApi::new(&request(800, 600));
    plain_backend.load_meshes(&[ground_at_depth(7, 0.6)]);
    let plain = plain_backend.render_offscreen_rgba(&packet(draws(), features()));

    assert_eq!(fogged.1, plain.1, "same framebuffer width");
    assert_eq!(fogged.2, plain.2, "same framebuffer height");
    assert_ne!(
        fogged.0, plain.0,
        "an authored red fog must not rasterize identically to no authored fog"
    );
    // It moved the image toward red specifically, not merely "somewhere".
    let sum = |bytes: &[u8], offset: usize| -> u64 {
        bytes
            .iter()
            .skip(offset)
            .step_by(4)
            .map(|&b| u64::from(b))
            .sum()
    };
    assert!(
        sum(&fogged.0, 0) > sum(&plain.0, 0),
        "red channel rises under a red fog"
    );
    assert!(
        sum(&fogged.0, 2) < sum(&plain.0, 2),
        "blue channel falls under a red fog"
    );
}

/// One skinned quad vertex: the 20-float stream (pos·normal·uv·colour·
/// joints·weights) fully weighted to bone 0.
fn skinned_vertex(pos: [f32; 3], color: [f32; 4]) -> [f32; 20] {
    [
        pos[0], pos[1], pos[2], // position
        0.0, 1.0, 0.0, // normal
        0.0, 0.0, // uv
        color[0], color[1], color[2], color[3], // colour
        0.0, 0.0, 0.0, 0.0, // joints (bone 0)
        1.0, 0.0, 0.0, 0.0, // weights (all on bone 0)
    ]
}

fn skinned_quad(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
    let c = [0.9, 0.1, 0.1, 1.0];
    let mut v = Vec::new();
    v.extend_from_slice(&skinned_vertex([-0.5, -0.5, 0.0], c));
    v.extend_from_slice(&skinned_vertex([0.5, -0.5, 0.0], c));
    v.extend_from_slice(&skinned_vertex([0.5, 0.5, 0.0], c));
    v.extend_from_slice(&skinned_vertex([-0.5, 0.5, 0.0], c));
    (id, v, vec![0, 1, 2, 0, 2, 3])
}

#[test]
fn skinned_body_renders_via_cpu_skinning() {
    use axiom_host::FrameFeatureSet;
    let mut backend = Canvas2dBackendApi::new(&request(800, 600));
    // A bake-once skinned quad, no ordinary meshes at all — so anything drawn
    // came exclusively through the CPU skinning path.
    backend.load_skinned_meshes(&[skinned_quad(3)]);
    // One skinned draw, identity palette (bone 0 = identity) → posed at bind.
    let skinned = vec![(
        3_u64,
        9_u64,
        IDENTITY,
        IDENTITY,
        [1.0, 1.0, 1.0, 1.0],
        vec![IDENTITY],
    )];
    let p = packet(Vec::new(), FrameFeatureSet::new(false, false, 0, 0));

    let report = backend.present_packet_skinned(&p, &skinned);
    // The skinned quad's two triangles were projected + rasterized — the
    // athlete geometry the plain `present_packet` (no skinned) would drop.
    assert_eq!(report.raster().rasterized_triangles, 2);
    assert!(report.raster().depth_written_pixels > 0);

    // The offscreen peer paints the same body into the RGBA buffer.
    let (rgba, w, h) = backend.render_offscreen_rgba_skinned(&p, &skinned);
    assert_eq!(rgba.len() as u32, w * h * 4);
    // Some pixel reads red-dominant (the quad's colour), distinct from the
    // bluish clear — proof the skinned geometry actually shaded pixels.
    assert!(rgba.chunks_exact(4).any(|px| px[0] > px[2]));
}

#[test]
fn skinned_draw_with_unloaded_mesh_is_dropped() {
    use axiom_host::FrameFeatureSet;
    let backend = Canvas2dBackendApi::new(&request(800, 600));
    // No skinned mesh uploaded, so the draw's mesh id resolves to nothing and
    // the draw is dropped before rasterization (nothing paints).
    let skinned = vec![(99_u64, 0_u64, IDENTITY, IDENTITY, [1.0; 4], vec![IDENTITY])];
    let p = packet(Vec::new(), FrameFeatureSet::new(false, false, 0, 0));
    let report = backend.present_packet_skinned(&p, &skinned);
    assert_eq!(report.raster().rasterized_triangles, 0);
}

#[test]
fn render_offscreen_rgba_returns_the_blittable_framebuffer() {
    use axiom_host::{FrameDrawItem, FrameFeatureSet};
    let mut backend = Canvas2dBackendApi::new(&request(800, 600));
    backend.load_meshes(&[ground(7)]);
    // Low tier at the 800×600 (4:3) surface → a 240×180 internal framebuffer
    // (aspect-matched to the surface, the forced-fallback default tier).
    backend.set_quality_level(1);
    let draws = vec![FrameDrawItem::new(
        1,
        7,
        9,
        IDENTITY,
        IDENTITY,
        [1.0, 0.0, 0.0, 1.0],
        false,
    )];
    let p = packet(draws, FrameFeatureSet::new(false, false, 0, 0));

    let (rgba, w, h) = backend.render_offscreen_rgba(&p);
    // The dimensions are the internal raster resolution, and the buffer is a
    // tight RGBA8 image of exactly that size.
    assert_eq!((w, h), (240, 180));
    assert_eq!(rgba.len() as u32, w * h * 4);
    // It is the same framebuffer `present_packet` would blit: same size, and
    // every pixel opaque.
    let report = backend.present_packet(&p);
    assert_eq!(report.raster().framebuffer_width, w);
    assert_eq!(report.raster().framebuffer_height, h);
    assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
    // Pure function of the packet: identical bytes every call.
    let (again, _, _) = backend.render_offscreen_rgba(&p);
    assert_eq!(rgba, again);
}

#[test]
fn frame_ambient_lifts_the_lit_result() {
    use axiom_host::{FrameAmbient, FrameDrawItem, FrameFeatureSet};
    let mut backend = Canvas2dBackendApi::new(&request(800, 600));
    backend.load_meshes(&[ground(7)]);
    backend.set_quality_level(1);
    let draws = vec![FrameDrawItem::new(
        1,
        7,
        9,
        IDENTITY,
        IDENTITY,
        [1.0, 1.0, 1.0, 1.0],
        false,
    )];
    // No directional light → the ground is lit by the hemisphere ambient alone.
    let base = packet(draws.clone(), FrameFeatureSet::new(false, false, 0, 0));
    let (dim, _, _) = backend.render_offscreen_rgba(&base);
    // A bright frame ambient (the `Some` path) lifts the ground above the default.
    let bright = base
        .clone()
        .with_ambient(FrameAmbient::new([0.95, 0.95, 0.95], [0.95, 0.95, 0.95]));
    let (lit, _, _) = backend.render_offscreen_rgba(&bright);
    assert_ne!(dim, lit);
    assert!(dim.iter().zip(&lit).any(|(d, l)| l > d));
}

#[test]
fn set_capability_profile_gates_the_volumetric_pass() {
    use axiom_host::{
        BackendCapabilityProfile, FrameCamera, FrameFeatureSet, FrameLight, FrameViewport,
        FrameVolumetrics, RenderCapability,
    };
    // A view_proj with m[11] = 1 puts a +z to-light on-screen, and a bright uniform
    // frame (no draws → just the bright clear) exceeds the god-ray leak threshold, so
    // the pass produces a real difference only when a backend runs it.
    let front_vp = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let vol = FramePacket::new(
        0,
        0,
        FrameViewport::new(800, 600),
        [0.9, 0.9, 0.9, 1.0],
        Some(FrameCamera::new(IDENTITY, IDENTITY, front_vp)),
        Vec::new(),
        vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])],
        IDENTITY,
        FrameFeatureSet::new(false, true, 1, 0),
    )
    .with_volumetrics(FrameVolumetrics::low_poly());
    // Default profile (all): the god-ray pass runs.
    let full = Canvas2dBackendApi::new(&request(800, 600));
    let (a, _, _) = full.render_offscreen_rgba(&vol);
    // set_capability_profile restricting Volumetrics: the pass is skipped.
    let mut restricted = Canvas2dBackendApi::new(&request(800, 600));
    restricted.set_capability_profile(
        BackendCapabilityProfile::all().without(RenderCapability::Volumetrics),
    );
    let (b, _, _) = restricted.render_offscreen_rgba(&vol);
    assert_ne!(
        a, b,
        "set_capability_profile gates the god-ray pass on Canvas 2D"
    );
}

#[test]
fn set_quality_level_changes_the_internal_resolution() {
    use axiom_host::{FrameDrawItem, FrameFeatureSet};
    let mut backend = Canvas2dBackendApi::new(&request(800, 600));
    backend.load_meshes(&[ground(7)]);
    let draws = vec![FrameDrawItem::new(
        1, 7, 9, IDENTITY, IDENTITY, [1.0; 4], false,
    )];
    // Level 0 → UltraLow, 160×120 at the 800×600 (4:3) surface.
    backend.set_quality_level(0);
    let r0 = backend.present_packet(&packet(
        draws.clone(),
        FrameFeatureSet::new(false, false, 0, 0),
    ));
    assert_eq!(r0.raster().framebuffer_width, 160);
    assert_eq!(r0.raster().framebuffer_height, 120);
    // Level 2 → Medium, 320×240 (more candidate pixels than UltraLow).
    backend.set_quality_level(2);
    let r2 = backend.present_packet(&packet(draws, FrameFeatureSet::new(false, false, 0, 0)));
    assert_eq!(r2.raster().framebuffer_width, 320);
    assert_eq!(r2.raster().framebuffer_height, 240);
    assert!(r2.raster().candidate_pixels > r0.raster().candidate_pixels);
}

#[test]
fn unknown_mesh_is_skipped_without_critical_violation() {
    use axiom_host::{FrameDrawItem, FrameFeatureSet};
    let backend = Canvas2dBackendApi::new(&request(640, 480));
    let draws = vec![FrameDrawItem::new(
        1, 404, 9, IDENTITY, IDENTITY, [1.0; 4], false,
    )];
    let report =
        backend.present_packet(&packet(draws, FrameFeatureSet::new(false, false, 0, 0)));
    assert_eq!(report.submitted_draws(), 0);
    assert_eq!(report.skipped_draws(), 1);
    assert_eq!(report.critical_coverage_skipped(), 0);
    assert_eq!(report.degraded_materials(), 0);
}

#[test]
fn reports_degraded_textures_and_shadows_and_materials() {
    use axiom_host::{FrameDrawItem, FrameFeatureSet};
    let mut backend = Canvas2dBackendApi::new(&request(320, 180));
    backend.load_meshes(&[ground(7)]);
    let draws = vec![FrameDrawItem::new(
        1, 7, 13, IDENTITY, IDENTITY, [1.0; 4], false,
    )];
    let report = backend.present_packet(&packet(draws, FrameFeatureSet::new(true, true, 1, 0)));
    assert!(report
        .degraded_features()
        .contains(&FrameFeature::AlbedoSampling));
    assert!(report.degraded_features().contains(&FrameFeature::Shadows));
    assert_eq!(report.degraded_materials(), 1);
}

/// "Skip it for the Canvas 2D version", done through the capability system
/// rather than an app-level backend check: the frame carries the full intent
/// and this backend enumerates, per frame, exactly the parts it did not
/// honour. A silent omission would be indistinguishable from a bug.
#[test]
fn reports_the_sky_specular_and_bloom_it_cannot_render() {
    use axiom_host::{FrameBloom, FrameDrawItem, FrameFeatureSet, FrameSky};
    let mut backend = Canvas2dBackendApi::new(&request(320, 180));
    backend.load_meshes(&[ground(7)]);
    let draws = vec![
        FrameDrawItem::new(1, 7, 13, IDENTITY, IDENTITY, [1.0; 4], false)
            .with_specular(axiom_kernel::Ratio::finite_or_zero(0.7)),
    ];
    let rich = packet(draws.clone(), FrameFeatureSet::new(false, false, 0, 0))
        .with_sky(FrameSky::gradient([0.02, 0.03, 0.06], [0.06, 0.08, 0.13]))
        .with_bloom(FrameBloom::moonlit());
    let report = backend.present_packet(&rich);
    let degraded = report.degraded_features();
    assert!(degraded.contains(&FrameFeature::Sky), "{degraded:?}");
    assert!(
        degraded.contains(&FrameFeature::SpecularHighlight),
        "{degraded:?}"
    );
    assert!(degraded.contains(&FrameFeature::Bloom), "{degraded:?}");
    // The whole-image colour grade is NOT dropped: this backend genuinely
    // performs it. That is the distinction `PostProcess` vs `Bloom` exists for.
    assert!(!degraded.contains(&FrameFeature::PostProcessing), "{degraded:?}");

    // ...and a frame that asks for none of the three declares none of them.
    let plain = packet(draws, FrameFeatureSet::new(false, false, 0, 0));
    let quiet = backend.present_packet(&plain);
    assert!(!quiet.degraded_features().contains(&FrameFeature::Sky));
    assert!(!quiet.degraded_features().contains(&FrameFeature::Bloom));
    // Specular still is: the draw authored it, even with no sky or bloom.
    assert!(quiet
        .degraded_features()
        .contains(&FrameFeature::SpecularHighlight));
}

/// The *substitute* half of the same mechanism, which the drop half above
/// does not exercise: a fog authored with a per-metre extinction rate is
/// still rendered here — as its normalized-depth ramp — and that
/// substitution is declared rather than silent. A fog carrying only a depth
/// window declares nothing, because there is nothing this backend failed to
/// honour about it.
#[test]
fn reports_the_distance_fog_it_substitutes_with_the_depth_ramp() {
    use axiom_host::{FrameDepthFog, FrameDrawItem, FrameFeatureSet};
    use axiom_kernel::Ratio;
    let mut backend = Canvas2dBackendApi::new(&request(320, 180));
    backend.load_meshes(&[ground(7)]);
    let draws = vec![FrameDrawItem::new(
        1, 7, 13, IDENTITY, IDENTITY, [1.0; 4], false,
    )];
    let window = FrameDepthFog::new(
        Ratio::finite_or_zero(0.9),
        Ratio::finite_or_zero(1.0),
        Ratio::finite_or_zero(0.8),
        [0.7, 0.75, 0.8],
    );
    let ramp_only = backend.present_packet(
        &packet(draws.clone(), FrameFeatureSet::new(false, false, 0, 0))
            .with_depth_fog(window),
    );
    assert!(!ramp_only
        .degraded_features()
        .contains(&FrameFeature::AerialPerspective));

    let with_air = backend.present_packet(
        &packet(draws, FrameFeatureSet::new(false, false, 0, 0))
            .with_depth_fog(window.with_extinction(Ratio::finite_or_zero(0.004))),
    );
    let degraded = with_air.degraded_features();
    assert!(
        degraded.contains(&FrameFeature::AerialPerspective),
        "{degraded:?}"
    );
}

#[test]
fn replace_geometry_keeps_the_draw_rasterizable() {
    use axiom_host::{FrameDrawItem, FrameFeatureSet};
    let mut backend = Canvas2dBackendApi::new(&request(320, 180));
    backend.load_meshes(&[ground(7)]);
    let draws = vec![FrameDrawItem::new(
        1, 7, 9, IDENTITY, IDENTITY, [1.0; 4], false,
    )];
    let p = packet(draws, FrameFeatureSet::new(false, false, 0, 0));
    assert_eq!(backend.present_packet(&p).submitted_draws(), 1);

    let (_, v, i) = ground(7);
    backend.replace_geometry(7, &v, &i);
    assert_eq!(backend.present_packet(&p).submitted_draws(), 1);
}

#[test]
fn renders_a_draw2d_list_with_a_sprite_at_the_canvas_size() {
    use axiom_host::{Common2d, Draw2dCommand, SpriteDraw2d, TextureId};
    use axiom_math::Vec2;

    let mut backend = Canvas2dBackendApi::new(&request(4, 4));
    // A 1×1 opaque red texture, blitted as a 1×1 sprite at the origin.
    backend.load_textures(&[(3, 1, 1, vec![255, 0, 0, 255])]);
    let one = Ratio::new(1.0).expect("finite");
    let opts = SpriteDraw2d::new(
        axiom_host::Rect::new(Vec2::ZERO, Vec2::ONE),
        Vec2::ZERO,
        axiom_host::Rgba::new(one, one, one, one),
        false,
        false,
    );
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::sprite(
        (0, axiom_math::Mat3::IDENTITY, Common2d::new(0, one)),
        TextureId::from_raw(3),
        opts,
    ));
    list.sort_commands();

    let (rgba, w, h) = backend.render_draw2d_rgba(&list);
    assert_eq!((w, h), (4, 4));
    assert_eq!(rgba.len() as u32, w * h * 4);
    // Pixel (0,0) is the opaque red sprite; an untouched pixel is transparent.
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    let untouched = ((3 * 4 + 3) * 4) as usize;
    assert_eq!(&rgba[untouched..untouched + 4], &[0, 0, 0, 0]);
}

// ---------------------------------------------------------------------------
// Authored surfaces: what this backend honours, and what it reports.
// ---------------------------------------------------------------------------

/// A vec4 base colour that is `Uv.x` in every lane.
fn uv_x_color() -> axiom_field::FieldGraph {
    let (builder, uv) =
        axiom_field::FieldBuilder::new(axiom_field::FieldId::of_name("c2d/api/uv"), 1).push(
            axiom_field::FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
    let (builder, lane) = builder.push(
        axiom_field::FieldOp::Component,
        vec![axiom_recipe::Param::int(0)],
        vec![uv],
    );
    let (builder, splat) = builder.push(
        axiom_field::FieldOp::Compose,
        vec![axiom_recipe::Param::int(4)],
        vec![lane, lane, lane, lane],
    );
    builder.build(splat)
}

/// A vec4 base colour that is the presentation `Time` in every lane — the
/// smallest thing whose rendered pixels are a function of the clock.
fn clock_color() -> axiom_field::FieldGraph {
    let (builder, clock) =
        axiom_field::FieldBuilder::new(axiom_field::FieldId::of_name("c2d/api/clock"), 1).push(
            axiom_field::FieldOp::Time,
            Vec::new(),
            Vec::new(),
        );
    let (builder, splat) = builder.push(
        axiom_field::FieldOp::Compose,
        vec![axiom_recipe::Param::int(4)],
        vec![clock, clock, clock, clock],
    );
    builder.build(splat)
}

/// **The presentation time is the packet's, on this backend too.** It used to be
/// a loose parameter on these two entries, which meant the software arm and the
/// GPU arm each had their own idea of what second it was; both now read
/// `FramePacket::time`, so a frame handed to either produces the same clock.
///
/// The test is the observable one: the same packet at two times rasterizes
/// different pixels, and the same packet at the same time rasterizes identical
/// ones — replayable, because the time is supplied and never sampled.
#[test]
fn a_clock_reading_surface_samples_the_packets_own_time_and_replays_exactly() {
    use axiom_host::FrameFeatureSet;
    let mut backend = Canvas2dBackendApi::new(&request(64, 48));
    backend.load_meshes(&[ground(7)]);
    let surface = axiom_surface::SurfaceBuilder::new()
        .field(axiom_surface::SurfaceChannel::BaseColor, clock_color())
        .build()
        .expect("a vec4 time field is a legal base colour");
    let at = |seconds: f32| {
        let frame = packet(
            vec![surfaced_draw(surface.digest().raw())],
            FrameFeatureSet::new(false, false, 0, 0),
        )
        .with_time(axiom_kernel::Seconds::finite_or_zero(seconds));
        backend
            .render_offscreen_rgba_with_surfaces(&frame, std::slice::from_ref(&surface))
            .0
    };
    let dark = at(0.05);
    let bright = at(0.8);
    assert_ne!(dark, bright, "the rasterized pixels must follow the clock");
    assert_eq!(dark, at(0.05), "the same tick must replay exactly");
}

fn surfaced_draw(program: u64) -> axiom_host::FrameDrawItem {
    axiom_host::FrameDrawItem::new(1, 7, 13, IDENTITY, IDENTITY, [1.0; 4], false)
        .with_surface_program(program)
}

/// A frame whose surface this backend was handed is **honoured**, not degraded:
/// the channels are evaluated per triangle instead of per fragment, which is a
/// substitute, and a substitute the backend actually performed is not a drop.
#[test]
fn a_surface_this_backend_was_handed_is_honoured_and_not_reported() {
    use axiom_host::FrameFeatureSet;
    let mut backend = Canvas2dBackendApi::new(&request(320, 180));
    backend.load_meshes(&[ground(7)]);
    let surface = axiom_surface::SurfaceBuilder::new()
        .field(axiom_surface::SurfaceChannel::BaseColor, uv_x_color())
        .build()
        .expect("a vec4 uv field is a legal base colour");
    let report = backend.present_packet_with_surfaces(
        &packet(
            vec![surfaced_draw(surface.digest().raw())],
            FrameFeatureSet::new(false, false, 0, 0),
        ),
        std::slice::from_ref(&surface),
    );
    assert!(
        !report
            .degraded_features()
            .contains(&FrameFeature::ProceduralSurface),
        "{:?}",
        report.degraded_features()
    );
    assert_eq!(report.submitted_draws(), 1);
    // And the pixels moved: the surface really was evaluated, not skipped.
    let (surfaced, _, _) = backend.render_offscreen_rgba_with_surfaces(
        &packet(
            vec![surfaced_draw(surface.digest().raw())],
            FrameFeatureSet::new(false, false, 0, 0),
        ),
        std::slice::from_ref(&surface),
    );
    let (plain, _, _) = backend.render_offscreen_rgba(&packet(
        vec![surfaced_draw(surface.digest().raw())],
        FrameFeatureSet::new(false, false, 0, 0),
    ));
    assert_ne!(surfaced, plain);
}

/// A draw naming a surface this backend was never handed is a real drop, and it
/// says so — the one thing that must never be a silent no-op.
#[test]
fn a_draw_naming_an_unhandled_surface_is_reported_dropped() {
    use axiom_host::FrameFeatureSet;
    let mut backend = Canvas2dBackendApi::new(&request(320, 180));
    backend.load_meshes(&[ground(7)]);
    let report = backend.present_packet_with_surfaces(
        &packet(
            vec![surfaced_draw(0xFEED_FACE)],
            FrameFeatureSet::new(false, false, 0, 0),
        ),
        &[],
    );
    assert!(report
        .degraded_features()
        .contains(&FrameFeature::ProceduralSurface));
    // A frame with no authored surface at all reports nothing — the drop is
    // keyed on what the frame asked for, not on the backend's shape.
    let quiet = backend.present_packet(&packet(
        vec![axiom_host::FrameDrawItem::new(
            1, 7, 13, IDENTITY, IDENTITY, [1.0; 4], false,
        )],
        FrameFeatureSet::new(false, false, 0, 0),
    ));
    assert!(!quiet
        .degraded_features()
        .contains(&FrameFeature::ProceduralSurface));
}

/// A surface that displaces geometry is reported: this path shades triangles,
/// it never moves them, however finely it samples.
#[test]
fn a_displacing_surface_is_reported_because_shading_cannot_move_geometry() {
    use axiom_host::FrameFeatureSet;
    let mut backend = Canvas2dBackendApi::new(&request(320, 180));
    backend.load_meshes(&[ground(7)]);
    let surface = axiom_surface::SurfaceBuilder::new()
        .constant(
            axiom_surface::SurfaceChannel::Displacement,
            axiom_field::FieldValue::vec3(axiom_math::Vec3::new(0.0, 1.0, 0.0)),
        )
        .build()
        .expect("a vec3 constant is a legal displacement");
    let report = backend.present_packet_with_surfaces(
        &packet(
            vec![surfaced_draw(surface.digest().raw())],
            FrameFeatureSet::new(false, false, 0, 0),
        ),
        std::slice::from_ref(&surface),
    );
    assert!(report
        .degraded_features()
        .contains(&FrameFeature::ProceduralSurface));
}

/// Roughness and metallic are **not faked**. A view-independent per-triangle
/// shade has no highlight for a roughness to tighten, so a surface that binds
/// either loses exactly the specular term — and the report names it.
#[test]
fn a_surface_binding_roughness_reports_the_specular_term_it_cannot_express() {
    use axiom_host::FrameFeatureSet;
    let mut backend = Canvas2dBackendApi::new(&request(320, 180));
    backend.load_meshes(&[ground(7)]);
    let rough = axiom_surface::SurfaceBuilder::new()
        .constant(
            axiom_surface::SurfaceChannel::Roughness,
            axiom_field::FieldValue::scalar(axiom_recipe::Scalar::new(0.1)),
        )
        .build()
        .expect("a scalar constant is a legal roughness");
    let report = backend.present_packet_with_surfaces(
        &packet(
            vec![surfaced_draw(rough.digest().raw())],
            FrameFeatureSet::new(false, false, 0, 0),
        ),
        std::slice::from_ref(&rough),
    );
    assert!(report
        .degraded_features()
        .contains(&FrameFeature::SpecularHighlight));
    // It is not a procedural-surface drop: the colour channels WERE honoured.
    assert!(!report
        .degraded_features()
        .contains(&FrameFeature::ProceduralSurface));
}

/// The capability lever still governs: a profile that clears the bit makes this
/// backend report the surface rather than evaluate it.
#[test]
fn a_profile_without_the_capability_reports_the_surface_it_declines_to_evaluate() {
    use axiom_host::FrameFeatureSet;
    let mut backend = Canvas2dBackendApi::new(&request(320, 180));
    backend.load_meshes(&[ground(7)]);
    backend.set_capability_profile(
        axiom_host::BackendCapabilityProfile::canvas2d()
            .without(RenderCapability::ProceduralSurface),
    );
    let surface = axiom_surface::SurfaceBuilder::new()
        .field(axiom_surface::SurfaceChannel::BaseColor, uv_x_color())
        .build()
        .expect("legal");
    let report = backend.present_packet_with_surfaces(
        &packet(
            vec![surfaced_draw(surface.digest().raw())],
            FrameFeatureSet::new(false, false, 0, 0),
        ),
        std::slice::from_ref(&surface),
    );
    assert!(report
        .degraded_features()
        .contains(&FrameFeature::ProceduralSurface));
}
