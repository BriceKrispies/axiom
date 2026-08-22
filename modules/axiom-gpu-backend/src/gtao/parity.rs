//! **CPU↔GPU parity for GTAO**, on a real adapter.
//!
//! [`super::reference`] is the semantic definition of the pass; [`super::wgsl`]
//! is a mirror of it written independently from the same GLSL. A mirror nobody
//! holds up to the original is a second definition waiting to drift, so every
//! function in [`super::wgsl::GTAO_WGSL`] is driven through both sides here and
//! compared.
//!
//! # THIS TEST HAS NEVER RUN
//!
//! It was written under the final wave's no-build rule
//! (`docs/work-manifests/shmup-port/12-final-wave-brief.md`): nothing in this
//! wave compiles until the orchestrator's integration pass. The budgets below are
//! **derived from the arithmetic, not measured**, and are labelled as such at
//! each constant. The integration run produces the real numbers; when it does,
//! record them as `MEASURED_WORST` beside the budget the way
//! [`crate::agx`] does, and re-derive the budget from the measurement rather than
//! leaving a reasoned guess in place.
//!
//! # Two tiers, split by whether `acos` is in the chain
//!
//! Everything without an `acos` is ordinary arithmetic plus one or two
//! well-conditioned transcendentals, and lives at [`TOLERANCE`].
//!
//! The three entry points that call `acos` are different **in kind**. Its
//! derivative `1 / sqrt( 1 - x² )` is **unbounded at the poles**, and `cosH`
//! genuinely reaches `0.999` when a tap is close — which is the case the whole
//! pass exists to shade. At `cosH = 0.9997` the amplification is 41x, so a
//! `2 ULP` `inverseSqrt` (`2.4e-7` relative, which is what WGSL permits and
//! Rust's `1.0 / sqrt` does not do) lands as `~1e-5` in the horizon angle before
//! `arc` has even been called. That is the hardware's conditioning, not the
//! transcription's error, and it gets [`ACOS_TOLERANCE`].
//!
//! Splitting the tiers rather than widening one budget is the same decision
//! `crate::surface_program::parity_transcendental` makes, and for the same
//! reason: a single loose number hides which stage is actually costing what.
//!
//! # What is NOT proven here
//!
//! The texture fetches. The three pass shaders
//! ([`super::wgsl::GTAO_CORE_PASS_WGSL`] and friends) are only *compiled* here
//! ([`the_three_passes_compile_against_a_real_device`]) — a sampler's subtexel
//! behaviour is the hardware's, and mixing it into this measurement would hide
//! the transcription inside the filter. That boundary is
//! [`crate::bloom_pyramid::parity`]'s, deliberately.

use super::reference::{self, Tap};
use super::wgsl::{GTAO_BLUR_PASS_WGSL, GTAO_CORE_PASS_WGSL, GTAO_TEMPORAL_PASS_WGSL, GTAO_WGSL};

/// How many contexts one sweep compares. Also the target's width, and one
/// fragment per context.
const SAMPLES: usize = 16;

/// Sixteen-byte lanes per context in the uniform block. Must match what
/// [`uniform_bytes`] packs and what `HARNESS_WGSL`'s `lane()` strides by.
const LANES: usize = 36;

/// The first lane of the sixteen horizon taps: eight `+dir`, then eight `-dir`.
const TAP_LANE: usize = 16;

/// The first of the four lanes carrying `proj_inv`'s columns.
const PROJ_INV_LANE: usize = 32;

/// `copy_texture_to_buffer` requires each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// The agreement budget for every entry point except the horizon fold,
/// **relative above unit magnitude**: `|got - want| / max(|want|, 1)`.
///
/// **DERIVED, NOT MEASURED — this test has never run.** The reasoning: the
/// heaviest of these entry points evaluates one `acos`/`cos` pair (`gtao_geom_fs`)
/// or one `exp` (`gtao_temporal_fs`, `gtao_blur_fs`) over unit-scale values. A
/// WGSL transcendental is permitted a few ULP and a GPU may contract a
/// multiply-add; both together are `~3e-7` at unit scale, and the longest
/// unbroken chain here is the blur's seven-tap sum (a dozen roundings, none
/// cancelling catastrophically). `3e-5` is roughly a hundred times that — more
/// headroom than a correct port should need, chosen so that a *first* run reports
/// a real number rather than a spurious failure. **Tighten it to the measurement
/// the integration run produces.**
const TOLERANCE: f32 = 3.0e-5;

/// The budget for the three entry points with an `acos` in the chain
/// (`gtao_slice_fs`, `gtao_integral_fs`, `gtao_horizon_fs`), for the reason in
/// this module's header: `acos` near its pole amplifies a `2 ULP` `inverseSqrt`
/// by up to 40x, and the sweep deliberately drives it there because that is the
/// case the pass exists for.
///
/// **DERIVED, NOT MEASURED.** `41 x 2.4e-7 ≈ 1e-5` into the horizon angle, `arc`
/// has unit gain in `h`, two arcs add, and `resolve` divides by three — so `~2e-5`
/// reaching the output, against which `5e-4` is 25x. That is deliberately generous
/// for a first run and is **too loose to keep**: re-derive it from the integration
/// measurement.
const ACOS_TOLERANCE: f32 = 5.0e-4;

/// Which entry points sit in the [`ACOS_TOLERANCE`] tier. Named here rather than
/// decided at the assertion, so the split is a stated fact about the shaders and
/// not a per-run judgement.
const ACOS_TIER: [&str; 3] = ["gtao_slice_fs", "gtao_integral_fs", "gtao_horizon_fs"];

/// One evaluation context: everything six entry points between them read.
struct Context {
    /// The shading point's view position, and the linear view depth it came from.
    p: [f32; 3],
    depth: f32,
    /// The decoded view normal, and the world occlusion radius in metres.
    normal: [f32; 3],
    radius: f32,
    /// The slice azimuth, the projection's `[1][1]`, and the target height.
    dir2: [f32; 2],
    p11: f32,
    resolution_y: f32,
    /// The fragment coordinate the two noises are taken at, the frame phase, and
    /// the temporal feedback.
    frag: [f32; 2],
    phase: f32,
    feedback: f32,
    /// The two horizon cosines fed straight to `slice_visibility`, an angle and
    /// the three arc arguments, and the blur's intensity/stage flags.
    cos_h_neg: f32,
    cos_h_pos: f32,
    arc_h: f32,
    arc_n: f32,
    arc_cos_n: f32,
    arc_sin_n: f32,
    intensity: f32,
    apply_curve: f32,
    /// The uv `view_pos` reconstructs from, and the temporal pass's pair.
    uv: [f32; 2],
    current_ao: f32,
    current_depth: f32,
    history_ao: f32,
    history_depth: f32,
    history_uv: [f32; 2],
    temporal_neighbours: [f32; 4],
    /// The blur's centre and its three `(+i, -i)` pairs, each `(ao, depth)`.
    blur_centre: [f32; 2],
    blur_taps: [[[f32; 2]; 2]; reference::BLUR_TAPS],
    /// The step jitter and the pixel radius `step_offset` is driven with.
    noise2: f32,
    radius_px_in: f32,
    /// The view vector, `1 / r²`, and which slice/step index the leaves use.
    v: [f32; 3],
    inv_r2: f32,
    slice_index: f32,
    step_index: f32,
    /// The sixteen horizon taps: eight `+dir`, then eight `-dir`.
    taps: [Tap; 16],
    /// Column-major, as everywhere else in this backend.
    proj_inv: [f32; 16],
}

/// A finite perspective inverse, exact for the standard column-major GL matrix
/// `[ p00 0 0 0 | 0 p11 0 0 | 0 0 (f+n)/(n-f) -1 | 0 0 2fn/(n-f) 0 ]`.
///
/// The caller passes a **modest** near/far ratio on purpose. The inverse's `w`
/// row is `a + b` with `a = (n-f)/2fn` and `b = (f+n)/2fn`, and those two cancel
/// to `1/f`: at `0.1 / 500` the sum loses 3.4 decimal digits before the
/// reconstruction has started, and a single permitted `fma` contraction on one
/// side then shows up as a `2e-4` relative difference in `h.w` that has nothing
/// to do with this port. Most of it cancels again in `dir / -dir.z`, but a
/// tolerance should not have to rely on that.
fn perspective_inverse(fov_y_degrees: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let p11 = 1.0 / ((fov_y_degrees * 0.5).to_radians().tan());
    let p00 = p11 / aspect;
    let a = (near - far) / (2.0 * far * near);
    let b = (far + near) / (2.0 * far * near);
    [
        1.0 / p00, 0.0, 0.0, 0.0, //
        0.0, 1.0 / p11, 0.0, 0.0, //
        0.0, 0.0, 0.0, a, //
        0.0, 0.0, -1.0, b,
    ]
}

/// The [`SAMPLES`] contexts, chosen to cross every regime the pass has: depths
/// from inside the `0.2 m` floor out past the `6 px` radius clamp; normals facing
/// the camera, grazing, and one that projects to nothing (the `continue`); taps
/// coincident with the shading point (the `2e-5` guard), inside the radius, at the
/// radius, and rejected; history on-screen, off-screen, and across a depth
/// discontinuity; and blur neighbourhoods that are flat, stepped, and separated by
/// the `1e4` sky sentinel.
fn contexts() -> Vec<Context> {
    let proj_inv = perspective_inverse(60.0, 16.0 / 9.0, 0.5, 50.0);
    (0..SAMPLES)
        .map(|index| {
            let t = index as f32;
            // 0.15 m .. ~120 m, crossing both ends of the pixel-radius clamp.
            let depth = 0.15 * 1.6_f32.powf(t);
            let radius = [1.35_f32, 0.9, 2.5, 0.35][index % 4];
            // A normal sweeping from camera-facing toward the slice plane's
            // edge. None of these is degenerate -- `projLen < 1e-4` needs a
            // normal exactly parallel to the slice axis, which no plausible
            // sweep hits and which `reference::tests` pins directly instead.
            let tilt = (t * 0.21).sin();
            let normal = {
                let raw = [tilt, [0.0_f32, 1.0][usize::from(index == 7)], 1.0 - tilt.abs()];
                let l = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
                [raw[0] / l, raw[1] / l, raw[2] / l]
            };
            let p = [
                (t * 0.31 - 2.0) * 0.1,
                (1.5 - t * 0.19) * 0.1,
                -depth,
            ];
            let v = {
                let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                [-p[0] / l, -p[1] / l, -p[2] / l]
            };
            let dir2 = reference::slice_direction(index % super::SLICES, t * 0.137);
            let inv_r2 = 1.0 / (radius * radius);

            // Sixteen taps. Every fourth is rejected; index 1 of each side is
            // coincident with the shading point (the 2e-5 guard); the rest march
            // outward from very close (which drives cosH toward its pole, the
            // case HORIZON_TOLERANCE is for) to beyond the radius.
            let taps = core::array::from_fn(|slot| {
                let side = [1.0_f32, -1.0][usize::from(slot >= 8)];
                let step = (slot % 8) as f32;
                let reach = 0.004 + step * step * radius * 0.02;
                Tap {
                    view_pos: [
                        p[0] + side * dir2[0] * reach,
                        p[1] + side * dir2[1] * reach,
                        // Toward the camera: a real occluder, not a hole.
                        p[2] + reach * 0.85,
                    ],
                    accepted: ((slot % 4) != 3) & ((slot % 8) != 1),
                }
            });

            let current_depth = depth;
            Context {
                p,
                depth,
                normal,
                radius,
                dir2,
                p11: 1.0 / (30.0_f32.to_radians().tan()),
                resolution_y: [1080.0_f32, 720.0, 2160.0, 540.0][index % 4],
                frag: [t * 97.0 + 0.5, t * 41.0 + 0.5],
                phase: (index % 64) as f32,
                feedback: super::TEMPORAL_FEEDBACK,
                // Across the whole legal range, including both poles of acos.
                cos_h_neg: -1.0 + t * (2.0 / (SAMPLES - 1) as f32),
                cos_h_pos: 1.0 - t * (2.0 / (SAMPLES - 1) as f32),
                arc_h: -core::f32::consts::FRAC_PI_2 + t * (core::f32::consts::PI / 15.0),
                arc_n: (t * 0.37).sin(),
                arc_cos_n: (t * 0.37).cos(),
                arc_sin_n: (t * 0.37).sin(),
                intensity: [1.1_f32, 1.25, 1.0, 0.8][index % 4],
                apply_curve: [0.0_f32, 1.0][index % 2],
                uv: [0.03 + t * 0.06, 0.97 - t * 0.06],
                current_ao: 0.05 + t * 0.09,
                current_depth,
                history_ao: 1.3 - t * 0.08,
                // Every fourth history is across a discontinuity, and one sits
                // at the 1e4 sky sentinel.
                history_depth: [
                    current_depth,
                    current_depth * 1.04,
                    current_depth + 3.0,
                    super::UNCOVERED_DEPTH_SENTINEL,
                ][index % 4],
                // Two of these are off screen, which zeroes the weight outright.
                history_uv: [
                    [0.5_f32, 0.5],
                    [-0.01, 0.5],
                    [0.5, 1.02],
                    [0.0, 1.0],
                ][index % 4],
                temporal_neighbours: [
                    0.05 + t * 0.09,
                    0.4 + (t * 0.5).sin() * 0.35,
                    1.2 - t * 0.06,
                    0.02 + t * 0.11,
                ],
                blur_centre: [0.15 + t * 0.07, current_depth],
                blur_taps: core::array::from_fn(|i| {
                    let reach = (i + 1) as f32;
                    [
                        [0.1 + t * 0.05 + reach * 0.02, current_depth + reach * 0.01],
                        // Every fourth context puts the far side across the sky
                        // sentinel, which must weigh exactly zero.
                        [
                            0.9 - t * 0.04,
                            [
                                current_depth - reach * 0.02,
                                current_depth,
                                current_depth * 1.5,
                                super::UNCOVERED_DEPTH_SENTINEL,
                            ][index % 4],
                        ],
                    ]
                }),
                noise2: reference::hash12([t * 3.0 + 0.5, t * 7.0 + 0.5]),
                radius_px_in: 6.0 + t * 8.0,
                v,
                inv_r2,
                slice_index: (index % super::SLICES) as f32,
                step_index: (index % super::STEPS) as f32,
                taps,
                proj_inv,
            }
        })
        .collect()
}

/// The harness: a fullscreen triangle whose fragment stage evaluates the entry
/// point at the context its pixel column names. Concatenated **after**
/// [`GTAO_WGSL`], so it calls the same text the three passes call.
const HARNESS_WGSL: &str = r#"
struct GtaoParityContexts { items: array<vec4<f32>, 576> };
@group(0) @binding(0) var<uniform> ctx: GtaoParityContexts;

fn lane(index: u32, slot: u32) -> vec4<f32> { return ctx.items[index * 36u + slot]; }

fn parity_proj_inv(i: u32) -> mat4x4<f32> {
    return mat4x4<f32>(lane(i, 32u), lane(i, 33u), lane(i, 34u), lane(i, 35u));
}

fn parity_frame(i: u32) -> AxiomGtaoSlice {
    let n = lane(i, 1u).xyz;
    let v = lane(i, 13u).xyz;
    return axiom_gtao_slice_frame(n, v, lane(i, 2u).xy);
}

// The four leaf functions: both noises, the pixel radius, and the step offset.
@fragment
fn gtao_leaf_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let l0 = lane(i, 0u);
    let l2 = lane(i, 2u);
    let l3 = lane(i, 3u);
    let l9 = lane(i, 9u);
    let l14 = lane(i, 14u);
    let phase = l3.z;
    return vec4<f32>(
        axiom_gtao_ign(l3.xy + vec2<f32>(phase * 5.588238, phase * 5.588238)),
        axiom_gtao_hash12(l3.xy * 0.371 + vec2<f32>(phase, phase)),
        axiom_gtao_radius_px(lane(i, 1u).w, l2.z, l2.w, l0.w),
        axiom_gtao_step_offset(l14.y, l9.z, l9.w),
    );
}

// The view-space reconstruction, and the arc integral.
@fragment
fn gtao_geom_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let l0 = lane(i, 0u);
    let l4 = lane(i, 4u);
    let l5 = lane(i, 5u);
    let p = axiom_gtao_view_pos(lane(i, 6u).xy, l0.w, parity_proj_inv(i));
    return vec4<f32>(p, axiom_gtao_arc(l4.z, l5.x, l5.y, l5.z));
}

// The slice frame, plus the azimuth it is built from.
@fragment
fn gtao_slice_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let f = parity_frame(i);
    return vec4<f32>(f.proj_len, f.cos_n, f.n, f.sin_n);
}

// The azimuth on its own, so a disagreement in `slice_frame` cannot be blamed on
// the direction that fed it.
@fragment
fn gtao_direction_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let l9 = lane(i, 9u);
    let l14 = lane(i, 14u);
    let d = axiom_gtao_slice_direction(l14.x, l9.z);
    return vec4<f32>(d.x, d.y, 0.0, 0.0);
}

// The horizon fold over sixteen taps, the slice integral, and the resolve.
@fragment
fn gtao_horizon_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let p = lane(i, 0u).xyz;
    let l13 = lane(i, 13u);
    let v = l13.xyz;
    let inv_r2 = l13.w;

    var cos_h_pos = -1.0;
    for (var t = 0u; t < 8u; t = t + 1u) {
        let tap = lane(i, 16u + t);
        let ds = vec3<f32>(tap.x - p.x, tap.y - p.y, tap.z - p.z);
        let updated = axiom_gtao_horizon_update(cos_h_pos, ds, v, inv_r2);
        cos_h_pos = select(cos_h_pos, updated, tap.w > 0.5);
    }
    var cos_h_neg = -1.0;
    for (var t = 0u; t < 8u; t = t + 1u) {
        let tap = lane(i, 24u + t);
        let ds = vec3<f32>(tap.x - p.x, tap.y - p.y, tap.z - p.z);
        let updated = axiom_gtao_horizon_update(cos_h_neg, ds, v, inv_r2);
        cos_h_neg = select(cos_h_neg, updated, tap.w > 0.5);
    }

    let contribution = axiom_gtao_slice_visibility(cos_h_neg, cos_h_pos, parity_frame(i));
    return vec4<f32>(
        cos_h_pos,
        cos_h_neg,
        contribution,
        axiom_gtao_resolve(contribution * 3.0),
    );
}

// `slice_visibility` driven from the two cosines DIRECTLY, across the whole
// legal range including both poles -- the fold above never reaches -1 on a
// context whose taps are all accepted.
@fragment
fn gtao_integral_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let l4 = lane(i, 4u);
    let f = parity_frame(i);
    let contribution = axiom_gtao_slice_visibility(l4.x, l4.y, f);
    return vec4<f32>(contribution, axiom_gtao_resolve(contribution * 3.0), 0.0, 0.0);
}

// The temporal accumulator: weight, clamped history, blended output.
@fragment
fn gtao_temporal_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let l3 = lane(i, 3u);
    let l6 = lane(i, 6u);
    let l7 = lane(i, 7u);
    let l8 = lane(i, 8u);
    let w = axiom_gtao_temporal_weight(l3.w, vec2<f32>(l7.z, l7.w), l7.y, l6.w);
    let h = axiom_gtao_temporal_clamp(l7.x, l6.z, l8.x, l8.y, l8.z, l8.w);
    return vec4<f32>(w, h, axiom_gtao_mix(l6.z, h, w), l6.w);
}

// The separable bilateral: the weighted sum, the weight total, the output, and
// one distance weight on its own.
@fragment
fn gtao_blur_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let l4 = lane(i, 4u);
    let l5 = lane(i, 5u);
    let c = lane(i, 9u).xy;
    var sum = c.x * 0.4;
    var wsum = 0.4;
    for (var k = 1; k <= 3; k = k + 1) {
        let w0 = axiom_gtao_blur_distance_weight(f32(k));
        let pair = lane(i, u32(9 + k));
        let wa = axiom_gtao_blur_tap_weight(w0, pair.y, c.y);
        let wb = axiom_gtao_blur_tap_weight(w0, pair.w, c.y);
        sum = sum + (pair.x * wa + pair.z * wb);
        wsum = wsum + (wa + wb);
    }
    return vec4<f32>(
        sum,
        wsum,
        axiom_gtao_blur_output(sum, wsum, l5.w, l4.w),
        axiom_gtao_blur_distance_weight(2.0),
    );
}
"#;

/// This module's handle on the crate's **one** shared adapter + device
/// ([`crate::test_gpu`]). Never `wgpu::Instance::default()`: twenty sites doing
/// that is what crashed the driver.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    backend: wgpu::Backend,
}

impl Gpu {
    fn acquire() -> Gpu {
        let gpu = crate::test_gpu::TestGpu::shared();
        Gpu {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            backend: gpu.backend,
        }
    }

    fn render(&self, module: &wgpu::ShaderModule, entry: &str, uniform: &[u8]) -> Vec<[f32; 4]> {
        let layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("axiom-gtao-parity-bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            &self.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("axiom-gtao-parity-uniform"),
                contents: uniform,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-gtao-parity-bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("axiom-gtao-parity-pl"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-gtao-parity-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("axiom_gtao_vs"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba32Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-gtao-parity-target"),
            size: wgpu::Extent3d {
                width: SAMPLES as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-gtao-parity-readback"),
            size: u64::from(row_bytes),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-gtao-parity-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: SAMPLES as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::Wait)
            .expect("the readback must complete");
        let mapped = slice.get_mapped_range();
        (0..SAMPLES)
            .map(|sample| {
                [0_usize, 1, 2, 3].map(|lane| {
                    let at = sample * 16 + lane * 4;
                    f32::from_le_bytes([
                        mapped[at],
                        mapped[at + 1],
                        mapped[at + 2],
                        mapped[at + 3],
                    ])
                })
            })
            .collect()
    }
}

/// The uniform block: [`LANES`] `vec4` per context, in the order `lane()`
/// unpacks. The lane map is stated once, here, and every entry point above reads
/// against it.
fn uniform_bytes(contexts: &[Context]) -> Vec<u8> {
    let bytes: Vec<u8> = contexts
        .iter()
        .flat_map(|c| {
            let mut lanes = vec![[0.0_f32; 4]; LANES];
            lanes[0] = [c.p[0], c.p[1], c.p[2], c.depth];
            lanes[1] = [c.normal[0], c.normal[1], c.normal[2], c.radius];
            lanes[2] = [c.dir2[0], c.dir2[1], c.p11, c.resolution_y];
            lanes[3] = [c.frag[0], c.frag[1], c.phase, c.feedback];
            lanes[4] = [c.cos_h_neg, c.cos_h_pos, c.arc_h, c.intensity];
            lanes[5] = [c.arc_n, c.arc_cos_n, c.arc_sin_n, c.apply_curve];
            lanes[6] = [c.uv[0], c.uv[1], c.current_ao, c.current_depth];
            lanes[7] = [
                c.history_ao,
                c.history_depth,
                c.history_uv[0],
                c.history_uv[1],
            ];
            lanes[8] = c.temporal_neighbours;
            lanes[9] = [c.blur_centre[0], c.blur_centre[1], c.noise2, c.radius_px_in];
            (0..reference::BLUR_TAPS).for_each(|i| {
                let pair = c.blur_taps[i];
                lanes[10 + i] = [pair[0][0], pair[0][1], pair[1][0], pair[1][1]];
            });
            lanes[13] = [c.v[0], c.v[1], c.v[2], c.inv_r2];
            lanes[14] = [c.slice_index, c.step_index, 0.0, 0.0];
            (0..16).for_each(|slot| {
                let tap = &c.taps[slot];
                lanes[TAP_LANE + slot] = [
                    tap.view_pos[0],
                    tap.view_pos[1],
                    tap.view_pos[2],
                    f32::from(tap.accepted),
                ];
            });
            (0..4).for_each(|column| {
                lanes[PROJ_INV_LANE + column] = [
                    c.proj_inv[column * 4],
                    c.proj_inv[column * 4 + 1],
                    c.proj_inv[column * 4 + 2],
                    c.proj_inv[column * 4 + 3],
                ];
            });
            lanes
        })
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect();
    // An equality, never a `resize`: `crate::exposure`'s harness lost a day of
    // confidence to a silent truncation of exactly this shape.
    assert_eq!(
        bytes.len(),
        SAMPLES * LANES * 16,
        "LANES must match what this function packs and what HARNESS_WGSL strides by"
    );
    bytes
}

/// The CPU side of one entry point, per context — the same six functions in the
/// same order the WGSL calls them.
fn expected(entry: &str, c: &Context) -> [f32; 4] {
    let frame = reference::slice_frame(c.normal, c.v, c.dir2);
    let stride = super::FRAME_NOISE_STRIDE;
    let pos: Vec<Tap> = (0..8)
        .map(|slot| Tap {
            view_pos: c.taps[slot].view_pos,
            accepted: c.taps[slot].accepted,
        })
        .collect();
    let neg: Vec<Tap> = (8..16)
        .map(|slot| Tap {
            view_pos: c.taps[slot].view_pos,
            accepted: c.taps[slot].accepted,
        })
        .collect();

    let table: Vec<(&str, [f32; 4])> = vec![
        (
            "gtao_leaf_fs",
            [
                reference::ign([c.frag[0] + c.phase * stride, c.frag[1] + c.phase * stride]),
                reference::hash12([
                    c.frag[0] * super::STEP_HASH_COORD_SCALE + c.phase,
                    c.frag[1] * super::STEP_HASH_COORD_SCALE + c.phase,
                ]),
                reference::radius_px(c.radius, c.p11, c.resolution_y, c.depth),
                reference::step_offset(c.step_index as usize, c.noise2, c.radius_px_in),
            ],
        ),
        ("gtao_geom_fs", {
            let p = reference::view_pos(c.uv, c.depth, &c.proj_inv);
            [
                p[0],
                p[1],
                p[2],
                reference::arc(c.arc_h, c.arc_n, c.arc_cos_n, c.arc_sin_n),
            ]
        }),
        (
            "gtao_slice_fs",
            [frame.proj_len, frame.cos_n, frame.n, frame.sin_n],
        ),
        ("gtao_direction_fs", {
            let d = reference::slice_direction(c.slice_index as usize, c.noise2);
            [d[0], d[1], 0.0, 0.0]
        }),
        ("gtao_horizon_fs", {
            let cos_h_pos = reference::horizon(&pos, c.p, c.v, c.inv_r2);
            let cos_h_neg = reference::horizon(&neg, c.p, c.v, c.inv_r2);
            let contribution = reference::slice_visibility(cos_h_neg, cos_h_pos, &frame);
            [
                cos_h_pos,
                cos_h_neg,
                contribution,
                reference::resolve_visibility(contribution * 3.0),
            ]
        }),
        ("gtao_integral_fs", {
            let contribution = reference::slice_visibility(c.cos_h_neg, c.cos_h_pos, &frame);
            [
                contribution,
                reference::resolve_visibility(contribution * 3.0),
                0.0,
                0.0,
            ]
        }),
        ("gtao_temporal_fs", {
            let w = reference::temporal_weight(
                c.feedback,
                c.history_uv,
                c.history_depth,
                c.current_depth,
            );
            let h = reference::temporal_clamp(c.history_ao, c.current_ao, c.temporal_neighbours);
            [
                w,
                h,
                reference::temporal_blend(c.current_ao, h, w),
                c.current_depth,
            ]
        }),
        ("gtao_blur_fs", {
            let (sum, wsum) = reference::blur_accumulate(c.blur_centre, &c.blur_taps);
            [
                sum,
                wsum,
                reference::blur_output(sum, wsum, c.apply_curve > 0.5, c.intensity),
                reference::blur_distance_weight(2),
            ]
        }),
    ];
    table
        .into_iter()
        .find(|(name, _)| *name == entry)
        .map(|(_, value)| value)
        .expect("every entry point compared must have a CPU reference here")
}

/// Compare one entry point's four lanes against the reference, and return the
/// worst scaled deviation together with the lane it came from. One assertion at
/// the end rather than one per lane, so a run reports the **worst** disagreement
/// rather than the first — which is what a budget has to be set from.
fn compare(
    gpu: &Gpu,
    module: &wgpu::ShaderModule,
    entry: &str,
    contexts: &[Context],
    uniform: &[u8],
) -> (f32, String) {
    let actual = gpu.render(module, entry, uniform);
    actual
        .iter()
        .zip(contexts)
        .enumerate()
        .flat_map(|(sample, (got, context))| {
            let want = expected(entry, context);
            (0..4)
                .map(move |lane| (sample, lane, got[lane], want[lane]))
                .collect::<Vec<(usize, usize, f32, f32)>>()
        })
        .map(|(sample, lane, got, want)| {
            let scaled = (got - want).abs() / f32::max(want.abs(), 1.0);
            (
                scaled,
                format!("{entry} sample {sample} lane {lane}: GPU {got} vs CPU {want}"),
            )
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .expect("the sweep compares at least one lane")
}

#[test]
fn the_gtao_wgsl_agrees_with_the_cpu_reference_on_a_real_adapter() {
    let gpu = Gpu::acquire();
    // The error scope is the SHARED device's, so it is entered exclusively; see
    // `crate::test_gpu::validating`.
    let (module, failure) = crate::test_gpu::validating(&gpu.device, || {
        gpu.device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("axiom-gtao-parity-shader"),
                source: wgpu::ShaderSource::Wgsl(format!("{GTAO_WGSL}\n{HARNESS_WGSL}").into()),
            })
    });
    assert!(
        failure.is_none(),
        "GTAO_WGSL + the parity harness must compile: {failure:?}"
    );

    let ctx = contexts();
    let uniform = uniform_bytes(&ctx);

    let per_entry: Vec<(&str, f32, f32, String)> = [
        "gtao_leaf_fs",
        "gtao_geom_fs",
        "gtao_slice_fs",
        "gtao_direction_fs",
        "gtao_horizon_fs",
        "gtao_integral_fs",
        "gtao_temporal_fs",
        "gtao_blur_fs",
    ]
    .iter()
    .map(|entry| {
        let (worst, at) = compare(&gpu, &module, entry, &ctx, &uniform);
        let budget = [TOLERANCE, ACOS_TOLERANCE][usize::from(ACOS_TIER.contains(entry))];
        (*entry, worst, budget, at)
    })
    .collect();

    // Every entry point's worst, not just the overall one: the budgets have to
    // be ATTRIBUTABLE, and "which stage costs what" is only visible if the
    // failure message carries all of them. This is also the measurement the
    // integration run is meant to hand back.
    let summary = per_entry
        .iter()
        .map(|(entry, worst, budget, at)| format!("{entry} {worst:e} / {budget:e} at {at}"))
        .collect::<Vec<String>>()
        .join(" | ");
    let over = per_entry
        .iter()
        .filter(|(_, worst, budget, _)| worst > budget)
        .count();
    assert!(
        over == 0,
        "GTAO parity on {:?}: {over} entry point(s) over budget. NOTE: BOTH \
         budgets were DERIVED from the arithmetic and have NEVER been measured \
         \u{2014} if the excess is small, the budget is what is wrong, not the \
         shader. Per entry point: {summary}",
        gpu.backend
    );

    // A harness that renders nothing scores a perfect zero against a reference
    // that also happens to be zero. Prove something actually ran.
    let rendered = gpu.render(&module, "gtao_slice_fs", &uniform);
    assert!(
        rendered.iter().any(|lanes| lanes.iter().any(|v| *v != 0.0)),
        "the sweep produced an all-zero buffer, so nothing was proven"
    );
}

/// The three real passes are only **compiled** here: what a sampler does with a
/// texel is the hardware's, not this port's, and folding it into the measurement
/// above would hide the transcription inside the filter. Compiling them is still
/// worth a test — a WGSL type error in a shader nothing binds is otherwise
/// invisible until integration day.
#[test]
fn the_three_passes_compile_against_a_real_device() {
    let gpu = Gpu::acquire();
    [
        ("core", GTAO_CORE_PASS_WGSL),
        ("temporal", GTAO_TEMPORAL_PASS_WGSL),
        ("blur", GTAO_BLUR_PASS_WGSL),
    ]
    .iter()
    .for_each(|(name, pass)| {
        let (_, failure) = crate::test_gpu::validating(&gpu.device, || {
            gpu.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-gtao-pass-compile"),
                    source: wgpu::ShaderSource::Wgsl(format!("{GTAO_WGSL}\n{pass}").into()),
                })
        });
        assert!(
            failure.is_none(),
            "the {name} pass must compile: {failure:?}"
        );
    });
}
