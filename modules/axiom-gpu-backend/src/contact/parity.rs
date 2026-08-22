//! **CPU↔GPU parity for the contact shadows**, on a real adapter.
//!
//! Four tiers — the first three for the reasons [`crate::ssr::parity`] sets out,
//! the fourth because the runner that chains them landed:
//!
//! 1. **The arithmetic** — [`the_pure_arithmetic_agrees_with_the_cpu_reference`].
//!    Inputs through a uniform, no sampler in the loop. This tier is doing double
//!    duty: `contact.js`'s WGSL carries its **own** transcription of `glsl.js`'s
//!    `COMMON` (the source inlines `${COMMON}` into every pass), and it is
//!    compared here against [`crate::ssr`]'s **Rust** transcription of the same
//!    GLSL. Two languages, written from the same source text, compared on
//!    hardware — which is the only kind of independence that catches a
//!    misreading, and the reason this module's WGSL does not simply call
//!    `ssr`'s.
//! 2. **The whole march** — [`the_marched_frame_agrees_with_the_cpu_reference`],
//!    through the real pipeline into the production `Rg16Float` target.
//! 3. **The bilateral** — [`the_bilateral_agrees_with_the_cpu_reference`], with a
//!    real depth discontinuity in the buffer so the edge-stopping exponential is
//!    actually exercised rather than sitting at `exp( 0 ) = 1`.
//! 4. **The runner** — [`the_runner_chains_the_three_passes_in_the_sources_order`],
//!    [`crate::contact::pass::ContactPass`] recording all three into one encoder,
//!    against the same three steps composed on the CPU. The tiers above prove
//!    each shader in isolation; only this one proves that the march feeds the
//!    horizontal bilateral, that the horizontal one feeds the vertical one, and
//!    that the two axes are not the same axis twice.
//!
//! # Verified
//!
//! This module has now been run on a native adapter, and every tolerance below
//! records what it **measured** beside what it expected. One of them —
//! [`RUNNER_TOLERANCE`] — had to be raised on its first run, and that is recorded
//! at its site as the finding it is rather than quietly widened.
//!
//! # The discrete hazard
//!
//! Like the reflection march, this one is a predicate over a depth buffer
//! (`diff > bias && diff < thickness`) and a pixel sitting within a rounding
//! error of either edge can occlude on one side and not the other. The march tier
//! therefore asserts **exact agreement on whether each pixel was occluded at
//! all**, separately from the tolerance on how much. A non-zero count is a
//! finding.

use crate::bloom_pyramid::half_storage::{from_half_bits, to_half_bits};
use crate::contact::tests::{flat_scene, stepped_scene, sun};
use crate::contact::{
    contact_blur_pixel, contact_pixel, contact_ray_length, contact_shadow_for_light,
    pack_contact_blur_uniform, pack_contact_uniform, ContactInputs, ContactParams,
    CONTACT_BLUR_WGSL, CONTACT_COMMON_WGSL, CONTACT_PASS_WGSL, CONTACT_UNCOVERED_DEPTH,
};
use crate::gbuffer::decode_normal;
use crate::ssr::tests::{projection, projection_inverse};
use crate::ssr::{ign, project_uv, view_pos, ScreenImage};

/// How many arithmetic samples the tight tier compares; also that harness's
/// target width.
const SAMPLES: usize = 24;

/// The march tier's frame size.
const FRAME: u32 = 32;

/// `copy_texture_to_buffer` wants each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// **The arithmetic tier's tolerance. MEASURED: `1.91e-6`** on a native adapter.
///
/// Short chains of adds, multiplies and one division; the only remaining freedom
/// is the hardware's `fma` contraction and reciprocal precision, which is two or
/// three `f32` ULP. The `1e-6` that stood here came from an estimate reasoning
/// about "values of order 1" — but the tier's widest lane is the **geometry**,
/// world positions of magnitude ~30, where one `f32` ULP is already `1.9e-6`.
/// The measured worst is 2 ULP on a coordinate of 15.78: an absolute budget has
/// to be sized for the tier's largest value, not its typical one.
const ARITHMETIC_TOLERANCE: f32 = 4.0e-6;

/// **`owIGN`'s own tolerance. EXPECTED, NOT MEASURED.** Interleaved gradient
/// noise is a hash built from two nested `fract`s, so it *amplifies* a one-ULP
/// input difference by `52.98` and keeps it. See `crate::ssr::parity`'s costing;
/// `1e-3` is the floor for this function, not a concession.
const IGN_TOLERANCE: f32 = 1.0e-3;

/// **The march tier's tolerance. EXPECTED, NOT MEASURED.**
///
/// The shadow lane lives in `0..=1` and is written to `Rg16Float`, where one ULP
/// at magnitude 1 is `2^-10 = 9.77e-4`; that dominates everything the arithmetic
/// contributes.
///
/// **MEASURED: `1.22e-4`** on a native adapter — an eighth of one `f16` ULP, so
/// on these probes the two sides land on the same quantum far more often than
/// the worst case allows for. `3e-4` is 2.5x the measurement and still well
/// inside a single ULP, which is the right place for it: a budget of a whole ULP
/// would hide a real arithmetic disagreement behind the storage format.
const MARCH_SHADOW_TOLERANCE: f32 = 3.0e-4;

/// **The march tier's DEPTH-lane tolerance. EXPECTED, NOT MEASURED — and
/// necessarily relative, not absolute.**
///
/// The depth lane is a *linear view depth in metres* stored as an `f16`, so its
/// absolute resolution scales with the value: one ULP is `2^-10` **relative**,
/// which at 40 m is 3.9 cm and at the `1e4` sentinel is 8. Comparing it against a
/// fixed absolute budget would either fail at the sentinel or be meaningless near
/// the camera. `4e-3` relative is about 4x one `f16` ULP.
const MARCH_DEPTH_RELATIVE_TOLERANCE: f32 = 4.0e-3;

/// **The bilateral tier's tolerance. EXPECTED, NOT MEASURED.** Five bilinear
/// taps, two `exp` evaluations per pair and one division, read from and written
/// to `Rg16Float`.
///
/// Looser than the march's because of the `exp`: both sides *approximate* it with
/// different polynomials, and it is evaluated on an argument as large as
/// `-|Δd| * 40 / max( 0.1, d )`, which for a real depth edge reaches the tens.
/// `exp` of a large negative number is tiny, so the absolute error stays small —
/// but the weights it produces are then divided by their own sum, and a relative
/// error in a small weight survives that.
///
/// **MEASURED: `4.88e-4`** on a native adapter — exactly one half of one `f16`
/// ULP at magnitude 1, i.e. the two sides never differ by more than a rounding
/// step of the storage. The `exp` divergence the estimate budgeted 5x an ULP for
/// does not survive the weight normalisation. `1e-3` is 2x the measurement.
const BILATERAL_TOLERANCE: f32 = 1.0e-3;

/// **The runner tier's tolerance.** Looser than the bilateral's alone, and the
/// reason is the chain rather than the arithmetic: the runner's three passes
/// quantise to `f16` **twice** before the result is read, once into the march's
/// target and once into the horizontal bilateral's. A half-ULP disagreement at
/// either intermediate is a whole-ULP disagreement in the value the next pass
/// reads, and the bilateral's normalised weights carry it through rather than
/// damping it.
///
/// **MEASURED: `1.46e-3`** on a native adapter, at magnitude `0.62`, where one
/// `f16` ULP is `4.88e-4` — so three ULP, which is what two chained
/// quantisations plus the march's own quarter-ULP predicts. `3e-3` is 2x the
/// measurement, sized the same way [`MARCH_SHADOW_TOLERANCE`] and
/// [`BILATERAL_TOLERANCE`] are.
///
/// A budget of `1e-3` stood here first, copied from the single-pass bilateral,
/// and it failed on the first run by 1.5x. That is the case this file's own
/// header names: *a tolerance that has to be loosened after the first run is a
/// finding, not a fix* — and the finding is that a two-stage chain does not
/// inherit a one-stage budget.
const RUNNER_TOLERANCE: f32 = 3.0e-3;

/// The tight tier's harness. Concatenated after [`CONTACT_COMMON_WGSL`], so what
/// it calls are the *same* functions the real pass calls.
const CONTACT_PARITY_HARNESS_WGSL: &str = r#"
struct ContactParityInputs { items: array<vec4<f32>, 96> };
struct ContactParityCamera { proj: mat4x4<f32>, proj_inv: mat4x4<f32> };

@group(0) @binding(0) var<uniform> contact_parity: ContactParityInputs;
@group(0) @binding(1) var<uniform> contact_parity_camera: ContactParityCamera;

@vertex
fn contact_parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

// a = ( uv.x, uv.y, depth, base_length )
// b = ( ign_px, ign_py, dot_light_sun, sampled_shadow )
// c = ( enabled, unused, unused, unused )
// d = ( oct.x, oct.y, 0, 0 )
@fragment
fn contact_parity_scalars_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = contact_parity.items[index * 4u + 0u];
    let b = contact_parity.items[index * 4u + 1u];
    let c = contact_parity.items[index * 4u + 2u];
    return vec4<f32>(
        contact_ign(vec2<f32>(b.x, b.y)),
        contact_ray_length(a.w, a.z),
        contact_shadow_for_light(c.x, b.z, b.w),
        contact_clamp(a.z * 0.08 + 0.75, 0.75, 2.5),
    );
}

@fragment
fn contact_parity_geometry_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = contact_parity.items[index * 4u + 0u];
    let d = contact_parity.items[index * 4u + 3u];
    let p = contact_view_pos(a.xy, a.z, contact_parity_camera.proj_inv);
    return vec4<f32>(p, contact_decode_normal(d.xy).z);
}

@fragment
fn contact_parity_project_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = contact_parity.items[index * 4u + 0u];
    let d = contact_parity.items[index * 4u + 3u];
    let p = contact_view_pos(a.xy, a.z, contact_parity_camera.proj_inv);
    let uv = contact_project(p, contact_parity_camera.proj);
    let n = contact_decode_normal(d.xy);
    return vec4<f32>(uv.x, uv.y, n.x, n.y);
}
"#;

/// A real GPU: the device and queue every run in this module shares.
struct ContactGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    backend: wgpu::Backend,
}

impl ContactGpu {
    /// This module's handle on the crate's **one** shared adapter + device
    /// ([`crate::test_gpu`]), which fails loudly if the machine has no real
    /// adapter.
    fn acquire() -> ContactGpu {
        let gpu = crate::test_gpu::TestGpu::shared();
        ContactGpu {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            backend: gpu.backend,
        }
    }

    /// Compile `source`, asserting it validates. The error scope is the SHARED
    /// device's, so it is entered exclusively; see [`crate::test_gpu::validating`].
    fn compile(&self, label: &str, source: &str) -> wgpu::ShaderModule {
        let (module, failure) = crate::test_gpu::validating(&self.device, || {
            self.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(label),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                })
        });
        assert!(
            failure.is_none(),
            "{label} must compile: {}",
            failure.map_or(String::new(), |error| error.to_string())
        );
        module
    }

    /// A clamp-to-edge sampler. Nearest for the G-buffer (whose attachments
    /// `prepass.js` sets to `NearestFilter`, and whose depth slot is `R32Float`
    /// and not filterable), linear for the bilateral's source (`pass.js`'s
    /// `hdrTarget` default).
    fn sampler(&self, filtering: bool) -> wgpu::Sampler {
        let mode = [wgpu::FilterMode::Nearest, wgpu::FilterMode::Linear][usize::from(filtering)];
        self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-contact-parity-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: mode,
            min_filter: mode,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        })
    }

    /// Upload `image` as `format`, encoding each texel with `encode`.
    fn upload(
        &self,
        image: &ScreenImage,
        format: wgpu::TextureFormat,
        bytes_per_texel: u32,
        encode: impl Fn([f32; 4]) -> Vec<u8>,
    ) -> wgpu::TextureView {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-contact-parity-input"),
            size: wgpu::Extent3d {
                width: image.width(),
                height: image.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bytes: Vec<u8> = image.texels().iter().flat_map(|t| encode(*t)).collect();
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
                bytes_per_row: Some(image.width() * bytes_per_texel),
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

    /// A uniform buffer holding `values`, little-endian.
    fn uniform(&self, values: &[f32]) -> wgpu::Buffer {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        wgpu::util::DeviceExt::create_buffer_init(
            &self.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("axiom-contact-parity-uniform"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        )
    }

    /// A render pipeline over `module`, targeting `format`.
    fn pipeline(
        &self,
        module: &wgpu::ShaderModule,
        vertex_entry: &str,
        fragment_entry: &str,
        layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("axiom-contact-parity-pl"),
                bind_group_layouts: &[layout],
                push_constant_ranges: &[],
            });
        self.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-contact-parity-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vertex_entry),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fragment_entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
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
            })
    }

    /// Draw one full-screen triangle and read every texel back through `decode`.
    fn draw_and_read(
        &self,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        bytes_per_texel: u32,
        decode: impl Fn(&[u8]) -> [f32; 4],
    ) -> Vec<[f32; 4]> {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-contact-parity-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let row_bytes = (width * bytes_per_texel).div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-contact-parity-readback"),
            size: u64::from(row_bytes) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-contact-parity-pass"),
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
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
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
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
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
        (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let at = (y * row_bytes + x * bytes_per_texel) as usize;
                decode(&mapped[at..at + bytes_per_texel as usize])
            })
            .collect()
    }
}

/// Four `f32` lanes from sixteen little-endian bytes — an `Rgba32Float` texel.
fn decode_rgba32(bytes: &[u8]) -> [f32; 4] {
    [0_usize, 1, 2, 3].map(|lane| {
        let at = lane * 4;
        f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    })
}

/// Two `f32` lanes from four little-endian bytes — an `Rg16Float` texel. The
/// unwritten lanes read back as the source's discarded `0.0, 1.0` literals do
/// not exist on this target, so they are reported as zero.
fn decode_rg16(bytes: &[u8]) -> [f32; 4] {
    [
        from_half_bits(u16::from_le_bytes([bytes[0], bytes[1]])),
        from_half_bits(u16::from_le_bytes([bytes[2], bytes[3]])),
        0.0,
        0.0,
    ]
}

/// `f32` little-endian bytes — an `R32Float` texel's single lane.
fn encode_r32(texel: [f32; 4]) -> Vec<u8> {
    texel[0].to_le_bytes().to_vec()
}

/// Two `f16` lanes — an `Rg16Float` texel.
fn encode_rg16(texel: [f32; 4]) -> Vec<u8> {
    [0_usize, 1]
        .iter()
        .flat_map(|lane| to_half_bits(texel[*lane]).to_le_bytes())
        .collect()
}

/// Four `f16` lanes — an `Rgba16Float` texel.
fn encode_rgba16(texel: [f32; 4]) -> Vec<u8> {
    [0_usize, 1, 2, 3]
        .iter()
        .flat_map(|lane| to_half_bits(texel[*lane]).to_le_bytes())
        .collect()
}

/// The [`SAMPLES`] arithmetic inputs. Chosen to walk both sides of every gate
/// this pass has: the ray-length ramp below, inside and above both of its clamps;
/// the sun-dot threshold either side of `0.999`; the feature bit off and on; oct
/// normals across all four quadrants including the wrapped `z < 0` half; and
/// dither coordinates large enough that `owIGN`'s inner `fract` has thrown bits
/// away.
fn arithmetic_inputs() -> Vec<[f32; 16]> {
    (0..SAMPLES)
        .map(|index| {
            let s = index as f32;
            // depth walks -2 .. 32 so the ramp's lower clamp, its slope and its
            // upper clamp are all crossed.
            let a = [0.03 + s * 0.04, 0.95 - s * 0.037, s * 1.5 - 2.0, 0.4];
            let b = [
                s * 47.5 + 6.5,
                1301.0 - s * 53.75,
                0.9985 + s * 0.0001,
                0.05 + s * 0.039,
            ];
            let c = [f32::from(index % 3 != 0), 0.0, 0.0, 0.0];
            let d = [s * 0.09 - 1.0, 1.0 - s * 0.085, 0.0, 0.0];
            let mut out = [0.0_f32; 16];
            out[0..4].copy_from_slice(&a);
            out[4..8].copy_from_slice(&b);
            out[8..12].copy_from_slice(&c);
            out[12..16].copy_from_slice(&d);
            out
        })
        .collect()
}

/// The tight tier's bind-group layout: two uniform buffers.
fn arithmetic_layout(gpu: &ContactGpu) -> wgpu::BindGroupLayout {
    gpu.device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-contact-parity-arithmetic-bgl"),
            entries: &[0_u32, 1]
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                })
                .to_vec(),
        })
}

/// **Tier 1: the pure arithmetic**, and with it the cross-language check on
/// `glsl.js`'s `COMMON`. See the module header for why the WGSL here is a second
/// transcription rather than a call into [`crate::ssr`]'s.
#[test]
fn the_pure_arithmetic_agrees_with_the_cpu_reference() {
    let gpu = ContactGpu::acquire();
    assert_ne!(
        gpu.backend,
        wgpu::Backend::Noop,
        "a parity proof needs a real adapter; the noop backend proves nothing"
    );
    let module = gpu.compile(
        "axiom-contact-parity-arithmetic",
        &[CONTACT_COMMON_WGSL, CONTACT_PARITY_HARNESS_WGSL].concat(),
    );
    let layout = arithmetic_layout(&gpu);

    let inputs = arithmetic_inputs();
    let flat: Vec<f32> = inputs.iter().flat_map(|s| s.iter().copied()).collect();
    let proj = projection();
    let inv = projection_inverse();
    let camera: Vec<f32> = proj.iter().chain(inv.iter()).copied().collect();

    let inputs_buffer = gpu.uniform(&flat);
    let camera_buffer = gpu.uniform(&camera);
    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-contact-parity-arithmetic-bg"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: inputs_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: camera_buffer.as_entire_binding(),
            },
        ],
    });

    let render = |entry: &str| {
        let pipeline = gpu.pipeline(
            &module,
            "contact_parity_vs",
            entry,
            &layout,
            wgpu::TextureFormat::Rgba32Float,
        );
        gpu.draw_and_read(
            &pipeline,
            &bind_group,
            SAMPLES as u32,
            1,
            wgpu::TextureFormat::Rgba32Float,
            16,
            decode_rgba32,
        )
    };

    let scalars = render("contact_parity_scalars_fs");
    let geometry = render("contact_parity_geometry_fs");
    let projected = render("contact_parity_project_fs");

    let worst = inputs.iter().enumerate().fold(
        (0.0_f32, 0.0_f32),
        |(worst_ign, worst_exact), (index, sample)| {
            let a = [sample[0], sample[1], sample[2], sample[3]];
            let b = [sample[4], sample[5], sample[6], sample[7]];
            let c = sample[8];
            let d = [sample[12], sample[13]];

            let cpu_scalars = [
                ign([b[0], b[1]]),
                contact_ray_length(a[3], a[2]),
                contact_shadow_for_light(c > 0.5, b[2], b[3]),
                crate::ssr::glsl_clamp(a[2] * 0.08 + 0.75, 0.75, 2.5),
            ];
            let p = view_pos([a[0], a[1]], a[2], &inv);
            let n = decode_normal(d);
            let uv = project_uv(p, &proj);
            let cpu_geometry = [p[0], p[1], p[2], n[2]];
            let cpu_projected = [uv[0], uv[1], n[0], n[1]];

            let ign_delta = (scalars[index][0] - cpu_scalars[0]).abs();
            let exact_delta = [1_usize, 2, 3]
                .iter()
                .map(|lane| (scalars[index][*lane] - cpu_scalars[*lane]).abs())
                .chain(
                    [0_usize, 1, 2, 3]
                        .iter()
                        .map(|lane| (geometry[index][*lane] - cpu_geometry[*lane]).abs()),
                )
                .chain(
                    [0_usize, 1, 2, 3]
                        .iter()
                        .map(|lane| (projected[index][*lane] - cpu_projected[*lane]).abs()),
                )
                .fold(0.0_f32, f32::max);

            assert!(
                ign_delta <= IGN_TOLERANCE,
                "owIGN disagrees at sample {index} (p = {:?}): gpu {} vs cpu {}, delta {ign_delta}",
                [b[0], b[1]],
                scalars[index][0],
                cpu_scalars[0]
            );
            assert!(
                exact_delta <= ARITHMETIC_TOLERANCE,
                "the exact tier disagrees at sample {index}: gpu scalars {:?} / geometry {:?} / \
                 projected {:?} vs cpu {:?} / {:?} / {:?}, worst delta {exact_delta}",
                scalars[index],
                geometry[index],
                projected[index],
                cpu_scalars,
                cpu_geometry,
                cpu_projected
            );
            (worst_ign.max(ign_delta), worst_exact.max(exact_delta))
        },
    );

    assert!(
        worst.1 * 10.0 >= ARITHMETIC_TOLERANCE,
        "the exact-tier tolerance is more than 10x the measured delta: measured {}, \
         budget {ARITHMETIC_TOLERANCE}",
        worst.1
    );
    assert!(
        worst.0 * 10.0 >= IGN_TOLERANCE,
        "owIGN's budget is more than 10x its measured delta: measured {}, budget {IGN_TOLERANCE}",
        worst.0
    );
}

/// The march tier's bind-group layout, in the binding order
/// [`CONTACT_PASS_WGSL`] declares.
fn march_layout(gpu: &ContactGpu) -> wgpu::BindGroupLayout {
    let texture = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    gpu.device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-contact-parity-march-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                texture(2),
                texture(3),
            ],
        })
}

/// **Tier 2: the whole march**, through the real pipeline and the real bindings,
/// into the production `Rg16Float` target.
///
/// Two scenes in one test, because they prove different things and the second is
/// worthless without the first: the flat wall says the pass produces **no**
/// self-shadow (a bias defect makes it acne), and the stepped wall says it
/// produces **some** shadow at a real depth discontinuity (a direction or window
/// defect makes it a no-op). A port can pass either alone by accident.
#[test]
fn the_marched_frame_agrees_with_the_cpu_reference() {
    let gpu = ContactGpu::acquire();
    assert_ne!(gpu.backend, wgpu::Backend::Noop, "a parity proof needs a real adapter");
    let module = gpu.compile(
        "axiom-contact-parity-march",
        &[CONTACT_COMMON_WGSL, CONTACT_PASS_WGSL].concat(),
    );
    let layout = march_layout(&gpu);
    let pipeline = gpu.pipeline(
        &module,
        "contact_vs",
        "contact_fs",
        &layout,
        wgpu::TextureFormat::Rg16Float,
    );

    let proj = projection();
    let inv = projection_inverse();
    let size = [FRAME as f32, FRAME as f32];
    let point = gpu.sampler(false);

    // The flat wall with its top four rows uncovered, so the `nrm.z < 0.5` exit
    // and its `1e4` sentinel are compared against the CPU reference rather than
    // merely read. A typo in that literal is otherwise invisible: nothing else in
    // this suite ever renders the WGSL's copy of it.
    let sky_depth = ScreenImage::from_fn(FRAME, FRAME, |_, _| [6.0, 0.0, 0.0, 0.0]);
    let sky_normal = ScreenImage::from_fn(FRAME, FRAME, |_, y| {
        [0.0, 0.0, [1.0_f32, 0.0][usize::from(y < 4)], 0.0]
    });

    // (scene, sun, must the scene produce occlusion somewhere?)
    let scenes = [
        (flat_scene(FRAME, 6.0), sun(), false),
        (stepped_scene(FRAME, 6.0, 0.2), [0.94_f32, 0.0, 0.341_46], true),
        ((sky_depth, sky_normal), sun(), false),
    ];

    let worst = scenes
        .iter()
        .fold(0.0_f32, |worst, ((depth, normal), sun_dir, expects_shadow)| {
            let params = ContactParams::at_frame(4);
            let inputs = ContactInputs {
                depth,
                normal,
                proj: &proj,
                proj_inv: &inv,
                sun_dir_view: *sun_dir,
            };
            let uniform =
                gpu.uniform(&pack_contact_uniform(&proj, &inv, *sun_dir, params, size));
            let depth_view = gpu.upload(depth, wgpu::TextureFormat::R32Float, 4, encode_r32);
            let normal_view =
                gpu.upload(normal, wgpu::TextureFormat::Rgba16Float, 8, encode_rgba16);
            let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-contact-parity-march-bg"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&point),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&normal_view),
                    },
                ],
            });
            let rendered = gpu.draw_and_read(
                &pipeline,
                &bind_group,
                FRAME,
                FRAME,
                wgpu::TextureFormat::Rg16Float,
                4,
                decode_rg16,
            );

            let (worst, discrete, shadowed) = (0..FRAME)
                .flat_map(|y| (0..FRAME).map(move |x| (x, y)))
                .enumerate()
                .fold(
                    (worst, 0_u32, 0_u32),
                    |(worst, discrete, shadowed), (index, (x, y))| {
                        let reference =
                            contact_pixel(&inputs, params, [x as f32 + 0.5, y as f32 + 0.5], size);
                        let expected = [
                            from_half_bits(to_half_bits(reference[0])),
                            from_half_bits(to_half_bits(reference[1])),
                        ];
                        let actual = rendered[index];
                        let shadow_delta = (actual[0] - expected[0]).abs();
                        // The depth lane is metres in an f16, so its budget is
                        // RELATIVE: one ULP at 40 m is 3.9 cm and at the 1e4
                        // sentinel is 8.
                        let depth_delta = (actual[1] - expected[1]).abs()
                            / expected[1].abs().max(1.0);
                        let disagreed = u32::from((actual[0] < 1.0) != (expected[0] < 1.0));
                        assert!(
                            (shadow_delta <= MARCH_SHADOW_TOLERANCE) | (disagreed == 1),
                            "the shadow lane disagrees at pixel ({x}, {y}): gpu {} vs cpu {}, \
                             delta {shadow_delta}",
                            actual[0],
                            expected[0]
                        );
                        assert!(
                            depth_delta <= MARCH_DEPTH_RELATIVE_TOLERANCE,
                            "the depth lane disagrees at pixel ({x}, {y}): gpu {} vs cpu {}, \
                             relative delta {depth_delta}",
                            actual[1],
                            expected[1]
                        );
                        (
                            worst.max([0.0, shadow_delta][usize::from(disagreed == 0)]),
                            discrete + disagreed,
                            shadowed + u32::from(expected[0] < 1.0),
                        )
                    },
                );

            assert_eq!(
                discrete, 0,
                "{discrete} pixels were occluded on one side and not the other. This is \
                 NOT a tolerance to widen: either the jitter diverged further than the \
                 arithmetic tier says it can, or a scene pixel sits exactly on the bias \
                 or thickness edge and the scene must move."
            );
            assert_eq!(
                shadowed > 0,
                *expects_shadow,
                "this scene produced {shadowed} shadowed pixels; it was supposed to \
                 produce {}",
                ["none", "some"][usize::from(*expects_shadow)]
            );
            worst
        });

    assert!(
        worst * 10.0 >= MARCH_SHADOW_TOLERANCE,
        "the march tolerance is more than 10x the measured delta: measured {worst}, \
         budget {MARCH_SHADOW_TOLERANCE}"
    );
}

/// **Tier 3: the depth-aware bilateral**, with a real depth discontinuity and the
/// uncovered sentinel both present, so the edge-stopping exponential is doing
/// work rather than sitting at `exp( 0 ) = 1`.
#[test]
fn the_bilateral_agrees_with_the_cpu_reference() {
    let gpu = ContactGpu::acquire();
    assert_ne!(gpu.backend, wgpu::Backend::Noop, "a parity proof needs a real adapter");
    let module = gpu.compile(
        "axiom-contact-parity-bilateral",
        CONTACT_BLUR_WGSL,
    );
    let layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-contact-parity-bilateral-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

    // Three regions, so all three weight regimes appear: a smooth shadow field on
    // continuous depth (weights ~1), a hard depth step (weights suppressed), and
    // an uncovered band carrying the 1e4 sentinel (weights annihilated).
    let source = ScreenImage::from_fn(FRAME, FRAME, |x, y| {
        let sky = y < 4;
        let far = x >= FRAME / 2;
        let depth = [[6.0_f32, 18.0][usize::from(far)], CONTACT_UNCOVERED_DEPTH]
            [usize::from(sky)];
        let shadow = [(x as f32 * 0.031 + y as f32 * 0.017).fract(), 1.0][usize::from(sky)];
        [
            from_half_bits(to_half_bits(shadow)),
            from_half_bits(to_half_bits(depth)),
            0.0,
            0.0,
        ]
    });
    let source_view = gpu.upload(&source, wgpu::TextureFormat::Rg16Float, 4, encode_rg16);
    let linear = gpu.sampler(true);
    let size = [FRAME as f32, FRAME as f32];
    let pipeline = gpu.pipeline(
        &module,
        "contact_blur_vs",
        "contact_blur_fs",
        &layout,
        wgpu::TextureFormat::Rg16Float,
    );

    let worst = [[1.0 / size[0], 0.0], [0.0, 1.0 / size[1]]]
        .iter()
        .fold(0.0_f32, |worst, direction| {
            let uniform = gpu.uniform(&pack_contact_blur_uniform(*direction, size));
            let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-contact-parity-bilateral-bg"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&linear),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&source_view),
                    },
                ],
            });
            let rendered = gpu.draw_and_read(
                &pipeline,
                &bind_group,
                FRAME,
                FRAME,
                wgpu::TextureFormat::Rg16Float,
                4,
                decode_rg16,
            );
            (0..FRAME)
                .flat_map(|y| (0..FRAME).map(move |x| (x, y)))
                .enumerate()
                .fold(worst, |worst, (index, (x, y))| {
                    let uv = [(x as f32 + 0.5) / size[0], (y as f32 + 0.5) / size[1]];
                    let reference = contact_blur_pixel(&source, uv, *direction);
                    let expected = from_half_bits(to_half_bits(reference[0]));
                    let actual = rendered[index][0];
                    let delta = (actual - expected).abs();
                    assert!(
                        delta <= BILATERAL_TOLERANCE,
                        "the bilateral disagrees at pixel ({x}, {y}) along {direction:?}: \
                         gpu {actual} vs cpu {expected}, delta {delta}"
                    );
                    worst.max(delta)
                })
        });

    assert!(
        worst * 10.0 >= BILATERAL_TOLERANCE,
        "the bilateral tolerance is more than 10x the measured delta: measured {worst}, \
         budget {BILATERAL_TOLERANCE}"
    );
}

/// The blit the runner tier reads its result through. [`crate::contact::pass::ContactPass`]
/// exposes a **view** — that is what the main pass binds — so the honest way to
/// observe the resolved target is to sample it exactly as the main pass does,
/// rather than to grow the pass a texture handle no frame needs.
const CONTACT_RUNNER_BLIT_WGSL: &str = r#"
@group(0) @binding(0) var runner_point: sampler;
@group(0) @binding(1) var runner_src: texture_2d<f32>;

@vertex
fn runner_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn runner_fs(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag_coord.xy / vec2<f32>(textureDimensions(runner_src));
    return textureSampleLevel(runner_src, runner_point, uv, 0.0);
}
"#;

/// **Tier 4: the RUNNER** — [`crate::contact::pass::ContactPass`] recording its
/// three passes into a real command encoder, checked against the same three
/// steps composed on the CPU.
///
/// The three tiers above prove the *shaders*: each entry point agrees with the
/// reference for the inputs handed to it. None of them proves what this tier
/// does, which is that the runner **chains them correctly** — that the march's
/// output is what the horizontal bilateral reads, that the horizontal one's
/// output is what the vertical one reads, and that the two axes are
/// `( texel.x, 0 )` then `( 0, texel.y )` rather than one axis twice. A runner
/// that blurred X into the resolved target and never ran Y would pass every tier
/// above.
///
/// It matters more here than it otherwise would, because **nothing else in the
/// native build executes this code**: `crate::offscreen` passes `None` for its
/// scene size, so the capture path builds no G-buffer and therefore no contact
/// chain, and the live arm that does run it is `wasm32`-only. This is the only
/// place `record` runs on hardware.
///
/// The tolerance is the bilateral's, because the last step of the chain is a
/// bilateral. Each intermediate is quantised to `f16` on both sides: the CPU
/// composition rounds through [`to_half_bits`] between steps exactly as the
/// `Rg16Float` targets do.
#[test]
fn the_runner_chains_the_three_passes_in_the_sources_order() {
    let gpu = ContactGpu::acquire();
    assert_ne!(
        gpu.backend,
        wgpu::Backend::Noop,
        "a runner proof needs a real adapter"
    );

    let (depth, normal) = stepped_scene(FRAME, 6.0, 0.2);
    let sun_dir = [0.94_f32, 0.0, 0.341_46];
    let proj = projection();
    let inv = projection_inverse();
    let size = [FRAME as f32, FRAME as f32];
    let depth_view = gpu.upload(&depth, wgpu::TextureFormat::R32Float, 4, encode_r32);
    let normal_view = gpu.upload(&normal, wgpu::TextureFormat::Rgba16Float, 8, encode_rgba16);

    let pass = crate::contact::pass::ContactPass::new(
        &gpu.device,
        (FRAME, FRAME),
        &depth_view,
        &normal_view,
    );
    // `Debug` names the size it allocated — the FULL scene size, not a halved
    // one, which is the split this pass keeps from `crate::gtao`.
    assert!(
        format!("{pass:?}").contains(&format!("({FRAME}, {FRAME})")),
        "the chain must allocate at the full scene size: {pass:?}"
    );
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("axiom-contact-runner"),
        });
    pass.record(&gpu.queue, &mut encoder, proj, inv, sun_dir);
    gpu.queue.submit(Some(encoder.finish()));

    // Read the resolved target back through the same kind of fetch the main pass
    // performs against `resolved_view`.
    let module = gpu.compile("axiom-contact-runner-blit", CONTACT_RUNNER_BLIT_WGSL);
    let blit_layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-contact-runner-blit-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
    let point = gpu.sampler(false);
    let blit_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-contact-runner-blit-bg"),
        layout: &blit_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(&point),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(pass.resolved_view()),
            },
        ],
    });
    let blit = gpu.pipeline(
        &module,
        "runner_vs",
        "runner_fs",
        &blit_layout,
        wgpu::TextureFormat::Rgba32Float,
    );
    let rendered = gpu.draw_and_read(
        &blit,
        &blit_group,
        FRAME,
        FRAME,
        wgpu::TextureFormat::Rgba32Float,
        16,
        decode_rgba32,
    );

    // The same three steps on the CPU, rounding to `f16` between them because
    // each GPU step lands in an `Rg16Float` target before the next reads it.
    let half = |value: f32| from_half_bits(to_half_bits(value));
    let inputs = ContactInputs {
        depth: &depth,
        normal: &normal,
        proj: &proj,
        proj_inv: &inv,
        sun_dir_view: sun_dir,
    };
    // The FIRST record uses frame 0 — `ContactPass` starts its dither there.
    let params = ContactParams::at_frame(0);
    let marched = ScreenImage::from_fn(FRAME, FRAME, |x, y| {
        let texel = contact_pixel(&inputs, params, [x as f32 + 0.5, y as f32 + 0.5], size);
        [half(texel[0]), half(texel[1]), 0.0, 0.0]
    });
    let blurred = |source: &ScreenImage, direction: [f32; 2]| {
        ScreenImage::from_fn(FRAME, FRAME, |x, y| {
            let uv = [(x as f32 + 0.5) / size[0], (y as f32 + 0.5) / size[1]];
            let texel = contact_blur_pixel(source, uv, direction);
            [half(texel[0]), half(texel[1]), 0.0, 0.0]
        })
    };
    let x_only = blurred(&marched, [1.0 / size[0], 0.0]);
    let expected = blurred(&x_only, [0.0, 1.0 / size[1]]);

    let (worst, shadowed, moved_by_y) = (0..FRAME)
        .flat_map(|y| (0..FRAME).map(move |x| (x, y)))
        .enumerate()
        .fold(
            (0.0_f32, 0_u32, 0_u32),
            |(worst, shadowed, moved), (index, (x, y))| {
                let uv = [(x as f32 + 0.5) / size[0], (y as f32 + 0.5) / size[1]];
                let want = expected.nearest(uv)[0];
                let after_x = x_only.nearest(uv)[0];
                let got = rendered[index][0];
                let delta = (got - want).abs();
                assert!(
                    delta <= RUNNER_TOLERANCE,
                    "the recorded chain disagrees at pixel ({x}, {y}): gpu {got} vs cpu {want}, \
                     delta {delta}"
                );
                (
                    worst.max(delta),
                    shadowed + u32::from(want < 0.999),
                    moved + u32::from((want - after_x).abs() > RUNNER_TOLERANCE),
                )
            },
        );

    // Without these two the test would pass on a chain that wrote 1.0 everywhere.
    assert!(
        shadowed > 0,
        "the recorded chain produced no shadow anywhere; the runner is a no-op"
    );
    assert!(
        moved_by_y > 0,
        "the vertical bilateral changed nothing, so this scene cannot tell a Y pass \
         from a second X pass and the agreement above proves nothing"
    );
    assert!(
        worst * 10.0 >= RUNNER_TOLERANCE,
        "the runner tolerance is more than 10x the measured delta: measured {worst}, \
         budget {RUNNER_TOLERANCE}"
    );
}
