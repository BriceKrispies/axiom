//! **The three lighting models, proved on the main pass's own shader.**
//!
//! [`crate::surface_program::parity`] holds the emitted *channel* expressions to
//! `axiom-field`'s evaluator, operator by operator.
//! [`crate::surface_program::parity_vertex`] does the same for the vertex stage.
//! This is the third proof, and it is a different kind: what
//! [`axiom_surface::LightingModel`] decides is not a value but *how much of the
//! lighting maths a surface takes*, so the only honest way to test it is to run
//! **the real `fs`** — the whole `crate::scene_wgsl` pass, spliced exactly as
//! `crate::scene_renderer` splices it, over a real pipeline with real vertex
//! attributes, a real light rig and the three real bind groups — and read the
//! pixel back.
//!
//! Nothing here restates the lighting maths. A restatement would be a second
//! definition, which is precisely what the parity modules exist to prevent. The
//! rig instead makes every term of the model **exactly one**: one directional
//! light straight down the surface normal, a unit light colour, a unit specular
//! strength, an unshadowed fragment and no fog — so each model's result is an
//! arithmetic sum a reader can check by eye, and the *difference* between two
//! models is the term the discriminant gated.
//!
//! ## What is proved
//!
//! * Each model renders its documented result for a known rig, numerically.
//! * `Unlit` is `base_color.rgb + emission`, and does not move when every light
//!   in the frame moves or is removed.
//! * `Lambert` is `LambertSpecular` minus exactly the Blinn-Phong term — proved
//!   by rendering `LambertSpecular` with the specular capability cleared and
//!   getting `Lambert`'s pixel, bit for bit.
//! * **`LambertSpecular` is pixel-identical to the shader before this work.**
//!   The control is not a remembered number: the test *reconstructs* the old
//!   `fs` out of the new one by deleting the two gate multiplies and the gate
//!   `select` (asserting each deletion actually matched), compiles both, and
//!   renders them into the same rig. Byte equality is the pass condition.
//! * `metallic` moves no pixel under any of the three models.
//! * The WGSL model codes are `axiom_surface::LightingModel`'s discriminants.
//!
//! ## This runs only with a real GPU
//!
//! Same rule as the other two: compiled only under `--features offscreen`, and
//! it **asserts** a real adapter was acquired rather than skipping.

use axiom_field::FieldValue;
use axiom_math::{Vec3, Vec4};
use axiom_recipe::Scalar;
use axiom_surface::{LightingModel, Surface, SurfaceBuilder, SurfaceChannel};

use crate::surface_program::emit_lighting::fragment_program;
use crate::surface_program::parity::ParityGpu;
use crate::surface_program::wgsl_template::{
    scene_shader, DEFAULT_DISPLACE_WGSL, DEFAULT_SURFACE_WGSL,
};

/// The capability bits this module names, mirrored from
/// `axiom_host::RenderCapability` exactly as the main pass's WGSL mirrors them.
const CAP_TEXTURES: u32 = 1;
const CAP_ALPHAMASK: u32 = 2;
const CAP_NORMALMAP: u32 = 4;
const CAP_SHADOWS: u32 = 8;
const CAP_SPECULAR: u32 = 512;
const CAP_AERIAL: u32 = 2048;

/// Every bit the main pass gates on — the richest frame this backend can draw,
/// used for the pixel-identity control so no term of the shader is skipped.
const CAP_ALL: u32 =
    CAP_TEXTURES | CAP_ALPHAMASK | CAP_NORMALMAP | CAP_SHADOWS | CAP_SPECULAR | CAP_AERIAL;

/// Bytes in the lighting uniform: a 96-byte header then 16 lights of 32 bytes.
/// The same number `crate::scene_renderer::LIGHTS_UBO_BYTES` computes, restated
/// here because a test may not reach into a `cfg`-gated private constant.
const LIGHTS_UBO_BYTES: usize = 96 + 16 * 32;

/// `copy_texture_to_buffer` requires each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// The absolute tolerance the **physical BRDF** is held to against its `f64`
/// reference — see [`the_physical_brdf_matches_a_transcription_of_the_source_glsl`],
/// which measures the real worst delta on every run and fails if this constant
/// has drifted more than ~100x away from it in either direction.
///
/// Looser than [`crate::surface_program::parity::TOLERANCE`]'s `1e-4` because the
/// two sides are not computing the same thing to the same precision: the shader
/// runs the whole chain in `f32` — an `exp2`, two `sqrt`s, a `normalize` and a
/// division whose denominator can be small — while the reference runs it in
/// `f64`, and the results are radiances that can exceed 1 rather than channels
/// living in `0..=1`.
const PHYSICAL_PARITY_TOLERANCE: f32 = 1.0e-3;

/// One frame's lighting environment, in the shape the uniform packs.
struct Rig {
    caps: u32,
    /// Hemisphere ambient, strength already folded in.
    sky: [f32; 4],
    ground: [f32; 4],
    /// `w` is the maximum fog fraction: `0` makes the whole fog term an exact
    /// no-op, which is what every test but the control uses.
    fog_color: [f32; 4],
    fog_range: [f32; 4],
    /// `xyz` = the camera's world position; `w` = the frame's surface time.
    camera: [f32; 4],
    /// `(v, col)` per light: `v.xyz` is the to-light direction or the world
    /// position, `v.w` the kind (0 directional, 1 point); `col.rgb` the colour
    /// and `col.w` the intensity.
    lights: Vec<([f32; 4], [f32; 4])>,
}

impl Rig {
    /// The rig every documented-result test uses: one directional light pointing
    /// straight up (the surface normal), unit colour and intensity, a dim
    /// hemisphere, an eye directly above so the Blinn-Phong half-vector is the
    /// normal too, and no fog. Every term is exactly one, so a model's result is
    /// a sum of the terms it kept.
    fn unit(caps: u32) -> Rig {
        Rig {
            caps,
            sky: [0.1, 0.1, 0.1, 0.0],
            ground: [0.0, 0.0, 0.0, 0.0],
            // `w = 0`: no fog at all, so a model's pixel is its lighting result.
            fog_color: [0.5, 0.5, 0.5, 0.0],
            fog_range: [0.0, 0.0, 0.0, 0.0],
            camera: [0.0, 10.0, 0.5, 0.0],
            lights: vec![([0.0, 1.0, 0.0, 0.0], [1.0, 1.0, 1.0, 1.0])],
        }
    }

    /// The lighting uniform's 608 bytes, laid out exactly as
    /// `crate::scene_renderer` packs them and as the WGSL `Lights` declares them.
    fn ubo(&self) -> Vec<u8> {
        let header: Vec<u8> = [self.lights.len() as u32, self.caps, 0, 0]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .chain(
                [
                    self.sky,
                    self.ground,
                    self.fog_color,
                    self.fog_range,
                    self.camera,
                ]
                .iter()
                .flatten()
                .flat_map(|lane| lane.to_le_bytes()),
            )
            .collect();
        let items: Vec<u8> = self
            .lights
            .iter()
            .flat_map(|(v, col)| [*v, *col])
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect();
        let mut bytes = header;
        bytes.extend(items);
        bytes.resize(LIGHTS_UBO_BYTES, 0);
        bytes
    }
}

/// One draw's per-vertex + per-instance streams: a full-target triangle at
/// `z = 0.5` with an identity MVP and an identity world, so the single fragment
/// this renders sits at world `(0, 0, 0.5)` with the normal it was given.
fn geometry(normal: [f32; 3], specular: f32, emissive: [f32; 3]) -> (Vec<u8>, Vec<u8>) {
    geometry_with_uv(normal, specular, emissive, SHARED_UV)
}

/// The degenerate uv every lighting test but one uses: all three corners share
/// it, so the screen-space cotangent frame has zero derivatives. That is the
/// case the main pass floors so a flat normal map cannot produce a NaN, and
/// exercising it is deliberate.
const SHARED_UV: [[f32; 2]; 3] = [[0.25, 0.75], [0.25, 0.75], [0.25, 0.75]];

/// A real uv gradient across the triangle, so the cotangent frame is
/// non-degenerate and a tangent-space normal can actually tilt the shading.
///
/// Needed because [`SHARED_UV`] makes the frame degenerate *by design*: with
/// zero uv derivatives `tangent` and `bitangent` are the zero vector, so
/// `mapped` collapses to the geometric normal and **no** tangent-space normal —
/// authored or textured — can move a pixel. A test of the normal channel run on
/// that rig measures the rig, not the shader.
const GRADIENT_UV: [[f32; 2]; 3] = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

fn geometry_with_uv(
    normal: [f32; 3],
    specular: f32,
    emissive: [f32; 3],
    uv: [[f32; 2]; 3],
) -> (Vec<u8>, Vec<u8>) {
    let corners = [[-1.0_f32, -3.0, 0.5], [-1.0, 1.0, 0.5], [3.0, 1.0, 0.5]];
    let vertices: Vec<u8> = corners
        .iter()
        .zip(uv.iter())
        .flat_map(|(position, corner_uv)| {
            [
                position[0],
                position[1],
                position[2],
                normal[0],
                normal[1],
                normal[2],
                corner_uv[0],
                corner_uv[1],
                1.0,
                1.0,
                1.0,
                1.0,
            ]
        })
        .flat_map(f32::to_le_bytes)
        .collect();
    let identity = [
        1.0_f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let instance: Vec<u8> = identity
        .iter()
        .chain(identity.iter())
        .copied()
        .chain([1.0, 1.0, 1.0, 1.0])
        .chain([emissive[0], emissive[1], emissive[2], specular])
        .flat_map(f32::to_le_bytes)
        .collect();
    (vertices, instance)
}

/// Render one fragment of the **main pass** with `program` spliced in and
/// `suffix` as its fragment half, and read the four lanes back.
///
/// `suffix` is a parameter for exactly one reason: the pixel-identity test hands
/// it a reconstruction of the pre-model `fs`. Every other caller hands it
/// `crate::scene_wgsl::SCENE_WGSL_SUFFIX`, the shader the engine ships.
fn render_lit(
    gpu: &ParityGpu,
    suffix: &str,
    program: &str,
    rig: &Rig,
    normal: [f32; 3],
    specular: f32,
    emissive: [f32; 3],
) -> [f32; 4] {
    render_lit_uv(gpu, suffix, program, rig, normal, specular, emissive, SHARED_UV)
}

/// [`render_lit`], choosing the triangle's uv — and therefore whether the
/// cotangent frame is degenerate. See [`GRADIENT_UV`].
#[allow(clippy::too_many_arguments)]
fn render_lit_uv(
    gpu: &ParityGpu,
    suffix: &str,
    program: &str,
    rig: &Rig,
    normal: [f32; 3],
    specular: f32,
    emissive: [f32; 3],
    uv: [[f32; 2]; 3],
) -> [f32; 4] {
    let source = scene_shader(
        crate::scene_wgsl::SCENE_WGSL_PREFIX,
        DEFAULT_DISPLACE_WGSL,
        program,
        suffix,
    );
    let module = gpu
        .compile(&source, 0, SurfaceChannel::BaseColor.bit())
        .expect("the main pass must compile with a generated program spliced in");
    let device = &gpu.device;
    let vertex_attrs = [
        (wgpu::VertexFormat::Float32x3, 0_u64, 0_u32),
        (wgpu::VertexFormat::Float32x3, 12, 1),
        (wgpu::VertexFormat::Float32x2, 24, 2),
        (wgpu::VertexFormat::Float32x4, 32, 3),
    ]
    .map(|(format, offset, shader_location)| wgpu::VertexAttribute {
        format,
        offset,
        shader_location,
    });
    let instance_attrs: Vec<wgpu::VertexAttribute> = (0..10_u32)
        .map(|index| wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: u64::from(index) * 16,
            shader_location: 4 + index,
        })
        .collect();
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axiom-lighting-parity-pipeline"),
        // Derived from the entry points, so the layout under test is the one the
        // shader itself declares — and the skinned pass's group 3 stays out of
        // it, exactly as `build_main_pipeline` leaves it out.
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[
                wgpu::VertexBufferLayout {
                    array_stride: 48,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertex_attrs,
                },
                wgpu::VertexBufferLayout {
                    array_stride: 160,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attrs,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                // No blend: this reads what `fs` RETURNED, which is the thing
                // under test. Compositing is a pipeline decision, not a lighting
                // one, and it is identical for all three models.
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    let (vertices, instance) = geometry_with_uv(normal, specular, emissive, uv);
    let vertex_buffer = buffer(gpu, &vertices, wgpu::BufferUsages::VERTEX);
    let instance_buffer = buffer(gpu, &instance, wgpu::BufferUsages::VERTEX);
    let lights = buffer(gpu, &rig.ubo(), wgpu::BufferUsages::UNIFORM);
    let shadow_uniform = buffer(
        gpu,
        &[1.0_f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
            .iter()
            .flat_map(|lane| lane.to_le_bytes())
            .collect::<Vec<u8>>(),
        wgpu::BufferUsages::UNIFORM,
    );
    // An opaque white albedo and a flat tangent-space normal: the two textures
    // the material group binds, so every capability bit can be turned on without
    // the sampled value deciding the answer.
    let albedo = color_texture(gpu, [255, 255, 255, 255]);
    let normal_map = color_texture(gpu, [128, 128, 255, 255]);
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    let shadow_view = depth_texture(gpu);
    let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        compare: Some(wgpu::CompareFunction::LessEqual),
        ..Default::default()
    });
    let material_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-lighting-parity-material"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&albedo),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&normal_map),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let lights_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-lighting-parity-lights"),
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: lights.as_entire_binding(),
        }],
    });
    let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-lighting-parity-shadow"),
        layout: &pipeline.get_bind_group_layout(2),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&shadow_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: shadow_uniform.as_entire_binding(),
            },
        ],
    });
    // Group 3: the surface parameter region the pass binds now that a generated
    // program can read one. These rigs declare no parameter, so the region is the
    // zero one — but it has to be BOUND, because `fs` names it and the pipeline
    // layout derived from the shader therefore demands it. That the pipeline
    // demands it at all is the proof the binding is live rather than decorative.
    let params = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-lighting-parity-params"),
        size: crate::surface_program::params::SURFACE_PARAM_REGION_BYTES,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let params_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-lighting-parity-params-group"),
        layout: &pipeline.get_bind_group_layout(3),
        entries: &[wgpu::BindGroupEntry {
            binding: crate::surface_program::compile::SURFACE_PARAMS_BINDING,
            resource: params.as_entire_binding(),
        }],
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-lighting-parity-target"),
        size: wgpu::Extent3d {
            width: 1,
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
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-lighting-parity-readback"),
        size: u64::from(ROW_ALIGN),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    // Clear the shadow map to "nothing in front of this fragment" first, so a
    // frame with the shadow capability on reads a defined, fully-lit map rather
    // than whatever the allocation happened to hold.
    encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("axiom-lighting-parity-shadow-clear"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &shadow_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
        .forget_lifetime();
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("axiom-lighting-parity-pass"),
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
        pass.set_bind_group(0, &material_group, &[]);
        pass.set_bind_group(1, &lights_group, &[]);
        pass.set_bind_group(2, &shadow_group, &[]);
        pass.set_bind_group(3, &params_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ROW_ALIGN),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::Wait)
        .expect("the readback must complete");
    let mapped = slice.get_mapped_range();
    [0_usize, 1, 2, 3].map(|lane| {
        let at = lane * 4;
        f32::from_le_bytes([mapped[at], mapped[at + 1], mapped[at + 2], mapped[at + 3]])
    })
}

/// One initialised buffer.
fn buffer(gpu: &ParityGpu, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    wgpu::util::DeviceExt::create_buffer_init(
        &gpu.device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("axiom-lighting-parity-buffer"),
            contents: bytes,
            usage,
        },
    )
}

/// A 1x1 `Rgba8Unorm` texture holding one texel.
fn color_texture(gpu: &ParityGpu, texel: [u8; 4]) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-lighting-parity-texture"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &texel,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// A 1x1 depth texture for the shadow lookup.
fn depth_texture(gpu: &ParityGpu) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-lighting-parity-shadow-map"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// The surface every documented-result test renders: an explicit constant base
/// colour and emission under `model`, so the pixel is a function of the model
/// and the rig alone.
fn surface_of(model: LightingModel) -> Surface {
    SurfaceBuilder::new()
        .lighting(model)
        .constant(
            SurfaceChannel::BaseColor,
            FieldValue::vec4(Vec4::new(0.2, 0.4, 0.6, 1.0)),
        )
        .constant(
            SurfaceChannel::Emission,
            FieldValue::vec4(Vec4::new(0.05, 0.0, 0.0, 0.0)),
        )
        .build()
        .expect("constant channels are legal under every lighting model")
}

/// Assert two pixels agree within the documented tolerance, naming the model.
fn assert_pixel(model: &str, expected: [f32; 4], actual: [f32; 4]) {
    (0..4).for_each(|lane| {
        let delta = (expected[lane] - actual[lane]).abs();
        assert!(
            delta <= crate::surface_program::parity::TOLERANCE,
            "{model} lane {lane}: expected {} got {} (delta {delta})",
            expected[lane],
            actual[lane]
        );
    });
}

/// A real adapter, or a loud failure.
fn gpu() -> ParityGpu {
    let gpu = ParityGpu::acquire();
    assert_ne!(
        gpu.backend,
        wgpu::Backend::Noop,
        "a lighting proof is worthless unless a real backend ran it"
    );
    gpu
}

/// **Each model renders its documented result.** The rig makes every term one,
/// so the three expectations are the three definitions written as arithmetic:
/// `Unlit` keeps only the base and the emission, `Lambert` adds the ambient and
/// the diffuse, `LambertSpecular` adds the highlight on top of that.
#[test]
fn the_three_lighting_models_render_their_documented_results() {
    let gpu = gpu();
    let rig = Rig::unit(CAP_SPECULAR);
    let base = [0.2_f32, 0.4, 0.6];
    let emission = [0.05_f32, 0.0, 0.0];
    // Unlit: base + emission. No ambient, no sun, no highlight.
    let unlit = [
        base[0] + emission[0],
        base[1] + emission[1],
        base[2] + emission[2],
        1.0,
    ];
    // Lambert: + base*sky (ambient, N straight up so the hemisphere is all sky)
    //          + base*light (N.L = 1, unit colour, unit intensity).
    let lambert = [
        base[0] * 0.1 + base[0] + emission[0],
        base[1] * 0.1 + base[1] + emission[1],
        base[2] * 0.1 + base[2] + emission[2],
        1.0,
    ];
    // LambertSpecular: + the light's own colour times a unit highlight. NOT
    // tinted by the base colour — a reflection is light that was not absorbed.
    let lambert_specular = [
        lambert[0] + 1.0,
        lambert[1] + 1.0,
        lambert[2] + 1.0,
        1.0,
    ];
    [
        ("Unlit", LightingModel::Unlit, unlit),
        ("Lambert", LightingModel::Lambert, lambert),
        (
            "LambertSpecular",
            LightingModel::LambertSpecular,
            lambert_specular,
        ),
    ]
    .iter()
    .for_each(|(name, model, expected)| {
        let program = fragment_program(&surface_of(*model)).expect("flattens");
        let actual = render_lit(
            &gpu,
            crate::scene_wgsl::SCENE_WGSL_SUFFIX,
            &program,
            &rig,
            [0.0, 1.0, 0.0],
            1.0,
            [0.0, 0.0, 0.0],
        );
        assert_pixel(name, *expected, actual);
    });
    // And they are three genuinely different pixels, so none of the above is
    // passing by coincidence.
    assert_ne!(unlit[0], lambert[0]);
    assert_ne!(lambert[0], lambert_specular[0]);
}

/// **`Unlit` does not see the lights.** Moving every light, changing their
/// colours and intensities, adding a point light, or removing all of them, leaves
/// an unlit fragment bit-identical — which is the property that makes the model
/// worth having rather than a dim `Lambert`.
#[test]
fn an_unlit_surface_is_unmoved_by_every_light_in_the_frame() {
    let gpu = gpu();
    let program = fragment_program(&surface_of(LightingModel::Unlit)).expect("flattens");
    let render = |rig: &Rig| {
        render_lit(
            &gpu,
            crate::scene_wgsl::SCENE_WGSL_SUFFIX,
            &program,
            rig,
            [0.0, 1.0, 0.0],
            1.0,
            [0.0, 0.0, 0.0],
        )
    };
    let lit = render(&Rig::unit(CAP_SPECULAR));
    // No lights at all.
    let mut dark = Rig::unit(CAP_SPECULAR);
    dark.lights.clear();
    // Three lights, moved, recoloured, one of them a point light beside the
    // fragment — and a brighter sky, which an unlit surface also ignores.
    let mut crowded = Rig::unit(CAP_SPECULAR);
    crowded.sky = [0.9, 0.2, 0.7, 0.0];
    crowded.ground = [0.4, 0.4, 0.1, 0.0];
    crowded.lights = vec![
        ([-0.3, 0.8, 0.5, 0.0], [1.0, 0.2, 0.1, 4.0]),
        ([0.0, 0.25, 0.5, 1.0], [0.1, 1.0, 0.3, 8.0]),
        ([0.6, -0.5, 0.2, 0.0], [0.2, 0.3, 1.0, 2.0]),
    ];
    assert_eq!(lit, render(&dark), "removing every light must change nothing");
    assert_eq!(lit, render(&crowded), "adding lights must change nothing");
    // The same surface under a LIT model does move — so the rig is not simply
    // producing the same pixel for everything.
    let program_lit =
        fragment_program(&surface_of(LightingModel::LambertSpecular)).expect("flattens");
    let moved = render_lit(
        &gpu,
        crate::scene_wgsl::SCENE_WGSL_SUFFIX,
        &program_lit,
        &crowded,
        [0.0, 1.0, 0.0],
        1.0,
        [0.0, 0.0, 0.0],
    );
    assert_ne!(lit, moved);
}

/// **`Lambert` is `LambertSpecular` minus exactly the highlight.** Proved by
/// taking the highlight away the *other* way — clearing the backend's specular
/// capability — and getting `Lambert`'s pixel bit for bit. The model gate and the
/// capability gate multiply, so either one alone is enough, and neither disturbs
/// the diffuse half.
#[test]
fn lambert_is_lambert_specular_with_exactly_the_highlight_removed() {
    let gpu = gpu();
    let render = |model: LightingModel, caps: u32| {
        let program = fragment_program(&surface_of(model)).expect("flattens");
        render_lit(
            &gpu,
            crate::scene_wgsl::SCENE_WGSL_SUFFIX,
            &program,
            &Rig::unit(caps),
            [0.0, 1.0, 0.0],
            1.0,
            [0.0, 0.0, 0.0],
        )
    };
    let lambert = render(LightingModel::Lambert, CAP_SPECULAR);
    let ungated = render(LightingModel::LambertSpecular, 0);
    assert_eq!(
        lambert, ungated,
        "the model gate and the capability gate must remove the same term"
    );
    // A `Lambert` surface stays matte even on a backend that CAN draw a
    // highlight, which is the whole point of the model.
    assert_ne!(lambert, render(LightingModel::LambertSpecular, CAP_SPECULAR));
    // And clearing the capability does not change a surface that never asked.
    assert_eq!(lambert, render(LightingModel::Lambert, 0));
}

/// **The compatibility proof: `LambertSpecular` is pixel-identical to the shader
/// before this work existed.**
///
/// The control is reconstructed from the shipped `fs` by deleting the two gate
/// multiplies and the gate `select` — each deletion asserted to have matched, so
/// the control cannot silently become a copy of the new shader — and both are
/// rendered into the richest rig this pass can draw: every capability bit on,
/// three lights of both kinds, a normal map, a shadow map and real fog. Byte
/// equality is the pass condition, on every lane.
#[test]
fn lambert_specular_reproduces_the_pre_model_shader_pixel_for_pixel() {
    let gpu = gpu();
    let shipped = crate::scene_wgsl::SCENE_WGSL_SUFFIX;
    let (control, edits) = [
        ("select(base.rgb, ambient_lit, gathers)", "ambient_lit"),
        (" * diffuse_gate", ""),
        (" * specular_gate", ""),
    ]
    .iter()
    .fold((String::from(shipped), 0_usize), |(text, edits), (from, to)| {
        let hits = text.matches(from).count();
        assert!(hits > 0, "the control must actually remove `{from}`");
        (text.replace(from, to), edits + hits)
    });
    // Three gate multiplies now, not two: the cloth transmission term is gated
    // by `diffuse_gate` as well, because an UNLIT surface gathers nothing and
    // transmission is a gather. The property this test exists to pin — that
    // every model gate is a MULTIPLIER, never a branch, so control flow stays
    // uniform for the derivative-dependent texture work — is unchanged, and the
    // count moving is the evidence a new gated term was added deliberately.
    assert_eq!(edits, 4, "one gate select and three gate multiplies");
    assert!(!control.contains("diffuse_gate;"));
    let rig = Rig {
        caps: CAP_ALL,
        sky: [0.35, 0.45, 0.7, 0.0],
        ground: [0.18, 0.14, 0.1, 0.0],
        // Real fog, so the depth term runs on both sides too.
        fog_color: [0.6, 0.65, 0.8, 0.75],
        fog_range: [0.1, 0.9, 0.05, 0.0],
        camera: [1.5, 3.0, -4.0, 0.0],
        lights: vec![
            ([0.2, 0.8, 0.4, 0.0], [1.0, 0.95, 0.85, 3.0]),
            ([1.0, 0.5, 0.25, 1.0], [0.2, 0.6, 1.0, 6.0]),
            ([-0.7, 0.3, -0.6, 0.0], [0.9, 0.3, 0.2, 1.5]),
        ],
    };
    // Both the DEFAULT program (what every existing draw runs) and a generated
    // one that states the default model: the compatibility claim covers both.
    [
        DEFAULT_SURFACE_WGSL.to_string(),
        fragment_program(&surface_of(LightingModel::LambertSpecular)).expect("flattens"),
    ]
    .iter()
    .for_each(|program| {
        let after = render_lit(
            &gpu,
            shipped,
            program,
            &rig,
            [0.3, 0.9, -0.2],
            0.8,
            [0.02, 0.03, 0.04],
        );
        let before = render_lit(
            &gpu,
            &control,
            program,
            &rig,
            [0.3, 0.9, -0.2],
            0.8,
            [0.02, 0.03, 0.04],
        );
        assert_eq!(
            after.map(f32::to_bits),
            before.map(f32::to_bits),
            "the default lighting model must be bit-identical to the shader \
             before the model existed"
        );
    });
}

/// **`metallic` is inert under every model but `Physical`.** It is authored,
/// digested, packed and emitted into `SurfaceOut` under all four — and only the
/// physical BRDF reads it. Blinn-Phong has no metal/dielectric split, so a
/// metalness there would have to be invented; `Physical` is where the source
/// puts it, and `the_physical_model_renders_its_documented_result` is where it
/// is proved live.
#[test]
fn metallic_is_inert_under_every_model_but_physical() {
    let gpu = gpu();
    let rig = Rig::unit(CAP_ALL);
    [
        LightingModel::Unlit,
        LightingModel::Lambert,
        LightingModel::LambertSpecular,
    ]
    .iter()
    .for_each(|model| {
        let pixels: Vec<[f32; 4]> = [0.0_f32, 0.5, 1.0]
            .iter()
            .map(|metallic| {
                let surface = SurfaceBuilder::new()
                    .lighting(*model)
                    .constant(
                        SurfaceChannel::BaseColor,
                        FieldValue::vec4(Vec4::new(0.2, 0.4, 0.6, 1.0)),
                    )
                    .constant(
                        SurfaceChannel::Metallic,
                        FieldValue::scalar(Scalar::new(*metallic)),
                    )
                    .build()
                    .expect("a scalar metallic is legal");
                // The channel really is carried: three distinct programs.
                let program = fragment_program(&surface).expect("flattens");
                assert!(program.contains("out.metallic = "));
                render_lit(
                    &gpu,
                    crate::scene_wgsl::SCENE_WGSL_SUFFIX,
                    &program,
                    &rig,
                    [0.0, 1.0, 0.0],
                    1.0,
                    [0.0, 0.0, 0.0],
                )
            })
            .collect();
        assert_eq!(
            pixels[0].map(f32::to_bits),
            pixels[1].map(f32::to_bits),
            "{model:?}: metallic must move no pixel"
        );
        assert_eq!(pixels[1].map(f32::to_bits), pixels[2].map(f32::to_bits));
    });
    // ...and under `Physical` the SAME three surfaces are three different
    // pixels, which is what makes the assertions above a statement about the
    // models rather than about a channel that never arrives.
    let physical: Vec<[f32; 4]> = [0.0_f32, 0.5, 1.0]
        .iter()
        .map(|metallic| {
            let program = fragment_program(&physical_surface(0.5, *metallic)).expect("flattens");
            render_lit(
                &gpu,
                crate::scene_wgsl::SCENE_WGSL_SUFFIX,
                &program,
                &rig,
                [0.0, 1.0, 0.0],
                1.0,
                [0.0, 0.0, 0.0],
            )
        })
        .collect();
    assert_ne!(physical[0].map(f32::to_bits), physical[1].map(f32::to_bits));
    assert_ne!(physical[1].map(f32::to_bits), physical[2].map(f32::to_bits));
}

// ---------------------------------------------------------------------------
// `LightingModel::Physical` — the Cook-Torrance BRDF, on a real adapter.
// ---------------------------------------------------------------------------

/// The base colour every physical test authors. Deliberately not grey: a metal's
/// highlight takes `F0 = base`, so an uneven colour is what makes "the metal's
/// highlight is tinted and the dielectric's is not" a visible fact.
const PHYS_BASE: [f64; 3] = [0.2, 0.4, 0.6];

/// A `Physical` surface with an explicit roughness and metalness.
fn physical_surface(roughness: f32, metallic: f32) -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::Physical)
        .constant(
            SurfaceChannel::BaseColor,
            FieldValue::vec4(Vec4::new(
                PHYS_BASE[0] as f32,
                PHYS_BASE[1] as f32,
                PHYS_BASE[2] as f32,
                1.0,
            )),
        )
        .constant(
            SurfaceChannel::Roughness,
            FieldValue::scalar(Scalar::new(roughness)),
        )
        .constant(
            SurfaceChannel::Metallic,
            FieldValue::scalar(Scalar::new(metallic)),
        )
        .build()
        .expect("scalar roughness and metalness are legal channels")
}

/// **The physical model's documented result, derived rather than transcribed.**
///
/// [`the_physical_brdf_matches_a_transcription_of_the_source_glsl`] below checks
/// the shader against a second reading of the same GLSL, which is worth a lot
/// but shares one risk: one person wrote both, so both can share a misreading.
/// This function closes that by taking a *different route to the same number* —
/// the closed forms the source's own algebra collapses to on the unit rig, where
/// `N`, `L`, `V` and `H` are all `(0, 1, 0)` and every dot product is exactly
/// one:
///
/// * `V_GGX_SmithCorrelated(alpha, 1, 1)` = `0.5 / (1·√(a2 + (1-a2)) + 1·√(…))`
///   = `0.5 / 2` = **exactly `0.25`**, for every roughness. The visibility term
///   drops out of the rig entirely.
/// * `D_GGX(alpha, 1)` = `RECIPROCAL_PI · a2 / ((a2 - 1) + 1)²`
///   = `RECIPROCAL_PI · a2 / a2²` = **`RECIPROCAL_PI / a2`**, i.e. the highlight
///   is inversely proportional to `roughness⁴`. That is the roughness remap made
///   arithmetic: if the shader squared once instead of twice, or remapped
///   `alpha = roughness` instead of `roughness²`, this number is wrong by a
///   factor of `roughness²`.
/// * `F_Schlick(f0, 1, 1)` = `f0·(1 - k) + k` with `k = exp2(-5.55473 - 6.98316)`
///   — a fixed constant, so at normal incidence the Fresnel is `f0` nudged the
///   whole way to white by `1.7e-4`.
///
/// The geometry-roughness term is an exact zero here: all three vertices carry
/// the same normal, so `dpdx(geo_n)` and `dpdy(geo_n)` are zero.
fn unit_rig_physical(roughness: f64, metalness: f64) -> [f64; 3] {
    let reciprocal_pi = 0.318_309_886_183_790_7_f64;
    // `max(roughnessFactor, 0.0525) + 0` then `min(., 1.0)`.
    let material_roughness = roughness.max(0.0525).min(1.0);
    let alpha = material_roughness * material_roughness;
    let a2 = alpha * alpha;
    let d = reciprocal_pi / a2;
    let v = 0.25;
    let fresnel = (-5.55473_f64 - 6.98316).exp2();
    // Direct irradiance is `(1,1,1)`; the hemisphere ambient is `0.1` sky with
    // the normal straight up and nothing in shadow.
    let irradiance_total = 1.0 + 0.1;
    [0, 1, 2].map(|lane| {
        let base = PHYS_BASE[lane];
        let diffuse_color = base * (1.0 - metalness);
        let f0 = 0.04 * (1.0 - metalness) + base * metalness;
        let f = f0 * (1.0 - fresnel) + fresnel;
        irradiance_total * (reciprocal_pi * diffuse_color) + f * (v * d)
    })
}

/// **The physical model renders its documented result, and roughness and
/// metalness are what move it.**
///
/// Three claims in one rig, because they are one claim: that the BRDF is really
/// running on the GPU with the authored channels as its inputs.
///
/// 1. Every (roughness, metalness) pair matches the closed form
///    [`unit_rig_physical`] derives from the source's algebra.
/// 2. Roughening the surface *strictly dims* the highlight — the `1/roughness⁴`
///    dependence, which no Blinn-Phong term has.
/// 3. A metal's highlight is *tinted by the base colour* and a dielectric's is
///    not — the `F0 = mix(0.04, base, metalness)` split, which the engine had no
///    way to express at all before this model.
#[test]
fn the_physical_model_renders_its_documented_result() {
    let gpu = gpu();
    let rig = Rig::unit(CAP_SPECULAR);
    let render = |roughness: f32, metallic: f32| {
        let program = fragment_program(&physical_surface(roughness, metallic)).expect("flattens");
        render_lit(
            &gpu,
            crate::scene_wgsl::SCENE_WGSL_SUFFIX,
            &program,
            &rig,
            [0.0, 1.0, 0.0],
            // A unit legacy specular strength, which this model must ignore.
            1.0,
            [0.0, 0.0, 0.0],
        )
    };
    let cases = [(0.5_f32, 0.0_f32), (0.5, 1.0), (0.9, 0.0), (0.25, 0.5)];
    let worst = cases.iter().fold(0.0_f64, |worst, (roughness, metallic)| {
        let expected = unit_rig_physical(f64::from(*roughness), f64::from(*metallic));
        let actual = render(*roughness, *metallic);
        assert_eq!(actual[3], 1.0, "opacity is untouched by the lighting model");
        (0..3).fold(worst, |worst, lane| {
            let delta = (expected[lane] - f64::from(actual[lane])).abs();
            assert!(
                delta <= f64::from(crate::surface_program::parity::TOLERANCE),
                "roughness {roughness} metalness {metallic} lane {lane}: \
                 expected {} got {} (delta {delta:e})",
                expected[lane],
                actual[lane]
            );
            worst.max(delta)
        })
    });
    // The measurement, asserted so the tolerance's justification cannot rot: the
    // hardware needs far less than the budget it is given.
    assert!(
        worst < 1.0e-6,
        "the closed form and the GPU agreed to {worst:e}; if that has grown, the \
         tolerance above is no longer 100x the hardware's error"
    );

    // (2) Rougher is dimmer, strictly, across the whole authored range.
    let dimming: Vec<f32> = [0.1_f32, 0.3, 0.5, 0.7, 1.0]
        .iter()
        .map(|roughness| render(*roughness, 0.0)[0])
        .collect();
    dimming.windows(2).for_each(|pair| {
        assert!(
            pair[0] > pair[1],
            "roughening a surface must dim its highlight: {} then {}",
            pair[0],
            pair[1]
        );
    });
    // And by a lot, not by drift: 0.1 -> 1.0 roughness is four orders of `alpha`.
    assert!(
        dimming[0] > dimming[4] * 100.0,
        "0.1 roughness gave {} and 1.0 gave {}; a 1/roughness^4 falloff should \
         span far more than that",
        dimming[0],
        dimming[4]
    );

    // (3) The metal's highlight is TINTED. A dielectric's `F0` is a colourless
    // 0.04, so its highlight is the light's own colour and the three lanes'
    // *specular* parts are equal; a metal's `F0` is the base colour, so they are
    // not. Measured as the ratio of the blue to the red lane once the diffuse is
    // gone (metalness 1 leaves specular alone).
    let metal = render(0.3, 1.0);
    let dielectric = render(0.3, 0.0);
    // Subtract the diffuse the closed form says is there, leaving the highlight.
    let dielectric_spec = [0, 1, 2].map(|lane| {
        f64::from(dielectric[lane]) - 1.1 * (three_brdf::RECIPROCAL_PI * PHYS_BASE[lane])
    });
    assert!(
        (dielectric_spec[2] - dielectric_spec[0]).abs() < 1.0e-4,
        "a dielectric's highlight must be colourless: red {} blue {}",
        dielectric_spec[0],
        dielectric_spec[2]
    );
    assert!(
        f64::from(metal[2]) > f64::from(metal[0]) * 2.5,
        "a metal's highlight must carry the base colour's hue: red {} blue {} \
         (the base is {:?})",
        metal[0],
        metal[2],
        PHYS_BASE
    );
}

/// **The legacy instance-stream specular lane is out of the picture under
/// `Physical`.**
///
/// That lane is derived from `Material::roughness` — the *old* per-material
/// number, packed into the emissive vec4's fourth component — and it is what
/// drives the Blinn-Phong highlight for the other three models. The physical
/// model must not consult it: its gloss comes from the authored `Roughness`
/// channel, which is the whole point of making that channel live. Sweeping the
/// lane across its full range must move nothing, while sweeping the *channel*
/// moves a lot.
#[test]
fn the_legacy_specular_lane_does_not_reach_the_physical_model() {
    let gpu = gpu();
    let rig = Rig::unit(CAP_ALL);
    let program = fragment_program(&physical_surface(0.4, 0.0)).expect("flattens");
    let pixels: Vec<[f32; 4]> = [0.0_f32, 0.5, 1.0]
        .iter()
        .map(|lane| {
            render_lit(
                &gpu,
                crate::scene_wgsl::SCENE_WGSL_SUFFIX,
                &program,
                &rig,
                [0.0, 1.0, 0.0],
                *lane,
                [0.0, 0.0, 0.0],
            )
        })
        .collect();
    assert_eq!(pixels[0].map(f32::to_bits), pixels[1].map(f32::to_bits));
    assert_eq!(pixels[1].map(f32::to_bits), pixels[2].map(f32::to_bits));
    // The same sweep on a `LambertSpecular` surface is three different pixels,
    // so the lane is genuinely reaching the shader and this is a statement about
    // the model rather than about a dead vertex attribute.
    let legacy = fragment_program(&surface_of(LightingModel::LambertSpecular)).expect("flattens");
    let moved: Vec<[f32; 4]> = [0.0_f32, 0.5, 1.0]
        .iter()
        .map(|lane| {
            render_lit(
                &gpu,
                crate::scene_wgsl::SCENE_WGSL_SUFFIX,
                &legacy,
                &rig,
                [0.0, 1.0, 0.0],
                *lane,
                [0.0, 0.0, 0.0],
            )
        })
        .collect();
    assert_ne!(moved[0].map(f32::to_bits), moved[1].map(f32::to_bits));
    assert_ne!(moved[1].map(f32::to_bits), moved[2].map(f32::to_bits));
}

/// **A GGX highlight is not a Blinn-Phong one — and the authored roughness is
/// what makes it not one.**
///
/// The engine's Blinn-Phong lobe has a single hard-coded width, `SPECULAR_POWER
/// = 48.0`; the instance-stream lane it reads scales that lobe and cannot
/// reshape it. So "is this really a new BRDF or an expensive rename?" has an
/// exact answer: measure how much of its peak each lobe keeps 30 degrees off the
/// half-vector, and check that the GGX one **brackets** the Blinn-Phong one —
/// tighter than `cos^48` at low roughness, broader at high. A rename cannot land
/// on both sides of a fixed exponent.
///
/// Each highlight is isolated before it is measured, because a whole pixel also
/// carries diffuse and ambient:
///
/// * the Blinn-Phong one by subtracting the same frame with `CAP_SPECULAR`
///   cleared, which `lambert_is_lambert_specular_with_exactly_the_highlight_removed`
///   establishes removes exactly that term and nothing else;
/// * the physical one by authoring metalness 1, which zeroes `diffuseColor` and
///   therefore both of the model's diffuse terms, leaving the pixel *equal* to
///   its highlight.
#[test]
fn a_ggx_lobes_width_is_the_authored_roughness_not_a_fixed_exponent() {
    let gpu = gpu();
    let straight = [0.0_f32, 1.0, 0.0];
    // 30 degrees off. Both `L` and `V` are `(0, 1, 0)` on the unit rig, so the
    // half-vector is too, and tilting the normal walks `N·H` to `cos 30`.
    let tilted = [0.5_f32, 0.866_025_4, 0.0];
    let render = |program: &str, caps: u32, normal: [f32; 3]| {
        f64::from(
            render_lit(
                &gpu,
                crate::scene_wgsl::SCENE_WGSL_SUFFIX,
                program,
                &Rig::unit(caps),
                normal,
                1.0,
                [0.0, 0.0, 0.0],
            )[0],
        )
    };
    let phong = fragment_program(&surface_of(LightingModel::LambertSpecular)).expect("flattens");
    let phong_highlight = |normal: [f32; 3]| render(&phong, CAP_SPECULAR, normal) - render(&phong, 0, normal);
    let phong_falloff = phong_highlight(tilted) / phong_highlight(straight);
    // `cos^48(cos 30°)` — the fixed lobe, and the number the engine had before.
    assert!(
        (phong_falloff - 0.866_025_4_f64.powi(48)).abs() < 1.0e-4,
        "the control must be the cos^48 lobe itself, and it measured {phong_falloff:e}"
    );
    let ggx_falloff = |roughness: f32| {
        let program = fragment_program(&physical_surface(roughness, 1.0)).expect("flattens");
        render(&program, CAP_SPECULAR, tilted) / render(&program, CAP_SPECULAR, straight)
    };
    let tight = ggx_falloff(0.1);
    let broad = ggx_falloff(0.6);
    assert!(
        tight < phong_falloff * 0.1,
        "a roughness-0.1 GGX lobe must be far TIGHTER than cos^48: it kept \
         {tight:e} and cos^48 kept {phong_falloff:e}"
    );
    assert!(
        broad > phong_falloff * 10.0,
        "a roughness-0.6 GGX lobe must be far BROADER than cos^48: it kept \
         {broad:e} and cos^48 kept {phong_falloff:e}"
    );
}

/// three.js r180's physical BRDF, transcribed a **second** time — from the same
/// GLSL text, into `f64` Rust — so the WGSL has something to disagree with.
///
/// Sources: `ShaderChunk/common.glsl.js` (`BRDF_Lambert`, `F_Schlick`,
/// `RECIPROCAL_PI`, `EPSILON`, `pow2`, `saturate`) and
/// `ShaderChunk/lights_physical_pars_fragment.glsl.js`
/// (`V_GGX_SmithCorrelated`, `D_GGX`, `BRDF_GGX`). The source's grouping is
/// reproduced: no division is rewritten as a reciprocal-multiply, and
/// `F * ( V * D )` keeps its parentheses.
mod three_brdf {
    pub(super) const RECIPROCAL_PI: f64 = 0.318_309_886_183_790_7;
    const EPSILON: f64 = 1.0e-6;

    fn pow2(x: f64) -> f64 {
        x * x
    }

    /// `#define saturate( a ) clamp( a, 0.0, 1.0 )` — GLSL's `clamp` is
    /// `min( max( x, minVal ), maxVal )`, written out rather than trusting
    /// `f64::clamp`'s ordering.
    fn saturate(a: f64) -> f64 {
        a.max(0.0).min(1.0)
    }

    pub(super) fn brdf_lambert(diffuse_color: [f64; 3]) -> [f64; 3] {
        diffuse_color.map(|lane| RECIPROCAL_PI * lane)
    }

    fn f_schlick(f0: [f64; 3], f90: f64, dot_vh: f64) -> [f64; 3] {
        let fresnel = ((-5.55473 * dot_vh - 6.98316) * dot_vh).exp2();
        f0.map(|lane| lane * (1.0 - fresnel) + (f90 * fresnel))
    }

    fn v_ggx_smith_correlated(alpha: f64, dot_nl: f64, dot_nv: f64) -> f64 {
        let a2 = pow2(alpha);
        let gv = dot_nl * (a2 + (1.0 - a2) * pow2(dot_nv)).sqrt();
        let gl = dot_nv * (a2 + (1.0 - a2) * pow2(dot_nl)).sqrt();
        0.5 / (gv + gl).max(EPSILON)
    }

    fn d_ggx(alpha: f64, dot_nh: f64) -> f64 {
        let a2 = pow2(alpha);
        let denom = pow2(dot_nh) * (a2 - 1.0) + 1.0;
        RECIPROCAL_PI * a2 / pow2(denom)
    }

    pub(super) fn brdf_ggx(
        light_dir: [f64; 3],
        view_dir: [f64; 3],
        normal: [f64; 3],
        f0: [f64; 3],
        f90: f64,
        roughness: f64,
    ) -> [f64; 3] {
        let alpha = pow2(roughness);
        let half_dir = normalize(add(light_dir, view_dir));
        let dot_nl = saturate(dot(normal, light_dir));
        let dot_nv = saturate(dot(normal, view_dir));
        let dot_nh = saturate(dot(normal, half_dir));
        let dot_vh = saturate(dot(view_dir, half_dir));
        let f = f_schlick(f0, f90, dot_vh);
        let v = v_ggx_smith_correlated(alpha, dot_nl, dot_nv);
        let d = d_ggx(alpha, dot_nh);
        f.map(|lane| lane * (v * d))
    }

    pub(super) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    pub(super) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    }

    pub(super) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    pub(super) fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
        [a[0] * k, a[1] * k, a[2] * k]
    }

    pub(super) fn normalize(a: [f64; 3]) -> [f64; 3] {
        scale(a, 1.0 / dot(a, a).sqrt())
    }
}

/// **CPU/GPU parity for the physical BRDF, off-axis, on three lights of both
/// kinds.**
///
/// The unit rig above collapses `V` to a constant and every dot product to one,
/// which is exactly what makes its expectations hand-derivable — and exactly what
/// leaves `V_GGX_SmithCorrelated`'s two `sqrt` terms, `D_GGX`'s denominator and
/// `F_Schlick`'s `dotVH` untested as *functions of their arguments*. This rig
/// gives every one of them a different, awkward value: a tilted normal, an
/// off-axis eye, two directional lights and one point light with real distance
/// attenuation.
///
/// The reference is [`three_brdf`], read from the source GLSL rather than from
/// the WGSL, plus the parts of the frame that are the engine's own (hemisphere
/// ambient, the point attenuation curve).
#[test]
fn the_physical_brdf_matches_a_transcription_of_the_source_glsl() {
    let gpu = gpu();
    // No shadows and no normal map, so `N` is the geometric normal and `atten`
    // is the light's own: the terms under test are the only variables left.
    let rig = Rig {
        caps: CAP_SPECULAR,
        sky: [0.35, 0.45, 0.7, 0.0],
        ground: [0.18, 0.14, 0.1, 0.0],
        fog_color: [0.6, 0.65, 0.8, 0.0],
        fog_range: [0.0, 0.0, 0.0, 0.0],
        camera: [1.5, 3.0, -4.0, 0.0],
        lights: vec![
            ([0.2, 0.8, 0.4, 0.0], [1.0, 0.95, 0.85, 3.0]),
            ([1.0, 0.5, 0.25, 1.0], [0.2, 0.6, 1.0, 6.0]),
            ([-0.7, 0.3, -0.6, 0.0], [0.9, 0.3, 0.2, 1.5]),
        ],
    };
    // The one fragment this rig rasterizes: the 1x1 target's centre is NDC
    // (0, 0), and the MVP and world matrices are both the identity.
    let world_pos = [0.0_f64, 0.0, 0.5];
    let vertex_normal = [0.3_f32, 0.9, -0.2];
    let normal = three_brdf::normalize(vertex_normal.map(f64::from));
    let view_dir = three_brdf::normalize(three_brdf::sub(
        [rig.camera[0], rig.camera[1], rig.camera[2]].map(f64::from),
        world_pos,
    ));
    let reference = |roughness: f64, metalness: f64| -> [f64; 3] {
        let base = PHYS_BASE;
        let diffuse_color = base.map(|lane| lane * (1.0 - metalness));
        // No geometry roughness: the three vertices share a normal, so both
        // derivatives of `geo_n` are exactly zero.
        let material_roughness = roughness.max(0.0525).min(1.0);
        let f0 = [0, 1, 2].map(|lane| 0.04 * (1.0 - metalness) + base[lane] * metalness);
        // `RE_IndirectDiffuse_Physical` over the engine's hemisphere ambient,
        // which is three's `getHemisphereLightIrradiance` term for term. The
        // frame casts no shadow, so `ambient_shade` is exactly 1.
        let hemi_weight = (normal[1] * 0.5 + 0.5).clamp(0.0, 1.0);
        let hemi = [0, 1, 2].map(|lane| {
            f64::from(rig.ground[lane]) * (1.0 - hemi_weight)
                + f64::from(rig.sky[lane]) * hemi_weight
        });
        let indirect_diffuse = {
            let lambert = three_brdf::brdf_lambert(diffuse_color);
            [0, 1, 2].map(|lane| hemi[lane] * lambert[lane])
        };
        let (total_diffuse, total_specular) = rig.lights.iter().fold(
                (indirect_diffuse, [0.0_f64; 3]),
                |(diffuse_sum, specular_sum), (v, col)| {
                    let is_point = v[3] > 0.5;
                    let (light_dir, atten) = [
                        (three_brdf::normalize([v[0], v[1], v[2]].map(f64::from)), 1.0),
                        {
                            let d = three_brdf::sub([v[0], v[1], v[2]].map(f64::from), world_pos);
                            let dist = three_brdf::dot(d, d).sqrt();
                            (
                                three_brdf::scale(d, 1.0 / dist.max(0.0001)),
                                1.0 / (1.0 + 0.09 * dist + 0.032 * dist * dist),
                            )
                        },
                    ][usize::from(is_point)];
                    let light_color = [0, 1, 2]
                        .map(|lane| f64::from(col[lane]) * f64::from(col[3]) * atten);
                    let dot_nl = three_brdf::dot(normal, light_dir).max(0.0).min(1.0);
                    let irradiance = three_brdf::scale(light_color, dot_nl);
                    let ggx = three_brdf::brdf_ggx(
                        light_dir,
                        view_dir,
                        normal,
                        f0,
                        1.0,
                        material_roughness,
                    );
                    let lambert = three_brdf::brdf_lambert(diffuse_color);
                    (
                        [0, 1, 2].map(|lane| diffuse_sum[lane] + irradiance[lane] * lambert[lane]),
                        [0, 1, 2]
                            .map(|lane| specular_sum[lane] + irradiance[lane] * ggx[lane]),
                    )
                },
            );
        // `outgoingLight = totalDiffuse + totalSpecular + totalEmissiveRadiance`,
        // three's own combination — with no emission authored here.
        [0, 1, 2].map(|lane| total_diffuse[lane] + total_specular[lane])
    };
    let worst = [
        (0.15_f64, 0.0_f64),
        (0.4, 0.0),
        (0.4, 1.0),
        (0.75, 0.35),
        (1.0, 1.0),
    ]
    .iter()
    .fold(0.0_f64, |worst, (roughness, metalness)| {
        let program = fragment_program(&physical_surface(*roughness as f32, *metalness as f32))
            .expect("flattens");
        let actual = render_lit(
            &gpu,
            crate::scene_wgsl::SCENE_WGSL_SUFFIX,
            &program,
            &rig,
            vertex_normal,
            1.0,
            [0.0, 0.0, 0.0],
        );
        // The authored channel is an `f32`, so the reference must be given the
        // same number the GPU got, not the `f64` literal it was written from.
        let expected = reference(f64::from(*roughness as f32), f64::from(*metalness as f32));
        (0..3).fold(worst, |worst, lane| {
            let delta = (expected[lane] - f64::from(actual[lane])).abs();
            assert!(
                delta <= f64::from(PHYSICAL_PARITY_TOLERANCE),
                "roughness {roughness} metalness {metalness} lane {lane}: \
                 the source says {} and the GPU said {} (delta {delta:e})",
                expected[lane],
                actual[lane]
            );
            worst.max(delta)
        })
    });
    // **The measurement the tolerance is derived from, re-taken every run.**
    // The per-lane assertions above already prove the budget covers the hardware;
    // this proves the budget is not a number fitted to a miss. A tolerance more
    // than 10x looser than the hardware needs is itself a failure, so the
    // declared constant must sit inside one decade of the worst real delta.
    //
    // Measured 2.97e-4 on a Vulkan adapter, against a declared 1.0e-3 — 3.4x. The
    // gap is not the transcription: it is `D_GGX`'s denominator,
    // `dotNH² · (a2 - 1) + 1`, which is a catastrophic cancellation when `a2` is
    // small and `dotNH` is near 1, so an `f32` shader and an `f64` reference part
    // company there by far more than either one's own epsilon. The low-roughness
    // case in the sweep is deliberately kept for exactly that reason.
    assert!(
        worst > 0.0,
        "an f32 GPU and an f64 reference agreeing to the bit means the reference \
         is not being evaluated"
    );
    assert!(
        f64::from(PHYSICAL_PARITY_TOLERANCE) <= worst * 10.0,
        "the worst delta measured {worst:e}, so the declared tolerance of {} is \
         more than 10x looser than the hardware needs; tighten it",
        PHYSICAL_PARITY_TOLERANCE
    );
}

/// An authored `Normal` channel reaches the lighting stage and changes the lit
/// result.
///
/// **This is the regression test for a defect that shipped precisely because it
/// had none.** `fs` used to resolve the normal with two `select`s reading the
/// same `CAP_NORMALMAP` bit and taking *opposite arms*: with the bit set the
/// tangent-space normal came from the texture and `surface.normal` was unused;
/// with it clear `surface.normal` was read into `nmap` and then `N` took the
/// geometric normal anyway. So an authored normal was computed on every path and
/// used on none, and `SurfaceChannel::NormalFromHeight` was dead on arrival.
///
/// Nothing caught it because every test asserted what the normal *map* did.
/// This one asserts what the **channel** does, which is the thing that was
/// broken.
#[test]
fn an_authored_normal_channel_actually_lights_differently() {
    let gpu = gpu();
    // Normal-mapping OFF, so the only tangent-space tilt available is the
    // authored one. Under the old code this configuration provably could not
    // move a pixel.
    let rig = Rig::unit(CAP_ALL & !CAP_NORMALMAP);
    let lit = |normal: Vec3| -> [f32; 4] {
        let surface = SurfaceBuilder::new()
            .lighting(LightingModel::Lambert)
            .constant(
                SurfaceChannel::BaseColor,
                FieldValue::vec4(Vec4::new(0.8, 0.8, 0.8, 1.0)),
            )
            .constant(SurfaceChannel::Normal, FieldValue::vec3(normal))
            .build()
            .expect("a vec3 normal is legal");
        let program = fragment_program(&surface).expect("flattens");
        assert!(program.contains("out.normal = "), "the channel must be carried");
        // The VALUE must reach the program too, not just the assignment — a
        // constant that flattened back to the default would make the render
        // comparison below vacuous.
        assert!(
            program.contains(&format!("{:?}", normal.x)) || normal.x == 0.0,
            "the authored x must appear in the program:
{program}"
        );
        render_lit_uv(
            &gpu,
            crate::scene_wgsl::SCENE_WGSL_SUFFIX,
            &program,
            &rig,
            // The vertex normal must AGREE with the triangle's plane. Every
            // other test here passes `(0, 1, 0)` while the triangle lies at
            // constant z, which makes `dp2` parallel to `geo_n`, so
            // `r1 = cross(dp2, geo_n)` is the zero vector and the x-tangent
            // vanishes. On that rig a tilt along x provably cannot move a
            // pixel — the first draft of this test used one and reported the
            // fix as broken. `(0, 0, 1)` gives a frame with both axes real.
            [0.0, 0.0, 1.0],
            0.0,
            [0.0, 0.0, 0.0],
            // A REAL uv gradient. On the shared-uv rig every other test uses,
            // the cotangent frame is degenerate and no tangent-space normal can
            // move a pixel at all.
            GRADIENT_UV,
        )
    };

    // The tangent-space identity must still be the geometric normal, to the bit.
    // That is the half of the old behaviour worth keeping, and the reason the
    // gate is now "is there any tilt" rather than "is a map bound".
    let flat = lit(Vec3::new(0.0, 0.0, 1.0));
    // A real tilt must light differently, and this one is chosen to be
    // unmissable: the light is `(0, 1, 0)` and the geometric normal is
    // `(0, 0, 1)`, so a flat surface takes **zero** diffuse and any tilt toward
    // the light adds some. The difference is the whole diffuse term, not a
    // fraction of it.
    let tilted = lit(Vec3::new(0.0, 0.8, 0.6));
    assert_ne!(
        flat.map(f32::to_bits),
        tilted.map(f32::to_bits),
        "an authored normal that changes no pixel is the defect this test exists          for: it means `surface.normal` is not reaching the lighting stage",
    );
    // And it is a *shading* difference, not noise.
    let moved = (0..3).map(|i| (flat[i] - tilted[i]).abs()).fold(0.0_f32, f32::max);
    assert!(
        moved > 0.01,
        "the authored tilt moved the lit result by only {moved:e}; that is drift,          not a normal reaching the light",
    );
}

/// The WGSL codes are `axiom_surface::LightingModel`'s discriminants — the same
/// kind of contract test the capability bits already have, and the one that
/// fails loudly if the wire order is ever reshuffled.
#[test]
fn the_wgsl_lighting_codes_are_the_surface_layers_discriminants() {
    let prelude = crate::surface_program::wgsl_template::SURFACE_PRELUDE_WGSL;
    [
        ("AXIOM_LIGHT_UNLIT", LightingModel::Unlit),
        ("AXIOM_LIGHT_LAMBERT", LightingModel::Lambert),
        (
            "AXIOM_LIGHT_LAMBERT_SPECULAR",
            LightingModel::LambertSpecular,
        ),
        ("AXIOM_LIGHT_PHYSICAL", LightingModel::Physical),
    ]
    .iter()
    .for_each(|(name, model)| {
        let declaration = format!("const {name}: u32 = {}u;", model.code());
        assert!(prelude.contains(&declaration), "missing `{declaration}`");
    });
    // The default program returns the DEFAULT model's code, which is what keeps
    // every draw that authored no surface rendering exactly as it did.
    assert!(DEFAULT_SURFACE_WGSL.contains(&format!(
        "return {}u;",
        LightingModel::default().code()
    )));
    // And the main pass spends the code on `select` and multiplies — never on a
    // branch, and never on a second entry point.
    let suffix = crate::scene_wgsl::SCENE_WGSL_SUFFIX;
    assert_eq!(suffix.matches("@fragment").count(), 1);
    assert!(suffix.contains("let model = axiom_lighting_model();"));
    assert!(suffix.contains("select(base.rgb, ambient_lit, gathers)"));
    // The fourth model is spent the same way: one more `select` on a VALUE, in
    // the same entry point. Not a second `@fragment`, not a second module.
    assert!(suffix.contains("model == AXIOM_LIGHT_PHYSICAL"));
    assert_eq!(
        suffix.matches("let model = axiom_lighting_model();").count(),
        1
    );
}
