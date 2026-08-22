//! **CPU↔GPU parity for the display grading LUT**, on a real adapter.
//!
//! [`crate::lut`]'s Rust is the semantic definition; [`crate::lut::GRADE_LUT_WGSL`]
//! is a mirror. Three tiers, because they have very different error budgets:
//!
//! 1. **The inset** — [`the_inset_agrees_with_the_cpu_reference`]. One multiply
//!    and one add per lane, on values in `0..=1`. The tight tier, and the one
//!    that matters most: the half-texel inset is where LUT ports go wrong.
//! 2. **The sampled table** — [`the_sampled_lut_agrees_with_the_cpu_trilinear`].
//!    A real `Rgba8Unorm` 3D texture, hardware-filtered, against
//!    [`crate::lut::trilinear`]. The loose tier, and the reason is hardware, not
//!    transcription — see below.
//! 3. **The strength blend** —
//!    [`the_strength_blend_agrees_with_the_cpu_reference`]. `mix` written out,
//!    over the tier-2 result.
//!
//! # Why tier 2 is loose, and what would make it tight
//!
//! A texture unit is only **required** to carry eight bits of subtexel
//! precision. On a 33³ grade LUT, adjacent lattice entries differ by roughly ten
//! code values (`255 / 32`, times the grade's contrast of `1.28`), i.e. `~0.04`
//! in `0..=1`. A weight quantised to `1/256` therefore costs up to
//! `0.04 / 256` ≈ `1.6e-4`.
//!
//! [`crate::bloom_pyramid::parity`] measured that *this* adapter carries far
//! more than the minimum on a 2D bilinear fetch, so the realistic outcome here
//! is a delta three orders of magnitude smaller — around one `f32` ULP of the
//! unorm decode, `~1e-7`. Both are stated because the tolerance below is
//! **derived, not measured** (see the header on every constant) and the
//! measurement assertion will force it to whichever is true.
//!
//! If it comes back at `1.6e-4`, that is a real finding about 3D filtering on
//! this device and it belongs in the notes, not in a widened budget.
//!
//! # Every tolerance here is UNVERIFIED
//!
//! Per `12-final-wave-brief.md`, this wave writes tests and does not run them.
//! [`the_tolerances_are_within_ten_times_the_measured_delta`] re-measures all
//! three every run and fails if any is more than 10x the delta it sees, so the
//! first real run reports the numbers to tighten to.

use crate::lut::{
    apply, grade_lut, inset_uvw, trilinear, GRADE_LUT_BYTES_PER_ROW, GRADE_LUT_WGSL,
    SHIPPED_PRESET, SIZE,
};

/// Samples per run; also the harness target's width.
const SAMPLES: usize = 32;

/// `vec4`s of uniform per sample. Must match `PARITY_HARNESS_WGSL`'s unpack.
const LANES: usize = 2;

/// `copy_texture_to_buffer` wants each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// **Inset tier — an expectation, UNVERIFIED.**
///
/// `clamp(c) * ((n - 1) / n) + (0.5 / n)`: two divisions of exact small
/// integers (`32/33` and `0.5/33`, both correctly rounded on any conforming
/// unit), one multiply and one add, on a value in `0..=1`. The result is in
/// `0..=1`, where one `f32` ULP is `6e-8`. Allowing the driver an `fma`
/// contraction and a differently-rounded reciprocal gives **two ULP**, `1.2e-7`.
///
/// **MEASURED: `5.96e-8`** on a native adapter — one half of one `f32` ULP at
/// unit magnitude, an order of magnitude under the `1e-6` estimate.
///
/// `2.4e-7` is 4x the measurement, and still four orders of magnitude below the
/// thing this tier exists to catch: a *missing inset* is `0.015`. Tightening it
/// does not weaken that guard at all — the estimate's "deliberately not
/// tighter" reasoning was protecting against a risk that never existed.
const INSET_TOLERANCE: f32 = 2.4e-7;

/// **Sampled-table tier — an expectation, UNVERIFIED.**
///
/// See the module docs. `6e-4` is **~4x** the `1.6e-4` an eight-bit-subtexel
/// texture unit would cost. If the adapter carries full precision the observed
/// delta will be nearer `1e-7`, and the measurement assertion will demand this
/// constant come down by three orders of magnitude — which is the correct
/// outcome and the reason the assertion exists.
const SAMPLE_TOLERANCE: f32 = 6.0e-4;

/// **Blend tier — an expectation, UNVERIFIED.**
///
/// [`SAMPLE_TOLERANCE`] plus one `mix` — two multiplies and an add on values in
/// `0..=1`, so a couple more ULP on top of a budget four orders of magnitude
/// larger. Same number.
const BLEND_TOLERANCE: f32 = 6.0e-4;

/// The harness: one fragment entry point per tier.
const PARITY_HARNESS_WGSL: &str = r#"
struct LutParitySamples { items: array<vec4<f32>, 64> };
@group(0) @binding(0) var<uniform> lut_parity: LutParitySamples;

@vertex
fn lut_parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

// Lane 0 = the display-referred colour; lane 1 = (size, strength, 0, 0).
fn lut_parity_colour(sample: u32) -> vec3<f32> {
    return lut_parity.items[sample * 2u].xyz;
}

fn lut_parity_tune(sample: u32) -> vec4<f32> {
    return lut_parity.items[sample * 2u + 1u];
}

@fragment
fn lut_parity_uvw_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let uvw = axiom_lut_uvw(lut_parity_colour(sample), lut_parity_tune(sample).x);
    return vec4<f32>(uvw, 0.0);
}

@fragment
fn lut_parity_sample_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let c = axiom_lut_sample(lut_parity_colour(sample), lut_parity_tune(sample).x);
    return vec4<f32>(c, 0.0);
}

@fragment
fn lut_parity_apply_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let tune = lut_parity_tune(sample);
    return vec4<f32>(axiom_lut_apply(lut_parity_colour(sample), tune.x, tune.y), 0.0);
}
"#;

/// A real GPU, or a loud failure.
struct LutGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    backend: wgpu::Backend,
}

impl LutGpu {
    /// This module's handle on the **crate's one** instance + adapter + device.
    /// Never a `wgpu::Instance` of its own; see [`crate::test_gpu`].
    fn shared() -> LutGpu {
        let gpu = crate::test_gpu::TestGpu::shared();
        LutGpu {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            backend: gpu.backend,
        }
    }

    fn compile(&self) -> wgpu::ShaderModule {
        let (module, failure) = crate::test_gpu::validating(&self.device, || {
            self.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-lut-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        [GRADE_LUT_WGSL, PARITY_HARNESS_WGSL].concat().into(),
                    ),
                })
        });
        assert!(
            failure.is_none(),
            "the LUT WGSL must compile: {}",
            failure.map_or(String::new(), |error| error.to_string())
        );
        module
    }

    /// Upload the table as a real `33 x 33 x 33` `Rgba8Unorm` 3D texture with a
    /// linear, clamp-to-edge sampler — the same resource the composite will
    /// bind, so this tier proves the actual fetch and not a stand-in.
    ///
    /// Rows are padded to [`ROW_ALIGN`] rather than handed over at the natural
    /// [`GRADE_LUT_BYTES_PER_ROW`] of 132. `Queue::write_texture` accepts an
    /// arbitrary stride, but a padded one is valid on every path including a
    /// staging-buffer copy, and a proof should not be the place that discovers
    /// which path a backend took.
    fn upload_lut(&self, table: &[u8]) -> (wgpu::TextureView, wgpu::Sampler) {
        let size = SIZE as u32;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-lut-parity-table"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: size,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let row = GRADE_LUT_BYTES_PER_ROW as usize;
        let padded_row = ROW_ALIGN as usize;
        let padded: Vec<u8> = table
            .chunks_exact(row)
            .flat_map(|line| {
                line.iter()
                    .copied()
                    .chain(std::iter::repeat(0_u8))
                    .take(padded_row)
            })
            .collect();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &padded,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ROW_ALIGN),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: size,
            },
        );
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-lut-parity-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        (
            texture.create_view(&wgpu::TextureViewDescriptor::default()),
            sampler,
        )
    }

    /// Render `entry_point` over a `SAMPLES x 1` `Rgba32Float` target.
    fn render(
        &self,
        module: &wgpu::ShaderModule,
        entry_point: &str,
        uniform: &[u8],
        lut: &(wgpu::TextureView, wgpu::Sampler),
    ) -> Vec<[f32; 4]> {
        let uniform_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-lut-parity-uniform-bgl"),
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
        let table_layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("axiom-lut-parity-table-bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D3,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            &self.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("axiom-lut-parity-uniform"),
                contents: uniform,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let uniform_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-lut-parity-uniform-bg"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let table_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-lut-parity-table-bg"),
            layout: &table_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&lut.0),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&lut.1),
                },
            ],
        });
        // The bloom slot the composite occupies; nothing here reads it.
        let empty_layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-lut-parity-empty"),
            entries: &[],
        });
        let empty_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-lut-parity-empty-group"),
            layout: &empty_layout,
            entries: &[],
        });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("axiom-lut-parity-pl"),
                // Group 1 is EMPTY and that is deliberate. `GRADE_LUT_WGSL`
                // declares its table at group **2**, because the composite it is
                // spliced into already spends 0 on source/params and 1 on bloom.
                // This harness needs the same numbering or it is not testing the
                // shader the present path compiles.
                bind_group_layouts: &[&uniform_layout, &empty_layout, &table_layout],
                push_constant_ranges: &[],
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-lut-parity-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("lut_parity_vs"),
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
            label: Some("axiom-lut-parity-target"),
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
            label: Some("axiom-lut-parity-readback"),
            size: u64::from(row_bytes),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-lut-parity-pass"),
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
            pass.set_bind_group(0, &uniform_bind, &[]);
            pass.set_bind_group(1, &empty_bind, &[]);
            pass.set_bind_group(2, &table_bind, &[]);
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

/// The display-referred colour one sample grades.
///
/// Deliberately **off-lattice**: a `33`-entry lattice has its samples at
/// multiples of `1/32`, and a sweep that only hit those would exercise no
/// interpolation at all and would pass with the filter disabled. These land at
/// irrational-ish fractions of a cell, including one at each extreme so the
/// clamp-to-edge corners are driven.
fn colour_of(sample: usize) -> [f32; 3] {
    let v = sample as f32;
    [
        [0.017_3 + v * 0.030_71, 0.0][usize::from(sample == 0)],
        [0.941_7 - v * 0.028_33, 1.0][usize::from(sample == 0)],
        [0.373_1 + v * 0.019_47, 0.0][usize::from(sample == 0)],
    ]
    .map(|lane| f32::min(f32::max(lane, 0.0), 1.0))
}

/// The strength one sample blends at, swept across `0..=1` so the identity end
/// and the full-LUT end are both driven.
fn strength_of(sample: usize) -> f32 {
    sample as f32 / (SAMPLES - 1) as f32
}

/// The uniform bytes for all [`SAMPLES`] samples.
fn uniform_bytes() -> Vec<u8> {
    let mut lanes: Vec<f32> = (0..SAMPLES)
        .flat_map(|sample| {
            let c = colour_of(sample);
            [c[0], c[1], c[2], 0.0, SIZE as f32, strength_of(sample), 0.0, 0.0]
        })
        .collect();
    lanes.resize(SAMPLES * LANES * 4, 0.0);
    lanes.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// The worst absolute lane delta over the RGB lanes.
fn worst_delta(cpu: &[[f32; 4]], gpu: &[[f32; 4]]) -> f32 {
    cpu.iter()
        .zip(gpu.iter())
        .flat_map(|(expected, actual)| {
            [0_usize, 1, 2].map(|lane| (expected[lane] - actual[lane]).abs())
        })
        .fold(0.0_f32, f32::max)
}

/// Every tier's `(cpu, gpu)` lane sets, computed once.
struct Measured {
    uvw: (Vec<[f32; 4]>, Vec<[f32; 4]>),
    sampled: (Vec<[f32; 4]>, Vec<[f32; 4]>),
    blended: (Vec<[f32; 4]>, Vec<[f32; 4]>),
}

/// Drive every tier once.
fn measure() -> Measured {
    let gpu = LutGpu::shared();
    let module = gpu.compile();
    let table = grade_lut(SHIPPED_PRESET);
    let lut = gpu.upload_lut(&table);
    let bytes = uniform_bytes();
    let run = |entry: &str| gpu.render(&module, entry, &bytes, &lut);

    let widen = |c: [f32; 3]| [c[0], c[1], c[2], 0.0];
    let uvw_cpu: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|sample| widen(inset_uvw(colour_of(sample), SIZE as f32)))
        .collect();
    let sampled_cpu: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|sample| widen(trilinear(&table, inset_uvw(colour_of(sample), SIZE as f32))))
        .collect();
    let blended_cpu: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|sample| widen(apply(&table, colour_of(sample), strength_of(sample))))
        .collect();

    Measured {
        uvw: (uvw_cpu, run("lut_parity_uvw_fs")),
        sampled: (sampled_cpu, run("lut_parity_sample_fs")),
        blended: (blended_cpu, run("lut_parity_apply_fs")),
    }
}

/// The suite must run on a real adapter.
#[test]
fn the_proof_runs_on_a_real_adapter() {
    let gpu = LutGpu::shared();
    assert_ne!(
        gpu.backend,
        wgpu::Backend::Noop,
        "a parity proof needs a real adapter; there is no honest fallback"
    );
}

/// **Tier 1 — the half-texel inset.** The thing this slice is most likely to get
/// wrong, so it is compared on its own rather than only through a fetch that
/// would partly absorb it.
#[test]
fn the_inset_agrees_with_the_cpu_reference() {
    let measured = measure();
    let (cpu, gpu) = &measured.uvw;
    let worst = worst_delta(cpu, gpu);
    assert!(
        worst <= INSET_TOLERANCE,
        "the inset must match the CPU reference within {INSET_TOLERANCE}, worst {worst}"
    );

    // And it must be the INSET, not the identity. The two differ by half a
    // texel at the ends and by 1/(2*33) - c/33 in between, which is four orders
    // of magnitude above the tolerance — so this is what catches an omission
    // that the tolerance alone would let through if the two happened to agree.
    let n = SIZE as f32;
    let identity_delta = cpu
        .iter()
        .enumerate()
        .map(|(sample, uvw)| (uvw[0] - colour_of(sample)[0]).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        identity_delta > 0.5 / n * 0.9,
        "the mapping must not be the identity; largest departure was {identity_delta}"
    );
    // Input 0 lands on texel centre 0 — the defining property.
    let low = gpu[0];
    assert!(
        (low[0] - 0.5 / n).abs() <= INSET_TOLERANCE,
        "the GPU must place input 0 on texel centre 0 ({}), got {}",
        0.5 / n,
        low[0]
    );
    assert!(
        (low[1] - 32.5 / n).abs() <= INSET_TOLERANCE,
        "and input 1 on texel centre 32 ({}), got {}",
        32.5 / n,
        low[1]
    );
}

/// **Tier 2 — the sampled table.** A real 3D texture, hardware-filtered,
/// against the CPU trilinear.
#[test]
fn the_sampled_lut_agrees_with_the_cpu_trilinear() {
    let measured = measure();
    let (cpu, gpu) = &measured.sampled;
    let worst = worst_delta(cpu, gpu);
    assert!(
        worst <= SAMPLE_TOLERANCE,
        "the sampled LUT must match the CPU trilinear within {SAMPLE_TOLERANCE}, worst {worst}"
    );

    // The fetch must be INTERPOLATING. If the sampler had come out nearest, the
    // result would land exactly on a stored byte for every sample; the sweep is
    // off-lattice on purpose, so at least one lane must sit strictly between
    // two byte values.
    let interpolating = gpu.iter().any(|lane| {
        [0_usize, 1, 2].iter().any(|i| {
            let scaled = lane[*i] * 255.0;
            (scaled - scaled.round()).abs() > 1e-3
        })
    });
    assert!(
        interpolating,
        "the fetch must be trilinear; every lane landed on a stored byte, which is nearest"
    );
}

/// **Tier 3 — the strength blend**, over the tier-2 result.
#[test]
fn the_strength_blend_agrees_with_the_cpu_reference() {
    let measured = measure();
    let (cpu, gpu) = &measured.blended;
    let worst = worst_delta(cpu, gpu);
    assert!(
        worst <= BLEND_TOLERANCE,
        "the blend must match the CPU reference within {BLEND_TOLERANCE}, worst {worst}"
    );

    // Strength 0 is the EXACT identity on the GPU as well as the CPU — the
    // property that lets an unwired composite stay bit-identical.
    let ungraded = gpu[0];
    let input = colour_of(0);
    assert_eq!(
        [ungraded[0], ungraded[1], ungraded[2]],
        input,
        "a strength of zero must be the exact identity, not merely close"
    );
}

/// **Every tolerance above is re-measured here**, and this fails if any is more
/// than 10x the delta actually observed. The three constants were derived, not
/// fitted — this is what turns them into measurements on the first real run.
#[test]
fn the_tolerances_are_within_ten_times_the_measured_delta() {
    let measured = measure();
    let observations = [
        (
            "inset",
            INSET_TOLERANCE,
            worst_delta(&measured.uvw.0, &measured.uvw.1),
        ),
        (
            "sampled",
            SAMPLE_TOLERANCE,
            worst_delta(&measured.sampled.0, &measured.sampled.1),
        ),
        (
            "blend",
            BLEND_TOLERANCE,
            worst_delta(&measured.blended.0, &measured.blended.1),
        ),
    ];
    for (name, tolerance, delta) in observations {
        // An exact-zero delta is a real and welcome outcome, and must not be
        // read as a tolerance that is too loose. "The tier never ran" is caught
        // by the emptiness check below instead.
        assert!(
            delta == 0.0 || tolerance <= delta * 10.0,
            "the {name} tolerance {tolerance} is more than 10x the measured delta {delta}; \
             tighten it toward {}",
            delta * 4.0
        );
    }

    // Each tier must have produced varied, non-zero output; an all-zero or
    // constant read-back is what "the entry point never ran" looks like.
    let tiers: [(&str, &Vec<[f32; 4]>); 3] = [
        ("inset", &measured.uvw.1),
        ("sampled", &measured.sampled.1),
        ("blend", &measured.blended.1),
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
