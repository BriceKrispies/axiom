//! **CPU↔GPU parity for screen-space reflections**, on a real adapter.
//!
//! [`crate::ssr`]'s Rust is the semantic definition; the WGSL is a mirror, and a
//! mirror nobody holds up to the original is a second definition waiting to
//! drift. This module holds it up in three tiers, because the three have
//! genuinely different error budgets and one number would let the loosest hide
//! the other two:
//!
//! 1. **The arithmetic** — [`the_pure_arithmetic_agrees_with_the_cpu_reference`].
//!    Inputs handed in through a uniform, no sampler in the loop, so what is
//!    measured is the transcription. Split again, because `owIGN` is *chaotic by
//!    construction* and cannot share a budget with a smoothstep — see
//!    [`IGN_TOLERANCE`].
//! 2. **The whole march** — [`the_marched_frame_agrees_with_the_cpu_reference`].
//!    The real pipeline, the real bindings, four uploaded textures, rendered into
//!    the production `Rgba16Float` target and read back.
//! 3. **The blur** — [`the_blur_agrees_with_the_cpu_reference`]. The real
//!    separable pass over an uploaded half-resolution image.
//!
//! # !! UNVERIFIED !!
//!
//! **This module has never been run.** The final wave of this port writes
//! everything and builds nothing; the orchestrator compiles and runs it in the
//! integration pass. Every tolerance below is *derived from the arithmetic* and
//! stated as expected, not measured, and each says so at its site. The
//! measurement assertions are written the way the rest of the crate writes them
//! (`…_is_within_ten_times_the_measured_delta`) so that the first real run either
//! confirms the reasoning or produces the number that replaces it. **A tolerance
//! that has to be loosened after the first run is a finding, not a fix** — work
//! out which side is wrong before touching it.
//!
//! # The one discrete hazard, and why it is asserted separately
//!
//! A march is not a smooth function of its inputs. The thickness test
//! (`diff < thickness + t * 0.06`) is a *predicate*, and a pixel whose `diff`
//! lands within a rounding error of that boundary can hit on one side and miss on
//! the other, producing an O(1) disagreement that no continuous tolerance should
//! ever be widened to absorb.
//! [`the_marched_frame_agrees_with_the_cpu_reference`] therefore asserts
//! **exact agreement on whether each pixel hit at all**, separately from the
//! tolerance on what it returned. If that count is ever non-zero the answer is to
//! find out why — a `pow` that diverged further than expected, or a scene pixel
//! sitting exactly on the boundary — never to loosen a budget.

use crate::bloom_pyramid::half_storage::{from_half_bits, to_half_bits};
use crate::gbuffer::decode_normal;
use crate::ssr::tests::{floor_scene, projection, projection_inverse};
use crate::ssr::{
    ign, ssr_blur_pixel, ssr_confidence, ssr_pixel, ssr_resolve, ssr_resolve_weight, view_pos,
    project_uv,
    pack_ssr_blur_uniform, pack_ssr_uniform, ScreenImage, SsrInputs, SsrParams, SSR_COMMON_WGSL,
    SSR_BLUR_WGSL, SSR_PASS_WGSL, SSR_START_T, SSR_STEPS,
};

/// How many arithmetic samples the tight tier compares; also that harness's
/// target width.
const SAMPLES: usize = 24;

/// `vec4`s of uniform per arithmetic sample. Must match the harness WGSL's
/// unpack.
const LANES: usize = 4;

/// The march tier's frame size. `32 * 8` bytes per row is exactly [`ROW_ALIGN`],
/// so the readback needs no row padding to reason about.
const FRAME: u32 = 32;

/// `copy_texture_to_buffer` wants each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// **The arithmetic tier's tolerance. EXPECTED, NOT MEASURED.**
///
/// Everything in the tier except `owIGN` is a short chain of adds, multiplies and
/// one division, on values of order 1. The emitter writes out `dot`, `clamp`,
/// `smoothstep` and the `normalize` divide by hand precisely so the only
/// remaining freedom is the hardware's: contracting `a * b + c` into a
/// single-rounding `fma`, and evaluating a reciprocal at a different precision.
/// Two or three `f32` ULP at magnitude 1 is `~3.6e-7`, which is what the `1e-6`
/// that stood here was sized for. **MEASURED: `1.91e-6`** on a native adapter —
/// and the reason is that "at magnitude 1" describes the *scalars* lane, not the
/// tier. The **geometry** lane carries world positions of magnitude ~30, where
/// one `f32` ULP is already `1.9e-6`, and the measured worst is a single ULP on a
/// coordinate of 27.49. An absolute budget has to be sized for the tier's widest
/// value. `4e-6` is 2.1x the measurement; the same correction was made to
/// [`crate::contact::parity`]'s twin, which had inherited the same reasoning.
///
/// The one exception is `pow( maxDist / 0.06, 1/28 )`, which both sides
/// *approximate* with different polynomials. `pow` on an argument of 400 with a
/// small exponent is well-conditioned — the result is `~1.2386` and a `1e-7`
/// relative error in the logarithm becomes `~2.6e-8` in the result — so it fits
/// inside this budget rather than needing the transcendental treatment `sky/`
/// and `material_shader/` needed.
const ARITHMETIC_TOLERANCE: f32 = 4.0e-6;

/// **`owIGN`'s own tolerance. EXPECTED, NOT MEASURED — and deliberately three
/// orders looser than [`ARITHMETIC_TOLERANCE`], which is a property of the
/// source's algorithm, not a concession.**
///
/// Interleaved gradient noise is `fract( 52.9829189 * fract( dot( p, k ) ) )`. It
/// is a *hash*: taking the fractional part deliberately discards the high bits,
/// so the output depends on the low bits of the input and nothing else. Costed
/// out at this pass's actual arguments:
///
/// - `p` is a pixel coordinate plus `frame * 7.331`, so `|p|` reaches `~1500` and
///   `dot( p, k )` reaches `~110`.
/// - An `f32` at 110 has a ULP of `~7.6e-6`; one contraction of that two-term dot
///   into an `fma` changes it by about that much.
/// - `fract` keeps the absolute error and drops the magnitude, then `× 52.98`
///   multiplies it to `~4e-4`, and the outer `fract` keeps it.
///
/// So `1e-3` is the *floor* for this function on any two implementations that do
/// not agree bit-for-bit on the inner dot, and no amount of care in the
/// transcription can tighten it. That is fine, and worth stating plainly: the
/// jitter perturbs the march's start by `0.06 * 1e-3 = 6e-5` metres, which is
/// four orders below the thickness window it feeds. **The dither is allowed to
/// disagree; the geometry is not.**
const IGN_TOLERANCE: f32 = 1.0e-3;

/// **The march tier's tolerance. EXPECTED, NOT MEASURED.**
///
/// The pass writes `Rgba16Float`, so both sides quantise on store and a value
/// sitting near a half-ULP boundary can round in opposite directions. An `f16`
/// ULP at magnitude 1 is `2^-10 = 9.77e-4`, and that is the dominant term — an
/// order of magnitude above everything the arithmetic contributes. `2e-3` is
/// about 2x one ULP, which leaves room for one differing round without leaving
/// room for a defect.
const MARCH_TOLERANCE: f32 = 2.0e-3;

/// **The blur tier's tolerance. EXPECTED, NOT MEASURED.** Same reasoning as
/// [`MARCH_TOLERANCE`]: five bilinear taps, a five-term weighted sum and one
/// division, read from and written to `Rgba16Float`, so one `f16` ULP dominates.
const BLUR_TOLERANCE: f32 = 2.0e-3;

/// The tight tier's harness: one fragment entry point per group of quantities,
/// each evaluating the sample its pixel column names.
///
/// Concatenated after [`SSR_COMMON_WGSL`], so what it calls are the *same*
/// functions the real pass calls, not a restatement of them.
const SSR_PARITY_HARNESS_WGSL: &str = r#"
struct SsrParityInputs { items: array<vec4<f32>, 96> };
struct SsrParityCamera { proj: mat4x4<f32>, proj_inv: mat4x4<f32> };

@group(0) @binding(0) var<uniform> ssr_parity: SsrParityInputs;
@group(0) @binding(1) var<uniform> ssr_parity_camera: SsrParityCamera;

@vertex
fn ssr_parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

// a = ( uv.x, uv.y, depth, roughness )
// b = ( facing, t, hit_diff, alpha )
// c = ( max_dist, thickness, ign_px, ign_py )
// d = ( oct.x, oct.y, 0, 0 )
@fragment
fn ssr_parity_scalars_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = ssr_parity.items[index * 4u + 0u];
    let b = ssr_parity.items[index * 4u + 1u];
    let c = ssr_parity.items[index * 4u + 2u];
    return vec4<f32>(
        ssr_ign(vec2<f32>(c.z, c.w)),
        ssr_confidence(a.xy, b.x, b.y, b.z, c.x, c.y),
        ssr_resolve_weight(b.w, a.w),
        pow( c.x / 0.06, 1.0 / f32( OW_SSR_STEPS ) ),
    );
}

@fragment
fn ssr_parity_geometry_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = ssr_parity.items[index * 4u + 0u];
    let d = ssr_parity.items[index * 4u + 3u];
    let p = ssr_view_pos(a.xy, a.z, ssr_parity_camera.proj_inv);
    return vec4<f32>(p, ssr_decode_normal(d.xy).z);
}

// The material's consumption. A fixed radiance so the reflection lanes are the
// only thing varying, and so the `mix` cannot pass by both sides being equal.
@fragment
fn ssr_parity_resolve_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = ssr_parity.items[index * 4u + 0u];
    let b = ssr_parity.items[index * 4u + 1u];
    let c = ssr_parity.items[index * 4u + 2u];
    // Lanes chosen for MAGNITUDE, not meaning: every one is of order 1, so an
    // absolute tolerance of 1e-6 is ~8 ULP rather than fractions of one.
    let reflection = vec4<f32>(a.x, a.y, c.y, b.w);
    let resolved = ssr_resolve(vec3<f32>(0.7, 0.35, 0.12), reflection, a.w);
    return vec4<f32>(resolved, 0.0);
}

@fragment
fn ssr_parity_project_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = ssr_parity.items[index * 4u + 0u];
    let d = ssr_parity.items[index * 4u + 3u];
    let p = ssr_view_pos(a.xy, a.z, ssr_parity_camera.proj_inv);
    let uv = ssr_project(p, ssr_parity_camera.proj);
    let n = ssr_decode_normal(d.xy);
    return vec4<f32>(uv.x, uv.y, n.x, n.y);
}
"#;

/// A real GPU: the device and queue every run in this module shares.
struct SsrGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    backend: wgpu::Backend,
}

impl SsrGpu {
    /// This module's handle on the crate's **one** shared adapter + device
    /// ([`crate::test_gpu`]), which fails loudly if the machine has no real
    /// adapter. Cloning handles rather than opening a device is what keeps the
    /// crate's GPU tests from crashing the driver.
    fn acquire() -> SsrGpu {
        let gpu = crate::test_gpu::TestGpu::shared();
        SsrGpu {
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

    /// A clamp-to-edge sampler. `filtering` picks nearest (the G-buffer, whose
    /// three attachments `prepass.js` sets to `NearestFilter`, and whose depth
    /// slot is `R32Float` and therefore not filterable at all) or linear (the
    /// previous resolved frame and the blur source, `pass.js`'s `hdrTarget`
    /// default).
    fn sampler(&self, filtering: bool) -> wgpu::Sampler {
        let mode = [wgpu::FilterMode::Nearest, wgpu::FilterMode::Linear][usize::from(filtering)];
        self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-ssr-parity-sampler"),
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
            label: Some("axiom-ssr-parity-input"),
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

    /// A uniform buffer holding `values`, little-endian, as the WGSL side reads
    /// them.
    fn uniform(&self, values: &[f32]) -> wgpu::Buffer {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        wgpu::util::DeviceExt::create_buffer_init(
            &self.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("axiom-ssr-parity-uniform"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        )
    }

    /// Draw one full-screen triangle with `pipeline` and `bind_group` into a
    /// `width x height` target of `format`, then read every texel back as four
    /// `f32` lanes through `decode`.
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
            label: Some("axiom-ssr-parity-target"),
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
            label: Some("axiom-ssr-parity-readback"),
            size: u64::from(row_bytes) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-ssr-parity-pass"),
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

    /// A render pipeline over `module` with `entry_point`, targeting `format`.
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
                label: Some("axiom-ssr-parity-pl"),
                bind_group_layouts: &[layout],
                push_constant_ranges: &[],
            });
        self.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-ssr-parity-pipeline"),
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
}

/// Four `f32` lanes from sixteen little-endian bytes — an `Rgba32Float` texel.
fn decode_rgba32(bytes: &[u8]) -> [f32; 4] {
    [0_usize, 1, 2, 3].map(|lane| {
        let at = lane * 4;
        f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    })
}

/// Four `f32` lanes from eight little-endian bytes — an `Rgba16Float` texel.
fn decode_rgba16(bytes: &[u8]) -> [f32; 4] {
    [0_usize, 1, 2, 3].map(|lane| {
        let at = lane * 2;
        from_half_bits(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
    })
}

/// The [`SAMPLES`] arithmetic inputs, chosen to exercise what is easy to get
/// wrong: UVs at and inside both screen borders (the edge fade's ramp), a facing
/// value on each side of the cutoff, a roughness on each side of the reversed
/// ramp, a thickness `hit_diff` that spans the fade, oct-encoded normals in all
/// four quadrants including the wrapped `z < 0` half, and pixel coordinates large
/// enough that `owIGN`'s inner `fract` has actually thrown bits away.
fn arithmetic_inputs() -> Vec<[f32; 4 * LANES]> {
    (0..SAMPLES)
        .map(|index| {
            let s = index as f32;
            let a = [
                0.02 + s * 0.04,
                0.97 - s * 0.038,
                1.5 + s * 1.7,
                s * 0.031,
            ];
            let b = [s * 0.041, 0.5 + s * 0.95, s * 0.027, 0.15 + s * 0.035];
            let c = [
                12.0 + s * 0.75,
                0.35 + s * 0.011,
                s * 61.5 + 3.5,
                911.0 - s * 37.25,
            ];
            // Oct coordinates that walk the full square, so both `select` arms
            // of the decoder's wrap are taken on both axes.
            let d = [s * 0.09 - 1.0, 1.0 - s * 0.085, 0.0, 0.0];
            let mut out = [0.0_f32; 4 * LANES];
            out[0..4].copy_from_slice(&a);
            out[4..8].copy_from_slice(&b);
            out[8..12].copy_from_slice(&c);
            out[12..16].copy_from_slice(&d);
            out
        })
        .collect()
}

/// The shared bind-group layout for the tight tier: two uniform buffers.
fn arithmetic_layout(gpu: &SsrGpu) -> wgpu::BindGroupLayout {
    gpu.device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-ssr-parity-arithmetic-bgl"),
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

/// **Tier 1: the pure arithmetic.** No sampler, no march — every input arrives
/// through a uniform, so a disagreement here is a transcription defect and
/// nothing else.
#[test]
fn the_pure_arithmetic_agrees_with_the_cpu_reference() {
    let gpu = SsrGpu::acquire();
    assert_ne!(
        gpu.backend,
        wgpu::Backend::Noop,
        "a parity proof needs a real adapter; the noop backend proves nothing"
    );
    let module = gpu.compile(
        "axiom-ssr-parity-arithmetic",
        &[SSR_COMMON_WGSL, SSR_PARITY_HARNESS_WGSL].concat(),
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
        label: Some("axiom-ssr-parity-arithmetic-bg"),
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
            "ssr_parity_vs",
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

    let scalars = render("ssr_parity_scalars_fs");
    let geometry = render("ssr_parity_geometry_fs");
    let projected = render("ssr_parity_project_fs");
    let resolved = render("ssr_parity_resolve_fs");

    let step_scale_of = |max_dist: f32| (max_dist / SSR_START_T).powf(1.0 / SSR_STEPS as f32);

    // The worst delta of each budget, tracked so the tolerances can be asserted
    // against a measurement rather than trusted.
    let worst = inputs.iter().enumerate().fold(
        (0.0_f32, 0.0_f32),
        |(worst_ign, worst_exact), (index, sample)| {
            let a = [sample[0], sample[1], sample[2], sample[3]];
            let b = [sample[4], sample[5], sample[6], sample[7]];
            let c = [sample[8], sample[9], sample[10], sample[11]];
            let d = [sample[12], sample[13]];

            let cpu_scalars = [
                ign([c[2], c[3]]),
                ssr_confidence([a[0], a[1]], b[0], b[1], b[2], c[0], c[1]),
                ssr_resolve_weight(b[3], a[3]),
                step_scale_of(c[0]),
            ];
            let p = view_pos([a[0], a[1]], a[2], &inv);
            let n = decode_normal(d);
            let uv = project_uv(p, &proj);
            let cpu_geometry = [p[0], p[1], p[2], n[2]];
            let cpu_projected = [uv[0], uv[1], n[0], n[1]];
            let mixed = ssr_resolve([0.7, 0.35, 0.12], [a[0], a[1], c[1], b[3]], a[3]);
            let cpu_resolved = [mixed[0], mixed[1], mixed[2], 0.0];

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
                .chain(
                    [0_usize, 1, 2, 3]
                        .iter()
                        .map(|lane| (resolved[index][*lane] - cpu_resolved[*lane]).abs()),
                )
                .fold(0.0_f32, f32::max);

            assert!(
                ign_delta <= IGN_TOLERANCE,
                "owIGN disagrees at sample {index} (p = {:?}): gpu {} vs cpu {}, delta {ign_delta}",
                [c[2], c[3]],
                scalars[index][0],
                cpu_scalars[0]
            );
            assert!(
                exact_delta <= ARITHMETIC_TOLERANCE,
                "the exact tier disagrees at sample {index}: gpu scalars {:?} / geometry {:?} / \
                 projected {:?} / resolved {:?} vs cpu {:?} / {:?} / {:?} / {:?}, worst delta \
                 {exact_delta}",
                scalars[index],
                geometry[index],
                projected[index],
                resolved[index],
                cpu_scalars,
                cpu_geometry,
                cpu_projected,
                cpu_resolved
            );
            (worst_ign.max(ign_delta), worst_exact.max(exact_delta))
        },
    );

    assert!(
        worst.1 * 10.0 >= ARITHMETIC_TOLERANCE,
        "the exact-tier tolerance is more than 10x the measured delta and is \
         therefore hiding something: measured {}, budget {ARITHMETIC_TOLERANCE}",
        worst.1
    );
    assert!(
        worst.0 * 10.0 >= IGN_TOLERANCE,
        "owIGN's budget is more than 10x its measured delta; tighten it toward \
         the measurement: measured {}, budget {IGN_TOLERANCE}",
        worst.0
    );
}

/// The march tier's bind-group layout: the uniform, the two samplers, and the
/// four textures, in the binding order [`SSR_PASS_WGSL`] declares.
fn march_layout(gpu: &SsrGpu) -> wgpu::BindGroupLayout {
    let texture = |binding: u32, filterable: bool| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let sampler = |binding: u32, filtering: bool| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(
            [
                wgpu::SamplerBindingType::NonFiltering,
                wgpu::SamplerBindingType::Filtering,
            ][usize::from(filtering)],
        ),
        count: None,
    };
    gpu.device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-ssr-parity-march-bgl"),
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
                sampler(1, false),
                sampler(2, true),
                texture(3, false),
                texture(4, false),
                texture(5, false),
                texture(6, true),
            ],
        })
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

/// **Tier 2: the whole march**, through the real pipeline and the real bindings,
/// into the production `Rgba16Float` target.
///
/// The scene is [`crate::ssr::tests`]'s mirror floor in front of a back wall,
/// built by intersecting each pixel's view ray with two real planes — see that
/// module for why a hand-painted depth ramp would make this test pass for the
/// wrong reason.
///
/// The march runs at the *same* resolution as the G-buffer here rather than the
/// production half, because the half-resolution shift is the frame graph's
/// arithmetic ([`crate::ssr::ssr_target_size`]) and not the shader's: the shader
/// reads normalised UVs and does not know its own scale. Testing at 1:1 keeps one
/// resampling out of a comparison that is about the march.
#[test]
fn the_marched_frame_agrees_with_the_cpu_reference() {
    let gpu = SsrGpu::acquire();
    assert_ne!(gpu.backend, wgpu::Backend::Noop, "a parity proof needs a real adapter");
    let module = gpu.compile(
        "axiom-ssr-parity-march",
        &[SSR_COMMON_WGSL, SSR_PASS_WGSL].concat(),
    );
    let layout = march_layout(&gpu);

    let (depth, normal, _still, color) = floor_scene();
    // The scene's velocity is zero, which would make the reprojection an
    // identity and leave `VELOCITY_TEXTURE_V_SIGN` and the lane order untested.
    // A small non-zero camera-pan velocity, distinct in `x` and `y` and of
    // opposite sign, is what makes a swapped or unsigned lane show up.
    let velocity = ScreenImage::from_fn(FRAME, FRAME, |_, _| {
        [
            from_half_bits(to_half_bits(0.011)),
            from_half_bits(to_half_bits(-0.007)),
            0.0,
            0.0,
        ]
    });
    let proj = projection();
    let inv = projection_inverse();
    let params = SsrParams::at_frame(9);
    let size = [FRAME as f32, FRAME as f32];
    let texel = [1.0 / size[0], 1.0 / size[1]];

    let uniform = gpu.uniform(&pack_ssr_uniform(&proj, &inv, params, texel, size));
    let point = gpu.sampler(false);
    let linear = gpu.sampler(true);
    let depth_view = gpu.upload(&depth, wgpu::TextureFormat::R32Float, 4, encode_r32);
    let normal_view = gpu.upload(&normal, wgpu::TextureFormat::Rgba16Float, 8, encode_rgba16);
    let velocity_view = gpu.upload(&velocity, wgpu::TextureFormat::Rg16Float, 4, encode_rg16);
    let color_view = gpu.upload(&color, wgpu::TextureFormat::Rgba16Float, 8, encode_rgba16);

    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-ssr-parity-march-bg"),
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
                resource: wgpu::BindingResource::Sampler(&linear),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&normal_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&velocity_view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&color_view),
            },
        ],
    });

    let pipeline = gpu.pipeline(
        &module,
        "ssr_vs",
        "ssr_fs",
        &layout,
        wgpu::TextureFormat::Rgba16Float,
    );
    let rendered = gpu.draw_and_read(
        &pipeline,
        &bind_group,
        FRAME,
        FRAME,
        wgpu::TextureFormat::Rgba16Float,
        8,
        decode_rgba16,
    );

    let inputs = SsrInputs {
        depth: &depth,
        normal: &normal,
        velocity: &velocity,
        color: &color,
        proj: &proj,
        proj_inv: &inv,
    };

    // Three quantities, because the march is not one kind of thing: a continuous
    // disagreement (bounded by MARCH_TOLERANCE), a discrete one (must be zero),
    // and how many pixels resolved a reflection at all (must not be zero, or the
    // whole comparison is vacuous).
    let (worst, discrete, hits) = (0..FRAME)
        .flat_map(|y| (0..FRAME).map(move |x| (x, y)))
        .enumerate()
        .fold((0.0_f32, 0_u32, 0_u32), |(worst, discrete, hits), (index, (x, y))| {
            let expected = ssr_pixel(
                &inputs,
                params,
                [x as f32 + 0.5, y as f32 + 0.5],
                size,
            )
            .map(|v| from_half_bits(to_half_bits(v)));
            let actual = rendered[index];
            let delta = [0_usize, 1, 2, 3]
                .iter()
                .map(|lane| (actual[*lane] - expected[*lane]).abs())
                .fold(0.0_f32, f32::max);
            let disagreed = u32::from((actual[3] > 0.0) != (expected[3] > 0.0));
            assert!(
                (delta <= MARCH_TOLERANCE) | (disagreed == 1),
                "the march disagrees at pixel ({x}, {y}): gpu {actual:?} vs cpu {expected:?}, \
                 delta {delta}"
            );
            (
                worst.max([0.0, delta][usize::from(disagreed == 0)]),
                discrete + disagreed,
                hits + u32::from(expected[3] > 0.0),
            )
        });

    assert_eq!(
        discrete, 0,
        "{discrete} pixels hit on one side and missed on the other. This is NOT a \
         tolerance to widen: either the march's `pow`/jitter path diverged further \
         than the arithmetic tier says it can, or a scene pixel sits exactly on the \
         thickness boundary and the scene must move."
    );
    assert!(
        hits > 0,
        "the mirror floor resolved no reflection on the GPU either; this test is \
         comparing two black frames and proving nothing"
    );
    assert!(
        worst * 10.0 >= MARCH_TOLERANCE,
        "the march tolerance is more than 10x the measured delta: measured {worst}, \
         budget {MARCH_TOLERANCE}"
    );
}

/// **Tier 3: the separable blur.** One uploaded half-resolution image, the real
/// pass, both axes' direction vectors.
#[test]
fn the_blur_agrees_with_the_cpu_reference() {
    let gpu = SsrGpu::acquire();
    assert_ne!(gpu.backend, wgpu::Backend::Noop, "a parity proof needs a real adapter");
    let module = gpu.compile("axiom-ssr-parity-blur", SSR_BLUR_WGSL);

    let layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-ssr-parity-blur-bgl"),
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

    // A reflection buffer with real structure in it: a bright diagonal streak on
    // a dim field, plus a confidence that varies independently of the colour, so
    // the alpha lane cannot pass by coincidence with the colour lanes.
    let source = ScreenImage::from_fn(FRAME, FRAME, |x, y| {
        let streak = f32::from(x == y) * 3.0;
        [
            (from_half_bits(to_half_bits(0.1 + streak))),
            from_half_bits(to_half_bits(x as f32 * 0.02)),
            from_half_bits(to_half_bits(y as f32 * 0.03)),
            from_half_bits(to_half_bits((x + y) as f32 * 0.01)),
        ]
    });
    let source_view = gpu.upload(&source, wgpu::TextureFormat::Rgba16Float, 8, encode_rgba16);
    let linear = gpu.sampler(true);
    let size = [FRAME as f32, FRAME as f32];
    let pipeline = gpu.pipeline(
        &module,
        "ssr_blur_vs",
        "ssr_blur_fs",
        &layout,
        wgpu::TextureFormat::Rgba16Float,
    );

    // Both axes, in the order the pass runs them: horizontal into B, then
    // vertical back into A.
    let worst = [[1.0 / size[0], 0.0], [0.0, 1.0 / size[1]]]
        .iter()
        .fold(0.0_f32, |worst, direction| {
            let uniform = gpu.uniform(&pack_ssr_blur_uniform(*direction, size));
            let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-ssr-parity-blur-bg"),
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
                wgpu::TextureFormat::Rgba16Float,
                8,
                decode_rgba16,
            );
            (0..FRAME)
                .flat_map(|y| (0..FRAME).map(move |x| (x, y)))
                .enumerate()
                .fold(worst, |worst, (index, (x, y))| {
                    let uv = [
                        (x as f32 + 0.5) / size[0],
                        (y as f32 + 0.5) / size[1],
                    ];
                    let expected = ssr_blur_pixel(&source, uv, *direction)
                        .map(|v| from_half_bits(to_half_bits(v)));
                    let actual = rendered[index];
                    let delta = [0_usize, 1, 2, 3]
                        .iter()
                        .map(|lane| (actual[*lane] - expected[*lane]).abs())
                        .fold(0.0_f32, f32::max);
                    assert!(
                        delta <= BLUR_TOLERANCE,
                        "the blur disagrees at pixel ({x}, {y}) along {direction:?}: \
                         gpu {actual:?} vs cpu {expected:?}, delta {delta}"
                    );
                    worst.max(delta)
                })
        });

    assert!(
        worst * 10.0 >= BLUR_TOLERANCE,
        "the blur tolerance is more than 10x the measured delta: measured {worst}, \
         budget {BLUR_TOLERANCE}"
    );
}
