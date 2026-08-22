//! **CPU↔GPU parity for the bloom pyramid**, on a real adapter.
//!
//! [`crate::bloom_pyramid::reference`] is the semantic definition; the WGSL is a
//! mirror, and a mirror nobody holds up to the original is a second definition
//! waiting to drift. This module holds it up, three ways, because the three have
//! genuinely different error budgets and folding them into one number would let
//! the loosest hide the other two:
//!
//! 1. **The tap tables** — [`the_wgsl_tap_tables_are_the_rust_ones`]. Rendered
//!    straight back out of the shader and compared **bit for bit**. Small
//!    integers in an `f32`, so there is no tolerance to argue about, and a
//!    transposed offset — the defect that would shear the whole pyramid by one
//!    texel and still look like a bloom — cannot survive it.
//! 2. **The filter arithmetic** — [`the_filters_agree_with_the_cpu_reference`].
//!    Tap colours handed in through a uniform, so the sampler is out of the loop
//!    and what is measured is the transcription. This is the tight tier.
//! 3. **The whole chain** — [`the_rendered_pyramid_matches_the_cpu_reference`].
//!    [`chain::BloomPyramid`] driven end to end over a real `Rgba16Float` source,
//!    read back and compared against [`reference::render`]. This is the loose
//!    tier, and the module docs below say exactly why and by how much.
//!
//! Every tolerance is **measured**, and the measurement is itself asserted, so a
//! budget cannot rot into a number nobody can justify.

use crate::bloom_pyramid::chain::{BloomPyramid, LEVEL_FORMAT};
use crate::bloom_pyramid::filters::tests::taps13;
use crate::bloom_pyramid::filters::{
    combine, downsample_karis, downsample_plain, upsample_tent, DOWN_TAPS, UP_TAPS,
};
use crate::bloom_pyramid::half_storage::{from_half_bits, to_half_bits};
use crate::bloom_pyramid::reference::tests::scene;
use crate::bloom_pyramid::reference::{render, Image};
use crate::bloom_pyramid::schedule::LEVELS_HIGH;
use crate::bloom_pyramid::wgsl::BLOOM_PYRAMID_WGSL;
use crate::bloom_pyramid::{BloomTuning, SOURCE_SETTINGS};

/// How many samples one arithmetic run compares; also the harness target's
/// width. Sixteen, so the thirteen downsample taps and the nine tent taps both
/// fit in one column-indexed render for the table comparison.
const SAMPLES: usize = 16;

/// `vec4`s of uniform per sample. Must match `PARITY_HARNESS_WGSL`'s unpack.
const LANES: usize = 15;

/// `copy_texture_to_buffer` wants each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// **The arithmetic tier's tolerance, derived from a measurement.** See
/// [`the_arithmetic_tolerance_is_within_ten_times_the_measured_delta`], which
/// re-measures every run and fails if this drifts loose.
///
/// The filters are one division, one reciprocal per group, and a long chain of
/// multiply-adds; the only differences that can remain are the hardware's
/// freedom to contract `a*b + c` into an `fma` and to evaluate the reciprocal at
/// a different precision.
///
/// **Measured: `1.907e-6`** — exactly `2^-19` — over the whole sample table on a
/// Vulkan adapter, on tap colours of order 1..15. That is two `f32` ULP at the
/// largest of them, which is the floor for a chain this long: the emitter writes
/// out `dot`, `clamp` and `mix` by hand precisely so the only remaining freedom
/// is contraction. `4e-6` is **2.1x** that measurement, inside the brief's 10x
/// rule with room for a driver that contracts differently.
const ARITHMETIC_TOLERANCE: f32 = 4.0e-6;

/// **The chain tier's tolerance, derived from a measurement.**
///
/// **Measured: `6.1035e-5`** — exactly `2^-14`, which is **one `f16` ULP** at the
/// magnitudes level 0 carries. Eleven `Rgba16Float` stores separate the source
/// from the answer, and one ULP of the storage format is the tightest a chain
/// through them can be: the reference models the same rounding
/// ([`crate::bloom_pyramid::half_storage`]) but applies it at a slightly
/// different point in the arithmetic than a driver keeping a wider intermediate,
/// and a single one-ULP disagreement is what that costs.
///
/// It is *not* the sampler. The tent upsample runs at `uRadius = 0.62` on the
/// widest two levels, so its taps land at a fractional texel offset, and a
/// texture unit is only *required* to carry eight bits of subtexel precision —
/// which would have quantised that `0.62` to `158/256` and cost a weight error
/// three orders of magnitude larger than this. The measurement says this adapter
/// carries far more, and that the CPU bilinear here matches it. If a future
/// adapter does only the minimum, this test is what will say so, loudly, rather
/// than a budget wide enough to hide it.
///
/// `3e-4` is **4.9x** the measurement, which leaves room for exactly that kind of
/// hardware difference without leaving room for a defect.
const CHAIN_TOLERANCE: f32 = 3.0e-4;

/// The harness: one fragment entry point per thing being compared, each
/// evaluating the sample its pixel column names.
const PARITY_HARNESS_WGSL: &str = r#"
struct BloomParitySamples { items: array<vec4<f32>, 240> };
@group(0) @binding(0) var<uniform> bloom_parity: BloomParitySamples;

@vertex
fn bloom_parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

fn bloom_parity_taps13(sample: u32) -> array<vec3<f32>, 13> {
    let base = sample * 15u;
    return array<vec3<f32>, 13>(
        bloom_parity.items[base + 0u].xyz,
        bloom_parity.items[base + 1u].xyz,
        bloom_parity.items[base + 2u].xyz,
        bloom_parity.items[base + 3u].xyz,
        bloom_parity.items[base + 4u].xyz,
        bloom_parity.items[base + 5u].xyz,
        bloom_parity.items[base + 6u].xyz,
        bloom_parity.items[base + 7u].xyz,
        bloom_parity.items[base + 8u].xyz,
        bloom_parity.items[base + 9u].xyz,
        bloom_parity.items[base + 10u].xyz,
        bloom_parity.items[base + 11u].xyz,
        bloom_parity.items[base + 12u].xyz,
    );
}

fn bloom_parity_taps9(sample: u32) -> array<vec3<f32>, 9> {
    let base = sample * 15u;
    return array<vec3<f32>, 9>(
        bloom_parity.items[base + 0u].xyz,
        bloom_parity.items[base + 1u].xyz,
        bloom_parity.items[base + 2u].xyz,
        bloom_parity.items[base + 3u].xyz,
        bloom_parity.items[base + 4u].xyz,
        bloom_parity.items[base + 5u].xyz,
        bloom_parity.items[base + 6u].xyz,
        bloom_parity.items[base + 7u].xyz,
        bloom_parity.items[base + 8u].xyz,
    );
}

@fragment
fn bloom_parity_karis_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let tune = bloom_parity.items[sample * 15u + 13u];
    return vec4<f32>(
        bloom_downsample_karis(bloom_parity_taps13(sample), tune.x, tune.y, tune.z),
        0.0,
    );
}

@fragment
fn bloom_parity_plain_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    return vec4<f32>(bloom_downsample_plain(bloom_parity_taps13(sample)), 0.0);
}

@fragment
fn bloom_parity_tent_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    return vec4<f32>(bloom_upsample_tent(bloom_parity_taps9(sample)), 0.0);
}

@fragment
fn bloom_parity_combine_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let tune = bloom_parity.items[sample * 15u + 13u];
    let hdr = bloom_parity.items[sample * 15u + 14u].xyz;
    return vec4<f32>(bloom_combine(hdr, bloom_parity.items[sample * 15u].xyz, tune.w), 0.0);
}

// The tap TABLES themselves, so a transposed offset cannot survive. Column n
// carries the nth downsample offset in `xy` and the nth tent offset in `zw`,
// with the tent's index held at its last entry past the ninth column.
@fragment
fn bloom_parity_taps_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let down = bloom_down_tap(min(index, 12u));
    let up = bloom_up_tap(min(index, 8u));
    return vec4<f32>(down.x, down.y, up.x, up.y);
}
"#;

/// A real GPU, or a loud failure. A parity test that silently passes when
/// nothing ran is worse than no parity test.
struct BloomGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    backend: wgpu::Backend,
}

impl BloomGpu {
    /// This module's handle on the **crate's one** instance + adapter + device.
    ///
    /// The reason there is exactly one is measured, and it lives in
    /// [`crate::test_gpu`]: roughly fifty `#[test]`s each opening their own is
    /// what makes this driver fall over with a `STATUS_ACCESS_VIOLATION`, inside
    /// whichever GPU test happens to be running when the count is reached. This
    /// module was the first to hold one device for all of its tests; the fixture
    /// finished the job for the whole suite. Here it is only a rename of the
    /// shared device into this module's harness vocabulary.
    fn shared() -> BloomGpu {
        let gpu = crate::test_gpu::TestGpu::shared();
        BloomGpu {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            backend: gpu.backend,
        }
    }

    /// Compile the pyramid's shared WGSL with the harness spliced after it.
    fn compile(&self) -> wgpu::ShaderModule {
        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let (module, failure) = crate::test_gpu::validating(&self.device, || {
            self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-bloom-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        [BLOOM_PYRAMID_WGSL, PARITY_HARNESS_WGSL].concat().into(),
                    ),
                })
        });
        assert!(
            failure.is_none(),
            "the bloom pyramid WGSL must compile: {}",
            failure.map_or(String::new(), |error| error.to_string())
        );
        module
    }

    /// Render `entry_point` over a `SAMPLES x 1` `Rgba32Float` target — a float
    /// target because an `Rgba8Unorm` one quantises to 1/255, four orders of
    /// magnitude coarser than the tolerance.
    fn render(&self, module: &wgpu::ShaderModule, entry_point: &str, uniform: &[u8]) -> Vec<[f32; 4]> {
        let layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("axiom-bloom-parity-bgl"),
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
                label: Some("axiom-bloom-parity-uniform"),
                contents: uniform,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-bloom-parity-bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("axiom-bloom-parity-pl"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-bloom-parity-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("bloom_parity_vs"),
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
            label: Some("axiom-bloom-parity-target"),
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
            label: Some("axiom-bloom-parity-readback"),
            size: u64::from(row_bytes),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-bloom-parity-pass"),
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

    /// Upload `image` into an `Rgba16Float` texture — the format the source's
    /// scene target would have had.
    fn upload(&self, image: &Image) -> wgpu::TextureView {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-bloom-parity-source"),
            size: wgpu::Extent3d {
                width: image.width(),
                height: image.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: LEVEL_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bytes: Vec<u8> = image
            .texels()
            .iter()
            .flat_map(|c| [c[0], c[1], c[2], 1.0])
            .flat_map(|v| to_half_bits(v).to_le_bytes())
            .collect();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width() * 8),
                rows_per_image: Some(image.height()),
            },
            wgpu::Extent3d {
                width: image.width(),
                height: image.height(),
                depth_or_array_layers: 1,
            },
        );
        texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// Read the pyramid's level 0 back as linear RGB, row-major.
    fn read_level(&self, pyramid: &BloomPyramid, size: (u32, u32)) -> Vec<[f32; 3]> {
        let row_bytes = (size.0 * 8).div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-bloom-level-readback"),
            size: u64::from(row_bytes) * u64::from(size.1),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        pyramid.copy_output_to_buffer(&mut encoder, &readback, row_bytes);
        self.queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::Wait)
            .expect("the readback must complete");
        let mapped = slice.get_mapped_range();
        (0..size.1)
            .flat_map(|y| (0..size.0).map(move |x| (x, y)))
            .map(|(x, y)| {
                let at = (y * row_bytes + x * 8) as usize;
                [0_usize, 1, 2].map(|lane| {
                    from_half_bits(u16::from_le_bytes([
                        mapped[at + lane * 2],
                        mapped[at + lane * 2 + 1],
                    ]))
                })
            })
            .collect()
    }
}

/// The tuning every arithmetic sample runs with: the source's live settings,
/// plus a per-sample exposure so the metering lane is exercised rather than
/// pinned at one.
fn tuning_of(sample: usize) -> BloomTuning {
    BloomTuning {
        exposure: 0.4 + sample as f32 * 0.17,
        threshold: SOURCE_SETTINGS.threshold,
        knee: SOURCE_SETTINGS.knee,
        strength: SOURCE_SETTINGS.strength,
    }
}

/// The HDR value the combine samples add into.
fn hdr_of(sample: usize) -> [f32; 3] {
    let v = sample as f32;
    [0.03 + v * 0.21, 0.11 + v * 0.07, 0.44 + v * 0.013]
}

/// The uniform bytes for all [`SAMPLES`] samples, padded to the harness's
/// declared array.
fn uniform_bytes() -> Vec<u8> {
    let mut lanes: Vec<f32> = (0..SAMPLES)
        .flat_map(|sample| {
            let taps = taps13(sample as f32);
            let tune = tuning_of(sample);
            let hdr = hdr_of(sample);
            taps.iter()
                .flat_map(|c| [c[0], c[1], c[2], 0.0])
                .chain([tune.exposure, tune.threshold, tune.knee, tune.strength])
                .chain([hdr[0], hdr[1], hdr[2], 0.0])
                .collect::<Vec<f32>>()
        })
        .collect();
    lanes.resize(SAMPLES * LANES * 4, 0.0);
    lanes.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// Both sides of the arithmetic tier: `(cpu, gpu)` lane sets for the four
/// entry points, concatenated in one order.
fn compare_arithmetic(gpu: &BloomGpu) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let module = gpu.compile();
    let bytes = uniform_bytes();
    let rendered: Vec<[f32; 4]> = ["bloom_parity_karis_fs", "bloom_parity_plain_fs", "bloom_parity_tent_fs", "bloom_parity_combine_fs"]
        .into_iter()
        .flat_map(|entry| gpu.render(&module, entry, &bytes))
        .collect();
    let widen = |c: [f32; 3]| [c[0], c[1], c[2], 0.0];
    let evaluated: Vec<[f32; 4]> = (0..SAMPLES)
        .map(|sample| {
            let tune = tuning_of(sample);
            widen(downsample_karis(
                taps13(sample as f32),
                tune.exposure,
                tune.threshold,
                tune.knee,
            ))
        })
        .chain((0..SAMPLES).map(|sample| widen(downsample_plain(taps13(sample as f32)))))
        .chain((0..SAMPLES).map(|sample| {
            // The harness's tent reads the FIRST NINE of the thirteen, so the
            // reference must too — `taps9` is a different table and would
            // silently compare two different things.
            let taps = taps13(sample as f32);
            let first_nine = [0_usize, 1, 2, 3, 4, 5, 6, 7, 8].map(|n| taps[n]);
            widen(upsample_tent(first_nine))
        }))
        .chain((0..SAMPLES).map(|sample| {
            let taps = taps13(sample as f32);
            widen(combine(hdr_of(sample), taps[0], tuning_of(sample).strength))
        }))
        .collect();
    (evaluated, rendered)
}

/// The worst absolute lane delta — the measurement a tolerance is set from.
fn worst_delta(cpu: &[[f32; 4]], gpu: &[[f32; 4]]) -> f32 {
    cpu.iter()
        .zip(gpu.iter())
        .flat_map(|(expected, actual)| {
            [0_usize, 1, 2, 3].map(|lane| (expected[lane] - actual[lane]).abs())
        })
        .fold(0.0_f32, f32::max)
}

/// **The tap tables, bit for bit.** No tolerance: the offsets are small integers
/// and an `f32` holds them exactly, so the WGSL's table either is the Rust one
/// or it is not.
#[test]
fn the_wgsl_tap_tables_are_the_rust_ones() {
    let gpu = BloomGpu::shared();
    assert_ne!(
        gpu.backend,
        wgpu::Backend::Noop,
        "the parity proof is worthless unless a real backend ran it"
    );
    let module = gpu.compile();
    let rendered = gpu.render(&module, "bloom_parity_taps_fs", &uniform_bytes());
    DOWN_TAPS.iter().enumerate().for_each(|(index, offset)| {
        assert_eq!(rendered[index][0].to_bits(), offset[0].to_bits(), "down tap {index} x");
        assert_eq!(rendered[index][1].to_bits(), offset[1].to_bits(), "down tap {index} y");
    });
    UP_TAPS.iter().enumerate().for_each(|(index, offset)| {
        assert_eq!(rendered[index][2].to_bits(), offset[0].to_bits(), "up tap {index} x");
        assert_eq!(rendered[index][3].to_bits(), offset[1].to_bits(), "up tap {index} y");
    });
}

/// **The arithmetic tier.** Every filter, every sample, on a real adapter.
#[test]
fn the_filters_agree_with_the_cpu_reference() {
    let gpu = BloomGpu::shared();
    assert_ne!(gpu.backend, wgpu::Backend::Noop);
    let (cpu, rendered) = compare_arithmetic(&gpu);
    let names = ["karis downsample", "plain downsample", "tent upsample", "combine"];
    cpu.iter()
        .zip(rendered.iter())
        .enumerate()
        .for_each(|(index, (expected, actual))| {
            let filter = names[index / SAMPLES];
            (0..4).for_each(|lane| {
                let delta = (expected[lane] - actual[lane]).abs();
                assert!(
                    delta <= ARITHMETIC_TOLERANCE,
                    "the {filter} disagrees at sample {} lane {lane}: CPU {} vs GPU {} \
                     (delta {delta}, tolerance {ARITHMETIC_TOLERANCE})",
                    index % SAMPLES,
                    expected[lane],
                    actual[lane]
                );
            });
        });
}

/// The arithmetic budget is derived from the hardware, not fitted to a miss.
/// Floored at [`f32::EPSILON`] because a tolerance cannot honestly be tighter
/// than the representation.
#[test]
fn the_arithmetic_tolerance_is_within_ten_times_the_measured_delta() {
    let gpu = BloomGpu::shared();
    let (cpu, rendered) = compare_arithmetic(&gpu);
    let measured = worst_delta(&cpu, &rendered);
    assert!(
        measured <= ARITHMETIC_TOLERANCE,
        "the measured worst delta {measured} exceeds the tolerance {ARITHMETIC_TOLERANCE}"
    );
    assert!(
        ARITHMETIC_TOLERANCE <= measured.max(f32::EPSILON) * 10.0,
        "the tolerance {ARITHMETIC_TOLERANCE} is more than 10x the measured worst delta \
         {measured}; derive it from the measurement"
    );
}

/// **A zero-strength bloom is bit-identical to no bloom, on the GPU too.** The
/// CPU side pins this in `filters`; here it is the shader's `max(strength, 0)`
/// and its multiply that have to produce an exact `+0.0`.
#[test]
fn a_zero_strength_bloom_is_bit_identical_on_the_gpu() {
    let gpu = BloomGpu::shared();
    let module = gpu.compile();
    // The same table with every strength zeroed, and again with it negative.
    let rewrite = |strength: f32| {
        let mut lanes: Vec<f32> = (0..SAMPLES)
            .flat_map(|sample| {
                let taps = taps13(sample as f32);
                let hdr = hdr_of(sample);
                taps.iter()
                    .flat_map(|c| [c[0], c[1], c[2], 0.0])
                    .chain([1.0, SOURCE_SETTINGS.threshold, SOURCE_SETTINGS.knee, strength])
                    .chain([hdr[0], hdr[1], hdr[2], 0.0])
                    .collect::<Vec<f32>>()
            })
            .collect();
        lanes.resize(SAMPLES * LANES * 4, 0.0);
        let bytes: Vec<u8> = lanes.into_iter().flat_map(f32::to_le_bytes).collect();
        gpu.render(&module, "bloom_parity_combine_fs", &bytes)
    };
    let zeroed = rewrite(0.0);
    let negative = rewrite(-2.5);
    let lit = rewrite(SOURCE_SETTINGS.strength);
    (0..SAMPLES).for_each(|sample| {
        let hdr = hdr_of(sample);
        (0..3).for_each(|lane| {
            assert_eq!(
                zeroed[sample][lane].to_bits(),
                hdr[lane].to_bits(),
                "sample {sample} lane {lane} moved at zero strength"
            );
            assert_eq!(
                negative[sample][lane].to_bits(),
                hdr[lane].to_bits(),
                "sample {sample} lane {lane} moved at a negative strength"
            );
        });
        // And the strength is not inert, so the identity is a disable.
        assert_ne!(lit[sample][0].to_bits(), hdr[0].to_bits());
    });
}

/// **The whole chain.** [`BloomPyramid`] over a real `Rgba16Float` source, read
/// back and compared against [`render`] texel for texel.
#[test]
fn the_rendered_pyramid_matches_the_cpu_reference() {
    let gpu = BloomGpu::shared();
    assert_ne!(gpu.backend, wgpu::Backend::Noop);
    let (cpu, rendered, size) = compare_chain(&gpu);
    cpu.iter()
        .zip(rendered.iter())
        .enumerate()
        .for_each(|(texel, (expected, actual))| {
            (0..3).for_each(|lane| {
                let delta = (expected[lane] - actual[lane]).abs();
                assert!(
                    delta <= CHAIN_TOLERANCE,
                    "the pyramid disagrees at texel ({}, {}) lane {lane}: CPU {} vs GPU {} \
                     (delta {delta}, tolerance {CHAIN_TOLERANCE})",
                    texel as u32 % size.0,
                    texel as u32 / size.0,
                    expected[lane],
                    actual[lane]
                );
            });
        });
}

/// The chain budget, measured. The bound it must stay inside is ten times the
/// measurement; the bound it must stay *above* is the sampler's own subtexel
/// quantisation, which is stated in [`CHAIN_TOLERANCE`]'s docs.
#[test]
fn the_chain_tolerance_is_within_ten_times_the_measured_delta() {
    let gpu = BloomGpu::shared();
    let (cpu, rendered, _) = compare_chain(&gpu);
    let measured = cpu
        .iter()
        .zip(rendered.iter())
        .flat_map(|(expected, actual)| [0_usize, 1, 2].map(|l| (expected[l] - actual[l]).abs()))
        .fold(0.0_f32, f32::max);
    assert!(
        measured <= CHAIN_TOLERANCE,
        "the measured worst chain delta {measured} exceeds the tolerance {CHAIN_TOLERANCE}"
    );
    assert!(
        CHAIN_TOLERANCE <= measured.max(f32::EPSILON) * 10.0,
        "the tolerance {CHAIN_TOLERANCE} is more than 10x the measured worst delta \
         {measured}; derive it from the measurement"
    );
    // And the comparison is not vacuous: the pyramid produced light.
    let brightest = rendered
        .iter()
        .flat_map(|c| [c[0], c[1], c[2]])
        .fold(0.0_f32, f32::max);
    assert!(brightest > 0.05, "the rendered pyramid is dark ({brightest})");
}

/// `(cpu, gpu, level-0 size)` for the whole chain over a 64x64 HDR scene.
fn compare_chain(gpu: &BloomGpu) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, (u32, u32)) {
    let source = scene(64, 64);
    let view = gpu.upload(&source);
    let pyramid = BloomPyramid::new(&gpu.device, &view, (64, 64), LEVELS_HIGH)
        .expect("a 64x64 source has levels");
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    pyramid.record(&gpu.queue, &mut encoder, SOURCE_SETTINGS);
    gpu.queue.submit(Some(encoder.finish()));
    let size = pyramid.output_size();
    let rendered = gpu.read_level(&pyramid, size);
    let expected = render(&source, SOURCE_SETTINGS, LEVELS_HIGH)
        .expect("the reference builds the same pyramid");
    assert_eq!((expected.width(), expected.height()), size);
    (expected.texels().to_vec(), rendered, size)
}
