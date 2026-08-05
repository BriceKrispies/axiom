//! The GPU **post chain**: bright-pass → separable blur → tonemapped, graded composite.
//!
//! This is the pass that makes a bright thing read as a *light*. Before it, the
//! GPU path had no post-process at all — `FramePostProcess` and
//! `FrameVolumetrics` are CPU loops over a read-back framebuffer, which the
//! Canvas 2D backend and the offscreen capture can afford and a live swap-chain
//! frame cannot. The scene went straight from the render target to the surface,
//! so an emissive material was simply a brighter patch of paint.
//!
//! It replaces [`crate::upscale::UpscaleBlit`] rather than sitting alongside it:
//! the composite is already a fullscreen triangle sampling the intermediate
//! target with a linear filter, so it upscales for free and there is no reason to
//! pay for two fullscreen passes. A frame carrying no bloom still goes through
//! this path, with an intensity of zero — the composite is then an exact copy,
//! which is what keeps a bloom-less frame byte-identical to the old blit.
//!
//! # The 8-bit ceiling, stated plainly
//!
//! The intermediate colour target is 8-bit sRGB (it matches the surface), so a
//! fragment that emitted `4.0` was already clamped to white *before* this chain
//! samples it. The headroom that a bloom would ideally spend is therefore gone:
//! everything above white blooms by the same amount, rather than a 4× light
//! blooming four times as hard as a 1× one.
//!
//! The full fix is an `Rgba16Float` intermediate, and it is deliberately not
//! taken here: half-float **render targets** are not guaranteed on the WebGL2
//! downlevel limits this engine deliberately requests on *both* browser arms
//! (`live_gpu_binding` asks for `downlevel_webgl2_defaults` even under WebGPU, to
//! keep the two in parity), so it would be a capability split exactly where the
//! engine has worked hard not to have one. Thresholding the clamped buffer still
//! produces the soft halo that is the point; it just cannot rank two blown
//! highlights against each other. [`axiom_host::FrameBloom::tonemap`] still earns
//! its keep on the *composite*, where source + bloom genuinely exceeds one.
//!
//! # The colour grade rides the composite
//!
//! [`axiom_host::FramePostProcess`] is the engine's one whole-frame grade, and
//! [`axiom_host::apply_frame_postprocess`] is its definition — a CPU loop over a
//! finished RGBA8 buffer. Every arm that *owns* its pixels as bytes runs exactly
//! that: the Canvas 2D software raster and the off-screen capture. A live
//! swap-chain frame never becomes bytes, so before this the grade simply did not
//! reach the browser — an app authored one, saw it in every capture, and
//! presented ungraded. That is a backend divergence, not a cost decision.
//!
//! The composite is where it belongs, because the composite is already the
//! fullscreen pass over the finished image. Two things make it the *same* grade
//! rather than a lookalike:
//!
//! - **Space.** The CPU stage grades display-encoded bytes (`byte / 255`). The
//!   render targets here are sRGB, so `textureSample` hands the shader *linear*
//!   values and the store re-encodes. `graded` therefore encodes to sRGB, applies
//!   the identical arithmetic, and decodes back — the round trip is what keeps
//!   the two arms the same picture instead of the same formula on different
//!   numbers.
//! - **Order.** It runs last, after the bloom composite and its rolloff, exactly
//!   as the CPU stage runs on the composited frame.
//!
//! An unauthored grade packs the identity (exposure 1, neutral balance, contrast
//! 1, saturation 1, black point 0), so a frame that authors none still presents
//! byte-identically to one from before the grade rode this pass.

use axiom_host::{FrameBloom, FramePostProcess};

/// Bright-pass, blur and composite, sharing one fullscreen-triangle vertex stage.
///
/// The three fragment entry points are in one module so they share the vertex
/// shader and one uniform layout; `params` is reinterpreted per pass rather than
/// each pass carrying its own buffer, because they never run concurrently.
const POST_WGSL: &str = r#"
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var out: VsOut;
    out.clip = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uv[vi];
    return out;
}

struct Params {
    // x = threshold, y = knee, z = intensity, w = rolloff knee.
    tune: vec4<f32>,
    // xy = the blur step in UV (radius * texel, along one axis), zw unused.
    step: vec4<f32>,
    // The frame's colour grade: x = exposure, y = contrast, z = saturation,
    // w = black point. The identity (1, 1, 1, 0) is packed when the app
    // authored none, so the composite's grade is then an exact no-op.
    grade: vec4<f32>,
    // rgb = the grade's per-channel white-balance gain; w unused.
    balance: vec4<f32>,
};
@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;
@group(1) @binding(0) var bloom_tex: texture_2d<f32>;
@group(1) @binding(1) var bloom_sampler: sampler;

// Rec.709 luminance — the same weighting `axiom_host::luminance` uses, so
// "bright" means one thing on both sides of the boundary.
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The quadratic knee from `FrameBloom::contribution`, mirrored exactly. A hard
// `> threshold` cut would draw a contour line across any smooth gradient that
// crosses it — and a night sky is exactly such a gradient.
fn contribution(l: f32) -> f32 {
    let lum = max(l, 0.0);
    let knee = max(params.tune.y, 1e-4);
    let soft = clamp(lum - params.tune.x + knee, 0.0, 2.0 * knee);
    let curved = soft * soft / (4.0 * knee);
    let surplus = max(curved, lum - params.tune.x);
    return clamp(surplus / max(lum, 1e-4), 0.0, 1.0);
}

// `FrameBloom::tonemap`, mirrored: identity below the knee, reciprocal shoulder
// above it, so the composite never clips to a flat white.
fn rolloff(x: f32) -> f32 {
    let v = max(x, 0.0);
    let over = max(v - params.tune.w, 0.0);
    let span = 1.0 - params.tune.w;
    return clamp(min(v, params.tune.w) + span * over / (span + over), 0.0, 1.0);
}

@fragment
fn fs_bright(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_sampler, in.uv).rgb;
    // Scale the pixel by how much of it blooms, so a bloomed highlight keeps its
    // own hue instead of turning white.
    return vec4<f32>(c * contribution(luma(c)), 1.0);
}

// Nine-tap Gaussian along `params.step`. Two of these (horizontal then vertical)
// give a separable 9x9 for the cost of 18 taps rather than 81.
@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4<f32> {
    let w = array<f32, 5>(0.2270270270, 0.1945945946, 0.1216216216, 0.0540540541, 0.0162162162);
    var sum = textureSample(src_tex, src_sampler, in.uv).rgb * w[0];
    for (var i: i32 = 1; i < 5; i = i + 1) {
        let o = params.step.xy * f32(i);
        sum = sum + textureSample(src_tex, src_sampler, in.uv + o).rgb * w[i];
        sum = sum + textureSample(src_tex, src_sampler, in.uv - o).rgb * w[i];
    }
    return vec4<f32>(sum, 1.0);
}

// Linear <-> sRGB, so the grade below runs on the same display-encoded numbers
// `axiom_host::apply_frame_postprocess` reads out of a byte buffer. The render
// targets are sRGB, so what this shader samples and stores is linear; grading
// there instead would be a different curve wearing the same parameters.
fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = v * 12.92;
    let hi = 1.055 * pow(v, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, v <= vec3<f32>(0.0031308));
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = v / 12.92;
    let hi = pow((v + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, v <= vec3<f32>(0.04045));
}

// The frame's colour grade, term for term the same chain as the CPU
// `grade_pixel`: black-point floor removal, then exposure x white balance, then
// the contrast S-curve about 0.5, then saturation about Rec.709 luma.
fn graded(linear: vec3<f32>) -> vec3<f32> {
    let d = srgb_encode(linear);
    // A floor is a subtract, and `max` is what makes it a floor rather than a
    // sign flip; `1 - black` is floored so a degenerate black point of 1.0
    // cannot divide by zero.
    let f = max((d - vec3<f32>(params.grade.w)) / max(1.0 - params.grade.w, 1e-6), vec3<f32>(0.0));
    let e = f * params.grade.x * params.balance.rgb;
    let k = (e - vec3<f32>(0.5)) * params.grade.y + vec3<f32>(0.5);
    let l = dot(k, vec3<f32>(0.2126, 0.7152, 0.0722));
    let s = vec3<f32>(l) + (k - vec3<f32>(l)) * params.grade.z;
    return srgb_decode(s);
}

@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    let scene = textureSample(src_tex, src_sampler, in.uv).rgb;
    let glow = textureSample(bloom_tex, bloom_sampler, in.uv).rgb * params.tune.z;
    let sum = scene + glow;
    // Rolled off per channel rather than on luminance: a saturated light that
    // blows one channel should desaturate toward white as it gets brighter,
    // which is what a real overexposed highlight does.
    let rolled = vec3<f32>(rolloff(sum.r), rolloff(sum.g), rolloff(sum.b));
    // The grade runs last, on the composited image — the same place, and the
    // same arithmetic, as the CPU stage the read-back arms run.
    return vec4<f32>(graded(rolled), 1.0);
}
"#;

/// How much smaller the bloom working targets are than the scene.
///
/// Half resolution: the blur is a low-frequency effect, so the detail thrown away
/// is detail the blur was about to destroy anyway, and it quarters the cost of
/// every tap. It also widens the effective radius for free, which is why the
/// authored radius can stay small.
const BLOOM_DOWNSCALE: u32 = 2;

/// The post chain's pipelines and its two half-resolution ping-pong targets.
pub(crate) struct PostChain {
    bright: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    /// Two uniform buffers, one per blur axis. **Not one buffer rewritten
    /// between passes**: `queue.write_buffer` is ordered against the encoder's
    /// *submission*, not against the passes inside it, so three writes to one
    /// buffer would all land before any pass ran and both blur passes would read
    /// the last one — blurring horizontally twice and never vertically. The bug
    /// is invisible in a still of a symmetric highlight, which is exactly why it
    /// is worth a buffer rather than a comment.
    params_h: wgpu::Buffer,
    params_v: wgpu::Buffer,
    /// Source bind group for each stage, in the order the stages run.
    scene_group: wgpu::BindGroup,
    ping_group: wgpu::BindGroup,
    pong_group: wgpu::BindGroup,
    /// The blurred bloom, as the composite's second texture.
    bloom_group: wgpu::BindGroup,
    ping_view: wgpu::TextureView,
    pong_view: wgpu::TextureView,
    bloom_size: (u32, u32),
}

impl std::fmt::Debug for PostChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostChain")
            .field("bloom_size", &self.bloom_size)
            .finish_non_exhaustive()
    }
}

impl PostChain {
    /// Build the chain for a scene target of `size` presented to `target_format`.
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        scene_format: wgpu::TextureFormat,
        scene_view: &wgpu::TextureView,
        size: (u32, u32),
    ) -> PostChain {
        let bloom_size = (
            (size.0 / BLOOM_DOWNSCALE).max(1),
            (size.1 / BLOOM_DOWNSCALE).max(1),
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-post-chain"),
            source: wgpu::ShaderSource::Wgsl(POST_WGSL.into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-post-sampler"),
            // Clamped, so a tap that walks off the edge repeats the border pixel
            // rather than wrapping a bright corner around to the far side.
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let make_params = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                // Four `vec4`s: tune, step, grade, balance — see `Params` in the WGSL.
                size: std::mem::size_of::<[f32; 16]>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let params_h = make_params("axiom-post-params-h");
        let params_v = make_params("axiom-post-params-v");

        let source_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-post-source"),
            entries: &[
                texture_entry(0),
                sampler_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bloom_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-post-bloom"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });

        let (ping_view, pong_view) = [0, 1].map(|i| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("axiom-bloom-target"),
                    size: wgpu::Extent3d {
                        width: bloom_size.0,
                        height: bloom_size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: scene_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor {
                    label: Some(["axiom-bloom-ping", "axiom-bloom-pong"][i]),
                    ..Default::default()
                })
        })
        .into();

        let source_group = |view: &wgpu::TextureView, label: &str, params: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &source_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params.as_entire_binding(),
                    },
                ],
            })
        };
        let bloom_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-post-bloom-group"),
            layout: &bloom_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&ping_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let one = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-post-layout"),
            bind_group_layouts: &[&source_layout],
            push_constant_ranges: &[],
        });
        let two = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-post-composite-layout"),
            bind_group_layouts: &[&source_layout, &bloom_layout],
            push_constant_ranges: &[],
        });
        let pipeline = |layout: &wgpu::PipelineLayout, entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-post-pipeline"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(format.into())],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };

        PostChain {
            bright: pipeline(&one, "fs_bright", scene_format),
            blur: pipeline(&one, "fs_blur", scene_format),
            composite: pipeline(&two, "fs_composite", target_format),
            // The scene and ping sources carry the horizontal step; the pong
            // source — read only by the vertical blur — carries the vertical one.
            scene_group: source_group(scene_view, "axiom-post-scene", &params_h),
            ping_group: source_group(&ping_view, "axiom-post-ping", &params_h),
            pong_group: source_group(&pong_view, "axiom-post-pong", &params_v),
            bloom_group,
            ping_view,
            pong_view,
            bloom_size,
            params_h,
            params_v,
        }
    }

    /// Record the whole chain: scene → bright → blur H → blur V → composite into
    /// `target`, with the frame's `bloom` and colour `grade`.
    ///
    /// A frame with no bloom runs the same four passes with an intensity of zero,
    /// so the composite reduces to a copy of the scene; a frame with no grade
    /// packs the grade identity, so the composite's grade is an exact no-op.
    /// Keeping the shape identical is deliberate: a frame that toggles either one
    /// on and off does not change which pipelines exist, so it cannot stutter on a
    /// pipeline the driver compiles the first time it is used.
    ///
    /// `grade` is the **live** arm's channel. An arm that reads its pixels back and
    /// runs [`axiom_host::apply_frame_postprocess`] over them passes `None` here —
    /// one grade per frame, whichever arm applies it.
    pub(crate) fn record(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        bloom: Option<&FrameBloom>,
        grade: Option<&FramePostProcess>,
    ) {
        let tune = bloom.map_or(
            // No bloom: a threshold above any possible luminance means the bright
            // pass yields black, and a zero intensity means the composite adds
            // nothing. Belt and braces, so neither alone has to be trusted.
            [f32::MAX, 1.0, 0.0, axiom_host::rolloff_knee().get()],
            |b| {
                [
                    b.threshold().get(),
                    b.knee().get(),
                    b.intensity().get(),
                    axiom_host::rolloff_knee().get(),
                ]
            },
        );
        let radius = bloom.map_or(0.0, |b| b.radius().get());
        let texel = (
            radius / self.bloom_size.0 as f32,
            radius / self.bloom_size.1 as f32,
        );
        // The grade identity when the app authored none: unit exposure, unit
        // contrast, unit saturation, zero black point, neutral balance.
        let (tone, balance) = grade.map_or(([1.0, 1.0, 1.0, 0.0], [1.0, 1.0, 1.0, 0.0]), |g| {
            let wb = g.white_balance();
            (
                [
                    g.exposure().get(),
                    g.contrast().get(),
                    g.saturation().get(),
                    g.black_point().get(),
                ],
                [wb[0], wb[1], wb[2], 0.0],
            )
        });

        let mut pass = |pipeline: &wgpu::RenderPipeline,
                        group: &wgpu::BindGroup,
                        view: &wgpu::TextureView,
                        second: Option<&wgpu::BindGroup>| {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-post-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, group, &[]);
            second.into_iter().for_each(|g| rp.set_bind_group(1, g, &[]));
            rp.draw(0..3, 0..1);
        };

        // Both buffers are written up front — which is fine precisely *because*
        // they are two buffers: each pass reads the one bound to it, so the write
        // ordering that would break a single shared buffer is irrelevant here.
        // The composite reads `params_h` (via `scene_group`), so the grade must
        // reach that buffer; it is written to both so the two stay one layout.
        let pack = |step: [f32; 2]| {
            [
                tune[0], tune[1], tune[2], tune[3], step[0], step[1], 0.0, 0.0, tone[0], tone[1],
                tone[2], tone[3], balance[0], balance[1], balance[2], balance[3],
            ]
        };
        queue.write_buffer(&self.params_h, 0, bytemuck::cast_slice(&pack([texel.0, 0.0])));
        queue.write_buffer(&self.params_v, 0, bytemuck::cast_slice(&pack([0.0, texel.1])));
        // scene → ping (bright) → pong (blur H) → ping (blur V) → target.
        pass(&self.bright, &self.scene_group, &self.ping_view, None);
        pass(&self.blur, &self.ping_group, &self.pong_view, None);
        pass(&self.blur, &self.pong_group, &self.ping_view, None);
        pass(
            &self.composite,
            &self.scene_group,
            target,
            Some(&self.bloom_group),
        );
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
