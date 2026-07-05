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

/// WGSL for the lit/textured/shadowed main pass: per-vertex position+normal+uv+
/// colour, per-instance MVP + world matrix + colour, a material albedo texture
/// (group 0), a lighting uniform (group 1), and a shadow map + light
/// view-projection (group 2). Each directional light is attenuated by a PCF
/// shadow lookup; point lights attenuate by distance.
const SCENE_WGSL: &str = r#"
@group(0) @binding(0) var albedo_tex: texture_2d<f32>;
@group(0) @binding(1) var albedo_sampler: sampler;
@group(0) @binding(2) var normal_tex: texture_2d<f32>;
@group(0) @binding(3) var normal_sampler: sampler;

struct Light {
    // xyz = to-light direction (directional) or world position (point); w = kind (0 dir, 1 point).
    v: vec4<f32>,
    // rgb = colour; w = intensity.
    col: vec4<f32>,
};
struct Lights {
    count: u32,
    // The frame's backend capability mask (BackendCapabilityProfile::bits). The
    // fragment shader gates its per-fragment features on these bits so the GPU
    // backend consults the same capability profile the Canvas 2D backend does.
    caps: u32,
    _pad1: u32,
    _pad2: u32,
    // Hemisphere ambient (rgb; w unused), strength folded in — a plain mix, no scale.
    sky: vec4<f32>,
    ground: vec4<f32>,
    items: array<Light, 16>,
};
@group(1) @binding(0) var<uniform> lights: Lights;

// Capability bits mirrored from axiom_host::RenderCapability (pinned by the host's
// `capability_bits_are_the_gpu_shader_contract` test): the four per-fragment features
// this main pass gates.
const CAP_TEXTURES: u32 = 1u;
const CAP_ALPHAMASK: u32 = 2u;
const CAP_NORMALMAP: u32 = 4u;
const CAP_SHADOWS: u32 = 8u;

@group(2) @binding(0) var shadow_map: texture_depth_2d;
@group(2) @binding(1) var shadow_samp: sampler_comparison;
struct ShadowU { light_vp: mat4x4<f32> };
@group(2) @binding(2) var<uniform> shadow: ShadowU;

// Skinning: the joint-matrix palette for the skinned pass (group 3). All skinned
// draws' palettes are concatenated; each draw's per-instance `joint_base` indexes
// the start of its own palette. Bound only by the skinned pipeline.
@group(3) @binding(0) var<storage, read> joint_palette: array<mat4x4<f32>>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    // Perspective-correct UV. (An affine `@interpolate(linear)` "swim" reads more
    // retro 32-bit, but compiles to a `noperspective` qualifier the WebGL2 GLSL target
    // rejects — it panics pipeline creation on the browser's downlevel path — so
    // the UV stays perspective-correct; nearest filtering carries the retro 32-bit look.)
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) world_pos: vec3<f32>,
};

@vertex
fn vs(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) vertex_color: vec4<f32>,
    @location(4) m0: vec4<f32>,
    @location(5) m1: vec4<f32>,
    @location(6) m2: vec4<f32>,
    @location(7) m3: vec4<f32>,
    @location(8) w0: vec4<f32>,
    @location(9) w1: vec4<f32>,
    @location(10) w2: vec4<f32>,
    @location(11) w3: vec4<f32>,
    @location(12) instance_color: vec4<f32>,
) -> VsOut {
    let mvp = mat4x4<f32>(m0, m1, m2, m3);
    let world = mat4x4<f32>(w0, w1, w2, w3);
    var out: VsOut;
    out.clip = mvp * vec4<f32>(position, 1.0);
    out.world_pos = (world * vec4<f32>(position, 1.0)).xyz;
    out.normal = (world * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv;
    out.color = vertex_color * instance_color;
    return out;
}

// Skinned vertex stage: identical to `vs` but the position/normal are first
// deformed by the linear-blend of the vertex's four joint matrices (from the
// palette, offset by the per-instance `joint_base`), then run through the same
// MVP/world as a rigid vertex. Bind-pose vertices with an identity palette are
// unchanged, so a skinned mesh at rest matches its baked geometry.
@vertex
fn vs_skinned(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) vertex_color: vec4<f32>,
    @location(4) joints: vec4<f32>,
    @location(5) weights: vec4<f32>,
    @location(6) m0: vec4<f32>,
    @location(7) m1: vec4<f32>,
    @location(8) m2: vec4<f32>,
    @location(9) m3: vec4<f32>,
    @location(10) w0: vec4<f32>,
    @location(11) w1: vec4<f32>,
    @location(12) w2: vec4<f32>,
    @location(13) w3: vec4<f32>,
    @location(14) instance_color: vec4<f32>,
    @location(15) joint_base: vec4<f32>,
) -> VsOut {
    let mvp = mat4x4<f32>(m0, m1, m2, m3);
    let world = mat4x4<f32>(w0, w1, w2, w3);
    let base = u32(joint_base.x);
    let skin = weights.x * joint_palette[base + u32(joints.x)]
             + weights.y * joint_palette[base + u32(joints.y)]
             + weights.z * joint_palette[base + u32(joints.z)]
             + weights.w * joint_palette[base + u32(joints.w)];
    let sp = skin * vec4<f32>(position, 1.0);
    let sn = skin * vec4<f32>(normal, 0.0);
    var out: VsOut;
    out.clip = mvp * sp;
    out.world_pos = (world * sp).xyz;
    out.normal = (world * sn).xyz;
    out.uv = uv;
    out.color = vertex_color * instance_color;
    return out;
}

// Fraction of the directional light reaching `world_pos` (1 = fully lit, 0 =
// fully shadowed), via a 3x3 PCF lookup into the shadow map. Fragments outside
// the shadow frustum (uv out of range or beyond the far plane) are treated as
// lit, so geometry past the shadow box (e.g. distant terrain) is not darkened.
//
// The PCF loop runs unconditionally (uniform control flow) and the frustum test
// is applied with `select` afterwards — `textureSampleCompare` uses implicit
// derivatives and so must not sit behind a possibly-non-uniform branch (an early
// `return` here is rejected by the browser's WGSL validator, though native wgpu
// accepts it).
fn shadow_factor(world_pos: vec3<f32>) -> f32 {
    let clip = shadow.light_vp * vec4<f32>(world_pos, 1.0);
    let ndc = clip.xyz / clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, ndc.y * -0.5 + 0.5);
    let dim = vec2<f32>(textureDimensions(shadow_map));
    let texel = 1.0 / dim;
    let bias = 0.0015;
    // 5x5 PCF with a slight kernel spread for a softer penumbra than a 3x3 tap.
    let spread = 1.25;
    var sum = 0.0;
    for (var dy = -2; dy <= 2; dy = dy + 1) {
        for (var dx = -2; dx <= 2; dx = dx + 1) {
            let off = vec2<f32>(f32(dx), f32(dy)) * texel * spread;
            sum = sum + textureSampleCompare(shadow_map, shadow_samp, uv + off, ndc.z - bias);
        }
    }
    let outside = uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0;
    return select(sum / 25.0, 1.0, outside);
}

// Fraction of hemisphere ambient a fully-shadowed fragment keeps. 1.0 = the shadow removes
// only the sun's diffuse (shadows wash out under full sky fill); <1.0 also dims the sky
// ambient in shadow, so the sun's cast shadows read with directional contrast. An explicit,
// minimal directional-shadow contrast control (kept lifted, never crushed to black).
const SHADOW_AMBIENT: f32 = 0.5;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let caps = lights.caps;
    // Textures capability: sample the albedo image, or fall back to flat white (the
    // per-vertex/instance `in.color` still tints it) — the GPU peer of the Canvas 2D
    // flat degrade. `select` evaluates both arms, so the sample stays uniform.
    let sampled = textureSample(albedo_tex, albedo_sampler, in.uv);
    let albedo = select(vec4<f32>(1.0, 1.0, 1.0, 1.0), sampled, (caps & CAP_TEXTURES) != 0u);
    // Alpha cutout capability: drop fully-transparent texels (foliage leaf-alpha cards)
    // so they neither shade nor write depth; the soft 0.5..1 rim still alpha-blends.
    // Gated on the AlphaMask bit; off → the quad renders opaque.
    let cut = ((caps & CAP_ALPHAMASK) != 0u) && (albedo.a < 0.5);
    if (cut) { discard; }
    let base = albedo * in.color;
    // Perturb the geometric normal by the material's tangent-space normal map. There is
    // no per-vertex tangent, so build the cotangent frame from screen-space derivatives
    // of world position + uv (Mikkelsen). Normal-mapping capability off → a flat
    // (0,0,1) tangent-space normal, so N stays the geometric normal.
    let geo_n = normalize(in.normal);
    let nmap = select(vec3<f32>(0.0, 0.0, 1.0), textureSample(normal_tex, normal_sampler, in.uv).xyz * 2.0 - 1.0, (caps & CAP_NORMALMAP) != 0u);
    let dp1 = dpdx(in.world_pos);
    let dp2 = dpdy(in.world_pos);
    let duv1 = dpdx(in.uv);
    let duv2 = dpdy(in.uv);
    let r1 = cross(dp2, geo_n);
    let r2 = cross(geo_n, dp1);
    let inv_det = 1.0 / max(dot(dp1, r1), 0.0001);
    let tangent = (r1 * duv1.x + r2 * duv2.x) * inv_det;
    let bitangent = (r1 * duv1.y + r2 * duv2.y) * inv_det;
    let inv_max = inverseSqrt(max(dot(tangent, tangent), dot(bitangent, bitangent)));
    let N = normalize(tangent * (nmap.x * inv_max) + bitangent * (nmap.y * inv_max) + geo_n * nmap.z);
    // Shadow capability off → fully lit (`shadow_factor` is still evaluated in uniform
    // control flow via `select`, so its `textureSampleCompare` derivatives stay valid).
    let shade = select(1.0, shadow_factor(in.world_pos), (caps & CAP_SHADOWS) != 0u);
    // Hemisphere ambient from the frame's ambient uniform (sky overhead, warm-dark
    // ground below, blended by the normal's up-component). Strength is folded into the
    // colours, so this is a plain mix — no extra scale. An absent frame ambient is
    // filled with the engine default upstream, so this stays identical by default.
    let hemi = mix(lights.ground.rgb, lights.sky.rgb, clamp(N.y * 0.5 + 0.5, 0.0, 1.0));
    // Shadowed ground receives less SKY ambient too, not just less sun, so the sun's cast
    // shadows read with real contrast instead of being washed flat by full ambient.
    let ambient_shade = mix(SHADOW_AMBIENT, 1.0, shade);
    var lit = base.rgb * hemi * ambient_shade;
    for (var i: u32 = 0u; i < lights.count; i = i + 1u) {
        let lt = lights.items[i];
        var L = normalize(lt.v.xyz);
        var atten = 1.0;
        if (lt.v.w > 0.5) {
            // Point light: direction + distance attenuation from world position.
            let d = lt.v.xyz - in.world_pos;
            let dist = length(d);
            L = d / max(dist, 0.0001);
            atten = 1.0 / (1.0 + 0.09 * dist + 0.032 * dist * dist);
        } else {
            // Directional light: cast shadows from the shadow map.
            atten = shade;
        }
        let diffuse = max(dot(N, L), 0.0) * atten;
        lit = lit + base.rgb * lt.col.rgb * lt.col.w * diffuse;
    }
    return vec4<f32>(lit, base.a);
}
"#;

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
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    // Hemisphere ambient (rgb; w unused), strength folded in — a plain mix, no scale.
    sky: vec4<f32>,
    ground: vec4<f32>,
    items: array<Light, 16>,
};
@group(1) @binding(0) var<uniform> lights: Lights;

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
    out.color = shade(surface, n, p);
    out.depth = clip.z / clip.w;
    return out;
}
"#;

/// Depth-buffer format used by both the camera depth and the shadow map.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Maximum lights uploaded per frame (must match the WGSL `array<Light, 16>`).
const MAX_LIGHTS: usize = 16;
/// Lighting uniform size in bytes: a 48-byte header (count + padding, then the
/// hemisphere-ambient `sky` + `ground` `vec4`s) plus `MAX_LIGHTS` × two `vec4`s
/// (32 bytes each) — std140-compatible.
const LIGHTS_UBO_BYTES: u64 = 48 + (MAX_LIGHTS as u64) * 32;
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
/// Floats per instance: mvp(16) + world(16) + colour(4) = 36.
const INSTANCE_FLOATS: usize = 36;
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
/// Max joint matrices across all skinned draws in one frame (the palette storage
/// buffer capacity). A soccer frame uses ~65; 1024 is a generous, bounded cap.
const PALETTE_CAP: usize = 1024;

/// One uploaded mesh's GPU buffers: its interleaved vertex stream and triangle
/// index buffer, plus the index count to draw.
#[derive(Debug)]
struct MeshBuffers {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
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
    /// The frame's hemisphere ambient, packed into the lights uniform each draw.
    ambient: axiom_host::FrameAmbient,
    /// The skinned (linear-blend-skinning) main-pass pipeline: same lighting/
    /// texturing/shadow as `pipeline`, but a 20-float vertex layout (with joints +
    /// weights) and a joint-matrix palette bound at group 3.
    skinned_pipeline: wgpu::RenderPipeline,
    /// Skinned meshes (20-float streams), uploaded once at bind like `meshes`.
    skinned_meshes: HashMap<u64, MeshBuffers>,
    /// Per-skinned-draw instance data (mvp + world + colour + joint_base).
    skinned_instance_buffer: wgpu::Buffer,
    /// The concatenated joint-matrix palette for every skinned draw this frame.
    palette_buffer: wgpu::Buffer,
    /// Group 3 of the skinned pass: the joint palette storage buffer.
    palette_bind_group: wgpu::BindGroup,
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
        materials: &[(u64, u32, u32, Vec<u8>)],
        normals: &[(u64, u32, u32, Vec<u8>)],
        max_instances: u32,
        shadow_size: u32,
        ambient: axiom_host::FrameAmbient,
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
            .map(|(id, w, h, rgba8)| {
                let (nw, nh, nrgba) = normals
                    .iter()
                    .find(|(nid, ..)| nid == id)
                    .map(|(_, nw, nh, nrgba)| (*nw, *nh, nrgba.as_slice()))
                    .unwrap_or((flat_normal.0, flat_normal.1, flat_normal.2.as_slice()));
                (
                    *id,
                    upload_material(device, queue, &material_layout, (*w, *h, rgba8), (nw, nh, nrgba)),
                )
            })
            .collect();

        // Lighting uniform (group 1): the frame's lights, rewritten each frame.
        let lights_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-lights-layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
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

        let pipeline = build_main_pipeline(
            device,
            format,
            &material_layout,
            &lights_layout,
            &shadow_sample_layout,
        );
        let shadow_pipeline = build_shadow_pipeline(device, &shadow_pass_layout);

        // Skinning: the joint-palette storage buffer (group 3), the skinned
        // pipeline, the skinned meshes (20-float streams), and the per-skinned-draw
        // instance buffer.
        let palette_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-palette-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let palette_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-joint-palette"),
            size: (PALETTE_CAP as u64) * 64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let palette_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-palette-bind-group"),
            layout: &palette_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: palette_buffer.as_entire_binding() }],
        });
        let skinned_pipeline = build_skinned_pipeline(
            device,
            format,
            &material_layout,
            &lights_layout,
            &shadow_sample_layout,
            &palette_layout,
        );
        let skinned_meshes: HashMap<u64, MeshBuffers> = skinned_mesh_set
            .iter()
            .map(|(id, vertices, indices)| (*id, upload_mesh(device, vertices, indices)))
            .collect();
        let skinned_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-skinned-instances"),
            size: SKINNED_INSTANCE_STRIDE * max_instances as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

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
            ambient,
            skinned_pipeline,
            skinned_meshes,
            skinned_instance_buffer,
            palette_buffer,
            palette_bind_group,
        }
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
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        skinned: &[SkinnedGpuDraw],
        clear: [f32; 4],
        sdf: Option<&SdfScene>,
        caps: u32,
    ) {
        // Gate the SDF raymarch pass on the frame's Sdf capability bit; a profile that
        // drops SDF renders meshes only (the same policy the Canvas 2D backend applies).
        let sdf = sdf.filter(|_| (caps & (axiom_host::RenderCapability::Sdf as u32)) != 0);
        queue.write_buffer(&self.lights_buffer, 0, &pack_lights(lights, self.ambient, caps));
        queue.write_buffer(
            &self.light_vp_buffer,
            0,
            bytemuck::cast_slice(&light_view_proj),
        );
        // Upload the SDF uniform on frames that carry a scene (zero-or-one, via the
        // Option iterator — no `if`).
        sdf.into_iter()
            .for_each(|scene| queue.write_buffer(&self.sdf_uniform_buffer, 0, &pack_sdf(scene)));

        // Pack instances back-to-back; record each batch's (mesh, material, byte
        // offset, count), capped at the instance-buffer capacity.
        let mut packed: Vec<f32> = Vec::new();
        let mut draws: Vec<(u64, u64, u64, u32)> = Vec::new();
        let mut written: u32 = 0;
        for (mesh_id, material_id, instances, count) in batches {
            let room = self.max_instances.saturating_sub(written);
            let count = (*count).min(room);
            let floats = (count as usize) * INSTANCE_FLOATS;
            let byte_offset = u64::from(written) * INSTANCE_STRIDE;
            packed.extend_from_slice(&instances[..floats.min(instances.len())]);
            draws.push((*mesh_id, *material_id, byte_offset, count));
            written += count;
        }
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&packed));

        // Pack every skinned draw's palette back-to-back (recording each draw's base
        // matrix index) and its instance (mvp + world + colour + joint_base), bounded
        // by the palette capacity.
        let mut palette_floats: Vec<f32> = Vec::new();
        let mut skinned_instances: Vec<f32> = Vec::new();
        let mut skinned_draws: Vec<(u64, u64, u64)> = Vec::new();
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
        queue.write_buffer(&self.palette_buffer, 0, bytemuck::cast_slice(&palette_floats));
        queue.write_buffer(&self.skinned_instance_buffer, 0, bytemuck::cast_slice(&skinned_instances));

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
            for (mesh_id, _material_id, byte_offset, count) in &draws {
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(1, &self.lights_bind_group, &[]);
            pass.set_bind_group(2, &self.shadow_sample_bind_group, &[]);
            for (mesh_id, material_id, byte_offset, count) in &draws {
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
            pass.set_pipeline(&self.skinned_pipeline);
            pass.set_bind_group(1, &self.lights_bind_group, &[]);
            pass.set_bind_group(2, &self.shadow_sample_bind_group, &[]);
            pass.set_bind_group(3, &self.palette_bind_group, &[]);
            for (mesh_id, material_id, inst_offset) in &skinned_draws {
                if let (Some(mesh), Some(material)) =
                    (self.skinned_meshes.get(mesh_id), self.materials.get(material_id))
                {
                    pass.set_bind_group(0, material, &[]);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_vertex_buffer(1, self.skinned_instance_buffer.slice(*inst_offset..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
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

/// Build the main (lit/textured/shadowed) pipeline for colour target `format`.
fn build_main_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    material_layout: &wgpu::BindGroupLayout,
    lights_layout: &wgpu::BindGroupLayout,
    shadow_sample_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("axiom-scene-shader"),
        source: wgpu::ShaderSource::Wgsl(SCENE_WGSL.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axiom-scene-pl"),
        bind_group_layouts: &[material_layout, lights_layout, shadow_sample_layout],
        push_constant_ranges: &[],
    });
    // Per-instance attributes: mvp columns (loc 4-7), world columns (loc 8-11),
    // then colour (loc 12) — one Float32x4 every 16 bytes, derived from the
    // 36-float instance stride so the layout cannot drift from the packing.
    let instance_attrs: Vec<wgpu::VertexAttribute> = (0..9)
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
        source: wgpu::ShaderSource::Wgsl(SCENE_WGSL.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axiom-skinned-pl"),
        bind_group_layouts: &[material_layout, lights_layout, shadow_sample_layout, palette_layout],
        push_constant_ranges: &[],
    });
    // Per-vertex: pos(0) normal(1) uv(2) colour(3) joints(4) weights(5).
    let vertex_attrs = [
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 24, shader_location: 2 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 3 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 48, shader_location: 4 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 64, shader_location: 5 },
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
    // the instance buffer (loc 1-4) at the world offset within the 36-float stride.
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
    }
}

/// Build a material's albedo bind group from RGBA8 pixels (sRGB texture + repeat
/// nearest sampler), bound at group 0 (binding 0 = texture, 1 = sampler).
fn upload_material(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    albedo: (u32, u32, &[u8]),
    normal: (u32, u32, &[u8]),
) -> wgpu::BindGroup {
    // Albedo is sRGB-encoded colour; the normal map is linear data (RGB = the
    // tangent-space normal), so it uses the non-sRGB format.
    let albedo = upload_texture(device, queue, albedo.0, albedo.1, albedo.2, wgpu::TextureFormat::Rgba8UnormSrgb);
    let normal = upload_texture(
        device,
        queue,
        normal.0,
        normal.1,
        normal.2,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("axiom-material-sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        // Nearest filtering for crunchy retro 32-bit texels (hard, un-smoothed texture
        // pixels). Solid-colour materials (1x1 white) are unaffected by the filter.
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
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

/// Upload one RGBA8 texture of the given format and return its default view.
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
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-material-texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Pack the frame's lights into the std140 lighting-uniform byte layout: a
/// 48-byte header — light count `u32` + capability mask `u32` + 8 bytes padding, then the hemisphere-ambient
/// `sky` + `ground` `vec4`s (rgb, w unused) — then `MAX_LIGHTS` entries of two
/// `vec4`s — `v = (vec.xyz, kind)` and `col = (colour.rgb, intensity)`. Entries past
/// the count stay zero. Capped at `MAX_LIGHTS`.
fn pack_lights(lights: &[(u32, [f32; 3], [f32; 3], f32)], ambient: axiom_host::FrameAmbient, caps: u32) -> Vec<u8> {
    let count = lights.len().min(MAX_LIGHTS);
    let mut bytes = Vec::with_capacity(LIGHTS_UBO_BYTES as usize);
    bytes.extend_from_slice(&(count as u32).to_le_bytes());
    // The capability mask occupies the first header pad slot (the WGSL `caps` field);
    // the remaining two u32 pads stay zero.
    bytes.extend_from_slice(&caps.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
    let (sky, ground) = (ambient.sky(), ambient.ground());
    [sky[0], sky[1], sky[2], 0.0, ground[0], ground[1], ground[2], 0.0]
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
