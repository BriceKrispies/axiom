//! The target-agnostic scene renderer shared by every GPU arm.
//!
//! This owns the *one* definition of how Axiom draws a frame: the WGSL shaders,
//! the vertex/instance buffer layouts, the material (albedo) + lighting + shadow
//! bind groups, the mesh/material caches, the directional **shadow-map depth
//! pre-pass**, and the per-frame instance packing + draw loop. It records into a
//! caller-supplied colour + depth [`wgpu::TextureView`] and knows nothing about
//! *where* those come from — a swap-chain surface (the wasm
//! [`crate::live_gpu_binding`]) or an off-screen texture read back to a PNG (the
//! native [`crate::offscreen`]). Both arms run byte-identical rendering; there is
//! no second hand-synced copy of the pipeline to drift.

use std::collections::HashMap;

use axiom_host::SdfScene;
use wgpu::util::DeviceExt;

// Floats per instance (`mvp(16) + world(16) + colour(4) + emissive(3)+pad(1)`) —
// owned by `frame_packet_adapter`, which does the packing this renderer's vertex
// layout is derived from.
use crate::frame_packet_adapter::INSTANCE_FLOATS;
use crate::mip_chain;
use crate::scene_wgsl::{SCENE_WGSL_PREFIX, SCENE_WGSL_SUFFIX};
use crate::surface_program::wgsl_template;


/// WGSL for the **sky pass**: a fullscreen triangle drawn before the scene that
/// evaluates the frame's [`axiom_host::FrameSky`] per pixel.
///
/// This is the arithmetic of [`axiom_host::FrameSky::radiance`], mirrored. That
/// function is the definition and this is the copy; the Rust side is the one
/// with tests, and every constant here (`MIN_ANGULAR_RADIUS`, `LIMB_SOFTNESS`,
/// `HAZE_HEIGHT_MIN`/`HAZE_HEIGHT_MAX`) is pinned to its value by
/// `sky_shader_constants_match_the_host_definition`.
///
/// Why a pass and not a clear colour: a flat clear cannot be a light. A night
/// scene whose only light is a directional lamp plus a hemisphere ambient reads
/// as "dark" rather than "moonlit" however carefully the values are tuned,
/// because nothing in frame *is* the source. Putting the moon on screen — and
/// giving the horizon a colour distinct from the zenith for fog to fade into —
/// is what the eye reads as moonlight.
///
/// The pixel's world ray comes from the inverse view-projection: unproject the
/// pixel at the near and far planes and take the difference. The renderer
/// inverts the camera matrix itself (via `axiom_math::Mat4::inverse`) rather
/// than making every caller supply an inverse, so an app authoring a sky supplies
/// only the sky.
const SKY_WGSL: &str = r#"
struct SkyU {
    inv_view_proj: mat4x4<f32>,
    // rgb = zenith colour; w unused.
    zenith: vec4<f32>,
    // rgb = horizon colour; w = the gradient's haze height (the up-component at
    // which it stands halfway to the zenith). Carried with the horizon stop
    // because it is that stop's reach: it says how far up the haze holds.
    horizon: vec4<f32>,
    // xyz = unit direction toward the body; w = its angular radius (radians).
    body: vec4<f32>,
    // rgb = the body's colour; w = the halo's cosine exponent.
    body_color: vec4<f32>,
    // x = halo strength; yzw unused.
    halo: vec4<f32>,
    // x = cloud coverage (0 = clear sky); y = the cloud field's scale; zw unused.
    cloud: vec4<f32>,
};
@group(0) @binding(0) var<uniform> sky: SkyU;

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> SkyOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: SkyOut;
    out.clip = vec4<f32>(pos[vi], 1.0, 1.0);
    out.ndc = pos[vi];
    return out;
}

// `FrameSky::cloud_octave`, mirrored: a separable sinusoid on a rotated lattice,
// remapped to 0..1.
fn cloud_octave(p: vec2<f32>, rotation: f32, frequency: f32) -> f32 {
    let sin_r = sin(rotation);
    let cos_r = cos(rotation);
    let x = (p.x * cos_r + p.y * sin_r) * frequency;
    let y = (p.y * cos_r - p.x * sin_r) * frequency;
    return sin(x) * sin(y) * 0.5 + 0.5;
}

// `FrameSky::cloud_field`, mirrored: the four `CLOUD_OCTAVES` summed by weight.
// Written out rather than looped, which is both what keeps it branch-free and what
// makes the weights visibly sum to exactly 1.0 — the property that pins the field
// to 0..1 and gives the coverage threshold exact ends.
fn cloud_field(p: vec2<f32>) -> f32 {
    return 0.50 * cloud_octave(p, 0.00, 1.00)
         + 0.25 * cloud_octave(p, 1.13, 2.31)
         + 0.15 * cloud_octave(p, 2.47, 4.73)
         + 0.10 * cloud_octave(p, 3.71, 9.17);
}

// `FrameSky::radiance`, mirrored. Branch-free, exactly as the Rust is — which is
// what made it portable here unchanged.
@fragment
fn fs(in: SkyOut) -> @location(0) vec4<f32> {
    // Unproject the pixel at two depths and take the difference: the world-space
    // ray through it, independent of where the camera is.
    let near = sky.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let far = sky.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - near.xyz / near.w);

    // The vertical gradient, smoothstepped so the horizon band is soft rather
    // than a seam. Below the horizon it holds the horizon colour: there is no
    // ground hemisphere here, because the ground is geometry.
    //
    // `FrameSky::haze_lift`, mirrored: the up-component is first reshaped so the
    // gradient's midpoint lands at the authored haze height instead of at a fixed
    // 0.5, which is what lets a near-level camera see more than the bottom of the
    // curve. HAZE_HEIGHT_MIN/MAX = 0.02/0.98 clamp away the two degenerate ends.
    //
    // Four arithmetic operations, and deliberately not `pow`: at the default
    // height of 0.5 the coefficient is 1 and this collapses to `up / (up + 1 - up)`
    // — exactly `up`, on this side as on the host's, so a sky that authors no haze
    // height is bit-for-bit the plain smoothstep this was. `pow(x, 1.0)` is
    // `exp2(1.0 * log2(x))` in WGSL and carries no such guarantee.
    let up = clamp(dir.y, 0.0, 1.0);
    let haze_h = clamp(sky.horizon.w, 0.02, 0.98);
    let haze_k = haze_h / (1.0 - haze_h);
    let lifted = up / (up + (1.0 - up) * haze_k);
    let blend = lifted * lifted * (3.0 - 2.0 * lifted);
    let gradient = sky.horizon.rgb * (1.0 - blend) + sky.zenith.rgb * blend;

    let cos_angle = dot(dir, sky.body.xyz);
    // The disc: a smooth step across the limb so the edge does not alias, sized
    // as a fraction of the radius so a bigger body gets a proportionally softer
    // edge.
    let limb = max(sky.body.w, 1.0e-4);
    let inner = cos(limb * 0.75);
    let outer = cos(limb);
    let span = max(abs(inner - outer), 1.1920929e-7);
    let disc = clamp((cos_angle - outer) / span, 0.0, 1.0);
    // The halo: the angular cosine raised to a power, so it falls off around the
    // body without a second radius to keep in sync.
    let halo = pow(max(cos_angle, 0.0), max(sky.body_color.w, 1.0)) * sky.halo.x;
    let behind = gradient + sky.body_color.rgb * (disc + halo);

    // The cloud layer, sampled on a plane one unit overhead rather than on the
    // dome, so the lumps foreshorten toward the horizon as real cumulus do. The up
    // component is floored (CLOUD_HORIZON_FLOOR) so a grazing ray lands somewhere
    // finite, and the density is faded across that same band (CLOUD_HORIZON_FADE)
    // so the layer dissolves into the haze rather than ending on a seam.
    let reach = max(sky.cloud.y, 0.0) / max(dir.y, 0.06);
    let field = cloud_field(vec2<f32>(dir.x, dir.z) * reach);
    // CLOUD_EDGE = 0.22. Threshold 1.0 at zero coverage — which the field, whose
    // maximum is exactly 1.0, cannot beat — so a clear sky is exactly clear.
    let threshold = 1.0 - clamp(sky.cloud.x, 0.0, 1.0) * (1.0 + 0.22);
    let shaped = clamp((field - threshold) / 0.22, 0.0, 1.0);
    let fade = clamp(dir.y / 0.10, 0.0, 1.0);
    let density = (shaped * shaped * (3.0 - 2.0 * shaped)) * (fade * fade * (3.0 - 2.0 * fade));

    // The cloud carries no colour of its own: CLOUD_FILL_GAIN = 1.6 of the sky
    // behind it fills its shaded body, and a broad forward lobe about the body
    // (CLOUD_SUN_GAIN = 0.35, CLOUD_FORWARD = 6.0) lights its sunward face. Mixed
    // rather than added, so cloud in front of the body occludes the disc.
    let sunlit = pow(max(cos_angle, 0.0), 6.0) * 0.35;
    let cloud = gradient * 1.6 + sky.body_color.rgb * sunlit;

    return vec4<f32>(mix(behind, cloud, density), 1.0);
}
"#;

/// The sky uniform's size in bytes: a `mat4x4` (64) plus six `vec4`s (96).
const SKY_UBO_BYTES: u64 = 64 + 6 * 16;

/// The fullscreen sky pass: its pipeline and the uniform it reads.
///
/// Held as a whole rather than as loose fields so "the look carries no sky" is
/// one `Option`, and a frame without one keeps the flat clear colour with no
/// pipeline built and no pass recorded.
#[derive(Debug)]
struct SkyPass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    sky: axiom_host::FrameSky,
}

impl SkyPass {
    /// Build the sky pipeline for a colour target of `format`.
    ///
    /// The pipeline declares the main pass's depth format with **writes off and
    /// an `Always` compare**, so the triangle fills every pixel behind the scene
    /// and then leaves the depth buffer exactly as it found it — the scene draws
    /// over it by ordinary depth testing, and nothing is occluded by the sky.
    /// Declaring the depth state (rather than `None`) is required: it is drawn
    /// inside the main pass, whose attachments every pipeline in it must match.
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        sky: axiom_host::FrameSky,
    ) -> SkyPass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-sky"),
            source: wgpu::ShaderSource::Wgsl(SKY_WGSL.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-sky-layout"),
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-sky-uniform"),
            size: SKY_UBO_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-sky-bind-group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-sky-pipeline-layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        SkyPass {
            pipeline: device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-sky-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    targets: &[Some(format.into())],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            }),
            uniform,
            bind_group,
            sky,
        }
    }
}

/// The camera's world position, recovered from the frame's view-projection alone.
///
/// The specular term is view-dependent, so the fragment stage needs the eye — but
/// no caller supplies one: the packet carries matrices, and every existing render
/// path was built around a shading model that never asked. Rather than widen that
/// contract all the way out to the apps, the eye is derived here from what the
/// frame already carries.
///
/// It is **not** the fourth column of the inverse, which is the tempting
/// one-liner and is wrong: that unprojects to a point on the centre ray at the
/// *near plane*, off by the near distance (measurably — it lands ~0.13 units out
/// on a 0.1-near camera). The eye is where every view ray converges, which is
/// exactly where clip `w` vanishes, so this unprojects the centre ray and
/// intersects it with the `w = 0` plane. That is exact for any perspective
/// projection and needs no knowledge of the fov, aspect, or near/far.
///
/// A degenerate or orthographic view-projection has no such convergence point
/// (`w` is constant, so the intersection divides by zero). The fallback is the
/// world origin: a wrong highlight direction, never a NaN that would poison every
/// lit pixel in the frame.
fn camera_eye(camera_view_proj: [f32; 16]) -> [f32; 3] {
    let m = camera_view_proj;
    axiom_math::Mat4::from_cols_array(m)
        .inverse()
        .map(|inv| {
            let unproject = |z: f32| {
                let p = inv.transform_vec4(axiom_math::Vec4::new(0.0, 0.0, z, 1.0));
                [p.x / p.w, p.y / p.w, p.z / p.w]
            };
            let near = unproject(0.0);
            let far = unproject(1.0);
            let dir = [0, 1, 2].map(|c| far[c] - near[c]);
            // clip.w as an affine function of world position: the eye is its root.
            let w_at = |p: [f32; 3]| m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15];
            let w_near = w_at(near);
            let w_slope = m[3] * dir[0] + m[7] * dir[1] + m[11] * dir[2];
            let t = -w_near / w_slope;
            let eye = [0, 1, 2].map(|c| near[c] + t * dir[c]);
            [0, 1, 2]
                .map(|c| [0.0, eye[c]][usize::from(eye[c].is_finite())])
        })
        .unwrap_or([0.0; 3])
}

/// Pack a [`axiom_host::FrameSky`] plus the camera's inverse view-projection
/// into the std140 layout `SkyU` describes.
///
/// A camera matrix that cannot be inverted (a degenerate projection) falls back
/// to the identity, which yields a usable — if wrong — ray rather than a NaN
/// that would poison every pixel of the frame. This is the same defensive
/// posture `FrameSky::normalize_or` takes on the Rust side.
fn pack_sky(sky: &axiom_host::FrameSky, camera_view_proj: [f32; 16]) -> Vec<u8> {
    let inv = axiom_math::Mat4::from_cols_array(camera_view_proj)
        .inverse()
        .unwrap_or(axiom_math::Mat4::IDENTITY)
        .as_cols_array();
    let dir = sky.body_direction();
    let (zenith, horizon, color) = (sky.zenith(), sky.horizon(), sky.body_color());
    let mut bytes = Vec::with_capacity(SKY_UBO_BYTES as usize);
    inv.iter()
        .chain(
            [
                zenith[0],
                zenith[1],
                zenith[2],
                0.0,
                horizon[0],
                horizon[1],
                horizon[2],
                sky.haze_height().get(),
                dir[0],
                dir[1],
                dir[2],
                sky.body_angular_radius().get(),
                color[0],
                color[1],
                color[2],
                sky.halo_falloff().get(),
                sky.halo_strength().get(),
                0.0,
                0.0,
                0.0,
                sky.cloud_coverage().get(),
                sky.cloud_scale().get(),
                0.0,
                0.0,
            ]
            .iter(),
        )
        .for_each(|f| bytes.extend_from_slice(&f.to_le_bytes()));
    bytes
}

/// WGSL for the shadow depth pre-pass: project each instance through the light
/// view-projection and the per-instance world matrix; depth-only, no fragment
/// output. Reads only position (per-vertex) and the world columns (per-instance).
const SHADOW_WGSL: &str = r#"
struct ShadowU { light_vp: mat4x4<f32> };
@group(0) @binding(0) var<uniform> shadow: ShadowU;

@vertex
fn vs(
    @location(0) position: vec3<f32>,
    @location(1) w0: vec4<f32>,
    @location(2) w1: vec4<f32>,
    @location(3) w2: vec4<f32>,
    @location(4) w3: vec4<f32>,
) -> @builtin(position) vec4<f32> {
    let world = mat4x4<f32>(w0, w1, w2, w3);
    return shadow.light_vp * world * vec4<f32>(position, 1.0);
}
"#;

/// WGSL for the **SDF raymarch pass**: a fullscreen-triangle vertex shader plus a
/// fragment shader that reconstructs each pixel's world ray (from the SDF
/// uniform's `inv_view_proj` + `camera_world_pos`), marches the primitive list
/// (sphere/box/plane, kind-dispatched, evaluated in each primitive's local frame
/// via its `inv_transform` and rescaled by the uniform scale in `params.w`),
/// shades the hit with the **shared lights UBO** (group 1, the same set the mesh
/// pass binds), and writes `@builtin(frag_depth)` = the hit's NDC z (through the
/// same `view_proj` the mesh pass uses) so the depth test composites it against
/// the triangle meshes. This is the GPU mirror of the canvas2d backend's
/// branchless CPU marcher; the data both read is the host's `SdfScene`, so the
/// two backends stay in parity. (WGSL is not held to the Rust Branchless Law, so
/// this shader uses ordinary `for`/`break`/`if` control flow.)
const SDF_WGSL: &str = r#"
struct Light {
    v: vec4<f32>,
    col: vec4<f32>,
};
struct Lights {
    count: u32,
    // The frame's backend capability mask, the same lane the mesh pass reads as
    // `caps` (it was `_pad0` here while nothing in this pass needed it).
    caps: u32,
    _pad1: u32,
    _pad2: u32,
    // Hemisphere ambient (rgb; w unused), strength folded in — a plain mix, no scale.
    sky: vec4<f32>,
    ground: vec4<f32>,
    // The frame's depth fog — this pass binds the SAME lights UBO as the mesh pass,
    // so its `Lights` declaration must stay layout-identical. rgb = fog colour,
    // w = maximum mix fraction; `fog_range.xy` = start / full-density NDC depth,
    // `fog_range.z` = the extinction rate per world metre.
    fog_color: vec4<f32>,
    fog_range: vec4<f32>,
    // Layout parity with the mesh pass's `camera` lane (unread here — the SDF
    // pass has its own camera in `SdfU`, but the shared buffer must still match).
    camera: vec4<f32>,
    items: array<Light, 16>,
};
@group(1) @binding(0) var<uniform> lights: Lights;

const CAP_AERIAL: u32 = 2048u;

// The marched hit's atmosphere: the mesh pass's `fog_factor`, unchanged, so a
// raymarched surface and a triangle at the same depth AND the same distance
// recede by exactly the same amount. The march gives the distance for free —
// `t` is the metres of ray already travelled — so this pass needs no extra data
// to evaluate the air term.
fn fog_factor(ndc_depth: f32, view_distance: f32) -> f32 {
    let span = max(abs(lights.fog_range.y - lights.fog_range.x), 1e-6);
    let screen = clamp((ndc_depth - lights.fog_range.x) / span, 0.0, 1.0);
    let rate = max(lights.fog_range.z, 0.0) * f32((lights.caps & CAP_AERIAL) != 0u);
    let air = 1.0 - exp2(-rate * max(view_distance, 0.0));
    let combined = 1.0 - (1.0 - screen) * (1.0 - air);
    return combined * clamp(lights.fog_color.w, 0.0, 1.0);
}

struct SdfPrim {
    inv_transform: mat4x4<f32>,
    params: vec4<f32>,
    color: vec4<f32>,
    kind: u32,
};
struct SdfU {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    camera_world_pos: vec4<f32>,
    march: vec4<f32>,
    count: u32,
    prims: array<SdfPrim, 16>,
};
@group(0) @binding(0) var<uniform> sdf: SdfU;

const MARCH_STEPS: u32 = 96u;
const AMBIENT: f32 = 0.15;
const GRAD_H: f32 = 0.002;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

// A single oversized triangle covering the viewport; its clip xy IS the NDC xy,
// so the interpolated `ndc` gives each fragment its pixel-centre NDC.
@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var verts = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    let p = verts[vi];
    var out: VsOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.ndc = p;
    return out;
}

// Unproject a clip-space point to world space (clip->world then perspective
// divide) — the GPU peer of the CPU marcher's `unproject`.
fn unproject(ndc_x: f32, ndc_y: f32, ndc_z: f32) -> vec3<f32> {
    let world = sdf.inv_view_proj * vec4<f32>(ndc_x, ndc_y, ndc_z, 1.0);
    return world.xyz / world.w;
}

fn box_distance(p: vec3<f32>, params: vec4<f32>) -> f32 {
    let q = abs(p) - params.xyz;
    let outside = length(max(q, vec3<f32>(0.0)));
    let inside = min(max(q.x, max(q.y, q.z)), 0.0);
    return outside + inside;
}

fn local_distance(kind: u32, p: vec3<f32>, params: vec4<f32>) -> f32 {
    if (kind == 0u) {
        return length(p) - params.x;
    }
    if (kind == 1u) {
        return box_distance(p, params);
    }
    return p.y;
}

// One primitive's signed distance: transform the world point into the
// primitive's local frame, evaluate the canonical local SDF, rescale by the
// transform's uniform scale (`params.w`).
fn primitive_distance(i: u32, p: vec3<f32>) -> f32 {
    let prim = sdf.prims[i];
    let local = (prim.inv_transform * vec4<f32>(p, 1.0)).xyz;
    return local_distance(prim.kind, local, prim.params) * prim.params.w;
}

fn scene_distance(p: vec3<f32>) -> f32 {
    var best = 1e30;
    for (var i: u32 = 0u; i < sdf.count; i = i + 1u) {
        best = min(best, primitive_distance(i, p));
    }
    return best;
}

fn scene_color(p: vec3<f32>) -> vec4<f32> {
    var best = 1e30;
    var col = vec4<f32>(0.0);
    for (var i: u32 = 0u; i < sdf.count; i = i + 1u) {
        let d = primitive_distance(i, p);
        if (d < best) {
            best = d;
            col = sdf.prims[i].color;
        }
    }
    return col;
}

fn surface_normal(p: vec3<f32>) -> vec3<f32> {
    let dx = scene_distance(p + vec3<f32>(GRAD_H, 0.0, 0.0)) - scene_distance(p - vec3<f32>(GRAD_H, 0.0, 0.0));
    let dy = scene_distance(p + vec3<f32>(0.0, GRAD_H, 0.0)) - scene_distance(p - vec3<f32>(0.0, GRAD_H, 0.0));
    let dz = scene_distance(p + vec3<f32>(0.0, 0.0, GRAD_H)) - scene_distance(p - vec3<f32>(0.0, 0.0, GRAD_H));
    return normalize(vec3<f32>(dx, dy, dz));
}

// One light's scalar diffuse term (the CPU marcher ignores light colour in the
// SDF path, using only intensity): directional uses its to-light direction with
// unit attenuation; point uses the direction to its world position with
// inverse-square attenuation.
fn light_diffuse(l: Light, n: vec3<f32>, hit: vec3<f32>) -> f32 {
    let intensity = l.col.w;
    let is_point = l.v.w > 0.5;
    let to = l.v.xyz - hit;
    let dist = length(to);
    let dir = select(normalize(l.v.xyz), to / max(dist, 0.0001), is_point);
    let atten = select(1.0, 1.0 / (1.0 + dist * dist), is_point);
    return max(dot(n, dir), 0.0) * intensity * atten;
}

fn shade(surface: vec4<f32>, n: vec3<f32>, hit: vec3<f32>) -> vec4<f32> {
    var lit = AMBIENT;
    for (var i: u32 = 0u; i < lights.count; i = i + 1u) {
        lit = lit + light_diffuse(lights.items[i], n, hit);
    }
    lit = min(lit, 1.0);
    return vec4<f32>(surface.rgb * lit, surface.a);
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs(in: VsOut) -> FsOut {
    let origin = sdf.camera_world_pos.xyz;
    let on_ray = unproject(in.ndc.x, in.ndc.y, 0.0);
    let dir = normalize(on_ray - origin);
    let max_dist = sdf.march.x;
    let eps = sdf.march.y;
    var t = 0.0;
    var hit = false;
    for (var i: u32 = 0u; i < MARCH_STEPS; i = i + 1u) {
        let p = origin + dir * t;
        let d = scene_distance(p);
        if (d < eps) {
            hit = true;
            break;
        }
        if (t > max_dist) {
            break;
        }
        t = t + d;
    }
    if (!hit) {
        discard;
    }
    let p = origin + dir * t;
    let clip = sdf.view_proj * vec4<f32>(p, 1.0);
    if (clip.w <= 1e-6) {
        discard;
    }
    let surface = scene_color(p);
    let n = surface_normal(p);
    var out: FsOut;
    let shaded = shade(surface, n, p);
    let ndc_depth = clip.z / clip.w;
    // Same atmosphere as the mesh pass, keyed on the hit's own NDC depth and on
    // `t` — the metres the ray marched to reach it — so a marched surface and a
    // triangle at the same distance recede by the same amount.
    out.color = vec4<f32>(
        mix(shaded.rgb, lights.fog_color.rgb, fog_factor(ndc_depth, t)),
        shaded.a,
    );
    out.depth = ndc_depth;
    return out;
}
"#;

/// The main pass's whole shader source: the two halves of [`SCENE_WGSL_PREFIX`]
/// / [`SCENE_WGSL_SUFFIX`] with the **default** surface program spliced between
/// them — the identity over the instance lanes and the exact zero offset.
///
/// This is the pipeline a draw naming `surface_program == 0` runs, and the one a
/// draw naming a program the preparation barrier never prepared falls back to. A
/// draw naming a **prepared** program runs a pipeline built from the same two
/// halves with that surface's generated program spliced in instead — see
/// `crate::surface_program::compile`.
fn scene_shader_source() -> String {
    wgsl_template::scene_shader(
        SCENE_WGSL_PREFIX,
        wgsl_template::DEFAULT_DISPLACE_WGSL,
        wgsl_template::DEFAULT_SURFACE_WGSL,
        SCENE_WGSL_SUFFIX,
    )
}

/// Depth-buffer format used by both the camera depth and the shadow map.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Maximum lights uploaded per frame (must match the WGSL `array<Light, 16>`).
const MAX_LIGHTS: usize = 16;
/// Lighting uniform size in bytes: an 80-byte header (count + caps + padding, the
/// hemisphere-ambient `sky` + `ground` `vec4`s, then the depth-fog `fog_color` +
/// `fog_range` `vec4`s, then the `camera` `vec4`) plus `MAX_LIGHTS` × two `vec4`s
/// (32 bytes each) —
/// std140-compatible. Both WGSL `Lights` declarations (the mesh pass and the SDF
/// pass, which bind the same buffer) must match this layout.
const LIGHTS_UBO_BYTES: u64 = 96 + (MAX_LIGHTS as u64) * 32;
/// Maximum SDF primitives uploaded per frame (must match the WGSL
/// `array<SdfPrim, 16>`). Primitives beyond this are dropped, the same honesty
/// the lights path uses — see [`pack_sdf`].
const MAX_SDF_PRIMITIVES: usize = 16;
/// One packed SDF primitive's std140 size: `inv_transform` mat4 (64) + `params`
/// vec4 (16) + `color` vec4 (16) + `kind` u32 padded to 16 = 112 bytes.
const SDF_PRIM_BYTES: u64 = 64 + 16 + 16 + 16;
/// SDF uniform size in bytes: a 176-byte header (`view_proj` 64 + `inv_view_proj`
/// 64 + `camera_world_pos` 16 + `march` 16 + `count` padded to 16) then
/// `MAX_SDF_PRIMITIVES` primitives. std140-compatible.
const SDF_UBO_BYTES: u64 = 176 + (MAX_SDF_PRIMITIVES as u64) * SDF_PRIM_BYTES;
/// Bytes per instance.
const INSTANCE_STRIDE: u64 = (INSTANCE_FLOATS as u64) * 4;
/// Bytes per vertex: position(3) + normal(3) + uv(2) + colour(4) = 12 f32.
const VERTEX_STRIDE: u64 = 12 * 4;
/// Byte offset of the world matrix within an instance (after the 16-float mvp).
const WORLD_OFFSET: u64 = 16 * 4;

/// Bytes per **skinned** vertex: the 12 standard floats + joints(4) + weights(4).
const SKINNED_VERTEX_STRIDE: u64 = 20 * 4;
/// Floats per **skinned** instance: mvp(16) + world(16) + colour(4) + joint_base(4).
const SKINNED_INSTANCE_FLOATS: usize = 40;
const SKINNED_INSTANCE_STRIDE: u64 = (SKINNED_INSTANCE_FLOATS as u64) * 4;
/// How many RGBA32F texels wide the joint-palette texture is. One matrix is four
/// texels, so a row holds 64 matrices; 256 texels is a 4 KiB row, which is
/// already a multiple of the 256-byte row alignment `write_texture` requires.
const PALETTE_ROW_TEXELS: u32 = 256;

/// How many rows the palette texture needs to hold [`PALETTE_CAP`] matrices.
const fn palette_rows() -> u32 {
    ((PALETTE_CAP as u32) * 4).div_ceil(PALETTE_ROW_TEXELS)
}

/// Upload this frame's packed palette into the top rows of the palette texture.
///
/// Only the rows the frame actually uses are written - a crowd of ten bodies
/// costs ten bodies' worth of upload, not the whole capacity - so padding to a
/// whole row is the only waste.
fn write_palette(queue: &wgpu::Queue, texture: &wgpu::Texture, floats: &[f32]) {
    let row_floats = (PALETTE_ROW_TEXELS * 4) as usize;
    let rows = floats.len().div_ceil(row_floats);
    let mut padded = floats.to_vec();
    padded.resize(rows * row_floats, 0.0);
    (rows > 0).then(|| {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&padded),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PALETTE_ROW_TEXELS * 16),
                rows_per_image: Some(rows as u32),
            },
            wgpu::Extent3d {
                width: PALETTE_ROW_TEXELS,
                height: rows as u32,
                depth_or_array_layers: 1,
            },
        );
    });
}

/// Max joint matrices across all skinned draws in one frame (the palette
/// texture's capacity).
///
/// This is a **crowd** bound, not a character bound: a skinned draw cannot be
/// instanced — each carries its own palette — so a frame drawing `n` bodies of
/// `b` bones needs `n · b` matrices. One articulated character is ~65; a scene
/// full of them is three figures times that, which is why the old 1024 was a
/// character's number standing in for a crowd's.
///
/// At 4096 the texture is 4096 × 64 B = 256 KB, a rounding error against a
/// frame's vertex traffic, and it is allocated only for a scene that actually
/// registers skinned meshes. A crowd past it stops drawing rather than
/// misdrawing (see the `break` below).
const PALETTE_CAP: usize = 4096;

/// One uploaded mesh's GPU buffers: its interleaved vertex stream and triangle
/// index buffer, plus the index count to draw.
#[derive(Debug)]
struct MeshBuffers {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    /// The mesh's local-space bounds, for the shadow pass's volume cull (see
    /// [`crate::shadow_cull`]). Computed once here because geometry never
    /// changes after upload; `None` for a degenerate stream, which the cull
    /// reads as "cannot be tested, so always submit".
    bounds: Option<axiom_math::Aabb>,
}

/// The shared, surface-free renderer: pipelines + caches + per-frame buffers +
/// shadow map. Its [`Self::record`] draws into any colour/depth view; the
/// surface-vs-offscreen plumbing lives in the callers.
#[derive(Debug)]
pub(crate) struct SceneRenderer {
    pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    meshes: HashMap<u64, MeshBuffers>,
    /// One albedo bind group (texture + sampler) per material id.
    materials: HashMap<u64, wgpu::BindGroup>,
    lights_buffer: wgpu::Buffer,
    lights_bind_group: wgpu::BindGroup,
    /// The directional light view-projection uniform (shared by the shadow pass
    /// and the main pass's PCF lookup), rewritten each frame.
    light_vp_buffer: wgpu::Buffer,
    /// Group 0 of the shadow pass: just the light view-projection.
    shadow_pass_bind_group: wgpu::BindGroup,
    /// Group 2 of the main pass: shadow map + comparison sampler + light VP.
    shadow_sample_bind_group: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    instance_buffer: wgpu::Buffer,
    max_instances: u32,
    /// The fullscreen-triangle SDF raymarch pipeline (composites after the mesh
    /// pass, reusing the camera depth buffer and the lights UBO).
    sdf_pipeline: wgpu::RenderPipeline,
    /// The SDF uniform (primitives + camera matrices + march tunables), rewritten
    /// each frame that carries an [`SdfScene`].
    sdf_uniform_buffer: wgpu::Buffer,
    /// Group 0 of the SDF pass: the SDF uniform.
    sdf_bind_group: wgpu::BindGroup,
    /// The app's authored render look, captured at bind. Its hemisphere ambient and
    /// depth fog are packed into the lights uniform each draw; its sky drives the
    /// sky pass below. Bind-time rather than per-frame because that is what the
    /// uniform layout is built around — changing the look mid-run does not move
    /// pixels without a rebind, which is the same contract the ambient always had.
    look: axiom_host::FrameRenderLook,
    /// The fullscreen sky pass, drawn behind the scene. Present only when the
    /// look carries a sky; without one the frame keeps its flat clear colour and
    /// no pipeline is built, so an app that authors no sky is unchanged.
    sky: Option<SkyPass>,
    /// The linear-blend-skinning resources, when the device can support them.
    /// [`None`] on a device without vertex-stage storage buffers — see [`Skinning`].
    skinning: Option<Skinning>,
    /// What every main-pass pipeline is built from, kept so a program compiled at
    /// the preparation barrier gets **the same** layout the default pipeline has.
    layouts: MainPassLayouts,
    /// The compiled surface programs, filled at the **preparation barrier** by
    /// [`Self::prepare_surfaces`] and never during a frame. Empty until an app
    /// prepares one, which is what every existing app does.
    surfaces: crate::surface_program::compile::SurfaceProgramCache,
}

/// The colour format and the four bind group layouts every main-pass pipeline —
/// the default one and every compiled surface program — is built from.
///
/// Retained as one field rather than four so that "the layout a surface program
/// is compiled against" is literally the same value as "the layout the default
/// pipeline was built against". A surface program built against its own layout
/// would make groups 1 (`lights`) and 2 (`shadow_sample`) invalid across a
/// pipeline switch, forcing them back inside the batch loop — the expensive
/// mistake this design exists to avoid.
#[derive(Debug)]
struct MainPassLayouts {
    format: wgpu::TextureFormat,
    material: wgpu::BindGroupLayout,
    lights: wgpu::BindGroupLayout,
    shadow_sample: wgpu::BindGroupLayout,
    /// The **one** layout every surface program's parameter group is built
    /// against (group 3, binding 1).
    surface: wgpu::BindGroupLayout,
}

/// Everything the skinned pass needs, grouped so it can be **absent**.
///
/// The joint palette is a storage buffer read from the VERTEX stage, which is a
/// WebGPU-class capability: WebGL2 has no storage buffers at all. Creating this
/// bind group layout on a device that cannot support it is not a soft failure —
/// wgpu rejects it as a validation error and the whole renderer panics at
/// construction.
///
/// So it is built only where it can work, and the skinned pass is skipped
/// otherwise. This is deliberately gated on the DEVICE rather than the backend:
/// the live browser arm requests `downlevel_webgl2_defaults` limits (0 storage
/// buffers per stage) on *both* its WebGPU and WebGL2 paths, so a backend check
/// would still build a layout the device had been told to refuse.
///
/// Previously these five resources were built unconditionally in
/// [`SceneRenderer::new`], which meant every browser 3D app died on the WebGL2
/// fallback — including apps with no skinned geometry whatsoever — for a feature
/// that arm does not even use (it passes no skinned meshes).
#[derive(Debug)]
struct Skinning {
    /// Same lighting/texturing/shadow as the main pipeline, but a 20-float vertex
    /// layout (with joints + weights) and a joint-matrix palette bound at group 3.
    pipeline: wgpu::RenderPipeline,
    /// Skinned meshes (20-float streams), uploaded once at bind like `meshes`.
    meshes: HashMap<u64, MeshBuffers>,
    /// Per-skinned-draw instance data (mvp + world + colour + joint_base).
    instance_buffer: wgpu::Buffer,
    /// The concatenated joint-matrix palette for every skinned draw this frame,
    /// four RGBA32F texels per matrix.
    palette_texture: wgpu::Texture,
    /// Group 3 of the skinned pass: the joint palette texture.
    palette_bind_group: wgpu::BindGroup,
}

impl Skinning {
    /// Build the skinned pass, or [`None`] when the scene has no skinned meshes.
    ///
    /// This used to be gated on `max_storage_buffers_per_shader_stage`, because
    /// the palette was a vertex-stage storage buffer — a WebGPU-class
    /// capability. That gate meant **every WebGL2 browser silently drew no
    /// skinned geometry at all**; and because the live arm requests
    /// `downlevel_webgl2_defaults` on its WebGPU path too so the two backends
    /// agree, it meant no live browser arm could draw a skinned body on *any*
    /// backend. A whole engine capability was unreachable from the browser, and
    /// nothing said so — the characters simply were not there.
    ///
    /// The palette is a texture now (see the WGSL above) and vertex texture
    /// fetch is guaranteed by GLES 3.0, so there is nothing left to gate on and
    /// nothing left to be quietly missing. The `Option` remains only so a scene
    /// with no skinned meshes skips building the pass at all.
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        material_layout: &wgpu::BindGroupLayout,
        lights_layout: &wgpu::BindGroupLayout,
        shadow_sample_layout: &wgpu::BindGroupLayout,
        skinned_mesh_set: &[(u64, Vec<f32>, Vec<u32>)],
        max_instances: u32,
    ) -> Option<Skinning> {
        (!skinned_mesh_set.is_empty()).then(|| {
            // Group 3 of the SKINNED pass carries two things: the joint palette
            // at binding 0, and the surface parameter region at binding 1 —
            // because the shared `fs` reads `surface_params` on both pipelines
            // and `downlevel_webgl2_defaults` guarantees only four bind groups,
            // so there is no fifth to move it to. The skinned pass binds the
            // ZERO region: it always runs the default program (its vertex stage
            // is at the 16-attribute ceiling and its draws carry no surface
            // program), and the default program reads no parameter.
            let palette_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-palette-layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::VERTEX,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: crate::surface_program::compile::SURFACE_PARAMS_BINDING,
                            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });
            let skinned_params = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-skinned-surface-params"),
                size: crate::surface_program::params::SURFACE_PARAM_REGION_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let palette_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-joint-palette"),
                size: wgpu::Extent3d {
                    width: PALETTE_ROW_TEXELS,
                    height: palette_rows(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let palette_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-palette-bind-group"),
                layout: &palette_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &palette_texture.create_view(&wgpu::TextureViewDescriptor::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: crate::surface_program::compile::SURFACE_PARAMS_BINDING,
                        resource: skinned_params.as_entire_binding(),
                    },
                ],
            });
            Skinning {
                pipeline: build_skinned_pipeline(
                    device,
                    format,
                    material_layout,
                    lights_layout,
                    shadow_sample_layout,
                    &palette_layout,
                ),
                meshes: skinned_mesh_set
                    .iter()
                    .map(|(id, vertices, indices)| (*id, upload_mesh(device, vertices, indices)))
                    .collect(),
                instance_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("axiom-skinned-instances"),
                    size: SKINNED_INSTANCE_STRIDE * max_instances as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                palette_texture,
                palette_bind_group,
            }
        })
    }
}

/// One skinned draw handed to [`SceneRenderer::record`]: the mesh + material to
/// draw, its MVP and world matrices (column-major), its colour tint, and the
/// joint-matrix palette (column-major) the vertex shader blends per vertex.
#[derive(Debug)]
pub(crate) struct SkinnedGpuDraw {
    pub(crate) mesh_id: u64,
    pub(crate) material_id: u64,
    pub(crate) mvp: [f32; 16],
    pub(crate) world: [f32; 16],
    pub(crate) color: [f32; 4],
    pub(crate) palette: Vec<[f32; 16]>,
}

impl SceneRenderer {
    /// Build both pipelines (for the given colour target `format`), the shadow
    /// map, upload every distinct mesh and material, and allocate the per-frame
    /// lighting + light-VP + instance buffers. `meshes` is `(mesh_id, 12-float
    /// vertices, indices)`; `materials` is `(material_id, width, height, RGBA8)`;
    /// `normals` is the optional per-material `(material_id, width, height, RGBA8)`
    /// tangent-space normal maps (materials absent from it get a flat normal).
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        skinned_mesh_set: &[(u64, Vec<f32>, Vec<u32>)],
        materials: &[axiom_host::MaterialTexture],
        normals: &[(u64, u32, u32, Vec<u8>)],
        max_instances: u32,
        shadow_size: u32,
        look: axiom_host::FrameRenderLook,
        device_max_anisotropy: u16,
    ) -> SceneRenderer {
        let max_instances = max_instances.max(1);
        // The shadow-atlas edge length is the device tier's choice
        // (`HostDeviceProfile::shadow_map_size`), floored to a usable minimum.
        let shadow_size = shadow_size.max(1);

        let meshes: HashMap<u64, MeshBuffers> = meshes
            .iter()
            .map(|(id, vertices, indices)| (*id, upload_mesh(device, vertices, indices)))
            .collect();

        // Material bind group layout (group 0): albedo texture + sampler (0,1) and a
        // normal-map texture + sampler (2,3). Materials with no normal map get a 1x1
        // flat normal, so they light exactly as before.
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-material-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // The default flat normal (1x1, RGB encodes the +Z tangent-space normal) used for
        // any material without an authored normal map.
        let flat_normal: (u32, u32, Vec<u8>) = (1, 1, vec![128, 128, 255, 255]);
        let materials: HashMap<u64, wgpu::BindGroup> = materials
            .iter()
            .map(|texture| {
                let id = texture.material_id();
                let (nw, nh, nrgba) = normals
                    .iter()
                    .find(|(nid, ..)| *nid == id)
                    .map(|(_, nw, nh, nrgba)| (*nw, *nh, nrgba.as_slice()))
                    .unwrap_or((flat_normal.0, flat_normal.1, flat_normal.2.as_slice()));
                (
                    id,
                    upload_material(
                        device,
                        queue,
                        &material_layout,
                        (texture.width(), texture.height(), texture.pixels()),
                        (nw, nh, nrgba),
                        texture.sampling(),
                        device_max_anisotropy,
                    ),
                )
            })
            .collect();

        // Lighting uniform (group 1): the frame's lights, rewritten each frame.
        // Visible to the VERTEX stage as well as the fragment stage, because its
        // `camera.w` lane carries the frame's surface time and `vs` reads it to
        // run a displacement program (`crate::scene_wgsl`). One uniform, one
        // per-frame write, both stages.
        let lights_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-lights-layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let lights_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-lights-ubo"),
            size: LIGHTS_UBO_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lights_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-lights-bind-group"),
            layout: &lights_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: lights_buffer.as_entire_binding(),
            }],
        });

        // Light view-projection uniform (one mat4 = 64 bytes), shared by the
        // shadow depth pass and the main pass's shadow lookup.
        let light_vp_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-light-vp-ubo"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Shadow map (a depth texture rendered from the light's POV).
        let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-shadow-map"),
            size: wgpu::Extent3d {
                width: shadow_size,
                height: shadow_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-shadow-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        // Shadow pass bind group layout (group 0): just the light VP.
        let shadow_pass_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("axiom-shadow-pass-layout"),
                entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
            });
        let shadow_pass_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-shadow-pass-bind-group"),
            layout: &shadow_pass_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_vp_buffer.as_entire_binding(),
            }],
        });

        // Main pass shadow-sampling bind group layout (group 2): shadow depth
        // texture + comparison sampler + light VP.
        let shadow_sample_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("axiom-shadow-sample-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                    uniform_entry(2, wgpu::ShaderStages::FRAGMENT),
                ],
            });
        let shadow_sample_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-shadow-sample-bind-group"),
            layout: &shadow_sample_layout,
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
                    resource: light_vp_buffer.as_entire_binding(),
                },
            ],
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-instances"),
            size: INSTANCE_STRIDE * max_instances as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // The ONE surface-parameter bind group layout, shared by the default
        // pipeline and by every program the preparation barrier compiles.
        let surface_layout = crate::surface_program::compile::surface_bind_group_layout(device);
        let pipeline = build_main_pipeline(
            device,
            format,
            &material_layout,
            &lights_layout,
            &shadow_sample_layout,
            &surface_layout,
            &scene_shader_source(),
        );
        let shadow_pipeline = build_shadow_pipeline(device, &shadow_pass_layout);

        // Skinning: the joint-palette storage buffer (group 3), the skinned
        // pipeline, the skinned meshes (20-float streams), and the per-skinned-draw
        // instance buffer — built ONLY where the device allows a storage buffer in
        // the vertex stage. See [`Skinning`] for why this is conditional.
        let skinning = Skinning::new(
            device,
            format,
            &material_layout,
            &lights_layout,
            &shadow_sample_layout,
            skinned_mesh_set,
            max_instances,
        );

        // SDF uniform (group 0 of the raymarch pass): primitives + camera matrices
        // + march tunables, rewritten each frame carrying an SdfScene.
        let sdf_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-sdf-layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let sdf_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-sdf-ubo"),
            size: SDF_UBO_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sdf_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-sdf-bind-group"),
            layout: &sdf_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: sdf_uniform_buffer.as_entire_binding(),
            }],
        });
        // The SDF pass reuses the lights UBO (group 1), so its pipeline layout pairs
        // the SDF layout with the same `lights_layout` the main pass binds.
        let sdf_pipeline = build_sdf_pipeline(device, format, &sdf_layout, &lights_layout);

        SceneRenderer {
            pipeline,
            shadow_pipeline,
            meshes,
            materials,
            lights_buffer,
            lights_bind_group,
            light_vp_buffer,
            shadow_pass_bind_group,
            shadow_sample_bind_group,
            shadow_view,
            instance_buffer,
            max_instances,
            sdf_pipeline,
            sdf_uniform_buffer,
            sdf_bind_group,
            // The sky pipeline, built only when the app authored a sky.
            sky: look
                .sky()
                .map(|sky| SkyPass::new(device, format, sky)),
            look,
            skinning,
            // No surface program until an app prepares one at the barrier. Every
            // existing app stays here, paying one shared zero bind group per pass
            // and not one draw call more.
            surfaces: crate::surface_program::compile::SurfaceProgramCache::empty(
                device,
                &surface_layout,
            ),
            layouts: MainPassLayouts {
                format,
                material: material_layout,
                lights: lights_layout,
                shadow_sample: shadow_sample_layout,
                surface: surface_layout,
            },
        }
    }

    /// **Compile every authored surface's program. At the barrier, and only
    /// here.**
    ///
    /// Driven from an app's `axiom_runtime::PreparationTask`, before
    /// `RuntimeState::Prepared` — the phase whose stated invariant is that the
    /// deterministic simulation cannot advance until preparation has completed.
    /// Shader compilation is exactly the shape of work that phase exists for:
    /// expensive, startup-only, producing runtime-ready in-memory data.
    ///
    /// Every program in `catalog` is compiled here, in the catalog's ascending
    /// digest order, and the draw loop below never compiles anything. A draw
    /// naming a program this call did not produce renders the default pipeline
    /// and the constant fallback while the frame reports
    /// `axiom_host::FrameFeature::ProceduralSurface`. That is the rule that keeps
    /// the doctrine `crate::post_chain` states at its render-target comment true:
    /// the set of pipelines a session holds is fixed before its first frame, so
    /// no frame can stutter compiling one.
    ///
    /// Calling it again replaces the cache wholesale — a second preparation, not
    /// an incremental one. There is no eviction and nothing is persisted.
    pub(crate) fn prepare_surfaces(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        catalog: &crate::surface_program::cache::SurfaceProgramCatalog,
    ) {
        self.surfaces = crate::surface_program::compile::SurfaceProgramCache::compile(
            device,
            queue,
            catalog,
            crate::surface_program::compile::SurfacePipelineInputs {
                format: self.layouts.format,
                material: &self.layouts.material,
                lights: &self.layouts.lights,
                shadow_sample: &self.layouts.shadow_sample,
                surface: &self.layouts.surface,
            },
        );
    }

    /// How many surface programs this renderer holds — the number a scene test
    /// asserts against so a variant explosion is a failing test rather than a
    /// slow frame.
    pub(crate) fn surface_program_count(&self) -> u32 {
        self.surfaces.len()
    }

    /// Record + submit one frame: a directional **shadow depth pre-pass** (the
    /// scene rendered from the light's POV through `light_view_proj`), then the
    /// main pass into `color_view` (cleared to `clear`) with depth `depth_view`.
    /// `lights` is uploaded into the lighting uniform; `batches`
    /// (`(mesh_id, material_id, [mvp(16)+world(16)+colour(4)] per instance,
    /// count)`) are packed once and drawn in both passes. A batch whose mesh or
    /// material id was never uploaded is skipped. The caller owns presenting /
    /// reading back `color_view`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        // The sub-rect of the colour/depth targets this frame draws into. The
        // targets are allocated at full tier size and a reduced render scale uses
        // only the lower-left corner, so adapting the scale costs a viewport
        // rather than a reallocation.
        viewport: (u32, u32),
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        // The **surface program** each batch draws with, in `batches` order —
        // `axiom_host::FrameDrawItem::surface_program`, which is the authored
        // surface's own digest. A batch with no entry here, and every entry that
        // is `0`, draws the default pipeline: that is the whole of what an app
        // authoring no surface does, and it costs it nothing.
        //
        // A parallel slice rather than a fifth tuple lane because the tuple is
        // the batch shape `GpuBackendApi::present_frame` publishes and
        // `axiom-shot` consumes; a program id is one `u64` per *batch*, so
        // carrying it beside them allocates a handful of words rather than
        // rewriting every instance stream.
        programs: &[u64],
        skinned: &[SkinnedGpuDraw],
        clear: [f32; 4],
        sdf: Option<&SdfScene>,
        caps: u32,
        // The frame's camera view-projection. Only the sky pass reads it (to
        // recover each pixel's world ray); the mesh pass gets its transforms
        // pre-multiplied per instance.
        camera_view_proj: [f32; 16],
        // The frame's SURFACE TIME in seconds — what a time-varying authored
        // surface samples in both the vertex and the fragment stage. Explicitly
        // supplied engine time (`axiom_host::FramePacket::time`), never a wall
        // clock, and an exact zero for a frame whose surfaces read no clock, so
        // such a frame's packed lighting uniform is byte-identical to what it
        // was before there was a clock at all.
        surface_time: f32,
    ) {
        // Gate the SDF raymarch pass on the frame's Sdf capability bit; a profile that
        // drops SDF renders meshes only (the same policy the Canvas 2D backend applies).
        let sdf = sdf.filter(|_| (caps & (axiom_host::RenderCapability::Sdf as u32)) != 0);
        queue.write_buffer(
            &self.lights_buffer,
            0,
            &pack_lights(
                lights,
                self.look.ambient(),
                // An unauthored fog packs as zero strength, an exact no-op in the
                // shader — so a look with no fog renders as one from before fog.
                self.look
                    .depth_fog()
                    .unwrap_or_else(axiom_host::FrameDepthFog::none),
                caps,
                camera_view_proj,
                surface_time,
            ),
        );
        queue.write_buffer(
            &self.light_vp_buffer,
            0,
            bytemuck::cast_slice(&light_view_proj),
        );
        // The sky this frame actually draws: present only when the look carried
        // one AND the frame's profile attempts the Sky capability. Resolved ONCE
        // here and used for both the uniform write and the draw — gating only the
        // write would leave a dropped-capability frame drawing a stale (or zeroed)
        // uniform over its clear colour, which is a black screen, not a degrade.
        let sky = self
            .sky
            .as_ref()
            .filter(|_| (caps & (axiom_host::RenderCapability::Sky as u32)) != 0);
        // Rewritten each frame: the sky's own parameters are fixed at bind, but
        // the camera moves, so the ray reconstruction does not.
        sky.into_iter().for_each(|s| {
            queue.write_buffer(&s.uniform, 0, &pack_sky(&s.sky, camera_view_proj));
        });
        // Upload the SDF uniform on frames that carry a scene (zero-or-one, via the
        // Option iterator — no `if`).
        sdf.into_iter()
            .for_each(|scene| queue.write_buffer(&self.sdf_uniform_buffer, 0, &pack_sdf(scene)));

        // Pack instances back-to-back; record each batch's (mesh, material, byte
        // offset, count), capped at the instance-buffer capacity.
        let mut packed: Vec<f32> = Vec::new();
        let mut draws: Vec<(u64, u64, u64, u32, u64)> = Vec::new();
        let mut written: u32 = 0;
        for (index, (mesh_id, material_id, instances, count)) in batches.iter().enumerate() {
            let room = self.max_instances.saturating_sub(written);
            let count = (*count).min(room);
            let floats = (count as usize) * INSTANCE_FLOATS;
            let byte_offset = u64::from(written) * INSTANCE_STRIDE;
            packed.extend_from_slice(&instances[..floats.min(instances.len())]);
            draws.push((
                *mesh_id,
                *material_id,
                byte_offset,
                count,
                programs.get(index).copied().unwrap_or_default(),
            ));
            written += count;
        }
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&packed));

        // Which of those draws can actually reach the shadow map.
        //
        // The shadow camera is a fixed ~40 m box that follows the view, while the
        // frame it is rendering reaches to the far plane — 1,650 m in a racing
        // course. Everything past the box was being submitted and clipped: a full
        // draw call and a full vertex load per batch, for zero texels, over EVERY
        // batch in the frame. On the WebGL2 path a draw costs ~52 GL calls
        // whether or not it contributes, so this was roughly half the frame's
        // submission cost.
        //
        // A batch survives if ANY of its instances can reach the volume — the
        // instances of one batch share a contiguous run of the buffer and are
        // drawn as one call, so the batch is the unit that can be dropped. A
        // frame with no usable light volume (degenerate shadow camera) keeps
        // every draw, exactly as before this existed: dropping a caster is
        // visible, keeping a redundant one is not.
        let volume = crate::shadow_cull::light_volume(&light_view_proj);
        let shadow_draws: Vec<&(u64, u64, u64, u32, u64)> = draws
            .iter()
            .filter(|(mesh_id, _, byte_offset, count, _)| {
                volume.as_ref().map_or(true, |frustum| {
                    let bounds = self.meshes.get(mesh_id).and_then(|m| m.bounds.as_ref());
                    bounds.map_or(true, |bounds| {
                        let first = (*byte_offset / INSTANCE_STRIDE) as usize;
                        (first..first + *count as usize).any(|i| {
                            packed
                                .get(i * INSTANCE_FLOATS + 16..i * INSTANCE_FLOATS + 32)
                                .map_or(true, |world| {
                                    crate::shadow_cull::casts_into(bounds, world, frustum)
                                })
                        })
                    })
                })
            })
            .collect();

        // Pack every skinned draw's palette back-to-back (recording each draw's base
        // matrix index) and its instance (mvp + world + colour + joint_base), bounded
        // by the palette capacity. Skipped entirely on a device with no skinned pass,
        // which leaves `skinned_draws` empty and the draw loop below a no-op.
        let mut skinned_draws: Vec<(u64, u64, u64)> = Vec::new();
        if let Some(skinning) = &self.skinning {
            let mut palette_floats: Vec<f32> = Vec::new();
            let mut skinned_instances: Vec<f32> = Vec::new();
            for d in skinned {
                let base = palette_floats.len() / 16;
                if base + d.palette.len() > PALETTE_CAP {
                    break;
                }
                for m in &d.palette {
                    palette_floats.extend_from_slice(m);
                }
                let byte_offset = (skinned_draws.len() as u64) * SKINNED_INSTANCE_STRIDE;
                skinned_instances.extend_from_slice(&d.mvp);
                skinned_instances.extend_from_slice(&d.world);
                skinned_instances.extend_from_slice(&d.color);
                skinned_instances.extend_from_slice(&[base as f32, 0.0, 0.0, 0.0]);
                skinned_draws.push((d.mesh_id, d.material_id, byte_offset));
            }
            write_palette(queue, &skinning.palette_texture, &palette_floats);
            queue.write_buffer(
                &skinning.instance_buffer,
                0,
                bytemuck::cast_slice(&skinned_instances),
            );
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("axiom-frame-encoder"),
        });

        // Shadow depth pre-pass: scene depth from the light's POV.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-shadow-pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.shadow_pass_bind_group, &[]);
            // The shadow pass runs its own depth-only shader and no surface
            // program: a displaced vertex therefore casts its UNdisplaced
            // shadow. That is a stated limit — `SHADOW_WGSL` is a separate
            // module with no `axiom_displace` in it, and threading one through
            // is its own change to the shadow pipeline — not a silent omission.
            for (mesh_id, _material_id, byte_offset, count, _program) in &shadow_draws {
                if let Some(mesh) = self.meshes.get(mesh_id) {
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_vertex_buffer(1, self.instance_buffer.slice(*byte_offset..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..*count);
                }
            }
        }

        // Main pass: lit + textured + shadowed.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0] as f64,
                            g: clear[1] as f64,
                            b: clear[2] as f64,
                            a: clear[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(
                0.0,
                0.0,
                viewport.0.max(1) as f32,
                viewport.1.max(1) as f32,
                0.0,
                1.0,
            );
            // The sky first, filling every pixel behind the scene. It writes no
            // depth and compares `Always`, so it neither occludes the geometry
            // drawn after it nor disturbs the depth buffer they test against —
            // the clear colour is simply replaced by a real sky. A look with no
            // sky records nothing here and the clear stands, exactly as before.
            if let Some(sky) = sky {
                pass.set_pipeline(&sky.pipeline);
                pass.set_bind_group(0, &sky.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            // The DEFAULT program's pipeline and the shared zero parameter group,
            // set once — the state every draw carrying `surface_program == 0`
            // runs under, which is every draw in every app that authors no
            // surface. Groups 1 and 2 are set exactly once per pass, as they
            // always have been: every surface pipeline shares this one's bind
            // group layouts, so switching pipelines below does not invalidate
            // them.
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(1, &self.lights_bind_group, &[]);
            pass.set_bind_group(2, &self.shadow_sample_bind_group, &[]);
            pass.set_bind_group(3, self.surfaces.default_bind_group(), &[]);
            // The program the pass is currently in. `draws` arrives GROUPED BY
            // PROGRAM (`crate::frame_packet_adapter` sorts on it), so this
            // changes once per distinct program in the frame — not once per
            // batch — and a frame with one program changes it zero times.
            let mut bound: u64 = 0;
            for (mesh_id, material_id, byte_offset, count, program) in &draws {
                if *program != bound {
                    bound = *program;
                    // A LOOKUP, never a compile. `None` means the preparation
                    // barrier never prepared this program: the draw falls back to
                    // the default pipeline and the constant channels
                    // `crate::frame_packet_adapter` folded into its instance
                    // stream, and the frame reports the drop through
                    // `axiom_host::FrameFeature::ProceduralSurface` (see
                    // `crate::GpuBackendApi::frame_degradations`). Compiling here
                    // would be the mid-session stutter `crate::post_chain`'s
                    // render-target comment exists to forbid — on the WebGL2
                    // fallback path `wgpu` cross-compiles WGSL to GLSL at
                    // pipeline creation, so a first-use compile is a guaranteed
                    // hitch.
                    let compiled = self.surfaces.program(*program);
                    pass.set_pipeline(
                        compiled.map_or(&self.pipeline, |program| program.pipeline()),
                    );
                    pass.set_bind_group(
                        3,
                        compiled.map_or(self.surfaces.default_bind_group(), |program| {
                            program.bind_group()
                        }),
                        &[],
                    );
                }
                if let (Some(mesh), Some(material)) =
                    (self.meshes.get(mesh_id), self.materials.get(material_id))
                {
                    pass.set_bind_group(0, material, &[]);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_vertex_buffer(1, self.instance_buffer.slice(*byte_offset..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..*count);
                }
            }

            // Skinned draws: the same lit/textured/shadowed fragment stage, via the
            // skinning pipeline with the joint palette bound at group 3. One draw per
            // skinned body (each carries its own palette; they cannot be instanced).
            // Absent on a device without vertex-stage storage buffers, where the pass
            // cannot exist at all — rigid geometry above is unaffected.
            if let Some(skinning) = &self.skinning {
                pass.set_pipeline(&skinning.pipeline);
                pass.set_bind_group(1, &self.lights_bind_group, &[]);
                pass.set_bind_group(2, &self.shadow_sample_bind_group, &[]);
                pass.set_bind_group(3, &skinning.palette_bind_group, &[]);
                for (mesh_id, material_id, inst_offset) in &skinned_draws {
                    if let (Some(mesh), Some(material)) =
                        (skinning.meshes.get(mesh_id), self.materials.get(material_id))
                    {
                        pass.set_bind_group(0, material, &[]);
                        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                        pass.set_vertex_buffer(1, skinning.instance_buffer.slice(*inst_offset..));
                        pass.set_index_buffer(
                            mesh.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                    }
                }
            }
        }

        // SDF raymarch pass: composite the frame's SDF shapes over the meshes.
        // Loads (does not clear) the same colour + depth attachments, so the
        // fullscreen marcher depth-tests against the mesh depth and writes its own
        // `frag_depth` — SDF and meshes occlude correctly. Runs zero-or-one times
        // (the Option iterator), only on frames carrying an SdfScene.
        sdf.into_iter().for_each(|_scene| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-sdf-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.sdf_pipeline);
            pass.set_bind_group(0, &self.sdf_bind_group, &[]);
            pass.set_bind_group(1, &self.lights_bind_group, &[]);
            pass.draw(0..3, 0..1);
        });

        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Replace one cached mesh's geometry mid-loop (12-float position+normal+uv+
    /// colour `vertices`, triangle-list `indices`). Used by terrain streaming.
    pub(crate) fn replace_geometry(
        &mut self,
        device: &wgpu::Device,
        mesh_id: u64,
        vertices: &[f32],
        indices: &[u32],
    ) {
        self.meshes
            .insert(mesh_id, upload_mesh(device, vertices, indices));
    }

    /// Replace the WHOLE uploaded mesh set (`(mesh_id, 12-float vertices,
    /// indices)`), rebuilding the id→buffers map. The 3D peer of
    /// [`Self::replace_geometry`]: where that swaps one existing mesh's geometry,
    /// this re-uploads the entire set, so a retained scene that registered new
    /// meshes AFTER bind (e.g. an `@axiom/game` game that `clearScene`s then
    /// authors its own meshes) has them all on the GPU, not just the bind-time set.
    pub(crate) fn load_meshes(
        &mut self,
        device: &wgpu::Device,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
    ) {
        self.meshes = meshes
            .iter()
            .map(|(id, vertices, indices)| (*id, upload_mesh(device, vertices, indices)))
            .collect();
    }
}

/// A uniform-buffer bind group layout entry at `binding` for the given stages.
fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Per-vertex layout: position(3) + normal(3) + uv(2) + colour(4).
fn vertex_layout() -> [wgpu::VertexAttribute; 4] {
    [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 12,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 24,
            shader_location: 2,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 32,
            shader_location: 3,
        },
    ]
}

/// The colour-target blend state, selected per draw from its resolved alpha:
/// straight **alpha blending** for the common translucent/opaque case (so a
/// material `opacity` / 2D `alpha` composites — replacing the hardcoded
/// `REPLACE`), or **additive** blending for glow draws. The 3D main pass uses
/// straight alpha; `additive` is the seam a per-draw glow pass selects.
fn blend_state(additive: bool) -> wgpu::BlendState {
    let alpha = wgpu::BlendState::ALPHA_BLENDING;
    let add = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    };
    [alpha, add][additive as usize]
}

/// Build the main (lit/textured/shadowed) pipeline for colour target `format`,
/// from `source` — the whole WGSL, with **some** surface program already spliced
/// into it.
///
/// `source` is a parameter rather than a call to [`scene_shader_source`] because
/// this is also how a *generated* program becomes a pipeline
/// (`crate::surface_program::compile::SurfaceProgramCache::compile` calls it,
/// once per prepared surface, at the preparation barrier). Every pipeline it
/// builds shares the same four bind group layouts, which is what lets the draw
/// loop switch pipelines mid-pass without re-setting groups 1 and 2.
pub(crate) fn build_main_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    material_layout: &wgpu::BindGroupLayout,
    lights_layout: &wgpu::BindGroupLayout,
    shadow_sample_layout: &wgpu::BindGroupLayout,
    surface_layout: &wgpu::BindGroupLayout,
    source: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("axiom-scene-shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axiom-scene-pl"),
        bind_group_layouts: &[
            material_layout,
            lights_layout,
            shadow_sample_layout,
            surface_layout,
        ],
        push_constant_ranges: &[],
    });
    // Per-instance attributes: mvp columns (loc 4-7), world columns (loc 8-11),
    // colour (loc 12), then emissive+pad (loc 13) — one Float32x4 every 16 bytes,
    // derived from the INSTANCE_FLOATS stride so the layout cannot drift from the
    // packing. 14 attributes with the 4 per-vertex ones, inside the WebGL2
    // guarantee of 16.
    let instance_attrs: Vec<wgpu::VertexAttribute> = (0..10)
        .map(|i| wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: (i as u64) * 16,
            shader_location: 4 + i,
        })
        .collect();
    let vertex_attrs = vertex_layout();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axiom-scene-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[
                wgpu::VertexBufferLayout {
                    array_stride: VERTEX_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertex_attrs,
                },
                wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attrs,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                // Straight alpha blending (was the hardcoded REPLACE): the
                // lowest-correct-layer fix so a material's `opacity` and the 2D
                // surface's `alpha` actually composite instead of overwriting.
                // `blend_state` selects per draw — additive is available for glow.
                blend: Some(blend_state(false)),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Build the skinned (linear-blend-skinning) main pipeline: the same
/// lit/textured/shadowed fragment stage, but a 20-float vertex layout carrying
/// per-vertex joints + weights, a `vs_skinned` vertex stage, and a joint-matrix
/// palette bound at group 3.
fn build_skinned_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    material_layout: &wgpu::BindGroupLayout,
    lights_layout: &wgpu::BindGroupLayout,
    shadow_sample_layout: &wgpu::BindGroupLayout,
    palette_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("axiom-scene-shader"),
        source: wgpu::ShaderSource::Wgsl(scene_shader_source().into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axiom-skinned-pl"),
        bind_group_layouts: &[
            material_layout,
            lights_layout,
            shadow_sample_layout,
            palette_layout,
        ],
        push_constant_ranges: &[],
    });
    // Per-vertex: pos(0) normal(1) uv(2) colour(3) joints(4) weights(5).
    let vertex_attrs = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 12,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 24,
            shader_location: 2,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 32,
            shader_location: 3,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 48,
            shader_location: 4,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 64,
            shader_location: 5,
        },
    ];
    // Per-instance: mvp(6-9) world(10-13) colour(14) joint_base(15) — 10 vec4s.
    let instance_attrs: Vec<wgpu::VertexAttribute> = (0..10)
        .map(|i| wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: (i as u64) * 16,
            shader_location: 6 + i,
        })
        .collect();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axiom-skinned-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_skinned"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[
                wgpu::VertexBufferLayout {
                    array_stride: SKINNED_VERTEX_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertex_attrs,
                },
                wgpu::VertexBufferLayout {
                    array_stride: SKINNED_INSTANCE_STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attrs,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend_state(false)),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Build the depth-only shadow pipeline (light-space projection, no fragment).
fn build_shadow_pipeline(
    device: &wgpu::Device,
    shadow_pass_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("axiom-shadow-shader"),
        source: wgpu::ShaderSource::Wgsl(SHADOW_WGSL.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axiom-shadow-pl"),
        bind_group_layouts: &[shadow_pass_layout],
        push_constant_ranges: &[],
    });
    // Position from the vertex buffer (loc 0); the four world-matrix columns from
    // the instance buffer (loc 1-4) at the world offset within the instance stride.
    let position_attr = [wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    }];
    let world_attrs: Vec<wgpu::VertexAttribute> = (0..4)
        .map(|i| wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: WORLD_OFFSET + (i as u64) * 16,
            shader_location: 1 + i,
        })
        .collect();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axiom-shadow-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[
                wgpu::VertexBufferLayout {
                    array_stride: VERTEX_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &position_attr,
                },
                wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &world_attrs,
                },
            ],
        },
        fragment: None,
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            // A slope-scaled depth bias reduces shadow acne on the depth pass.
            bias: wgpu::DepthBiasState {
                constant: 2,
                slope_scale: 2.0,
                clamp: 0.0,
            },
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Build the SDF raymarch pipeline for colour target `format`: a
/// fullscreen-triangle (no vertex buffers) whose fragment writes
/// `@builtin(frag_depth)`, depth-tested `Less` and depth-writing into the shared
/// camera depth buffer so it composites with the mesh pass. Bind group 0 is the
/// SDF uniform; group 1 is the same lights UBO the main pass binds. Alpha
/// blending lets a translucent SDF surface composite (opaque shapes overwrite).
fn build_sdf_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    sdf_layout: &wgpu::BindGroupLayout,
    lights_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("axiom-sdf-shader"),
        source: wgpu::ShaderSource::Wgsl(SDF_WGSL.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axiom-sdf-pl"),
        bind_group_layouts: &[sdf_layout, lights_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axiom-sdf-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend_state(false)),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Build a mesh's GPU buffers from an interleaved 12-float vertex stream and a
/// triangle-list index buffer.
fn upload_mesh(device: &wgpu::Device, vertices: &[f32], indices: &[u32]) -> MeshBuffers {
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("axiom-mesh-vertices"),
        contents: bytemuck::cast_slice(vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("axiom-mesh-indices"),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    MeshBuffers {
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
        bounds: crate::shadow_cull::local_bounds(vertices, MESH_VERTEX_FLOATS),
    }
}

/// Floats per uploaded mesh vertex: position(3) + normal(3) + uv(2) + colour(4).
/// The same layout [`vertex_layout`] declares, named here because the bounds
/// scan walks the stream by it.
const MESH_VERTEX_FLOATS: usize = 12;

/// Build a material's albedo bind group from RGBA8 pixels (sRGB texture + a
/// repeat sampler resolved from the material's own sampling mode), bound at
/// group 0 (binding 0 = texture, 1 = sampler).
///
/// `sampling` is the material's authored [`axiom_host::TextureSampling`] and
/// `device_max_anisotropy` is what the adapter reports; together they resolve —
/// in the pure, tested [`crate::texture_sampling`] — to filters and an anisotropy
/// clamp that are already valid for this device.
fn upload_material(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    albedo: (u32, u32, &[u8]),
    normal: (u32, u32, &[u8]),
    sampling: axiom_host::TextureSampling,
    device_max_anisotropy: u16,
) -> wgpu::BindGroup {
    // Albedo is sRGB-encoded colour; the normal map is linear data (RGB = the
    // tangent-space normal), so it uses the non-sRGB format.
    let albedo = upload_texture(
        device,
        queue,
        albedo.0,
        albedo.1,
        albedo.2,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let normal = upload_texture(
        device,
        queue,
        normal.0,
        normal.1,
        normal.2,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    // The filters and anisotropy clamp are decided by the pure, tested
    // `texture_sampling` module rather than written out here, so the rule
    // ("magnification is hard unless the material asked for anisotropy;
    // minification is always linear across the mip chain; anisotropy never
    // exceeds the device") is asserted by unit tests instead of living only in
    // this descriptor.
    let config = crate::texture_sampling::sampler_config(sampling, device_max_anisotropy);
    let filter = |kind| match kind {
        crate::texture_sampling::FilterKind::Nearest => wgpu::FilterMode::Nearest,
        crate::texture_sampling::FilterKind::Linear => wgpu::FilterMode::Linear,
    };
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("axiom-material-sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: filter(config.mag),
        min_filter: filter(config.min),
        mipmap_filter: filter(config.mipmap),
        anisotropy_clamp: config.anisotropy,
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-material-bind-group"),
        layout,
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
                resource: wgpu::BindingResource::TextureView(&normal),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    })
}

/// Upload one RGBA8 texture of the given format, **with its full mip chain**,
/// and return its default view.
///
/// The chain is not optional decoration. Without it a minified sample has only
/// the base level to read, so it returns one arbitrary texel of the many a pixel
/// covers — which is the moiré and the crawl on any surface that recedes. The
/// levels are built on the CPU by [`crate::mip_chain`] and written here, in order,
/// **after** the base: `write_texture` is queued work, and a level written before
/// the level it was derived from would be reading a texture that does not exist
/// yet. Building them on the CPU rather than with a GPU blit chain keeps the
/// filtering arithmetic pure, native-testable and inside the coverage gate, and
/// costs one bind-time pass over a texture that is at most a few hundred
/// kilobytes.
///
/// The encoding is derived from the format, not passed in, so the two can never
/// disagree: an `Rgba8UnormSrgb` albedo must average in linear light, and an
/// `Rgba8Unorm` normal map must not. See [`crate::mip_chain`] for why that
/// distinction is load-bearing.
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    rgba8: &[u8],
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    let width = width.max(1);
    let height = height.max(1);
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let encoding = match format {
        wgpu::TextureFormat::Rgba8UnormSrgb => mip_chain::TexelEncoding::Srgb,
        _ => mip_chain::TexelEncoding::Linear,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-material-texture"),
        size,
        mip_level_count: mip_chain::level_count(width, height),
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let write = |level: u32, w: u32, h: u32, pixels: &[u8]| {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    };
    write(0, width, height, rgba8);
    mip_chain::build(width, height, rgba8, encoding)
        .iter()
        .enumerate()
        .for_each(|(index, level)| {
            write(
                index as u32 + 1,
                level.width(),
                level.height(),
                level.pixels(),
            );
        });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Pack the frame's lights into the std140 lighting-uniform byte layout: an
/// 80-byte header — light count `u32` + capability mask `u32` + 8 bytes padding, the hemisphere-ambient
/// `sky` + `ground` `vec4`s (rgb, w unused), then the depth-fog `fog_color`
/// (rgb + max mix fraction) and `fog_range` (start, full-density, extinction rate
/// per metre, 0) `vec4`s —
/// then `MAX_LIGHTS` entries of two
/// `vec4`s — `v = (vec.xyz, kind)` and `col = (colour.rgb, intensity)`. Entries past
/// the count stay zero. Capped at `MAX_LIGHTS`.
///
/// An absent [`axiom_host::FrameDepthFog`] packs as [`axiom_host::FrameDepthFog::none`]
/// — zero strength — so the shader's fog term is an exact no-op and a frame that
/// authors no fog renders byte-identically to one from before fog existed.
fn pack_lights(
    lights: &[(u32, [f32; 3], [f32; 3], f32)],
    ambient: axiom_host::FrameAmbient,
    depth_fog: axiom_host::FrameDepthFog,
    caps: u32,
    camera_view_proj: [f32; 16],
    // The frame's surface time, packed into the `camera` vec4's fourth lane —
    // an unread pad until now, so a time-varying surface costs this uniform
    // nothing and a frame with none packs the exact zero the lane always held.
    surface_time: f32,
) -> Vec<u8> {
    let count = lights.len().min(MAX_LIGHTS);
    let mut bytes = Vec::with_capacity(LIGHTS_UBO_BYTES as usize);
    bytes.extend_from_slice(&(count as u32).to_le_bytes());
    // The capability mask occupies the first header pad slot (the WGSL `caps` field);
    // the remaining two u32 pads stay zero.
    bytes.extend_from_slice(&caps.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
    let (sky, ground) = (ambient.sky(), ambient.ground());
    let fog = depth_fog.color();
    let eye = camera_eye(camera_view_proj);
    [
        sky[0],
        sky[1],
        sky[2],
        0.0,
        ground[0],
        ground[1],
        ground[2],
        0.0,
        fog[0],
        fog[1],
        fog[2],
        depth_fog.strength().get(),
        depth_fog.near().get(),
        depth_fog.far().get(),
        // The Beer-Lambert extinction rate per world metre, in what used to be a
        // pad lane — so the distance term costs the uniform nothing and both
        // WGSL `Lights` declarations stay byte-identical to what they were.
        depth_fog.extinction().get(),
        0.0,
        eye[0],
        eye[1],
        eye[2],
        surface_time,
    ]
    .iter()
    .for_each(|f| bytes.extend_from_slice(&f.to_le_bytes()));
    (0..MAX_LIGHTS).for_each(|i| {
        let (kind, vec, color, intensity) =
            lights
                .get(i)
                .copied()
                .unwrap_or((0, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 0.0));
        [
            vec[0],
            vec[1],
            vec[2],
            kind as f32,
            color[0],
            color[1],
            color[2],
            intensity,
        ]
        .iter()
        .for_each(|f| bytes.extend_from_slice(&f.to_le_bytes()));
    });
    bytes
}

/// Pack the frame's [`SdfScene`] into the std140 SDF-uniform byte layout that
/// mirrors the WGSL `SdfU`: a 176-byte header — `view_proj` (mat4, 64),
/// `inv_view_proj` (mat4, 64), `camera_world_pos` (vec4, 16), `march` (vec4, 16),
/// `count` (u32 padded to 16) — then exactly `MAX_SDF_PRIMITIVES` entries of
/// `inv_transform` (mat4, 64), `params` (vec4, 16), `color` (vec4, 16), `kind`
/// (u32 padded to 16). Entries past the count stay zero; primitives past the cap
/// are dropped (the same honesty `pack_lights` uses).
fn pack_sdf(scene: &SdfScene) -> Vec<u8> {
    let count = scene.primitives().len().min(MAX_SDF_PRIMITIVES);
    let mut bytes = Vec::with_capacity(SDF_UBO_BYTES as usize);
    let push = |bytes: &mut Vec<u8>, floats: &[f32]| {
        floats
            .iter()
            .for_each(|f| bytes.extend_from_slice(&f.to_le_bytes()));
    };
    push(&mut bytes, &scene.view_proj());
    push(&mut bytes, &scene.inv_view_proj());
    let cam = scene.camera_world_pos();
    push(&mut bytes, &[cam[0], cam[1], cam[2], 0.0]);
    push(&mut bytes, &scene.march());
    bytes.extend_from_slice(&(count as u32).to_le_bytes());
    bytes.extend_from_slice(&[0u8; 12]);
    (0..MAX_SDF_PRIMITIVES).for_each(|i| {
        let (inv, params, color, kind) = scene
            .primitives()
            .get(i)
            .map(|p| (p.inv_transform(), p.params(), p.color(), p.kind()))
            .unwrap_or(([0.0; 16], [0.0; 4], [0.0; 4], 0));
        push(&mut bytes, &inv);
        push(&mut bytes, &params);
        push(&mut bytes, &color);
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 12]);
    });
    bytes
}

/// Create a depth-buffer texture view of the given size (the camera depth buffer
/// each arm attaches; the shadow map is created internally).
pub(crate) fn create_depth_view(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
