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
    // x = halo strength; y = the frame's SCENE-LINEAR EXPOSURE (the mesh pass's
    // `lights.scene_exposure`, carried here because this pass binds its own
    // uniform and writes into the same colour target — a sky metered at a
    // different stop from the world it sits behind is the seam this avoids);
    // zw unused.
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

    return vec4<f32>(mix(behind, cloud, density) * sky.halo.y, 1.0);
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

/// **The frame's sun**: the normalized world-space direction *toward* the first
/// **directional** light, or the zero vector when the frame has none.
///
/// The first directional is the sun by the engine's existing convention rather
/// than by a new one: `light_view_proj` is "the directional shadow caster's"
/// matrix ([`axiom_host::FramePacket::light_view_proj`]) and the main pass gives
/// every directional that caster's `shade`. Naming it here is what lets the
/// contact shadow's `dot( lightDir, sunDir ) < 0.999` test pick the sun out of
/// the light loop instead of darkening every light by it.
///
/// The zero vector is the honest answer for a frame with no directional light,
/// and it is also the *useful* one: `dot( anything, 0 )` is `0`, which fails the
/// test for every light, so the contact term degrades to an exact identity
/// rather than to a wrong direction. A light whose direction is itself the zero
/// vector (an app publishing an unset direction) normalizes to the same thing,
/// which is why the failure is routed through [`axiom_math::Vec3::normalize`]'s
/// result rather than through a hand-rolled length test.
fn sun_direction(lights: &[(u32, [f32; 3], [f32; 3], f32)]) -> [f32; 3] {
    lights
        .iter()
        .find(|(kind, ..)| *kind == 0)
        .map(|(_, direction, ..)| axiom_math::Vec3::new(direction[0], direction[1], direction[2]))
        .and_then(|v| v.normalize().ok())
        .map_or([0.0; 3], |n| [n.x, n.y, n.z])
}

/// Pack a [`axiom_host::FrameSky`] plus the camera's inverse view-projection
/// into the std140 layout `SkyU` describes.
///
/// A camera matrix that cannot be inverted (a degenerate projection) falls back
/// to the identity, which yields a usable — if wrong — ray rather than a NaN
/// that would poison every pixel of the frame. This is the same defensive
/// posture `FrameSky::normalize_or` takes on the Rust side.
/// **`?debug=normal|shadow|ao|albedo|ambient|geonormal`** — replace the mesh
/// pass's final colour with one intermediate of its lighting.
///
/// The instrument of last resort for a device you cannot attach a debugger to.
/// A phone rendering a black world under a correct sky has had exactly one of
/// its lighting terms collapse, and from the outside the candidates are
/// indistinguishable: a zero normal, a shadow factor stuck at zero, an occlusion
/// texture reading black and a black albedo all produce the same picture. Each
/// mode paints one of them directly, so one screenshot per mode names the
/// culprit instead of narrowing it.
///
/// `0` -- every value not listed, and every native build -- is an ordinary
/// frame: the shader's probe arithmetic is an exact identity there.
#[cfg(target_arch = "wasm32")]
fn debug_probe() -> u32 {
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let Some(value) = search
        .split("debug=")
        .nth(1)
        .map(|rest| rest.split('&').next().unwrap_or(rest))
    else {
        return 0;
    };
    match value {
        "normal" => 1,
        "shadow" => 2,
        "ao" => 3,
        "albedo" => 4,
        "ambient" => 5,
        "geonormal" => 6,
        "contact" => 7,
        _ => 0,
    }
}

/// Native builds have no URL to read, so the frame is never probed.
#[cfg(not(target_arch = "wasm32"))]
fn debug_probe() -> u32 {
    0
}

/// Say once, on the browser console, what stop the scene pass is metering at.
///
/// Per-frame and therefore unreachable from the bind-time report, but the value
/// is constant for a run, so once is the whole story. `wasm32` only: this is a
/// diagnostic for the machine that has no console anyone can read.
#[cfg(target_arch = "wasm32")]
fn report_scene_exposure(scene_exposure: f32, caps: u32) {
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    (!SAID.swap(true, std::sync::atomic::Ordering::Relaxed)).then(|| {
        web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
            "axiom: scene_exposure = {scene_exposure} (frame caps = {caps:#010x}, \n             hdr bit = {})",
            caps & (axiom_host::RenderCapability::HdrTargets as u32) != 0
        )));
    });
}

/// Native builds have no console to say it on.
#[cfg(not(target_arch = "wasm32"))]
fn report_scene_exposure(_scene_exposure: f32, _caps: u32) {}

fn pack_sky(
    sky: &axiom_host::FrameSky,
    camera_view_proj: [f32; 16],
    // The frame's scene-linear exposure, into the `halo` vec4's second lane. An
    // unmetered frame passes `1.0`, which is the exact identity the lane held
    // when it was an unused pad.
    scene_exposure: f32,
) -> Vec<u8> {
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
                scene_exposure,
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
// The main pass's `ShadowU` carries a second `sun` lane (see `crate::scene_wgsl`);
// this pass declares only the half it reads. A uniform buffer LARGER than the
// struct bound to it is legal — the reverse is the validation error — so the two
// declarations may differ here and only here.
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
    // The frame's scene-linear exposure — layout parity with the mesh pass, and
    // read here for the same reason: a raymarched surface and a triangle at the
    // same place must be metered by the same stop.
    scene_exposure: f32,
    // The shader probe — layout parity with the mesh pass, unread here.
    debug_mode: u32,
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
    return vec4<f32>(surface.rgb * lit * lights.scene_exposure, surface.a);
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
const LIGHTS_UBO_BYTES: u64 = 208 + (MAX_LIGHTS as u64) * 32;
/// The shadow-caster uniform (the main pass's `ShadowU`, group 2 binding 2; the
/// shadow depth pass's group 0 binding 0): the light view-projection `mat4`
/// (64 bytes) then the `sun` `vec4` (16) — `xyz` the normalized world direction
/// toward the sun, `w` the contact-shadow feature bit.
///
/// The depth pass's own `ShadowU` declares only the matrix, which is legal
/// against a larger buffer; the main pass declares both and would be a
/// validation error against a 64-byte one.
const SHADOW_LIGHT_UBO_BYTES: u64 = 64 + 16;
/// Floats in [`SHADOW_LIGHT_UBO_BYTES`].
const SHADOW_LIGHT_UBO_FLOATS: usize = (SHADOW_LIGHT_UBO_BYTES / 4) as usize;
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
    /// The depth/normal/velocity prepass and the ambient occlusion built on it.
    ///
    /// `Some` together or `None` together: GTAO reads two G-buffer channels, so
    /// a device that cannot hold the attachments has neither. The main pass then
    /// binds a 1x1 white AO and its multiply is exactly one, which is what makes
    /// "a device without the bit renders the bytes it always did" a property of
    /// the code rather than a hope.
    prepass: Option<(crate::gbuffer::GBufferTargets, crate::gbuffer::GBufferPass)>,
    /// Whether the bound device can actually hold the prepass's attachments.
    ///
    /// Separate from "the chain was built" and from the frame's capability word,
    /// because those are both statements of INTENT and this is a statement about
    /// the hardware. A device that cannot render one of the G-buffer's formats
    /// does not politely decline the pass — its attachments never resolve, and
    /// the targets keep their clear. Since the occlusion terms MULTIPLY, that
    /// clear is not "no data", it is "fully occluded": the ambient, the indirect
    /// fill and (through the contact term) the sun are all multiplied away.
    ///
    /// So execution asks the device, not the intent.
    prepass_usable: bool,
    gtao: Option<crate::gtao::pass::GtaoPass>,
    /// The screen-space contact shadow built on the same prepass. `Some` exactly
    /// when `prepass` is, for the same reason `gtao` is: it reads the G-buffer's
    /// depth and normal, so a device that cannot hold the attachments has no
    /// chain, binds the 1x1 white neutral, and renders what it always did.
    contact: Option<crate::contact::pass::ContactPass>,
    /// The prepass's own per-instance stream: `world`, `prev_world`,
    /// `material_id`, `coverage`. A **separate** buffer from the forward pass's,
    /// because the two shaders read different things — the forward pass wants a
    /// premultiplied mvp and a colour, the prepass wants the world pair it
    /// differences a velocity from. Sharing one would mean packing both layouts
    /// into every instance for every app, including the ones with no prepass.
    prepass_instances: Option<wgpu::Buffer>,
    /// Last frame's unjittered view-projection, for the velocity difference.
    /// `Cell` because `record` takes `&self`.
    prev_view_proj: std::cell::Cell<[f32; 16]>,
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
    /// vertices, indices)`.
    ///
    /// `materials` carries **all five** of a material's textures: the albedo on
    /// the carrier itself, and the tangent-space normal map, the
    /// `(occlusion, roughness, metalness, height)` pack, the micro-detail tile and
    /// the macro variation field as optional maps on it. Any map a material did
    /// not author binds this function's neutral 1x1 instead (see `flat_normal` and
    /// friends below), so a material that authors none renders exactly as it did
    /// before those slots existed.
    ///
    /// The normal maps used to arrive in a **second slice parallel to this one**,
    /// which only the off-screen path ever filled — the live browser arm passed
    /// `&[]`, so it had no normal maps at all. Folding the maps into the carrier
    /// deleted that parameter and that whole class of bug: there is now exactly
    /// one thing to pass, and every arm passes it.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        skinned_mesh_set: &[(u64, Vec<f32>, Vec<u32>)],
        materials: &[axiom_host::MaterialTexture],
        max_instances: u32,
        shadow_size: u32,
        look: axiom_host::FrameRenderLook,
        // Whether this device can hold the prepass's attachments at all — see the
        // field of the same name.
        prepass_usable: bool,
        device_max_anisotropy: u16,
        // The colour target's full allocated size, and therefore the size of the
        // G-buffer and the half-resolution ambient-occlusion chain built beside
        // it. Not the per-frame viewport: like the colour target, these are
        // allocated once at tier size and a reduced render scale uses the
        // lower-left corner, so adapting the scale costs a viewport rather than
        // four reallocations.
        //
        // `None` builds no G-buffer and no AO — the arm every caller that only
        // wants a lit frame takes, and the arm that renders exactly the bytes it
        // always did.
        scene_size: Option<(u32, u32)>,
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
                // The runtime material shader's three extra maps (4, 5, 6). They
                // share the samplers already bound at 1 and 3 rather than adding
                // their own: a sampler is a filtering rule, and these want the
                // same rule the material already asked for.
                //
                // All three are LINEAR data, never sRGB — an ORM triple, a
                // tangent-space normal and a noise field are measurements, not
                // colours. A material that authors none of them gets a neutral
                // 1x1 (see `NEUTRAL_ORM_HEIGHT` and friends), which is what keeps
                // every existing draw pixel-identical now that the bindings exist.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
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
        // The default flat normal (1x1, RGB encodes the +Z tangent-space normal) used for
        // any material without an authored normal map.
        let flat_normal: (u32, u32, Vec<u8>) = (1, 1, vec![128, 128, 255, 255]);
        // The runtime material shader's three maps, in their NEUTRAL form: what a
        // material that authors none of them must see so the shader's own
        // defaults decide the result rather than leftover texture memory.
        //
        // These bytes are the compatibility contract. A material can now author
        // all three (`axiom_host::MaterialTexture`'s optional maps), but the
        // overwhelming majority author none, and for those the frame must be the
        // frame it was before the slots existed — so these values do not move
        // without a byte-identity proof moving with them.
        //
        // `ORM+height` is (occlusion, roughness, metalness, height). Occlusion 1
        // is "unoccluded" and metalness 0 is "not metal" — the values that make
        // those terms identities. Roughness 1 is the *unscaled* value the
        // parameter block's `[scale, offset, minimum]` remap then applies, so a
        // material states its roughness in parameters rather than inheriting a
        // texture it never authored. Height 0 disables parallax at the texture
        // as well as at the parameter, which matters because `parallax` and the
        // height map are two independent ways to switch it off.
        let neutral_orm_height: (u32, u32, Vec<u8>) = (1, 1, vec![255, 255, 0, 0]);
        // Detail: binding 5 packs `(normal.x, normal.y, micro_albedo, height)`,
        // so its identity is 128 in the FIRST THREE lanes — a flat tangent-space
        // normal in `.rg` and, critically, 0.5 in `.b`. `.b` is the micro-albedo
        // speckle, which the shader decodes as `(b - 0.5) * 1.25`; the 255 a
        // "flat normal" instinct would put there decodes to `+0.625` and
        // brightens every material that supplies no detail map. Same class of
        // bug as "macro neutral is mid-grey, not zero".
        //
        // `.a` (height) stays 0, matching the source: `owMicro = (a - 0.5) * 2`
        // is then `-1`, which only ever *darkens* through `max(-micro, 0)`, and
        // `owDetailP.z` (the strength) is 0 for any material with no detail
        // block, so the whole layer multiplies out to identity regardless.
        let neutral_detail: (u32, u32, Vec<u8>) = (1, 1, vec![128, 128, 128, 0]);
        // Macro: mid-grey. The macro layer is a *variation* around a midpoint,
        // so 0.5 is its identity — zero would darken every surface by the full
        // macro amplitude, which is the failure a naive "neutral is zero" would
        // produce.
        let neutral_macro: (u32, u32, Vec<u8>) = (1, 1, vec![128, 128, 128, 255]);
        let materials: HashMap<u64, wgpu::BindGroup> = materials
            .iter()
            .map(|texture| {
                (
                    texture.material_id(),
                    upload_material(
                        device,
                        queue,
                        &material_layout,
                        (texture.width(), texture.height(), texture.pixels()),
                        // Each of the four: what the material authored, or this
                        // function's neutral. `map_or_neutral` is the ONE place
                        // that fallback is decided, so a slot cannot quietly grow
                        // a different default from its three siblings.
                        map_or_neutral(texture.normal(), &flat_normal),
                        map_or_neutral(texture.orm_height(), &neutral_orm_height),
                        map_or_neutral(texture.detail(), &neutral_detail),
                        map_or_neutral(texture.macro_field(), &neutral_macro),
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

        // The shadow-caster uniform, shared by the shadow depth pass and the main
        // pass's shadow lookup: one mat4 (64 bytes) plus the SUN lane (16) the
        // contact shadow's light test reads. See `SHADOW_LIGHT_UBO_BYTES`.
        let light_vp_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-light-vp-ubo"),
            size: SHADOW_LIGHT_UBO_BYTES,
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
        // **`Nearest`, and it has to be `Nearest`.**
        //
        // This sampler reads a `Depth32Float` texture through
        // `textureSampleCompare`. WebGPU exempts a COMPARISON sampler from the
        // filterable-format rule, so `Linear` here passes validation on every
        // backend and reaches the driver unchallenged -- but wgpu's own GLES
        // adapter does not advertise `SAMPLED_LINEAR` for depth formats, and
        // GLES 3.0's texture-completeness rule (3.8.13) has no carve-out for
        // depth compare: a non-filterable texture sampled with `LINEAR` is
        // INCOMPLETE, and an incomplete sampler returns 0.0. For a shadow
        // compare 0.0 means FULLY SHADOWED -- on all twenty-five taps of the PCF
        // kernel, for every fragment in the frame.
        //
        // That is invisible on a permissive implementation. ANGLE-on-D3D11 and
        // every WebGPU arm do hardware PCF and look perfect, so the fault only
        // appears on a strict mobile GLES driver: a world lit by ambient alone
        // under a sky that is completely unaffected, because the sky pass binds
        // no shadow map. It is not a quality difference between devices; it is
        // one class of device rendering a black picture.
        //
        // `Nearest` costs nothing worth having. Each tap still returns 0 or 1
        // through the compare, and the 5x5 kernel still averages to twenty-six
        // penumbra steps -- the edge is marginally crisper and nothing else
        // changes -- while the frame stops depending on a filterability the
        // format is not required to have anywhere.
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-shadow-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
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
                    // The resolved ambient occlusion, in group 2 with the other
                    // screen-space lighting inputs rather than in a group 4 of
                    // its own: WebGPU's default `maxBindGroups` is **four**, so
                    // 0..3 is the whole budget and a fifth group would refuse to
                    // create a pipeline on a conforming device.
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    // The resolved contact shadow, beside the occlusion and for
                    // the same reason: both are screen-space lighting inputs and
                    // group 2 is where those live. It shares binding 4's sampler
                    // rather than adding a sixth entry — the fetch is 1:1, where
                    // linear and nearest are the same sample.
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
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
        // Built before the bind group below, because that group binds one of the
        // two: the real AO when this device runs the chain, and a 1x1 white
        // texture when it does not.
        // `BackendCapabilityProfile::all()` is the honest argument here, not a
        // shortcut: `GBufferTargets::new`'s profile gate asks whether the
        // *attachment set* is permitted, and the DEVICE's answer to that is what
        // `live_gpu_binding::device_gbuffer` already measured before deciding
        // whether to pass a `scene_size` at all. The per-FRAME mask is a
        // different question and is honoured where it belongs — `record` simply
        // does not run these passes for a frame whose `caps` drops the bit.
        let prepass = scene_size.and_then(|(width, height)| {
            crate::gbuffer::GBufferTargets::new(
                device,
                axiom_host::BackendCapabilityProfile::all(),
                width,
                height,
            )
            .map(|targets| (targets, crate::gbuffer::GBufferPass::new(device)))
        });
        let prepass_instances = prepass.as_ref().map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-gbuffer-instances"),
                size: u64::from(max_instances)
                    * crate::gbuffer::GBUFFER_INSTANCE_FLOATS as u64
                    * 4,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let gtao = prepass.as_ref().zip(scene_size).map(|((targets, _), size)| {
            crate::gtao::pass::GtaoPass::new(
                device,
                size,
                targets.view(crate::gbuffer::GBufferChannel::Depth),
                targets.view(crate::gbuffer::GBufferChannel::Normal),
                targets.view(crate::gbuffer::GBufferChannel::Velocity),
            )
        });
        // The unoccluded neutral. `Rg16Float` to match the real chain's format,
        // and `1.0` in the visibility lane so `ambient * ao` is `ambient`.
        let white_ao = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-ao-neutral"),
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
        queue.write_texture(
            white_ao.as_image_copy(),
            &[255_u8, 255, 255, 255],
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
        let white_ao_view = white_ao.create_view(&wgpu::TextureViewDescriptor::default());
        let ao_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-ao-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            // LINEAR, unlike the nearest sampler the GTAO chain reads its
            // G-buffer with: this fetch upsamples a half-resolution, already
            // bilaterally blurred signal, so filtering is what stops the
            // occlusion reading as visible half-res blocks.
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        // The contact-shadow chain, built from the SAME prepass attachments the
        // occlusion chain reads and gated identically: `Some` exactly when the
        // prepass is, so a device that cannot hold a G-buffer has neither and the
        // main pass binds the white neutral for both.
        let contact = prepass.as_ref().zip(scene_size).map(|((targets, _), size)| {
            crate::contact::pass::ContactPass::new(
                device,
                size,
                targets.view(crate::gbuffer::GBufferChannel::Depth),
                targets.view(crate::gbuffer::GBufferChannel::Normal),
            )
        });
        let ao_view = gtao
            .as_ref()
            .map_or(&white_ao_view, crate::gtao::pass::GtaoPass::resolved_view);
        // The same 1x1 white neutral: `r = 1.0` is "no contact occlusion", so the
        // sun multiply is exactly one and a device without the chain renders the
        // bytes it always did.
        let contact_view = contact
            .as_ref()
            .map_or(&white_ao_view, crate::contact::pass::ContactPass::resolved_view);
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(ao_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&ao_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(contact_view),
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
            prepass,
            prepass_usable,
            gtao,
            contact,
            prepass_instances,
            // Identity, so the first frame differences against itself and
            // produces a zero velocity — which is the honest answer for a frame
            // with no predecessor.
            prev_view_proj: std::cell::Cell::new([
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ]),
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
        // The frame's camera, as the host publishes it: view, projection and
        // their product, together.
        //
        // All three travel because a product cannot be split back into its
        // factors and the passes below need different halves. The sky pass
        // inverts the view-projection to recover each pixel's world ray; the
        // depth/normal prepass works in VIEW space and needs the view; the
        // ambient occlusion built on that prepass inverts the PROJECTION to turn
        // a linear depth back into a view-space position, and reads
        // `projection[5]` to scale a world radius into pixels. The mesh pass
        // needs none of them — its transforms arrive pre-multiplied per instance.
        camera: axiom_host::FrameCamera,
        // The frame's SURFACE TIME in seconds — what a time-varying authored
        // surface samples in both the vertex and the fragment stage. Explicitly
        // supplied engine time (`axiom_host::FramePacket::time`), never a wall
        // clock, and an exact zero for a frame whose surfaces read no clock, so
        // such a frame's packed lighting uniform is byte-identical to what it
        // was before there was a clock at all.
        surface_time: f32,
        // The frame's GPU timestamp clock, when the device can time passes at
        // all. `None` — every device without `TIMESTAMP_QUERY`, every WebGL2
        // browser, and every caller that is not measuring — leaves each
        // `timestamp_writes` below exactly the `None` it has always been, so the
        // recorded command stream is bit-identical to the untimed one.
        clock: Option<&crate::gpu_pass_clock::GpuPassClock>,
    ) {
        // Gate the SDF raymarch pass on the frame's Sdf capability bit; a profile that
        // drops SDF renders meshes only (the same policy the Canvas 2D backend applies).
        let sdf = sdf.filter(|_| (caps & (axiom_host::RenderCapability::Sdf as u32)) != 0);
        // The stop this frame is metered at, for the arms that have to apply it
        // themselves. `1.0` — an exact identity — on every frame that either
        // authored no tone map or kept one; the authored exposure only on a
        // device that could not give the float attachment and therefore has no
        // composite to apply it in. Decided from the same `caps` word the pass
        // gates every other capability on.
        let scene_exposure =
            crate::hdr_target::ldr_scene_exposure(self.look.tonemap(), caps);
        report_scene_exposure(scene_exposure, caps);
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
                scene_exposure,
                debug_probe(),
                // An unauthored fill is every-lane zero, an exact no-op — so a
                // look with none renders as one from before the fill existed.
                self.look
                    .indirect()
                    .unwrap_or_else(axiom_host::FrameIndirect::none),
                camera.view_proj(),
                surface_time,
            ),
        );
        // **The sun**: the frame's FIRST directional light, normalized, in world
        // space. That is the same light the shadow map is rendered from — every
        // directional in the loop already takes `shade` from it — so naming it
        // here does not invent a convention, it writes the one the pass already
        // runs on. A frame with no directional light normalizes nothing and packs
        // the zero vector, which fails the `0.999` test for every light and makes
        // the contact term an exact identity.
        //
        // This is the reconciliation `render/materialpatch.js`'s port notes left
        // open (§5.2): the light loop now *does* know which light is the sun, so
        // a second directional receives the cascade but not the sun's contact ray.
        let sun_dir_world = sun_direction(lights);
        // The feature bit (`owFeat.y`): the chain ran for THIS frame, which needs
        // both a built chain and a capability mask that kept the G-buffer. Off,
        // the main pass reads a stale target — the targets stay allocated once
        // built — so this cannot be decided at build time.
        let contact_live = self.contact.is_some()
            & self.prepass_usable
            & ((caps & (axiom_host::RenderCapability::GBuffer as u32)) != 0);
        let mut shadow_uniform = [0.0_f32; SHADOW_LIGHT_UBO_FLOATS];
        shadow_uniform[..16].copy_from_slice(&light_view_proj);
        shadow_uniform[16..19].copy_from_slice(&sun_dir_world);
        shadow_uniform[19] = f32::from(u8::from(contact_live));
        queue.write_buffer(
            &self.light_vp_buffer,
            0,
            bytemuck::cast_slice(&shadow_uniform),
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
            queue.write_buffer(
                &s.uniform,
                0,
                &pack_sky(&s.sky, camera.view_proj(), scene_exposure),
            );
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

        // DEPTH / NORMAL / VELOCITY PREPASS, then the ambient occlusion built on
        // it. Both before the forward pass, because the forward pass SAMPLES the
        // AO — `FramePass::Prepass(3) < Gtao(4) < ForwardWorld(7)` in the frame
        // graph's own ordering.
        //
        // Skipped for a frame whose capability mask drops the G-buffer even
        // though the device could hold one: the targets stay allocated (they are
        // built once) and the main pass reads whatever the last frame left,
        // which is why the mask is checked here rather than only at build.
        self.prepass
            .as_ref()
            .zip(self.prepass_instances.as_ref())
            .filter(|_| self.prepass_usable)
            .filter(|_| (caps & (axiom_host::RenderCapability::GBuffer as u32)) != 0)
            .map(|((targets, pass), instances)| {
                // The prepass reads `world` and `prev_world`; the forward stream
                // carries the world matrix at floats 16..32 of each instance.
                // `prev_world` is the SAME matrix: per-instance motion history
                // does not exist yet, so an object's own movement contributes no
                // velocity and only the camera's does. Stated rather than hidden
                // — a temporal pass fed a zero object velocity smears moving
                // geometry, and that is the shape of the defect to look for.
                let packed_prepass: Vec<f32> = (0..packed.len() / INSTANCE_FLOATS)
                    .flat_map(|i| {
                        let base = i * INSTANCE_FLOATS;
                        let world: [f32; 16] = packed[base + 16..base + 32]
                            .try_into()
                            .expect("an instance carries 16 world floats");
                        crate::gbuffer::pack_gbuffer_instance(&world, &world, 0.0, 1.0)
                    })
                    .collect();
                queue.write_buffer(instances, 0, bytemuck::cast_slice(&packed_prepass));

                let view_proj = camera.view_proj();
                let uniform = crate::gbuffer::pack_gbuffer_uniform(
                    // No TAA jitter is applied yet, so the rasterised transform
                    // and the unjittered one are the same matrix. They stay
                    // separate lanes anyway: collapsing them is the mistake that
                    // makes a temporal resolve smear, and it would be invisible
                    // until the jitter lands.
                    &view_proj,
                    &view_proj,
                    &self.prev_view_proj.get(),
                    &camera.view(),
                );
                let draws_gb: Vec<crate::gbuffer::GBufferDraw<'_>> = draws
                    .iter()
                    .filter_map(|(mesh_id, _material, byte_offset, count, _program)| {
                        self.meshes.get(mesh_id).map(|mesh| {
                            crate::gbuffer::GBufferDraw {
                                vertices: &mesh.vertex_buffer,
                                indices: &mesh.index_buffer,
                                index_count: mesh.index_count,
                                // The prepass stream has its own stride, so the
                                // forward pass's byte offset does not transfer.
                                instance_offset: (byte_offset / INSTANCE_STRIDE)
                                    * (crate::gbuffer::GBUFFER_INSTANCE_FLOATS as u64 * 4),
                                instance_count: *count,
                            }
                        })
                    })
                    .collect();
                pass.record(queue, &mut encoder, targets, &uniform, instances, &draws_gb);
                self.prev_view_proj.set(view_proj);

                // The AO chain. `projection[5]` is `uP11`, the one element the
                // source reads to scale a world radius into pixels.
                //
                // The projection inverse is computed once and both chains take
                // it: GTAO turns a linear depth back into a view position with
                // it, and the contact march does the same before stepping along
                // the sun. A degenerate projection falls back to the projection
                // itself, which is wrong but finite — the same posture
                // `camera_eye` takes, and for the same reason.
                let projection = axiom_math::Mat4::from_cols_array(camera.projection());
                let proj_inv = projection
                    .inverse()
                    .map_or(camera.projection(), |m| m.as_cols_array());
                self.gtao.as_ref().map(|gtao| {
                    gtao.record(queue, &mut encoder, proj_inv, camera.projection()[5]);
                });
                // The contact chain, on the same prepass. It marches along the
                // sun in VIEW space, so the world direction packed into the
                // shadow uniform above is rotated into view here — the view
                // matrix is rigid, so the rotated vector is still unit length,
                // which the march depends on (a non-unit step silently rescales
                // the ray).
                self.contact.as_ref().map(|contact| {
                    let view = axiom_math::Mat4::from_cols_array(camera.view());
                    let sun_view = view.transform_vector(axiom_math::Vec3::new(
                        sun_dir_world[0],
                        sun_dir_world[1],
                        sun_dir_world[2],
                    ));
                    contact.record(
                        queue,
                        &mut encoder,
                        camera.projection(),
                        proj_inv,
                        [sun_view.x, sun_view.y, sun_view.z],
                    );
                });
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
                timestamp_writes: clock
                    .map(|clock| clock.writes(crate::gpu_pass_clock::PASS_SHADOW)),
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
                timestamp_writes: clock.map(|clock| clock.writes(crate::gpu_pass_clock::PASS_MAIN)),
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
                timestamp_writes: clock.map(|clock| clock.writes(crate::gpu_pass_clock::PASS_SDF)),
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
#[allow(clippy::too_many_arguments)]
/// One material map as [`upload_material`] wants it — `(width, height, texels)` —
/// resolving an absent map to the caller's neutral 1x1.
///
/// The whole fallback rule for all four non-albedo maps lives here and nowhere
/// else. That is the point: four slots each writing their own `unwrap_or` is four
/// chances for one of them to pick a different default, and the difference
/// between "the material authored nothing" and "the material authored black" is
/// the difference between an unchanged frame and a frame with the macro layer
/// subtracted from it.
fn map_or_neutral<'a>(
    map: Option<&'a axiom_host::MapPixels>,
    neutral: &'a (u32, u32, Vec<u8>),
) -> (u32, u32, &'a [u8]) {
    map.map_or(
        (neutral.0, neutral.1, neutral.2.as_slice()),
        |authored| (authored.width(), authored.height(), authored.pixels()),
    )
}

fn upload_material(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    albedo: (u32, u32, &[u8]),
    normal: (u32, u32, &[u8]),
    // `(occlusion, roughness, metalness, height)`, linear.
    orm_height: (u32, u32, &[u8]),
    // The shared micro-detail tile: tangent-space normal in RGB, height in A.
    detail: (u32, u32, &[u8]),
    // The macro variation field.
    macro_field: (u32, u32, &[u8]),
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
    // All three material-shader maps are linear data, never sRGB: an ORM triple
    // is three measurements, a tangent-space normal is a direction, and the
    // macro field is a noise amplitude. Uploading any of them as `Rgba8UnormSrgb`
    // would apply a decode curve to a number that is not a colour — the same
    // class of mistake as G16 in `01-engine-gaps.md`, where baked field textures
    // were written linear and bound as sRGB so every baked tile read dark.
    let orm_height = upload_texture(
        device,
        queue,
        orm_height.0,
        orm_height.1,
        orm_height.2,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    let detail = upload_texture(
        device,
        queue,
        detail.0,
        detail.1,
        detail.2,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    let macro_field = upload_texture(
        device,
        queue,
        macro_field.0,
        macro_field.1,
        macro_field.2,
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
            // 4, 5, 6: the runtime material shader's maps. They reuse the
            // sampler bound at 1 and 3 — one filtering rule per material, which
            // is what the material asked for.
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&orm_height),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&detail),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&macro_field),
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
    // The frame's scene-linear exposure, into the header lane after `caps`. An
    // unmetered frame passes `1.0` — the exact identity the lane held while it
    // was `_pad1`, so such a frame's uniform is byte-identical to before.
    scene_exposure: f32,
    // The shader probe (`?debug=`). `0` is an ordinary frame.
    debug_mode: u32,
    // The frame's two-band indirect fill. Every lane zero is the identity.
    indirect: axiom_host::FrameIndirect,
    camera_view_proj: [f32; 16],
    // The frame's surface time, packed into the `camera` vec4's fourth lane —
    // an unread pad until now, so a time-varying surface costs this uniform
    // nothing and a frame with none packs the exact zero the lane always held.
    surface_time: f32,
) -> Vec<u8> {
    let count = lights.len().min(MAX_LIGHTS);
    let mut bytes = Vec::with_capacity(LIGHTS_UBO_BYTES as usize);
    bytes.extend_from_slice(&(count as u32).to_le_bytes());
    // The capability mask occupies the first header pad slot (the WGSL `caps`
    // field), the exposure the second and the shader probe the third.
    bytes.extend_from_slice(&caps.to_le_bytes());
    bytes.extend_from_slice(&scene_exposure.to_le_bytes());
    bytes.extend_from_slice(&debug_mode.to_le_bytes());
    let (sky, ground) = (ambient.sky(), ambient.ground());
    let fog = depth_fog.color();
    let eye = camera_eye(camera_view_proj);
    let (fill_sky, fill_ground, gain) = (
        indirect.sky_fill(),
        indirect.ground_fill(),
        indirect.fill_gain(),
    );
    // The unit direction TOWARD the key light. A frame's directional light
    // carries the direction its light TRAVELS, so the sun sits the other way;
    // the sun-bounce wrap negates this again to reach the anti-sun hemisphere.
    // `find` rather than an index: a frame may lead with point lights, and one
    // with no directional light at all yields a zero the wrap tolerates.
    let sun_toward = lights
        .iter()
        .find(|(kind, _, _, _)| *kind == 0)
        .map_or([0.0_f32; 3], |(_, v, _, _)| [-v[0], -v[1], -v[2]]);
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
        // ---- the fill lanes, in `Lights`' declared order ------------------
        fill_sky[0],
        fill_sky[1],
        fill_sky[2],
        0.0,
        fill_ground[0],
        fill_ground[1],
        fill_ground[2],
        0.0,
        gain[0],
        gain[1],
        0.0,
        0.0,
        indirect.ibl_diffuse(),
        indirect.interior_floor(),
        // The live room count. Zero until an app builds interior volumes; the
        // gate degrades to its AO arm, which is what the source does before the
        // world appears.
        0.0,
        0.0,
        // The algorithm's own constants, from the ONE place they are defined.
        crate::indirect_lighting::FILL_DIR[0],
        crate::indirect_lighting::FILL_DIR[1],
        crate::indirect_lighting::FILL_DIR[2],
        crate::indirect_lighting::FILL_DIR[3],
        crate::indirect_lighting::AO_STRENGTH[0],
        crate::indirect_lighting::AO_STRENGTH[1],
        0.0,
        0.0,
        sun_toward[0],
        sun_toward[1],
        sun_toward[2],
        0.0,
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

/// The material-map lane, proved on a real adapter.
///
/// Native + `offscreen` only: these render a frame and read it back, which is
/// what `crate::offscreen::render_to_rgba` exists for, and which the wasm arm
/// cannot do. Same shape as `gbuffer`'s byte-identity proof, and for the same
/// reason — a contract change that claims to move no pixel has to show it.
#[cfg(all(test, not(target_arch = "wasm32"), feature = "offscreen"))]
mod map_tests {
    use axiom_host::{MapPixels, MaterialTexture};

    /// The captured edge length. 64 px keeps the readback small while leaving the
    /// quad tens of pixels across.
    const EDGE: u32 = 64;

    /// Column-major identity.
    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];

    /// A column-major projection mapping world `(x, y, *)` to clip
    /// `(x/2, y/2, 0.5, 1)`. Deliberately trivial: what is under test is the
    /// material bind group, not a perspective divide.
    const HALF_SCALE_VP: [f32; 16] = [
        0.5, 0.0, 0.0, 0.0, //
        0.0, 0.5, 0.0, 0.0, //
        0.0, 0.0, 0.0, 0.0, //
        0.0, 0.0, 0.5, 1.0,
    ];

    /// A quad in the `z = 0` object plane with a `+z` normal and **varying** uv.
    ///
    /// The varying uv is load-bearing, not decoration. The main pass builds its
    /// cotangent frame from screen-space uv derivatives (`scene_wgsl`), so a quad
    /// whose four corners share one uv has a degenerate frame and the shader
    /// deliberately falls back to the geometric normal — on which a normal map, by
    /// design, changes nothing. A test using such a quad would "prove" the lane
    /// works by proving nothing at all.
    fn quad() -> (Vec<f32>, Vec<u32>) {
        let corner =
            |x: f32, y: f32, u: f32, v: f32| [x, y, 0.0, 0.0, 0.0, 1.0, u, v, 1.0, 1.0, 1.0, 1.0];
        let vertices = [
            corner(-1.2, -1.2, 0.0, 0.0),
            corner(1.2, -1.2, 1.0, 0.0),
            corner(1.2, 1.2, 1.0, 1.0),
            corner(-1.2, 1.2, 0.0, 1.0),
        ]
        .concat();
        (vertices, vec![0, 1, 2, 0, 2, 3])
    }

    /// Render one quad with `material`'s textures and read the frame back, or
    /// `None` when this machine has no native adapter.
    fn render(material: MaterialTexture) -> Option<Vec<u8>> {
        let (vertices, indices) = quad();
        let translate = {
            let mut m = IDENTITY;
            m[14] = -5.0;
            m
        };
        crate::offscreen::render_to_rgba(
            EDGE,
            EDGE,
            &[(1_u64, vertices, indices)],
            std::slice::from_ref(&material),
            &[(0, [0.35, -0.5, 0.79], [1.0, 0.95, 0.85], 1.0)],
            IDENTITY,
            axiom_host::FrameCamera::IDENTITY,
            &[(1_u64, 1_u64, [HALF_SCALE_VP, translate].concat(), 1)],
            &[],
            &[],
            [0.05, 0.06, 0.08, 1.0],
            None,
            axiom_host::FrameRenderLook::default(),
            None,
            axiom_host::BackendCapabilityProfile::all(),
            None,
            None,
            1,
        )
        .map(|(pixels, _timing)| pixels)
    }

    /// A material with a mid-grey albedo and no authored maps — what every app
    /// that predates the map slots produces.
    fn unmapped() -> MaterialTexture {
        MaterialTexture::new(1, 1, 1, vec![170, 170, 170, 255])
    }

    /// **THE HARD CONSTRAINT.** A material that supplies no extra maps must render
    /// byte-identical to one that explicitly binds the backend's neutrals.
    ///
    /// This is the proof that `map_or_neutral`'s absent arm and its present arm
    /// agree, which is the whole compatibility claim of the contract change: every
    /// existing app authors nothing, takes the absent arm, and must land on
    /// exactly the texels the backend bound before the slots existed. If a neutral
    /// is ever retuned without its `Option` arm following, this fails.
    ///
    /// The neutral bytes are written out here rather than read from
    /// `super::SceneRenderer` on purpose: an independently-stated expectation is
    /// the only kind that can disagree with the implementation.
    #[test]
    fn a_material_with_no_maps_matches_one_that_binds_the_neutrals_byte_for_byte() {
        let neutral = |bytes: Vec<u8>| Some(MapPixels::new(1, 1, bytes));
        let explicit = unmapped()
            // Flat +Z tangent-space normal.
            .with_normal(neutral(vec![128, 128, 255, 255]))
            // (occlusion 1, roughness 1, metalness 0, height 0).
            .with_orm_height(neutral(vec![255, 255, 0, 0]))
            // Flat detail normal, zero detail height.
            .with_detail(neutral(vec![128, 128, 255, 0]))
            // Mid-grey: the macro layer varies AROUND a midpoint, so zero would
            // darken every surface by the full macro amplitude.
            .with_macro_field(neutral(vec![128, 128, 128, 255]));

        let Some(absent) = render(unmapped()) else {
            // No native adapter on this machine; nothing to prove either way.
            return;
        };
        let present = render(explicit).expect("the same adapter answered once already");
        assert_eq!(absent.len() as u32, EDGE * EDGE * 4);
        // Comparing two blank frames would prove nothing.
        assert!(
            absent.chunks_exact(4).any(|p| p[0] > 16 || p[1] > 16),
            "the control frame rendered nothing to compare"
        );
        let differing = absent
            .iter()
            .zip(present.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differing, 0,
            "authoring the neutral maps explicitly moved {differing} of {} bytes; \
             the absent-map fallback and the neutral constants have diverged",
            absent.len()
        );
    }

    /// **The lane is live.** An authored normal map reaches the GPU and changes
    /// the shading.
    ///
    /// The byte-identity test above passes trivially if the maps are silently
    /// dropped, which is precisely the defect being fixed — the live browser arm
    /// passed `&[]` for its normal maps for as long as that lane existed, and
    /// nothing caught it. This is the assertion that would have.
    #[test]
    fn an_authored_normal_map_changes_the_shaded_frame() {
        let Some(flat) = render(unmapped()) else {
            return;
        };
        // A tangent-space normal tilted hard along +x: `(255, 128, 128)` decodes
        // to about `(1.0, 0.004, 0.004)`, which the cotangent frame turns into a
        // large change in `N` and therefore in the Lambert term.
        let tilted = render(
            unmapped().with_normal(Some(MapPixels::new(1, 1, vec![255, 128, 128, 255]))),
        )
        .expect("the same adapter answered once already");
        let differing = flat
            .iter()
            .zip(tilted.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            differing > 0,
            "an authored normal map moved 0 of {} bytes — the map never reached \
             the bind group",
            flat.len()
        );
    }

    /// The fallback rule itself, without a GPU: present takes the map's own extent
    /// and texels, absent takes the neutral's. Both arms, stated once.
    #[test]
    fn map_or_neutral_takes_the_authored_map_and_falls_back_to_the_neutral() {
        let neutral: (u32, u32, Vec<u8>) = (1, 1, vec![128, 128, 255, 255]);
        assert_eq!(
            super::map_or_neutral(None, &neutral),
            (1, 1, [128, 128, 255, 255].as_slice()),
            "an absent map binds the neutral"
        );
        let authored = MapPixels::new(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            super::map_or_neutral(Some(&authored), &neutral),
            (2, 1, [1, 2, 3, 4, 5, 6, 7, 8].as_slice()),
            "an authored map binds its own extent and texels"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The metering lane, at the byte.**
    ///
    /// A [`axiom_host::FrameTonemap`] carries a curve AND a scene-linear
    /// exposure. A backend without
    /// [`axiom_host::RenderCapability::HdrTargets`] must drop the first and keep
    /// the second - the curve needs headroom, a multiply does not - so the
    /// exposure travels to the shader in the header lane that used to be
    /// `_pad1`. `crate::hdr_target::ldr_scene_exposure` decides the value; this
    /// pins where it lands, which is the half a wrong offset would silently
    /// break (the lane beside it is `caps`, a bitmask that would read as a
    /// nonsense exposure and vice versa).
    #[test]
    fn the_lights_header_carries_the_scene_exposure_in_the_lane_after_caps() {
        let packed = |exposure: f32| {
            pack_lights(
                &[],
                axiom_host::FrameAmbient::default_hemisphere(),
                axiom_host::FrameDepthFog::none(),
                0xABCD_1234,
                exposure,
                0,
                axiom_host::FrameIndirect::none(),
                [0.0; 16],
                0.0,
            )
        };
        let bytes = packed(4.0);
        // count, then caps, then the exposure: the WGSL `Lights` header, in order.
        assert_eq!(&bytes[0..4], &0_u32.to_le_bytes());
        assert_eq!(&bytes[4..8], &0xABCD_1234_u32.to_le_bytes());
        assert_eq!(&bytes[8..12], &4.0_f32.to_le_bytes());
        // The last header lane is the shader probe, and an unprobed frame writes
        // the zero the lane held when it was a pad.
        assert_eq!(&bytes[12..16], &0_u32.to_le_bytes());

        // An unmetered frame writes the exact identity. This is the byte-identity
        // guarantee for every app that authors no tone map: a `1.0` here is a
        // multiply the shader cannot observe, not a nearly-one.
        assert_eq!(&packed(1.0)[8..12], &1.0_f32.to_le_bytes());
    }

    /// The sky pass binds its OWN uniform and writes into the SAME colour
    /// target, so it has to be metered by the same stop or the sky and the world
    /// in front of it are graded differently - a seam at the horizon. Its lane is
    /// the `halo` vec4's second component: 64 bytes of matrix, then four `vec4`s,
    /// then `halo.x`.
    #[test]
    fn the_sky_uniform_carries_the_same_exposure_as_the_world() {
        let bytes = pack_sky(&axiom_host::FrameSky::gradient([0.2, 0.4, 0.9], [0.7, 0.8, 0.9]), [0.0; 16], 4.0);
        const HALO_Y: usize = 64 + 4 * 16 + 4;
        assert_eq!(&bytes[HALO_Y..HALO_Y + 4], &4.0_f32.to_le_bytes());
        assert_eq!(
            &pack_sky(&axiom_host::FrameSky::gradient([0.2, 0.4, 0.9], [0.7, 0.8, 0.9]), [0.0; 16], 1.0)[HALO_Y..HALO_Y + 4],
            &1.0_f32.to_le_bytes()
        );
    }

    /// Both passes must actually READ their lane. The values above are inert if
    /// the shader ignores them, and a shader is text here - so this is assertable
    /// without a device, the same way `post_chain` pins its composite source.
    #[test]
    fn both_colour_passes_apply_the_scene_exposure_they_are_handed() {
        assert!(
            crate::scene_wgsl::SCENE_WGSL_PREFIX.contains("scene_exposure: f32"),
            "the mesh pass stopped declaring the metering lane"
        );
        assert!(
            crate::scene_wgsl::SCENE_WGSL_SUFFIX.contains("lights.scene_exposure"),
            "the mesh pass stopped applying the metering it is handed"
        );
        assert!(
            SKY_WGSL.contains("sky.halo.y"),
            "the sky pass stopped applying the metering it is handed"
        );
        assert!(
            SDF_WGSL.contains("lights.scene_exposure"),
            "the raymarch pass stopped applying the metering it is handed"
        );
    }
}
