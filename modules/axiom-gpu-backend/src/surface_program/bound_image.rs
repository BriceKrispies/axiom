//! **The picture.** A prepared surface program, bound to the real renderer, and
//! the pixels it produced.
//!
//! Every other proof in this directory is about *text* or *values*:
//! [`crate::surface_program::parity`] holds the emitted channel expressions to
//! `axiom-field`'s evaluator, [`crate::surface_program::parity_vertex`] does the
//! same for the vertex stage, and
//! [`crate::surface_program::parity_lighting`] renders the main pass's `fs`
//! against a synthetic rig. None of them could show that an authored surface
//! *changes a frame*, because until the program cache existed the main pass
//! compiled only the DEFAULT programs — so this is the debt those manifests left.
//!
//! What runs here is the whole thing: a real
//! [`crate::scene_renderer::SceneRenderer`], a real mesh and material, a real
//! [`crate::surface_program::cache::SurfaceProgramCatalog`] prepared at the
//! barrier and compiled by [`crate::surface_program::compile`], a real
//! `record`, and a texture read back. The assertions are on **captured pixels**,
//! never on a shader string.
//!
//! ## The rig
//!
//! One quad in the `z = 0.5` plane spanning NDC `[-0.5, 0.5]` in both axes, an
//! identity MVP and an identity world, a 1×1 white albedo, no lights, and an
//! `Unlit` lighting model. `Unlit` is what makes the readback legible: the pass's
//! whole lighting sum collapses to `base_color.rgb + emission`, so a pixel *is*
//! the surface's base colour and a silhouette *is* where the vertex stage put the
//! geometry. Nothing about the lighting maths is restated — the shader that runs
//! is `crate::scene_wgsl`, spliced exactly as the renderer splices it.
//!
//! The target is `Rgba8Unorm` — linear, not sRGB — so a byte read back is the
//! channel the fragment stage wrote, times 255.

use axiom_field::{FieldBuilder, FieldGraph, FieldId, FieldOp, FieldValue};
use axiom_host::{BackendCapabilityProfile, FrameAmbient, FrameCamera, FrameRenderLook, MaterialTexture};
use axiom_math::{Vec3, Vec4};
use axiom_recipe::Param;
use axiom_surface::{LightingModel, Surface, SurfaceBuilder, SurfaceChannel};

use crate::scene_renderer::{create_depth_view, SceneRenderer};
use crate::surface_program::cache::SurfaceProgramCatalog;
use crate::surface_program::parity::ParityGpu;

/// **One** device for every test in this module.
///
/// Each capture builds its own `SceneRenderer` — pipelines, shadow map, buffers,
/// the lot — so the thing under test is rebuilt from scratch every time; what is
/// shared is only the adapter beneath it. Acquiring a fresh `wgpu::Device` per
/// test *and* running the tests in parallel is what a driver actually objects
/// to, and this module would otherwise open five at once alongside the parity
/// modules' own.
fn gpu() -> &'static ParityGpu {
    static SHARED: std::sync::OnceLock<ParityGpu> = std::sync::OnceLock::new();
    SHARED.get_or_init(ParityGpu::acquire)
}

/// The captured image's edge length. 64 texels is exactly one 256-byte row, the
/// alignment `copy_texture_to_buffer` requires, so the readback needs no
/// unpadding step to go wrong in.
const EDGE: u32 = 64;

/// The one mesh id and the one material id the rig draws.
const MESH: u64 = 1;
const MATERIAL: u64 = 1;

/// The column-major identity.
const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// The colour target: **linear**, so a byte read back is the channel the shader
/// wrote rather than that channel through an sRGB curve.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A quad in the `z = 0.5` plane spanning NDC `[-0.5, 0.5]`, with `uv.x` running
/// 0 → 1 left to right. Twelve floats per vertex: position, normal, uv, colour.
fn quad() -> (u64, Vec<f32>, Vec<u32>) {
    let corners = [
        (-0.5_f32, -0.5_f32, 0.0_f32, 0.0_f32),
        (0.5, -0.5, 1.0, 0.0),
        (0.5, 0.5, 1.0, 1.0),
        (-0.5, 0.5, 0.0, 1.0),
    ];
    let vertices: Vec<f32> = corners
        .iter()
        .flat_map(|(x, y, u, v)| [*x, *y, 0.5, 0.0, 0.0, 1.0, *u, *v, 1.0, 1.0, 1.0, 1.0])
        .collect();
    (MESH, vertices, vec![0, 1, 2, 0, 2, 3])
}

/// The rig's one instance: an identity MVP, an identity world, a white tint and
/// no emissive or specular.
fn instance() -> Vec<f32> {
    IDENTITY
        .iter()
        .chain(IDENTITY.iter())
        .copied()
        .chain([1.0, 1.0, 1.0, 1.0])
        .chain([0.0, 0.0, 0.0, 0.0])
        .collect()
}

/// Prepare `surfaces`, draw the quad with `program`, and read the frame back as
/// `EDGE * EDGE * 4` linear RGBA8 bytes.
///
/// `program` is what the draw names. Handing `0` draws the default pipeline,
/// which is what every existing app does — and handing a digest `surfaces` does
/// not contain is the frame-time cache miss, which must render the fallback
/// rather than compile or panic.
fn capture(gpu: &ParityGpu, surfaces: &[Surface], program: u64, time: f32) -> Vec<u8> {
    let device = &gpu.device;
    let queue = &gpu.queue;
    let mut renderer = SceneRenderer::new(
        device,
        queue,
        FORMAT,
        std::slice::from_ref(&quad()),
        &[],
        &[MaterialTexture::new(MATERIAL, 1, 1, vec![255, 255, 255, 255])],
        1,
        64,
        // A WHITE hemisphere ambient, so a fragment the default (LambertSpecular)
        // program resolves is plain white: `ambient_lit = base * hemi * 1`. That
        // makes the unsurfaced control a legible picture rather than a black
        // frame, which is the only way "the surface changed it" means anything.
        FrameRenderLook::lit_by(FrameAmbient::new([1.0; 3], [1.0; 3])),
        // This harness renders on the native adapter, which filters half-float
        // colour and holds every G-buffer format.
        true,
        true,
        1,
        // No G-buffer: this harness is comparing one surface program's output
        // against another's, and an ambient-occlusion term would be a second
        // thing moving between the two captures.
        None,
    );
    // THE BARRIER. Every shader this frame can possibly run is compiled here,
    // before a single draw is recorded.
    let catalog = SurfaceProgramCatalog::prepare(surfaces, BackendCapabilityProfile::all())
        .expect("the rig never approaches the program cap");
    renderer.prepare_surfaces(device, queue, &catalog);

    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-bound-image-target"),
        size: wgpu::Extent3d {
            width: EDGE,
            height: EDGE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_view = create_depth_view(device, EDGE, EDGE);
    renderer.record(
        device,
        queue,
        &color_view,
        &depth_view,
        (EDGE, EDGE),
        &[],
        // An IDENTITY light view-projection. The shadow pass renders the same
        // quad at the same depth, so every fragment samples itself and
        // `shadow_factor` returns exactly 1 — the shadow capability stays ON and
        // contributes an exact identity, rather than being switched off to make
        // the rig work.
        IDENTITY,
        &[(MESH, MATERIAL, instance(), 1)],
        &[program],
        &[],
        [0.0, 0.0, 0.0, 1.0],
        None,
        BackendCapabilityProfile::all().bits(),
        FrameCamera::IDENTITY,
        time,
        // No GPU pass clock. This rig is about pixels, and an untimed frame is
        // exactly the command stream this backend recorded before timing existed
        // — which is what makes these captures the proof that timing costs a
        // frame that does not use it nothing at all.
        None,
    );
    readback(gpu, &color)
}

/// Copy the target back to the CPU. `EDGE * 4` is exactly the 256-byte row
/// alignment, so the buffer is the image with no padding to strip.
fn readback(gpu: &ParityGpu, texture: &wgpu::Texture) -> Vec<u8> {
    let bytes = (EDGE * EDGE * 4) as u64;
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-bound-image-readback"),
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("axiom-bound-image-encoder"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(EDGE * 4),
                rows_per_image: Some(EDGE),
            },
        },
        wgpu::Extent3d {
            width: EDGE,
            height: EDGE,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(std::iter::once(encoder.finish()));
    staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::Wait)
        .expect("the readback must complete");
    let pixels = staging.slice(..).get_mapped_range().to_vec();
    staging.unmap();
    pixels
}

/// The red channel of the pixel at `(x, y)`, top-left origin.
fn red(image: &[u8], x: u32, y: u32) -> u8 {
    image[((y * EDGE + x) * 4) as usize]
}

/// How many pixels of the image are lit at all — the silhouette's area.
fn covered(image: &[u8]) -> usize {
    image.chunks_exact(4).filter(|texel| texel[0] > 8).count()
}

/// The mean column of the lit pixels — where the silhouette *is*.
fn centroid_column(image: &[u8]) -> f32 {
    let lit: Vec<u32> = (0..EDGE * EDGE)
        .filter(|index| image[(*index * 4) as usize] > 8)
        .map(|index| index % EDGE)
        .collect();
    lit.iter().sum::<u32>() as f32 / lit.len().max(1) as f32
}

/// A vec4 base colour whose every lane is `Uv.x` — a horizontal ramp that no
/// constant fallback can produce.
fn uv_ramp() -> FieldGraph {
    let (builder, uv) = FieldBuilder::new(FieldId::of_name("gpu/image/uv"), 1).push(
        FieldOp::Uv,
        Vec::new(),
        Vec::new(),
    );
    let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
    let (builder, one) = builder.push_const(FieldValue::scalar(axiom_recipe::Scalar::new(1.0)));
    let (builder, splat) = builder.push(
        FieldOp::Compose,
        vec![Param::int(4)],
        vec![lane, lane, lane, one],
    );
    builder.build(splat)
}

/// A vec4 base colour whose every lane is the frame's surface **time** — the
/// simplest thing that must differ between tick N and tick N+60.
fn time_ramp() -> FieldGraph {
    let (builder, clock) = FieldBuilder::new(FieldId::of_name("gpu/image/time"), 1).push(
        FieldOp::Time,
        Vec::new(),
        Vec::new(),
    );
    let (builder, one) = builder.push_const(FieldValue::scalar(axiom_recipe::Scalar::new(1.0)));
    let (builder, splat) = builder.push(
        FieldOp::Compose,
        vec![Param::int(4)],
        vec![clock, clock, clock, one],
    );
    builder.build(splat)
}

/// An `Unlit` surface with a white constant base colour and a constant
/// displacement of `offset` along +X. `Unlit` collapses the pass's lighting sum
/// to the base colour, so what the capture shows is the silhouette.
fn pushed(offset: f32) -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::Unlit)
        .constant(
            SurfaceChannel::BaseColor,
            FieldValue::vec4(Vec4::new(1.0, 1.0, 1.0, 1.0)),
        )
        .constant(
            SurfaceChannel::Displacement,
            FieldValue::vec3(Vec3::new(offset, 0.0, 0.0)),
        )
        .build()
        .expect("a vec3 constant is a legal displacement")
}

/// **A displacing surface renders a visibly different silhouette.**
///
/// The same mesh, the same instance, the same camera. One draw names a surface
/// whose displacement channel pushes every vertex +0.5 along object X; the other
/// names a surface that displaces nothing. The quad spans NDC `[-0.5, 0.5]`, so
/// +0.5 moves it a quarter of the target's width — sixteen of sixty-four columns.
///
/// This is the assertion manifest 10 could not make: it proves the **vertex**
/// half of a generated program reaches a real frame, not merely that it compiles.
#[test]
fn a_displacing_surface_moves_the_silhouette_in_a_captured_frame() {
    let gpu = gpu();
    // Both surfaces displace, so both compile a program: the ONLY difference
    // between the two frames is the offset the vertex stage adds.
    let still = pushed(-0.25);
    let moved = pushed(0.25);
    assert_ne!(
        still.digest().raw(),
        moved.digest().raw(),
        "two different displacements are two different surfaces"
    );

    let a = capture(gpu, std::slice::from_ref(&still), still.digest().raw(), 0.0);
    let b = capture(gpu, std::slice::from_ref(&moved), moved.digest().raw(), 0.0);

    // Both drew a quad of the same area — a displacement moved it, it did not
    // eat it.
    let area_a = covered(&a);
    let area_b = covered(&b);
    assert!(area_a > 900, "the undisplaced quad must cover ~32x32: {area_a}");
    assert!(
        (area_a as i64 - area_b as i64).abs() < 200,
        "a translation must preserve area: {area_a} vs {area_b}"
    );

    // …and it moved by the sixteen columns +0.5 NDC is on a 64-wide target.
    let shift = centroid_column(&b) - centroid_column(&a);
    assert!(
        (shift - 16.0).abs() < 1.5,
        "a +0.5 object-space displacement must move the silhouette 16 columns, moved {shift}"
    );

    // The images differ in a large number of pixels — a silhouette moved, not a
    // shade nudged.
    let differing = a
        .chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(x, y)| x != y)
        .count();
    assert!(differing > 500, "only {differing} pixels moved");
}

/// **A field-authored base colour renders the field's colours, not the constant
/// fallback.**
///
/// The surface's base colour is `Uv.x` splatted across RGB, so the quad must be a
/// horizontal ramp: dark at its left edge, bright at its right. The control is
/// the identical draw naming **no** surface, which renders the flat white the
/// material and instance lanes resolve to — and a flat image is exactly what a
/// constant fallback would produce, which is why the two are compared.
#[test]
fn a_field_authored_base_colour_renders_the_fields_ramp_and_not_a_flat_fallback() {
    let gpu = gpu();
    let ramped = SurfaceBuilder::new()
        .lighting(LightingModel::Unlit)
        .field(SurfaceChannel::BaseColor, uv_ramp())
        .build()
        .expect("a vec4 uv field is a legal base colour");
    let image = capture(
        &gpu,
        std::slice::from_ref(&ramped),
        ramped.digest().raw(),
        0.0,
    );
    // The quad occupies columns 16..48. Sample inside it, near each edge and in
    // the middle, on the row through its centre.
    let row = EDGE / 2;
    let left = red(&image, 18, row);
    let middle = red(&image, 32, row);
    let right = red(&image, 45, row);
    assert!(
        left < middle && middle < right,
        "uv.x must ramp left to right: {left} < {middle} < {right}"
    );
    assert!(left < 40, "the left edge must be near-black, got {left}");
    assert!(right > 200, "the right edge must be near-white, got {right}");

    // The control: the same geometry with no surface at all. Flat white — which
    // is what the ramp would look like if the program had not been bound.
    let flat = capture(gpu, &[], 0, 0.0);
    assert_eq!(red(&flat, 18, row), red(&flat, 45, row));
    assert!(red(&flat, 32, row) > 200);
    assert_ne!(
        red(&image, 18, row),
        red(&flat, 18, row),
        "a bound program must change the picture"
    );
}

/// **Tick N replayed twice is byte-identical, and tick N differs from tick
/// N+60 for a time-varying surface.**
///
/// The two halves of determinism, on real pixels: nothing about a frame depends
/// on iteration order or a wall clock, and the one input the frame *supplies* —
/// its surface time — is the one thing that moves the picture.
#[test]
fn a_replayed_tick_is_byte_identical_and_a_later_tick_is_not() {
    let gpu = gpu();
    let timed = SurfaceBuilder::new()
        .lighting(LightingModel::Unlit)
        .field(SurfaceChannel::BaseColor, time_ramp())
        .build()
        .expect("a vec4 time field is a legal base colour");
    let id = timed.digest().raw();
    let surfaces = std::slice::from_ref(&timed);

    // Tick N, twice: preparation, compilation and the draw are all deterministic,
    // so the bytes are equal — not close.
    let once = capture(gpu, surfaces, id, 0.25);
    let again = capture(gpu, surfaces, id, 0.25);
    assert_eq!(once, again, "a replayed tick must be byte-identical");

    // Tick N+60 (a second later at 60 Hz): the same program, the same pipeline,
    // a different uniform — and a different picture.
    let later = capture(gpu, surfaces, id, 0.75);
    assert_ne!(once, later, "a time-varying surface must move with the clock");
    let row = EDGE / 2;
    assert!(
        red(&later, 32, row) > red(&once, 32, row) + 80,
        "t=0.75 must be brighter than t=0.25: {} vs {}",
        red(&later, 32, row),
        red(&once, 32, row)
    );
}

/// **A frame naming a program the barrier never prepared renders the constant
/// fallback.** It does not compile one, and it does not panic — the draw comes
/// back as the flat white the default pipeline resolves, pixel-identical to the
/// same draw naming no surface at all.
#[test]
fn a_cache_miss_renders_the_constant_fallback_rather_than_compiling() {
    let gpu = gpu();
    let never_prepared = SurfaceBuilder::new()
        .lighting(LightingModel::Unlit)
        .field(SurfaceChannel::BaseColor, uv_ramp())
        .build()
        .expect("legal");
    // An EMPTY preparation, and a draw naming a real digest anyway.
    let missed = capture(gpu, &[], never_prepared.digest().raw(), 0.0);
    let plain = capture(gpu, &[], 0, 0.0);
    assert_eq!(
        missed, plain,
        "a miss must render exactly what a draw with no surface renders"
    );
}

/// **`surface_program == 0` is pixel-identical whether the cache is empty or
/// full.** Content that uses no surface pays nothing and, more importantly, sees
/// nothing: preparing eight programs cannot perturb a frame that names none.
#[test]
fn an_unsurfaced_frame_is_pixel_identical_with_a_full_program_cache() {
    let gpu = gpu();
    let bare = capture(gpu, &[], 0, 0.0);
    let loaded: Vec<Surface> = (0..8)
        .map(|index| pushed(0.1 * (index as f32 + 1.0)))
        .collect();
    let with_cache = capture(gpu, &loaded, 0, 0.0);
    assert_eq!(
        bare, with_cache,
        "eight compiled programs must not move an unsurfaced pixel"
    );
}

/// **The skinned pass still draws.**
///
/// Group 3 of the skinned pipeline now carries two bindings — the joint palette
/// it always had, and the surface parameter region the shared `fs` reads — because
/// `wgpu::Limits::downlevel_webgl2_defaults` guarantees only four bind groups and
/// the skinned pass already spends the last one. Getting that layout wrong does
/// not degrade: it fails pipeline creation or draw validation outright, and the
/// symptom in a browser is *every character silently missing*. So it is drawn.
///
/// The skinned pass runs the DEFAULT program (its vertex stage is at the
/// 16-attribute ceiling and its draws carry no surface program), so what this
/// asserts is that the frame is unchanged — a lit quad, where a broken layout
/// would give an empty one or a panic.
#[test]
fn the_skinned_pass_still_draws_with_the_surface_parameter_group_bound() {
    let gpu = gpu();
    // The 20-float skinned vertex: the 12 rigid floats plus joints and weights.
    // Every vertex is bound entirely to joint 0, whose palette matrix is the
    // identity, so a skinned quad at rest is its baked geometry.
    let (_, rigid, indices) = quad();
    let vertices: Vec<f32> = rigid
        .chunks_exact(12)
        .flat_map(|vertex| {
            vertex
                .iter()
                .copied()
                .chain([0.0, 0.0, 0.0, 0.0])
                .chain([1.0, 0.0, 0.0, 0.0])
        })
        .collect();
    let mut renderer = SceneRenderer::new(
        &gpu.device,
        &gpu.queue,
        FORMAT,
        &[],
        &[(MESH, vertices, indices)],
        &[MaterialTexture::new(MATERIAL, 1, 1, vec![255, 255, 255, 255])],
        1,
        64,
        FrameRenderLook::lit_by(FrameAmbient::new([1.0; 3], [1.0; 3])),
        // This harness renders on the native adapter, which filters half-float
        // colour and holds every G-buffer format.
        true,
        true,
        1,
        // No G-buffer: this harness is comparing one surface program's output
        // against another's, and an ambient-occlusion term would be a second
        // thing moving between the two captures.
        None,
    );
    renderer.prepare_surfaces(
        &gpu.device,
        &gpu.queue,
        &SurfaceProgramCatalog::default(),
    );
    let color = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-bound-image-skinned-target"),
        size: wgpu::Extent3d {
            width: EDGE,
            height: EDGE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_view = create_depth_view(&gpu.device, EDGE, EDGE);
    renderer.record(
        &gpu.device,
        &gpu.queue,
        &color_view,
        &depth_view,
        (EDGE, EDGE),
        &[],
        IDENTITY,
        &[],
        &[],
        &[crate::scene_renderer::SkinnedGpuDraw {
            mesh_id: MESH,
            material_id: MATERIAL,
            mvp: IDENTITY,
            world: IDENTITY,
            color: [1.0; 4],
            palette: vec![IDENTITY],
        }],
        [0.0, 0.0, 0.0, 1.0],
        None,
        BackendCapabilityProfile::all().bits(),
        FrameCamera::IDENTITY,
        0.0,
        None,
    );
    let image = readback(gpu, &color);
    let area = covered(&image);
    assert!(
        area > 900,
        "the skinned quad must still paint its ~32x32 of pixels, painted {area}"
    );
}
