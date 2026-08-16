//! **CPU↔GPU parity.** The proof that a generated shader means what
//! `axiom-field`'s evaluator means.
//!
//! `FieldGraph::evaluate` is the definition of the field language; everything
//! else — this emitter included — is a *mirror*. A mirror that is never held up
//! to the original is a second definition waiting to drift, so every operator in
//! the algebra is driven through both sides here, on a real GPU, and compared.
//!
//! ## How
//!
//! One test graph per operator, adapted to a `Vec4` and bound to a surface's
//! base colour. The CPU side calls `FieldGraph::evaluate` at each of
//! [`SAMPLES`] evaluation contexts. The GPU side compiles the emitted
//! `axiom_surface` against the same fixed prelude the main pass uses, renders a
//! `SAMPLES x 1` `Rgba32Float` target where fragment *i* evaluates context *i*,
//! and reads the four lanes back. `Rgba32Float` because a `Rgba8Unorm` target
//! quantises to 1/255, which is forty times coarser than the tolerance.
//!
//! ## Tolerance: `1e-4` absolute, never byte equality
//!
//! A GPU is allowed to contract `a * b + c` into a single-rounding `fma`, to
//! evaluate a reciprocal at a different precision, and to reassociate. The
//! emitter removes every *avoidable* difference — it writes out `dot`, `mix`,
//! `smoothstep` and the normalize reciprocal by hand rather than calling the
//! builtins, whose factoring is unspecified — but the ones that remain are the
//! hardware's, not the emitter's. `1e-4` absolute on channels that live in
//! `0..=1` is roughly 12 bits of headroom: far tighter than anything visible,
//! and far looser than the last mantissa bit.
//!
//! ## The failure message names the OPERATOR
//!
//! Not a WGSL line, which no author wrote. Each case carries the operator's
//! name and every assertion quotes it, because "`Fbm` disagrees at sample 7" is
//! a fixable report and "shader line 214" is not.
//!
//! ## This runs only with a real GPU
//!
//! The module is compiled only under `--features offscreen`, and it **asserts**
//! an adapter was acquired rather than skipping. A parity test that silently
//! passes when nothing ran is worse than no parity test.

use axiom_field::{
    EvalContext, FieldBuilder, FieldGraph, FieldId, FieldOp, FieldType, FieldValue,
};
use axiom_kernel::{Seconds, StableHash};
use axiom_math::{Vec2, Vec3, Vec4};
use axiom_recipe::{Param, Scalar};
use axiom_surface::{Surface, SurfaceBuilder, SurfaceChannel};

use crate::surface_program::emit::surface_function;
use crate::surface_program::params::{pack, ParamLayout};
use crate::surface_program::program_error::{SurfaceProgramError, SurfaceProgramFault};
use crate::surface_program::wgsl_template::{
    scene_shader, DEFAULT_DISPLACE_WGSL, DEFAULT_SURFACE_WGSL, SURFACE_PRELUDE_WGSL,
};

/// How many evaluation contexts one run compares. Also the target's width, and
/// one fragment per context.
pub(super) const SAMPLES: usize = 24;

/// The absolute tolerance, documented in this module's header. Never zero.
pub(super) const TOLERANCE: f32 = 1.0e-4;

/// `copy_texture_to_buffer` requires each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// The harness shader: a fullscreen triangle whose fragment stage evaluates the
/// generated `axiom_surface` at the context its pixel column names, plus a second
/// entry point that renders the lattice hash itself.
pub(super) const PARITY_HARNESS_WGSL: &str = r#"
struct ParityContexts { items: array<vec4<f32>, 72> };
struct ParityCells { items: array<vec4<u32>, 48> };

@group(0) @binding(0) var<uniform> contexts: ParityContexts;
@group(0) @binding(1) var<uniform> parity_params: SurfaceParams;
@group(0) @binding(2) var<uniform> cells: ParityCells;

@vertex
fn parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn parity_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = contexts.items[index * 3u + 0u];
    let b = contexts.items[index * 3u + 1u];
    let c = contexts.items[index * 3u + 2u];
    let result = axiom_surface(
        SurfaceIn(a.xyz, b.xy, c.xyz, a.w, vec4<f32>(1.0, 1.0, 1.0, 1.0), vec3<f32>(0.0, 0.0, 0.0)),
        parity_params,
    );
    return result.base_color;
}

// The VERTEX stage's program, evaluated at the context its pixel column names.
// It is the same `axiom_displace` the main pass's `vs` calls, with the same five
// arguments in the same order — so what this compares is the function the frame
// actually runs, not a restatement of it.
@fragment
fn parity_displace_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = contexts.items[index * 3u + 0u];
    let b = contexts.items[index * 3u + 1u];
    let c = contexts.items[index * 3u + 2u];
    return vec4<f32>(axiom_displace(a.xyz, c.xyz, b.xy, a.w, parity_params), 0.0);
}

// The lattice hash, rendered as four 16-bit halves. Every half is below 65536
// and therefore exact in an f32, so this comparison is bit-for-bit despite
// travelling through a float target.
@fragment
fn parity_hash_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let a = cells.items[index * 2u + 0u];
    let b = cells.items[index * 2u + 1u];
    let h = axiom_hash_cell(
        vec2<u32>(a.y, a.x),
        vec3<i32>(bitcast<i32>(a.z), bitcast<i32>(a.w), bitcast<i32>(b.x)),
    );
    return vec4<f32>(
        f32(h.x >> 16u),
        f32(h.x & 0xFFFFu),
        f32(h.y >> 16u),
        f32(h.y & 0xFFFFu),
    );
}
"#;

/// A real GPU: the device and queue every run in this module shares.
pub(super) struct ParityGpu {
    // Reachable from the sibling parity modules: `parity_vertex` proves the
    // vertex stage through this module's own `render`, while `parity_lighting`
    // has to drive the MAIN PASS's real pipeline — four vertex-stage inputs,
    // three bind groups and a light rig — which needs the device itself.
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) backend: wgpu::Backend,
}

impl ParityGpu {
    /// Acquire a native adapter, or fail the test loudly.
    pub(super) fn acquire() -> ParityGpu {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU parity test needs a real adapter; there is no honest fallback");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("axiom-surface-parity"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("the adapter must yield a device");
        ParityGpu {
            device,
            queue,
            backend: adapter.get_info().backend,
        }
    }

    /// Compile `source`, capturing a validation failure as a structured error
    /// naming `program_id` and the channels the program covered.
    pub(super) fn compile(
        &self,
        source: &str,
        program_id: u64,
        channels: u16,
    ) -> Result<wgpu::ShaderModule, SurfaceProgramError> {
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("axiom-surface-parity-shader"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        pollster::block_on(self.device.pop_error_scope()).map_or(Ok(module), |error| {
            Err(SurfaceProgramError::new(
                program_id,
                channels,
                SurfaceProgramFault::Compilation,
                error.to_string(),
            ))
        })
    }

    /// Render `entry_point` over a `SAMPLES x 1` `Rgba32Float` target and read
    /// the four lanes of every pixel back.
    pub(super) fn render(
        &self,
        module: &wgpu::ShaderModule,
        entry_point: &str,
        contexts: &[u8],
        params: &[u8],
        cells: &[u8],
    ) -> Vec<[f32; 4]> {
        let layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("axiom-surface-parity-bgl"),
                entries: &[0_u32, 1, 2]
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
            });
        let buffers = [contexts, params, cells].map(|bytes| {
            wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-surface-parity-uniform"),
                    contents: bytes,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            )
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-surface-parity-bg"),
            layout: &layout,
            entries: &[0_usize, 1, 2]
                .map(|index| wgpu::BindGroupEntry {
                    binding: index as u32,
                    resource: buffers[index].as_entire_binding(),
                })
                .to_vec(),
        });
        let pipeline_layout =
            self.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("axiom-surface-parity-pl"),
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-surface-parity-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("parity_vs"),
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
            label: Some("axiom-surface-parity-target"),
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
            label: Some("axiom-surface-parity-readback"),
            size: u64::from(row_bytes),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-surface-parity-pass"),
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

/// The [`SAMPLES`] evaluation contexts, chosen to exercise the parts of the
/// algebra that are easy to get wrong: negative coordinates (the lattice hash
/// sign-extends them), non-integer coordinates (gradient noise is exactly zero
/// at a lattice node, so an integer grid would hide every hash error), several
/// lattice cells, and a moving clock.
pub(super) fn contexts() -> Vec<EvalContext> {
    (0..SAMPLES)
        .map(|index| {
            let t = index as f32;
            EvalContext::new(
                Vec3::new(t * 0.37 - 4.0, t * -0.53 + 2.5, t * 0.19 - 1.25),
                Vec2::new(t * 0.041, 1.0 - t * 0.037),
                Vec3::new(t * 0.07 - 0.8, 0.6, t * -0.05 + 0.4),
                Seconds::finite_or_zero(t * 0.25),
            )
        })
        .collect()
}

/// The context uniform's bytes: three `vec4` per sample — `(point, time)`,
/// `(uv, 0, 0)`, `(normal, 0)` — matching what `parity_fs` unpacks.
pub(super) fn context_bytes(contexts: &[EvalContext]) -> Vec<u8> {
    let mut bytes: Vec<u8> = contexts
        .iter()
        .flat_map(|context| {
            let point = context.point();
            let uv = context.uv();
            let normal = context.normal();
            [
                point.x,
                point.y,
                point.z,
                context.time().get(),
                uv.x,
                uv.y,
                0.0,
                0.0,
                normal.x,
                normal.y,
                normal.z,
                0.0,
            ]
        })
        .flat_map(f32::to_le_bytes)
        .collect();
    bytes.resize(SAMPLES * 3 * 16, 0);
    bytes
}

/// One test case: an operator's name and the graph that exercises it.
struct Case {
    operator: &'static str,
    graph: FieldGraph,
}

/// A fresh builder for a named graph.
pub(super) fn builder(name: &str) -> FieldBuilder {
    FieldBuilder::new(FieldId::of_name(name), 1)
}

/// Adapt a node of `ty` into a `Vec4`, so every case can bind to base colour:
/// lanes are extracted and recomposed, which costs two more operators that are
/// themselves under test.
fn widen(
    from: FieldBuilder,
    node: axiom_recipe::NodeId,
    ty: FieldType,
) -> (FieldBuilder, axiom_recipe::NodeId) {
    let lanes = usize::from(ty.lanes());
    let (with_lanes, extracted) = (0..4).fold(
        (from, Vec::new()),
        |(builder, mut extracted), lane: usize| {
            let (builder, id) = builder.push(
                FieldOp::Component,
                vec![Param::int((lane % lanes) as u32)],
                vec![node],
            );
            extracted.push(id);
            (builder, extracted)
        },
    );
    with_lanes.push(FieldOp::Compose, vec![Param::int(4)], extracted)
}

/// A `Vec4` graph built by pushing `op` over the given source nodes, then
/// widening whatever it produced.
fn case(operator: &'static str, graph: FieldGraph) -> Case {
    Case { operator, graph }
}

/// One `Vec3` constant node.
fn vec3_const(
    from: FieldBuilder,
    x: f32,
    y: f32,
    z: f32,
) -> (FieldBuilder, axiom_recipe::NodeId) {
    from.push_const(FieldValue::vec3(Vec3::new(x, y, z)))
}

/// A case for a two-input arithmetic operator over `Point` and a `Vec3`
/// constant.
fn binary_case(operator: &'static str, name: &str, op: FieldOp) -> Case {
    let (b, point) = builder(name).push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, other) = vec3_const(b, 0.5, -1.5, 2.0);
    let (b, applied) = b.push(op, Vec::new(), vec![point, other]);
    let (b, wide) = widen(b, applied, FieldType::Vec3);
    case(operator, b.build(wide))
}

/// Every operator in the algebra, each as one `Vec4` graph.
fn cases() -> Vec<Case> {
    let mut all = vec![
        {
            let (b, node) =
                builder("p/const").push_const(FieldValue::vec4(Vec4::new(0.25, 0.5, 0.75, 1.0)));
            case("Const", b.build(node))
        },
        {
            let (b, node) = builder("p/point").push(FieldOp::Point, Vec::new(), Vec::new());
            let (b, wide) = widen(b, node, FieldType::Vec3);
            case("Point", b.build(wide))
        },
        {
            let (b, node) = builder("p/uv").push(FieldOp::Uv, Vec::new(), Vec::new());
            let (b, wide) = widen(b, node, FieldType::Vec2);
            case("Uv", b.build(wide))
        },
        {
            let (b, node) = builder("p/normal").push(FieldOp::Normal, Vec::new(), Vec::new());
            let (b, wide) = widen(b, node, FieldType::Vec3);
            case("Normal", b.build(wide))
        },
        {
            let (b, node) = builder("p/time").push(FieldOp::Time, Vec::new(), Vec::new());
            let (b, wide) = widen(b, node, FieldType::Scalar);
            case("Time", b.build(wide))
        },
        {
            let (b, slot) = builder("p/param")
                .declare("tint", FieldValue::vec4(Vec4::new(0.125, 0.375, 0.625, 0.875)));
            let (b, node) = b.push_param(slot, FieldType::Vec4);
            case("Param", b.build(node))
        },
        binary_case("Add", "p/add", FieldOp::Add),
        binary_case("Sub", "p/sub", FieldOp::Sub),
        binary_case("Mul", "p/mul", FieldOp::Mul),
        binary_case("Min", "p/min", FieldOp::Min),
        binary_case("Max", "p/max", FieldOp::Max),
        {
            let (b, point) = builder("p/abs").push(FieldOp::Point, Vec::new(), Vec::new());
            let (b, applied) = b.push(FieldOp::Abs, Vec::new(), vec![point]);
            let (b, wide) = widen(b, applied, FieldType::Vec3);
            case("Abs", b.build(wide))
        },
        {
            let (b, point) = builder("p/clamp").push(FieldOp::Point, Vec::new(), Vec::new());
            let (b, lo) = vec3_const(b, 0.0, -1.0, 3.0);
            let (b, hi) = vec3_const(b, 1.0, 1.0, 1.0);
            let (b, applied) = b.push(FieldOp::Clamp, Vec::new(), vec![point, lo, hi]);
            let (b, wide) = widen(b, applied, FieldType::Vec3);
            // The third lane has lo > hi, the documented degenerate case.
            case("Clamp", b.build(wide))
        },
        {
            let (b, point) = builder("p/mix").push(FieldOp::Point, Vec::new(), Vec::new());
            let (b, other) = vec3_const(b, 1.0, 2.0, 3.0);
            let (b, t) = b.push_const(FieldValue::scalar(Scalar::new(1.75)));
            let (b, applied) = b.push(FieldOp::Mix, Vec::new(), vec![point, other, t]);
            let (b, wide) = widen(b, applied, FieldType::Vec3);
            // t is outside 0..=1: Mix extrapolates, and must do so on both sides.
            case("Mix", b.build(wide))
        },
        {
            let (b, point) = builder("p/smooth").push(FieldOp::Point, Vec::new(), Vec::new());
            let (b, e0) = vec3_const(b, -1.0, 0.0, 2.0);
            let (b, e1) = vec3_const(b, 1.0, 2.0, 2.0);
            let (b, applied) = b.push(FieldOp::Smoothstep, Vec::new(), vec![e0, e1, point]);
            let (b, wide) = widen(b, applied, FieldType::Vec3);
            // The third lane's edges are equal, the documented degenerate case.
            case("Smoothstep", b.build(wide))
        },
        {
            let (b, point) = builder("p/dot").push(FieldOp::Point, Vec::new(), Vec::new());
            let (b, normal) = b.push(FieldOp::Normal, Vec::new(), Vec::new());
            let (b, applied) = b.push(FieldOp::Dot, Vec::new(), vec![point, normal]);
            let (b, wide) = widen(b, applied, FieldType::Scalar);
            case("Dot", b.build(wide))
        },
        {
            let (b, point) = builder("p/length").push(FieldOp::Point, Vec::new(), Vec::new());
            let (b, applied) = b.push(FieldOp::Length, Vec::new(), vec![point]);
            let (b, wide) = widen(b, applied, FieldType::Scalar);
            case("Length", b.build(wide))
        },
        {
            let (b, point) = builder("p/normalize").push(FieldOp::Point, Vec::new(), Vec::new());
            let (b, applied) = b.push(FieldOp::Normalize, Vec::new(), vec![point]);
            let (b, wide) = widen(b, applied, FieldType::Vec3);
            case("Normalize", b.build(wide))
        },
        {
            let (b, uv) = builder("p/compose").push(FieldOp::Uv, Vec::new(), Vec::new());
            let (b, x) = b.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
            let (b, y) = b.push(FieldOp::Component, vec![Param::int(1)], vec![uv]);
            let (b, node) = b.push(FieldOp::Compose, vec![Param::int(3)], vec![x, y, x]);
            let (b, wide) = widen(b, node, FieldType::Vec3);
            case("Compose", b.build(wide))
        },
        {
            let (b, point) = builder("p/component").push(FieldOp::Point, Vec::new(), Vec::new());
            let (b, node) = b.push(FieldOp::Component, vec![Param::int(2)], vec![point]);
            let (b, wide) = widen(b, node, FieldType::Scalar);
            case("Component", b.build(wide))
        },
        noise_case(),
        fbm_case(),
        transform_case(),
    ];
    all.sort_by_key(|entry| entry.operator);
    all
}

/// `Noise` at the object-space point, with a seed whose halves are both
/// non-trivial so a swapped high/low word cannot pass.
fn noise_case() -> Case {
    let (b, point) = builder("p/noise").push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, node) = b.push(
        FieldOp::Noise,
        vec![Param::from_bits(0x89AB_CDEF), Param::from_bits(0x0123_4567)],
        vec![point],
    );
    let (b, wide) = widen(b, node, FieldType::Scalar);
    case("Noise", b.build(wide))
}

/// `Fbm` at the object-space point: four octaves, a non-default lacunarity and
/// gain, so every knob word has to reach the shader.
fn fbm_case() -> Case {
    let (b, point) = builder("p/fbm").push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, node) = b.push(
        FieldOp::Fbm,
        vec![
            Param::from_bits(0x0000_004D),
            Param::from_bits(0x0000_0000),
            Param::int(4),
            Param::from_bits(1.5_f32.to_bits()),
            Param::from_bits(2.25_f32.to_bits()),
            Param::from_bits(0.375_f32.to_bits()),
        ],
        vec![point],
    );
    let (b, wide) = widen(b, node, FieldType::Scalar);
    case("Fbm", b.build(wide))
}

/// `Transform` of the object-space point through a matrix whose four columns
/// live in parameter slots 0..4 — a translate + scale, so translation, scale and
/// the affine `w` are all exercised.
fn transform_case() -> Case {
    let columns = [
        Vec4::new(2.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 0.5, 0.0, 0.0),
        Vec4::new(0.0, 0.0, -1.0, 0.0),
        Vec4::new(0.25, -0.75, 1.5, 1.0),
    ];
    let (b, slots) = columns.iter().enumerate().fold(
        (builder("p/transform"), Vec::new()),
        |(builder, mut slots), (index, column)| {
            let (builder, slot) = builder.declare(&format!("col{index}"), FieldValue::vec4(*column));
            slots.push(slot);
            (builder, slots)
        },
    );
    let (b, point) = b.push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, node) = b.push(
        FieldOp::Transform,
        slots
            .iter()
            .map(|slot| Param::int(u32::from(slot.raw())))
            .collect(),
        vec![point],
    );
    let (b, wide) = widen(b, node, FieldType::Vec3);
    case("Transform", b.build(wide))
}

/// The surface one case's graph becomes: the graph on base colour, nothing else
/// bound.
fn surface_of(graph: &FieldGraph) -> Surface {
    SurfaceBuilder::new()
        .field(SurfaceChannel::BaseColor, graph.clone())
        .build()
        .expect("every parity case is a legal vec4 base colour")
}

/// Run one case on both sides and return `(cpu, gpu)` lane sets.
fn compare(gpu: &ParityGpu, case: &Case, contexts: &[EvalContext]) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let surface = surface_of(&case.graph);
    let flat = surface.flatten().expect("a flat surface flattens to itself");
    let program = surface_function(&surface).expect("every parity case emits");
    let module = gpu
        .compile(
            &[
                SURFACE_PRELUDE_WGSL,
                DEFAULT_DISPLACE_WGSL,
                &program,
                PARITY_HARNESS_WGSL,
            ]
            .concat(),
            surface.digest().raw(),
            SurfaceChannel::BaseColor.bit(),
        )
        .unwrap_or_else(|error| panic!("{} must emit compiling WGSL: {error}", case.operator));
    let params = pack(
        ParamLayout::of(surface.requirements().param_count()),
        &flat,
    );
    let rendered = gpu.render(
        &module,
        "parity_fs",
        &context_bytes(contexts),
        &params,
        &vec![0_u8; 48 * 16],
    );
    let evaluated = contexts
        .iter()
        .map(|context| {
            let lanes = flat
                .binding(SurfaceChannel::BaseColor)
                .as_graph()
                .evaluate(context)
                .unwrap_or_else(|error| {
                    panic!("{} must evaluate on the CPU: {error:?}", case.operator)
                })
                .as_vec4();
            [lanes.x, lanes.y, lanes.z, lanes.w]
        })
        .collect();
    (evaluated, rendered)
}

/// Assert two lane sets agree to [`TOLERANCE`], naming the operator.
pub(super) fn assert_parity(operator: &str, cpu: &[[f32; 4]], gpu: &[[f32; 4]]) {
    cpu.iter()
        .zip(gpu.iter())
        .enumerate()
        .for_each(|(sample, (expected, actual))| {
            (0..4).for_each(|lane| {
                let delta = (expected[lane] - actual[lane]).abs();
                assert!(
                    delta <= TOLERANCE,
                    "{operator} disagrees at sample {sample} lane {lane}: \
                     CPU {} vs GPU {} (delta {delta}, tolerance {TOLERANCE})",
                    expected[lane],
                    actual[lane]
                );
            });
        });
}

/// **The noise hash parity test.** Written first, and the one that matters most:
/// if the WGSL 64-bit FNV-1a and the kernel's disagree by a single bit, every
/// noise-driven surface differs on the GPU from the CPU and from every bake.
///
/// Bit-for-bit, not within a tolerance — the comparison is over integers small
/// enough to be exact in an `f32`.
#[test]
fn the_wgsl_lattice_hash_is_the_kernels_stable_hash_bit_for_bit() {
    let gpu = ParityGpu::acquire();
    assert_ne!(
        gpu.backend,
        wgpu::Backend::Noop,
        "the parity proof is worthless unless a real backend ran it"
    );
    // Seeds and lattice cells, deliberately including negative coordinates (the
    // hash sign-extends them into the 64-bit word space) and a seed whose two
    // 32-bit halves differ.
    let cells: Vec<(u64, [i32; 3])> = (0..SAMPLES)
        .map(|index| {
            let i = index as i32;
            (
                0x0123_4567_89AB_CDEF_u64.wrapping_mul(index as u64 + 1),
                [i - 12, -i * 3 + 5, i * 7 - 40],
            )
        })
        .collect();
    let bytes: Vec<u8> = cells
        .iter()
        .flat_map(|(seed, cell)| {
            [
                *seed as u32,
                (*seed >> 32) as u32,
                cell[0] as u32,
                cell[1] as u32,
                cell[2] as u32,
                0,
                0,
                0,
            ]
        })
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut padded = bytes;
    padded.resize(48 * 16, 0);
    let module = gpu
        .compile(
            &[
                SURFACE_PRELUDE_WGSL,
                DEFAULT_DISPLACE_WGSL,
                DEFAULT_SURFACE_WGSL,
                PARITY_HARNESS_WGSL,
            ]
            .concat(),
            0,
            SurfaceChannel::BaseColor.bit(),
        )
        .expect("the prelude must compile");
    let rendered = gpu.render(
        &module,
        "parity_hash_fs",
        &vec![0_u8; 72 * 16],
        &vec![0_u8; 512],
        &padded,
    );
    cells
        .iter()
        .zip(rendered.iter())
        .enumerate()
        .for_each(|(index, ((seed, cell), lanes))| {
            let expected = StableHash::of_words(&[
                *seed,
                cell[0] as i64 as u64,
                cell[1] as i64 as u64,
                cell[2] as i64 as u64,
            ])
            .raw();
            let actual = (u64::from(lanes[0] as u32) << 48)
                | (u64::from(lanes[1] as u32) << 32)
                | (u64::from(lanes[2] as u32) << 16)
                | u64::from(lanes[3] as u32);
            assert_eq!(
                actual, expected,
                "the WGSL FNV-1a must equal StableHash::of_words at cell {index} \
                 (seed {seed:#x}, cell {cell:?})"
            );
        });
}

/// The full sweep: every operator, on a real GPU, at the documented tolerance.
#[test]
fn every_operator_agrees_with_the_cpu_evaluator_within_the_documented_tolerance() {
    let gpu = ParityGpu::acquire();
    assert_ne!(gpu.backend, wgpu::Backend::Noop);
    let contexts = contexts();
    let all = cases();
    assert_eq!(
        all.len(),
        axiom_field::FIELD_OP_COUNT,
        "one case per operator, or the sweep has a hole"
    );
    all.iter().for_each(|entry| {
        let (cpu, rendered) = compare(&gpu, entry, &contexts);
        assert_parity(entry.operator, &cpu, &rendered);
    });
}

/// A noise-driven surface is the one whose failure would be invisible until a
/// bake and a frame disagreed, so it gets its own named test as well as its row
/// in the sweep.
#[test]
fn noise_and_fbm_agree_with_the_cpu_evaluator_across_the_lattice() {
    let gpu = ParityGpu::acquire();
    let contexts = contexts();
    [noise_case(), fbm_case()].iter().for_each(|entry| {
        let (cpu, rendered) = compare(&gpu, entry, &contexts);
        assert_parity(entry.operator, &cpu, &rendered);
        // A noise field that read as a constant would pass a tolerance check
        // against a constant CPU side, so prove the signal actually varies.
        let spread = cpu
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), lanes| {
                (low.min(lanes[0]), high.max(lanes[0]))
            });
        assert!(
            spread.1 - spread.0 > 0.05,
            "{} must vary across the sampled lattice, or the parity is vacuous",
            entry.operator
        );
    });
}

/// The main pass's own shader — the two scene halves with the default program
/// spliced between them — must compile. This is the pixel-identity guarantee's
/// first half: the frame every existing app draws still has a shader.
#[test]
fn the_main_pass_shader_compiles_with_the_default_program_spliced_in() {
    let gpu = ParityGpu::acquire();
    let source = scene_shader(
        crate::scene_wgsl::SCENE_WGSL_PREFIX,
        DEFAULT_DISPLACE_WGSL,
        DEFAULT_SURFACE_WGSL,
        crate::scene_wgsl::SCENE_WGSL_SUFFIX,
    );
    gpu.compile(&source, 0, SurfaceChannel::BaseColor.bit())
        .expect("the main pass must compile with the default program");
    // And the default program is the identity over the lanes `fs` used to read
    // inline, which is the other half of the guarantee.
    assert!(source.contains("out.base_color = in.albedo;"));
    assert!(source.contains("let base = vec4<f32>(surface.base_color.rgb, surface.opacity);"));
    assert!(source.contains("let emitted = lit + surface.emission;"));
}

/// A shader the device rejects is reported as a structured error naming the
/// surface's digest and the channels its program covered — never a panic, never
/// a silently black draw.
#[test]
fn a_shader_that_will_not_compile_is_reported_as_a_structured_error() {
    let gpu = ParityGpu::acquire();
    let error = gpu
        .compile("fn broken( {", 0xDEAD_BEEF, SurfaceChannel::Opacity.bit())
        .expect_err("that is not WGSL");
    assert_eq!(error.program_id(), 0xDEAD_BEEF);
    assert_eq!(error.fault(), SurfaceProgramFault::Compilation);
    assert_eq!(error.channel_names(), vec!["opacity"]);
    assert!(error.to_string().contains("0x00000000deadbeef"));
}
