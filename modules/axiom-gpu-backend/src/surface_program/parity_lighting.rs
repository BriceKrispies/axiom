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
use axiom_math::Vec4;
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
    let corners = [[-1.0_f32, -3.0, 0.5], [-1.0, 1.0, 0.5], [3.0, 1.0, 0.5]];
    let vertices: Vec<u8> = corners
        .iter()
        .flat_map(|position| {
            [
                position[0],
                position[1],
                position[2],
                normal[0],
                normal[1],
                normal[2],
                // One shared uv, which also makes the tangent frame degenerate —
                // the case the main pass floors so a flat normal map cannot
                // produce a NaN. Exercising it here is deliberate.
                0.25,
                0.75,
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
    let (vertices, instance) = geometry(normal, specular, emissive);
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
    assert_eq!(edits, 3, "one gate select and two gate multiplies");
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

/// **`metallic` is reserved and inert.** It is authored, digested, packed and
/// emitted into `SurfaceOut` — and it moves no pixel, under any model. That is
/// deliberate (`SPEC-11`: *"Resist PBR scope creep"*), and a channel that looks
/// wired but is not is worse than one documented dead, so it is pinned here.
#[test]
fn metallic_is_reserved_and_changes_no_pixel() {
    let gpu = gpu();
    let rig = Rig::unit(CAP_ALL);
    LightingModel::ALL.iter().for_each(|model| {
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
}
