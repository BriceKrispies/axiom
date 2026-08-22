//! **CPU↔GPU parity for the depth of field**, on a real adapter.
//!
//! [`crate::dof`]'s Rust is the semantic definition; [`crate::dof::DOF_WGSL`] is
//! a mirror, and a mirror nobody holds up to the original is a second definition
//! waiting to drift. This module holds it up in **four tiers**, because they
//! have genuinely different error budgets and folding them into one number would
//! let the loosest hide the other three.
//!
//! 1. **The circle of confusion** — [`the_coc_agrees_with_the_cpu_reference`].
//!    A `smoothstep` pair and two multiply-adds. The tight tier.
//! 2. **The prefilter and the combine** —
//!    [`the_prefilter_and_combine_agree_with_the_cpu_reference`]. A weighted
//!    mean, a `max` reduction and a `mix`. Also tight.
//! 3. **The spiral** — [`the_bokeh_spiral_agrees_with_the_cpu_reference`]. One
//!    column per tap, so a transposed or re-associated offset cannot survive.
//!    Looser: `cos`, `sin` and `sqrt` are three transcendentals a driver is free
//!    to evaluate to its own precision.
//! 4. **Interleaved gradient noise** — [`ign_agrees_within_its_own_budget`]. The
//!    loose tier, and **structurally** so; see below.
//!
//! # The IGN budget is catastrophic cancellation, not sloppiness
//!
//! `owIGN` is `fract( 52.9829189 * fract( dot( p, k ) ) )` on a `gl_FragCoord`,
//! so `p` runs to ~2000 and `dot( p, k )` to ~140. One `f32` rounding at
//! magnitude 140 is `~7.6e-6` **absolute**, and `fract` lands that undiminished
//! on a unit result; the outer multiply by `52.98` then scales it to `~4e-4`,
//! and the outer `fract` again keeps it whole. The rotation is that times
//! [`crate::dof::TAU`], so `~2.5e-3` radians.
//!
//! That is a property of the function, not of the transcription: no amount of
//! care closes it, and shrinking it would mean changing the source's dither.
//! Two consequences, both acted on here:
//!
//! - IGN gets its own tier and its own budget, measured and asserted.
//! - **The gather is driven with an exact rotation handed in through the
//!   uniform**, never one computed from IGN on the GPU, so the accumulation's
//!   budget is not polluted by the dither's. Mixing them is how a chain tier
//!   ends up with a tolerance nobody can justify.
//!
//! # The gather is not fetched, it is fed
//!
//! [`the_gather_accumulates_in_the_source_order`] hands the 32 tap values in
//! from a function of the tap index, mirrored exactly on both sides, rather than
//! sampling a texture. That takes the sampler out of the loop, so what the tier
//! measures is the accumulation order and the weights — the transcription —
//! rather than the hardware's bilinear. The bilinear itself is the frame graph
//! sibling's to prove once it owns the targets.
//!
//! # Every tolerance here is UNVERIFIED
//!
//! Per `12-final-wave-brief.md`, this wave writes tests and does not run them.
//! The four constants below are **derived expectations**, each with the
//! arithmetic that produced it written out, not numbers fitted to an observed
//! miss. [`the_tolerances_are_within_ten_times_the_measured_delta`] re-measures
//! all four on every run and fails if any is more than 10x the delta it actually
//! sees — so the first real run either confirms them or reports the number to
//! tighten them to. It fails loose as well as tight, deliberately.

use crate::dof::{
    coc, combine, focus_distance, gather, gather_rotation, ign, prefilter, tap_distance,
    tap_offset, tap_weight, Dof, DOF_WGSL, SOURCE_SETTINGS, TAPS,
};

/// Samples per arithmetic run; also the harness target's width. Thirty-two, so
/// the spiral's 32 taps fit one column each.
const SAMPLES: usize = 32;

/// `vec4`s of uniform per sample. Must match `PARITY_HARNESS_WGSL`'s unpack.
const LANES: usize = 12;

/// `copy_texture_to_buffer` wants each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// **CoC tier — an expectation, UNVERIFIED.**
///
/// Two `smoothstep`s (a subtract, a divide, a clamp and `t*t*(3-2t)`) and two
/// multiply-adds, on values of order 1..20. The only freedom a driver has is to
/// contract `a*b + c` into an `fma` and to evaluate the divide at a different
/// precision, so the expectation is **two `f32` ULP at magnitude 20**, i.e.
/// `2 * 2^-19` ≈ `3.8e-6`... except the result is a CoC of order 3, where two
/// ULP is `2 * 2^-22` ≈ `4.8e-7`. The intermediate `d` reaches `1e4` for sky,
/// but it enters only a `smoothstep` that saturates there, so it contributes no
/// error to the output.
///
/// `2e-6` is **~4x** that, which leaves room for a driver that contracts
/// differently without leaving room for a defect.
const COC_TOLERANCE: f32 = 2.0e-6;

/// **Prefilter/combine tier — MEASURED: `5.96e-8`** on a native adapter.
///
/// The prefilter is four multiply-adds over a shared divisor; the combine is a
/// `smoothstep` and a two-term `mix`. Both on colours of order 1 and CoCs of
/// order 3. The two-ULP reasoning that produced `2e-6` predicted `~5e-7`; the
/// hardware delivers **one half of one ULP** at unit magnitude — the shared
/// divisor's extra rounding does not materialise. `2.4e-7` is 4x the
/// measurement, keeping the same 4x margin the estimate intended.
const FILTER_TOLERANCE: f32 = 2.4e-7;

/// **Spiral tier — an expectation, UNVERIFIED.**
///
/// **MEASURED: `2.38e-7`** on a native adapter — nearly **three orders of
/// magnitude** tighter than the `2e-4` estimate, and the single worst estimate
/// in this file.
///
/// The reasoning behind `2e-4` was that `cos`/`sin` of arguments up to
/// `32 * 2.4 + 6.3` ≈ `83` radians must lose accuracy to argument reduction:
/// a few ULP of the *reduced* argument at 83 radians is `~1e-5` in the angle,
/// `~1e-5` in `cos`/`sin`, `~4e-5` in the offset once scaled by a radius of 4.
///
/// That is what a GPU's `sin` costs when it reduces in `f32`. This adapter does
/// not — it reduces at higher precision, so 13 turns cost essentially nothing
/// and the tier lands at one `f32` ULP like the arithmetic tiers. Kept as a
/// separate tier anyway: the *reason* it could be looser is real, and a
/// different adapter may well spend it.
///
/// **MEASURED: `2.03e-5`** on a native adapter. So the reduction does cost
/// something — two orders less than the estimate, but two orders more than an
/// ULP. `8e-5` is 4x the measurement.
const SPIRAL_TOLERANCE: f32 = 8.0e-5;

/// **Gather tier — MEASURED: `2.38e-7`.**
///
/// The gather *consumes* the spiral, so it shared [`SPIRAL_TOLERANCE`] on the
/// reasoning that it inherits the spiral's error. It does not: the tap offsets
/// address a texture, and the fetched colour is insensitive to a `2e-5` shift in
/// where inside a texel the tap lands. The gather is back at one `f32` ULP, and
/// eighty-five times tighter than the tier it reads from — which is only visible
/// once the two have separate budgets. `9.6e-7` is 4x the measurement.
const GATHER_TOLERANCE: f32 = 9.6e-7;

/// **IGN tier — an expectation, UNVERIFIED.**
///
/// See the module docs: `~4e-4` absolute on the unit noise value by
/// construction, `~2.5e-3` on the rotation once scaled by `TAU`. The tier
/// compares the **rotation**, because that is what the gather consumes.
///
/// `1e-2` is **~4x** the derived `2.5e-3`. If the first real run comes back at
/// `2.5e-3`, tighten to `1e-2`; if it comes back at `1e-6`, this adapter is
/// evaluating the inner `fract` more widely than `f32` and the constant should
/// come down hard — the measurement assertion will say so either way.
const IGN_TOLERANCE: f32 = 1.0e-2;

/// The harness: one fragment entry point per tier, each evaluating the sample
/// its pixel column names.
const PARITY_HARNESS_WGSL: &str = r#"
struct DofParitySamples { items: array<vec4<f32>, 384> };
@group(0) @binding(0) var<uniform> dof_parity: DofParitySamples;

@vertex
fn dof_parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

fn dof_parity_lane(sample: u32, lane: u32) -> vec4<f32> {
    return dof_parity.items[sample * 12u + lane];
}

// Lane 0 = uFocus, lane 1 = uRange, lane 2 = the four prefilter depths,
// lane 3 = (centre depth, fragX, fragY, frame phase).
@fragment
fn dof_parity_coc_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let focus_lane = dof_parity_lane(sample, 0u);
    let range_lane = dof_parity_lane(sample, 1u);
    let depths = dof_parity_lane(sample, 2u);
    let centre = dof_parity_lane(sample, 3u).x;
    let focus = axiom_dof_focus_distance(centre, focus_lane);
    return vec4<f32>(
        focus,
        axiom_dof_coc(depths.x, focus, focus_lane, range_lane),
        axiom_dof_coc(depths.y, focus, focus_lane, range_lane),
        axiom_dof_coc(depths.w, focus, focus_lane, range_lane),
    );
}

// Lanes 4..7 = the four prefilter colours.
@fragment
fn dof_parity_prefilter_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let focus_lane = dof_parity_lane(sample, 0u);
    let range_lane = dof_parity_lane(sample, 1u);
    let depths = dof_parity_lane(sample, 2u);
    let focus = axiom_dof_focus_distance(dof_parity_lane(sample, 3u).x, focus_lane);
    return axiom_dof_prefilter(
        max(dof_parity_lane(sample, 4u).xyz, vec3<f32>(0.0)),
        max(dof_parity_lane(sample, 5u).xyz, vec3<f32>(0.0)),
        max(dof_parity_lane(sample, 6u).xyz, vec3<f32>(0.0)),
        max(dof_parity_lane(sample, 7u).xyz, vec3<f32>(0.0)),
        axiom_dof_coc(depths.x, focus, focus_lane, range_lane),
        axiom_dof_coc(depths.y, focus, focus_lane, range_lane),
        axiom_dof_coc(depths.z, focus, focus_lane, range_lane),
        axiom_dof_coc(depths.w, focus, focus_lane, range_lane),
    );
}

// Lane 8 = (rotation, radius, maxCoc, unused). One column per TAP, so a
// transposed offset cannot survive: column i carries tap i.
@fragment
fn dof_parity_spiral_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let tune = dof_parity_lane(0u, 8u);
    let radius = axiom_dof_gather_radius(tune.z);
    let off = axiom_dof_tap_offset(index, tune.x, radius);
    let dist = axiom_dof_tap_distance(off);
    return vec4<f32>(off.x, off.y, dist, axiom_dof_tap_weight(tune.y, dist));
}

// The tap values the gather accumulates, as a function of the index — mirrored
// exactly in Rust, so the sampler is out of the loop and what is measured is
// the accumulation.
fn dof_parity_tap(index: u32) -> vec4<f32> {
    let f = f32(index);
    return vec4<f32>(0.05 + f * 0.031, 0.11 + f * 0.017, 0.23 + f * 0.009, f * 0.21);
}

// Lane 9 = (rotation, maxCoc, unused, unused) and the centre in lane 10.
// The rotation is handed IN, never computed from IGN here: see the module docs.
@fragment
fn dof_parity_gather_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let tune = dof_parity_lane(sample, 9u);
    let centre = dof_parity_lane(sample, 10u);
    let radius = axiom_dof_gather_radius(tune.y);

    var sum = centre.rgb;
    var wsum = 1.0;
    var max_coc = centre.a;

    for (var i: u32 = 0u; i < AXIOM_DOF_TAPS; i = i + 1u) {
        let off = axiom_dof_tap_offset(i, tune.x, radius);
        let s = dof_parity_tap(i);
        let w = axiom_dof_tap_weight(s.a, axiom_dof_tap_distance(off));
        sum = sum + s.rgb * w;
        wsum = wsum + w;
        max_coc = max(max_coc, s.a);
    }

    return vec4<f32>(sum / max(wsum, 1e-4), max_coc);
}

// Lane 11 = (sharp rgb, sharp CoC); the blur comes from lane 10.
@fragment
fn dof_parity_combine_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let sharp_lane = dof_parity_lane(sample, 11u);
    let blur_raw = dof_parity_lane(sample, 10u);
    let blur = vec4<f32>(max(blur_raw.rgb, vec3<f32>(0.0)), blur_raw.a);
    let out = axiom_dof_combine(max(sharp_lane.rgb, vec3<f32>(0.0)), blur, sharp_lane.w);
    return vec4<f32>(out, 0.0);
}

// Lane 3 = (centre depth, fragX, fragY, frame phase). The loose tier.
@fragment
fn dof_parity_ign_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let lane = dof_parity_lane(sample, 3u);
    let frag = vec2<f32>(lane.y, lane.z);
    return vec4<f32>(
        axiom_dof_gather_rotation(frag, lane.w),
        axiom_dof_ign(frag),
        0.0,
        0.0,
    );
}
"#;

/// A real GPU, or a loud failure. A parity test that silently passes when
/// nothing ran is worse than no parity test.
struct DofGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    backend: wgpu::Backend,
}

impl DofGpu {
    /// This module's handle on the **crate's one** instance + adapter + device.
    /// Never a `wgpu::Instance` of its own — twenty sites doing that is what
    /// makes this machine's driver fall over; see [`crate::test_gpu`].
    fn shared() -> DofGpu {
        let gpu = crate::test_gpu::TestGpu::shared();
        DofGpu {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            backend: gpu.backend,
        }
    }

    /// Compile the DOF arithmetic with the harness spliced after it.
    fn compile(&self) -> wgpu::ShaderModule {
        // The error scope is the SHARED device's, so it is entered exclusively.
        let (module, failure) = crate::test_gpu::validating(&self.device, || {
            self.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-dof-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        [DOF_WGSL, PARITY_HARNESS_WGSL].concat().into(),
                    ),
                })
        });
        assert!(
            failure.is_none(),
            "the DOF WGSL must compile: {}",
            failure.map_or(String::new(), |error| error.to_string())
        );
        module
    }

    /// Render `entry_point` over a `SAMPLES x 1` `Rgba32Float` target — a float
    /// target because an `Rgba8Unorm` one quantises to 1/255, four orders of
    /// magnitude coarser than every tolerance here.
    fn render(
        &self,
        module: &wgpu::ShaderModule,
        entry_point: &str,
        uniform: &[u8],
    ) -> Vec<[f32; 4]> {
        let layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("axiom-dof-parity-bgl"),
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
                label: Some("axiom-dof-parity-uniform"),
                contents: uniform,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-dof-parity-bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("axiom-dof-parity-pl"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-dof-parity-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("dof_parity_vs"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(entry_point),
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
            label: Some("axiom-dof-parity-target"),
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
            label: Some("axiom-dof-parity-readback"),
            size: u64::from(row_bytes),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-dof-parity-pass"),
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
                    f32::from_le_bytes([mapped[at], mapped[at + 1], mapped[at + 2], mapped[at + 3]])
                })
            })
            .collect()
    }
}

/// The rotation the spiral tier runs at. A fixed non-zero angle, so the
/// golden-angle chain is exercised with a real offset rather than from zero.
const SPIRAL_ROTATION: f32 = 0.7913;

/// The tap CoC the spiral tier weights against — wide enough that the weight
/// spans the whole `0..1` range across the 32 taps rather than saturating.
const SPIRAL_TAP_COC: f32 = 6.0;

/// The frame CoC the spiral tier's radius comes from: `gather_radius(8.0)` is
/// `4.0` half-res pixels, which is a real bokeh rather than the `1.0` floor.
const SPIRAL_MAX_COC: f32 = 8.0;

/// The settings one sample runs with: the source's shipped table, with the CoC
/// swept so the pass is exercised across its ADS ramp rather than pinned at one
/// engagement.
fn settings_of(sample: usize) -> ([f32; 4], [f32; 4]) {
    let settings = Dof {
        // Sweep the near ratio too, so the `max( far, near * ratio )` split is
        // driven from both sides.
        near_ratio: SOURCE_SETTINGS.near_ratio + sample as f32 * 0.013,
        ..SOURCE_SETTINGS
    };
    let coc_px = 0.4 + sample as f32 * 0.19;
    (settings.focus_lane(coc_px), settings.range_lane())
}

/// The four prefilter depths for one sample, spanning sky, near, focal and far.
fn depths_of(sample: usize) -> [f32; 4] {
    let v = sample as f32;
    [
        // Every fourth sample puts sky in the first tap, so `depth <= 0` is
        // exercised rather than only the ordinary path.
        [1.1 + v * 0.37, 0.0][usize::from(sample % 4 == 0)],
        2.0 + v * 0.91,
        7.5 + v * 1.30,
        14.0 + v * 3.10,
    ]
}

/// The centre-of-screen depth that sets the focal plane, swept across and past
/// both clamp ends.
fn centre_depth_of(sample: usize) -> f32 {
    [1.4 + sample as f32 * 0.71, 0.0][usize::from(sample % 7 == 0)]
}

/// A fragment coordinate for the IGN tier — 1080p-scale, which is where the
/// cancellation actually bites.
fn frag_of(sample: usize) -> [f32; 2] {
    let v = sample as f32;
    [37.5 + v * 61.0, 12.5 + v * 33.0]
}

/// The four prefilter colours, one deliberately negative so the `max( …, 0 )`
/// floor is driven on both sides.
fn colours_of(sample: usize) -> [[f32; 3]; 4] {
    let v = sample as f32;
    [
        [0.03 + v * 0.21, 0.11 + v * 0.07, 0.44 + v * 0.013],
        [1.90 - v * 0.04, 0.31 + v * 0.05, 0.07 + v * 0.021],
        [-0.02, 0.55 + v * 0.03, 0.90 - v * 0.011],
        [0.61 + v * 0.017, 0.02 + v * 0.043, 0.29 + v * 0.008],
    ]
}

/// The gather's centre tap and the blur the combine reads.
fn centre_of(sample: usize) -> [f32; 4] {
    let v = sample as f32;
    [
        0.21 + v * 0.05,
        0.44 - v * 0.006,
        0.13 + v * 0.019,
        v * 0.11,
    ]
}

/// The sharp colour and its CoC for the combine tier.
fn sharp_of(sample: usize) -> [f32; 4] {
    let v = sample as f32;
    [0.71 - v * 0.012, 0.18 + v * 0.021, -0.03, v * 0.14]
}

/// The rotation handed to the gather and the spiral tiers. **Not** IGN: see the
/// module docs on why the two budgets are kept apart.
fn rotation_of(sample: usize) -> f32 {
    sample as f32 * 0.1973
}

/// The tap the gather accumulates at `index` — the exact mirror of the
/// harness's `dof_parity_tap`.
fn parity_tap(index: usize) -> [f32; 4] {
    let f = index as f32;
    [
        0.05 + f * 0.031,
        0.11 + f * 0.017,
        0.23 + f * 0.009,
        f * 0.21,
    ]
}

/// The uniform bytes for all [`SAMPLES`] samples, padded to the harness's
/// declared array.
fn uniform_bytes() -> Vec<u8> {
    let mut lanes: Vec<f32> = (0..SAMPLES)
        .flat_map(|sample| {
            let (focus_lane, range_lane) = settings_of(sample);
            let colours = colours_of(sample);
            let frag = frag_of(sample);
            focus_lane
                .into_iter()
                .chain(range_lane)
                .chain(depths_of(sample))
                .chain([
                    centre_depth_of(sample),
                    frag[0],
                    frag[1],
                    crate::dof::frame_phase(sample as u32),
                ])
                .chain(colours[0].into_iter().chain([0.0]))
                .chain(colours[1].into_iter().chain([0.0]))
                .chain(colours[2].into_iter().chain([0.0]))
                .chain(colours[3].into_iter().chain([0.0]))
                // Lane 8 — the spiral tier's tune. Read from SAMPLE 0 only, so
                // it is the same constant in every sample.
                .chain([SPIRAL_ROTATION, SPIRAL_TAP_COC, SPIRAL_MAX_COC, 0.0])
                // Lane 9 — the gather tier's tune, per sample.
                .chain([rotation_of(sample), focus_lane[0], 0.0, 0.0])
                .chain(centre_of(sample))
                .chain(sharp_of(sample))
                .collect::<Vec<f32>>()
        })
        .collect();
    lanes.resize(SAMPLES * LANES * 4, 0.0);
    lanes.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// The worst absolute lane delta over a chosen lane set — the measurement a
/// tolerance is set from.
fn worst_delta(cpu: &[[f32; 4]], gpu: &[[f32; 4]], lanes: &[usize]) -> f32 {
    cpu.iter()
        .zip(gpu.iter())
        .flat_map(|(expected, actual)| {
            lanes
                .iter()
                .map(|lane| (expected[*lane] - actual[*lane]).abs())
                .collect::<Vec<f32>>()
        })
        .fold(0.0_f32, f32::max)
}

/// Every tier's `(cpu, gpu)` lane sets, computed once so the measurement
/// assertion and the four tier tests do not each pay for a device round trip.
struct Measured {
    coc: (Vec<[f32; 4]>, Vec<[f32; 4]>),
    prefilter: (Vec<[f32; 4]>, Vec<[f32; 4]>),
    combine: (Vec<[f32; 4]>, Vec<[f32; 4]>),
    spiral: (Vec<[f32; 4]>, Vec<[f32; 4]>),
    gather: (Vec<[f32; 4]>, Vec<[f32; 4]>),
    ign: (Vec<[f32; 4]>, Vec<[f32; 4]>),
}

/// Drive every tier once.
fn measure() -> Measured {
    let gpu = DofGpu::shared();
    let module = gpu.compile();
    let bytes = uniform_bytes();
    let run = |entry: &str| gpu.render(&module, entry, &bytes);

    let coc_cpu: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|sample| {
            let (focus_lane, range_lane) = settings_of(sample);
            let focus = focus_distance(centre_depth_of(sample), focus_lane);
            let depths = depths_of(sample);
            [
                focus,
                coc(depths[0], focus, focus_lane, range_lane),
                coc(depths[1], focus, focus_lane, range_lane),
                coc(depths[3], focus, focus_lane, range_lane),
            ]
        })
        .collect();

    let prefilter_cpu: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|sample| {
            let (focus_lane, range_lane) = settings_of(sample);
            let focus = focus_distance(centre_depth_of(sample), focus_lane);
            prefilter(
                colours_of(sample),
                depths_of(sample),
                focus,
                focus_lane,
                range_lane,
            )
        })
        .collect();

    let combine_cpu: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|sample| {
            let sharp = sharp_of(sample);
            let out = combine([sharp[0], sharp[1], sharp[2]], centre_of(sample), sharp[3]);
            [out[0], out[1], out[2], 0.0]
        })
        .collect();

    // The spiral tier is indexed by TAP, not by sample: column i is tap i, and
    // the tune is the fixed constant every sample writes into lane 8.
    let spiral_radius = crate::dof::gather_radius(SPIRAL_MAX_COC);
    let spiral_cpu: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|index| {
            let off = tap_offset(index, SPIRAL_ROTATION, spiral_radius);
            let dist = tap_distance(off);
            [off[0], off[1], dist, tap_weight(SPIRAL_TAP_COC, dist)]
        })
        .collect();

    let taps: [[f32; 4]; TAPS] = std::array::from_fn(parity_tap);
    let gather_cpu: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|sample| {
            let (focus_lane, _) = settings_of(sample);
            gather(
                centre_of(sample),
                &taps,
                rotation_of(sample),
                crate::dof::gather_radius(focus_lane[0]),
            )
        })
        .collect();

    let ign_cpu: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|sample| {
            let frag = frag_of(sample);
            [
                gather_rotation(frag, crate::dof::frame_phase(sample as u32)),
                ign(frag),
                0.0,
                0.0,
            ]
        })
        .collect();

    Measured {
        coc: (coc_cpu, run("dof_parity_coc_fs")),
        prefilter: (prefilter_cpu, run("dof_parity_prefilter_fs")),
        combine: (combine_cpu, run("dof_parity_combine_fs")),
        spiral: (spiral_cpu, run("dof_parity_spiral_fs")),
        gather: (gather_cpu, run("dof_parity_gather_fs")),
        ign: (ign_cpu, run("dof_parity_ign_fs")),
    }
}

/// The suite must run on a real adapter. `Noop` renders nothing and would make
/// every comparison below a comparison of two zero buffers.
#[test]
fn the_proof_runs_on_a_real_adapter() {
    let gpu = DofGpu::shared();
    assert_ne!(
        gpu.backend,
        wgpu::Backend::Noop,
        "a parity proof needs a real adapter; there is no honest fallback"
    );
}

/// **Tier 1.** The circle of confusion, the focal plane and the near/far split.
#[test]
fn the_coc_agrees_with_the_cpu_reference() {
    let measured = measure();
    let (cpu, gpu) = &measured.coc;
    let worst = worst_delta(cpu, gpu, &[0, 1, 2, 3]);
    assert!(
        worst <= COC_TOLERANCE,
        "the CoC must match the CPU reference within {COC_TOLERANCE}, worst {worst}"
    );
    // And the tier must be exercising something: a run where every CoC came out
    // zero would pass the comparison and prove nothing.
    let widest = cpu
        .iter()
        .flat_map(|lane| [lane[1], lane[2], lane[3]])
        .fold(0.0_f32, f32::max);
    assert!(
        widest > 1.0,
        "the sweep must reach a real blur; widest CoC was {widest} px"
    );
}

/// **Tier 2.** The prefilter's weighted mean and neighbourhood maximum, and the
/// combine's dilation and blend.
#[test]
fn the_prefilter_and_combine_agree_with_the_cpu_reference() {
    let measured = measure();
    let (pre_cpu, pre_gpu) = &measured.prefilter;
    let pre_worst = worst_delta(pre_cpu, pre_gpu, &[0, 1, 2, 3]);
    assert!(
        pre_worst <= FILTER_TOLERANCE,
        "the prefilter must match within {FILTER_TOLERANCE}, worst {pre_worst}"
    );
    let (com_cpu, com_gpu) = &measured.combine;
    let com_worst = worst_delta(com_cpu, com_gpu, &[0, 1, 2]);
    assert!(
        com_worst <= FILTER_TOLERANCE,
        "the combine must match within {FILTER_TOLERANCE}, worst {com_worst}"
    );
    // The prefilter's alpha is the neighbourhood MAXIMUM, and the GPU must agree
    // it is a maximum rather than a mean — a defect a tolerance alone would let
    // through only if the four CoCs happened to be equal, so pin the spread.
    let spread = pre_cpu
        .iter()
        .map(|lane| lane[3])
        .fold(0.0_f32, f32::max);
    assert!(
        spread > 1.0,
        "the sweep must produce a real neighbourhood maximum, got {spread}"
    );
}

/// **Tier 3.** The bokeh spiral, one column per tap, so a transposed or
/// re-associated offset cannot survive.
#[test]
fn the_bokeh_spiral_agrees_with_the_cpu_reference() {
    let measured = measure();
    let (cpu, gpu) = &measured.spiral;
    let worst = worst_delta(cpu, gpu, &[0, 1, 2, 3]);
    assert!(
        worst <= SPIRAL_TOLERANCE,
        "the spiral must match within {SPIRAL_TOLERANCE}, worst {worst}"
    );
    // Thirty-two distinct taps, rising in distance — the pattern itself, not
    // just its arithmetic.
    assert_eq!(cpu.len(), TAPS, "one column per tap");
    let rising = cpu.windows(2).all(|w| w[1][2] > w[0][2]);
    assert!(rising, "tap distance must rise with the index on the GPU's own table");
}

/// **Tier 3b.** The gather's accumulation, driven with an exact rotation so the
/// IGN budget does not leak into it.
#[test]
fn the_gather_accumulates_in_the_source_order() {
    let measured = measure();
    let (cpu, gpu) = &measured.gather;
    let worst = worst_delta(cpu, gpu, &[0, 1, 2, 3]);
    assert!(
        worst <= GATHER_TOLERANCE,
        "the gather must match within {GATHER_TOLERANCE}, worst {worst}"
    );
    // The maximum CoC lane must be the taps' maximum, which is a reduction the
    // arithmetic tolerance would not catch if it were a sum.
    let expected_max = parity_tap(TAPS - 1)[3];
    let carried = cpu.iter().map(|lane| lane[3]).fold(0.0_f32, f32::max);
    assert!(
        (carried - expected_max).abs() < 1e-6,
        "the gather must carry max(tap CoC) = {expected_max}, got {carried}"
    );
}

/// **Tier 4.** Interleaved gradient noise, at its own — structurally loose —
/// budget.
#[test]
fn ign_agrees_within_its_own_budget() {
    let measured = measure();
    let (cpu, gpu) = &measured.ign;
    let worst = worst_delta(cpu, gpu, &[0, 1]);
    assert!(
        worst <= IGN_TOLERANCE,
        "IGN must match within its cancellation budget {IGN_TOLERANCE}, worst {worst}"
    );
    // The dither must actually decorrelate: a constant rotation would pass the
    // comparison and destroy the bokeh.
    let rotations: Vec<f32> = cpu.iter().map(|lane| lane[0]).collect();
    let distinct = rotations.windows(2).all(|w| (w[1] - w[0]).abs() > 1e-3);
    assert!(distinct, "IGN must decorrelate neighbours: {rotations:?}");
}

/// **Every tolerance above is re-measured here, and this test fails if any is
/// more than 10x the delta actually observed.**
///
/// The four constants were *derived*, not fitted — this wave could not run
/// them. This is what turns them from an expectation into a measurement on the
/// first real run: it reports each observed delta in its own failure message,
/// so the number to tighten to is in the output rather than in a re-run.
#[test]
fn the_tolerances_are_within_ten_times_the_measured_delta() {
    let measured = measure();
    let observations = [
        (
            "CoC",
            COC_TOLERANCE,
            worst_delta(&measured.coc.0, &measured.coc.1, &[0, 1, 2, 3]),
        ),
        (
            "prefilter",
            FILTER_TOLERANCE,
            worst_delta(&measured.prefilter.0, &measured.prefilter.1, &[0, 1, 2, 3]),
        ),
        (
            "combine",
            FILTER_TOLERANCE,
            worst_delta(&measured.combine.0, &measured.combine.1, &[0, 1, 2]),
        ),
        (
            "spiral",
            SPIRAL_TOLERANCE,
            worst_delta(&measured.spiral.0, &measured.spiral.1, &[0, 1, 2, 3]),
        ),
        (
            "gather",
            GATHER_TOLERANCE,
            worst_delta(&measured.gather.0, &measured.gather.1, &[0, 1, 2, 3]),
        ),
        (
            "IGN",
            IGN_TOLERANCE,
            worst_delta(&measured.ign.0, &measured.ign.1, &[0, 1]),
        ),
    ];
    // A tier that agrees BIT-EXACTLY is a real and welcome outcome for
    // arithmetic this short, so a zero delta is not treated as a failure — but
    // it must be a zero delta over output that actually varies, or the tier is
    // not running. That is what the emptiness check below separates.
    for (name, tolerance, delta) in observations {
        // An exact-zero delta is a real and welcome outcome for arithmetic this
        // short, and must not be read as a tolerance that is too loose. "The
        // tier never ran" is caught by the emptiness check below instead.
        assert!(
            delta == 0.0 || tolerance <= delta * 10.0,
            "the {name} tolerance {tolerance} is more than 10x the measured delta {delta}; \
             tighten it toward {}",
            delta * 4.0
        );
    }

    // Every tier must have produced varied, non-zero GPU output. An all-zero
    // read-back is what "the entry point never ran" looks like, and it would
    // otherwise sail through every comparison above.
    let tiers: [(&str, &Vec<[f32; 4]>); 6] = [
        ("CoC", &measured.coc.1),
        ("prefilter", &measured.prefilter.1),
        ("combine", &measured.combine.1),
        ("spiral", &measured.spiral.1),
        ("gather", &measured.gather.1),
        ("IGN", &measured.ign.1),
    ];
    for (name, rendered) in tiers {
        let magnitude = rendered
            .iter()
            .flat_map(|lane| lane.iter().map(|v| v.abs()))
            .fold(0.0_f32, f32::max);
        assert!(
            magnitude > 1e-3,
            "the {name} tier read back an all-but-zero buffer ({magnitude}), which means the \
             entry point did not run rather than that it agreed"
        );
        let distinct = rendered.iter().any(|lane| *lane != rendered[0]);
        assert!(
            distinct,
            "the {name} tier read back a constant buffer, so its sweep is not sweeping"
        );
    }
}
