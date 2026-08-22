//! The main pass's WGSL, in the two halves a **surface program** sits between.
//!
//! One shader, one split point, and the split is the whole reason this file
//! exists apart from [`crate::scene_renderer`]: the pass's fragment stage now
//! opens by calling `axiom_surface` for the fragment's six appearance channels,
//! and that function is *generated* from an authored
//! `axiom_surface::Surface` rather than written here. So the text is a prefix
//! and a suffix with a program-shaped hole between them, and
//! [`crate::surface_program::wgsl_template::scene_shader`] fills it by
//! concatenation — never by a preprocessor, never by textual substitution.
//!
//! **The lighting MATHS did not move.** The Blinn-Phong model, the 5x5 PCF
//! shadow lookup, the hemisphere ambient, the distance fog and the capability
//! gates are byte-for-byte what they were. What a generated program now also
//! supplies is a three-valued `axiom_surface::LightingModel` saying *how much of
//! that maths this surface takes* — the whole model is present in this one
//! shader and the discriminant selects between its terms with `select` and two
//! multipliers, so three models cost zero additional pipelines. The default is
//! `LambertSpecular`, whose gates are exactly one, so an existing frame is
//! unchanged to the bit.

/// The **first half** of the WGSL for the lit/textured/shadowed main pass:
/// per-vertex position+normal+uv+colour, per-instance MVP + world matrix +
/// colour, a material albedo texture (group 0), a lighting uniform (group 1),
/// and a shadow map + light view-projection (group 2). Each directional light is
/// attenuated by a PCF shadow lookup; point lights attenuate by distance.
///
/// The pass's WGSL is two halves with a **surface program** between them. The
/// split point is immediately before `fs`, which calls `axiom_surface` for the
/// fragment's six appearance channels and then runs the lighting maths on the
/// result. `crate::surface_program::wgsl_template::scene_shader` performs the
/// splice by concatenation — the same mechanism
/// `crate::surface_encode::shader_source` uses for the sRGB curve, and the only
/// shader composition this crate has.
///
/// The lighting model, the PCF, the hemisphere ambient, the fog and the tonemap
/// are **untouched** by the split: a program supplies channel values, never a way
/// of being lit.
pub(crate) const SCENE_WGSL_PREFIX: &str = r#"
@group(0) @binding(0) var albedo_tex: texture_2d<f32>;
@group(0) @binding(1) var albedo_sampler: sampler;
@group(0) @binding(2) var normal_tex: texture_2d<f32>;
@group(0) @binding(3) var normal_sampler: sampler;
// The runtime material shader's maps (`crate::material_shader`). Linear data,
// all three, sampled through the samplers above. A material that authors none
// of them binds a neutral 1x1 (see `scene_renderer`'s `neutral_*` constants), so
// declaring them here changes no existing pixel — the default surface program
// never names them.
@group(0) @binding(4) var material_orm_tex: texture_2d<f32>;
@group(0) @binding(5) var material_detail_tex: texture_2d<f32>;
@group(0) @binding(6) var material_macro_tex: texture_2d<f32>;

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
    // Atmospheric depth fog (the frame's `axiom_host::FrameDepthFog`): rgb = the
    // colour distance recedes toward, w = the maximum mix fraction. A frame that
    // carries no fog packs w = 0, so `fog_factor` is 0 for every fragment and the
    // whole term is an exact no-op.
    fog_color: vec4<f32>,
    // x = fog start, y = fog full-density depth (both normalized device depth);
    // z = the Beer-Lambert extinction rate per world metre (0 = no air term, the
    // default, which makes this an exact no-op); w unused.
    fog_range: vec4<f32>,
    // xyz = the camera's world position, recovered on the CPU from the frame's
    // view-projection. Specular is view-dependent — that is the whole
    // difference between it and the Lambert term — so the fragment stage cannot
    // compute one without knowing where the frame is being watched from.
    //
    // w = the frame's SURFACE TIME in seconds: `axiom_host::FramePacket::time`,
    // explicitly supplied engine time and never a wall clock, in what used to be
    // a pad lane. It rides this uniform rather than one of its own because this
    // uniform is already written once per frame, so a time-varying surface costs
    // no extra write at all — and a frame whose surfaces read no clock packs an
    // exact zero here, which is the byte the lane always held.
    camera: vec4<f32>,
    // ---- the two-band indirect fill (`axiom_host::FrameIndirect`) ---------
    //
    // The term that stops geometry the key light does not reach from collapsing
    // to black. `sky`/`ground` above are a hemisphere AMBIENT — one `mix` by the
    // normal's up-component — which cannot say "a vertical wall sees half the sky
    // dome" and carries no warm ground bounce at all. These are two separately
    // gated bands, and they are what a shaded facade is actually lit by.
    //
    // Every lane is ZERO for a frame that authors no fill, and every term they
    // feed is an add or a multiply, so such a frame renders byte-identically to
    // one from before these lanes existed.
    //
    // rgb = the level-folded band colour; w unused.
    fill_sky: vec4<f32>,
    fill_ground: vec4<f32>,
    // x = band gain, y = sun-bounce gain; zw unused.
    fill_gain: vec4<f32>,
    // x = the image-based diffuse budget, y = the indirect floor inside an
    // interior volume, z = the live room count (0 until an app builds volumes,
    // which degrades the interior gate to its AO arm exactly as the source does
    // before the world appears); w unused.
    fill_indirect: vec4<f32>,
    // The two band gates and the AO strengths. Constants of the algorithm that
    // the source nonetheless carries as uniforms — `owFillDir` is never written
    // and `owAoStrength` never after construction — so they ride here rather
    // than as WGSL literals: `crate::indirect_lighting`'s Rust constants stay
    // the single definition, and no second copy can drift from them.
    fill_dir: vec4<f32>,
    fill_ao_strength: vec4<f32>,
    // xyz = the unit direction TOWARD the key light, which the sun-bounce wrap
    // negates to find the anti-sun hemisphere. Zero on a frame with no
    // directional light, which makes the wrap a constant and harmless.
    fill_sun_dir: vec4<f32>,
    items: array<Light, 16>,
};
@group(1) @binding(0) var<uniform> lights: Lights;

// Capability bits mirrored from axiom_host::RenderCapability (pinned by the host's
// `capability_bits_are_the_gpu_shader_contract` test): the per-fragment features
// this main pass gates.
const CAP_TEXTURES: u32 = 1u;
const CAP_ALPHAMASK: u32 = 2u;
const CAP_NORMALMAP: u32 = 4u;
const CAP_SHADOWS: u32 = 8u;
const CAP_SPECULAR: u32 = 512u;
const CAP_AERIAL: u32 = 2048u;

// `axiom_host::FrameDepthFog::mix_fraction`, mirrored. That function is the
// definition and this is the copy; the Rust side is the one with tests.
//
// Two terms, composed as independent extinction (`1 - (1-a)*(1-b)`):
//
// * the **screen ramp** on normalized device depth, which the Canvas 2D
//   post-pass also runs on its z-buffer — the part that keeps the two backends'
//   horizons in parity;
// * the **air term**, Beer-Lambert on the fragment's world distance from the
//   camera. NDC depth is hyperbolic in distance, so a ramp linear in it spends
//   its whole range on the near field and switches at one screen row; this term
//   is the one that can grade a ground plane running to the horizon.
//
// The air term is `axiom_host::RenderCapability::AerialPerspective`, which this
// backend has (it holds `world_pos` and the camera) and Canvas 2D does not. The
// gate is expressed as *zeroing the rate*, so a profile without the capability
// evaluates the screen ramp alone — bit-identical to the declared substitute —
// rather than taking a second code path.
//
// Degenerate inputs are safe: the span is floored before the divide, the rate
// and the distance are floored at zero, and the result is clamped.
fn fog_factor(ndc_depth: f32, view_distance: f32) -> f32 {
    let span = max(abs(lights.fog_range.y - lights.fog_range.x), 1e-6);
    let screen = clamp((ndc_depth - lights.fog_range.x) / span, 0.0, 1.0);
    let rate = max(lights.fog_range.z, 0.0) * f32((lights.caps & CAP_AERIAL) != 0u);
    let air = 1.0 - exp2(-rate * max(view_distance, 0.0));
    let combined = 1.0 - (1.0 - screen) * (1.0 - air);
    return combined * clamp(lights.fog_color.w, 0.0, 1.0);
}

// How tight a specular highlight is. One engine gloss profile, because the
// instance payload has exactly one free lane and it is spent on *strength*: how
// much a surface catches the highlight is the axis that separates tarmac from
// car paint from chrome, while how tight it is barely moves between them at this
// art scale. 48 is a broad, wet-road sheen rather than a pinpoint glint — which
// is what a low moon on damp asphalt actually looks like.
const SPECULAR_POWER: f32 = 48.0;

// ---------------------------------------------------------------------------
// THE PHYSICAL BRDF — `axiom_surface::LightingModel::Physical`.
//
// three.js r180's `MeshStandardMaterial` maths, transcribed from the GLSL TEXT
// of `ShaderChunk/common.glsl.js` (`BRDF_Lambert`, `F_Schlick`, `RECIPROCAL_PI`,
// `EPSILON`, `saturate`, `pow2`) and
// `ShaderChunk/lights_physical_pars_fragment.glsl.js` (`V_GGX_SmithCorrelated`,
// `D_GGX`, `BRDF_GGX`).
//
// **The source's grouping is the specification.** Float multiply and add are not
// associative, so `F * ( V * D )` is a different number from `F * V * D`, and
// `RECIPROCAL_PI * a2 / pow2( denom )` is a different number from
// `RECIPROCAL_PI * ( a2 / pow2( denom ) )`. Neither division below is rewritten
// as a reciprocal-multiply and no multiply chain is re-associated; the
// parentheses here are the source's own.
//
// `saturate`, `clamp` and `mix` are written out rather than called. GLSL pins
// their factoring (`clamp` is `min(max(x, lo), hi)`, `mix` is `x*(1-a) + y*a`)
// and WGSL explicitly permits its builtins to factor differently, so calling the
// builtin would hand the driver a licence the source never gave it. That is the
// same rule `crate::surface_program::emit` follows for the field algebra.
//
// This block costs a non-physical program nothing: `axiom_lighting_model()` is a
// nullary function returning a literal, so the `select` in `fs` that reaches
// these is a compile-time constant and the whole physical arm is dead-stripped.
// ---------------------------------------------------------------------------

// `common.glsl.js`: `#define RECIPROCAL_PI 0.3183098861837907`.
const AXIOM_PBR_RECIPROCAL_PI: f32 = 0.3183098861837907;
// `common.glsl.js`: `#define EPSILON 1e-6`.
const AXIOM_PBR_EPSILON: f32 = 1e-6;

// `float pow2( const in float x ) { return x*x; }`.
fn axiom_pbr_pow2(x: f32) -> f32 {
    return x * x;
}

// `#define saturate( a ) clamp( a, 0.0, 1.0 )`, with GLSL's `clamp` written out.
fn axiom_pbr_saturate(a: f32) -> f32 {
    return min(max(a, 0.0), 1.0);
}

// `vec3 BRDF_Lambert( const in vec3 diffuseColor )`. The `1/PI` that makes the
// physical model radiometric — and that the other three models do not have.
fn axiom_pbr_brdf_lambert(diffuse_color: vec3<f32>) -> vec3<f32> {
    return AXIOM_PBR_RECIPROCAL_PI * diffuse_color;
}

// `vec3 F_Schlick( const in vec3 f0, const in float f90, const in float dotVH )`.
//
// The Epic/SIGGRAPH'13 `exp2` variant, which is the code that RUNS in three; the
// classic `pow( 1.0 - dotVH, 5.0 )` sits above it commented out and is a
// different number, so taking it would be a transcription defect.
fn axiom_pbr_f_schlick(f0: vec3<f32>, f90: f32, dot_vh: f32) -> vec3<f32> {
    let fresnel = exp2((-5.55473 * dot_vh - 6.98316) * dot_vh);
    return f0 * (1.0 - fresnel) + (f90 * fresnel);
}

// `float V_GGX_SmithCorrelated( const in float alpha, const in float dotNL, const in float dotNV )`
// — Smith height-correlated, Frostbite course notes listing 2.
//
// This is **V, not G**: the `1 / (4 dotNL dotNV)` denominator of the
// Cook-Torrance form is already folded in, which is why `BRDF_GGX` below returns
// `F * ( V * D )` with no further division. The `EPSILON` floor is the source's
// and it is what keeps a grazing fragment finite.
fn axiom_pbr_v_ggx_smith_correlated(alpha: f32, dot_nl: f32, dot_nv: f32) -> f32 {
    let a2 = axiom_pbr_pow2(alpha);
    let gv = dot_nl * sqrt(a2 + (1.0 - a2) * axiom_pbr_pow2(dot_nv));
    let gl = dot_nv * sqrt(a2 + (1.0 - a2) * axiom_pbr_pow2(dot_nl));
    return 0.5 / max(gv + gl, AXIOM_PBR_EPSILON);
}

// `float D_GGX( const in float alpha, const in float dotNH )` — Trowbridge-Reitz,
// "Microfacet Models for Refraction through Rough Surfaces" equation (33).
//
// `alpha` is **roughness squared** (Disney's reparameterisation), and the squaring
// happens in `axiom_pbr_brdf_ggx` exactly where the source does it.
fn axiom_pbr_d_ggx(alpha: f32, dot_nh: f32) -> f32 {
    let a2 = axiom_pbr_pow2(alpha);
    let denom = axiom_pbr_pow2(dot_nh) * (a2 - 1.0) + 1.0;
    return AXIOM_PBR_RECIPROCAL_PI * a2 / axiom_pbr_pow2(denom);
}

// `vec3 BRDF_GGX( const in vec3 lightDir, const in vec3 viewDir, const in vec3 normal, const in PhysicalMaterial material )`.
//
// The source reads exactly three fields off `PhysicalMaterial`, so they are
// passed directly rather than dragging a struct across: this port has no
// clearcoat, no iridescence, no anisotropy and no sheen, so every `#ifdef` inside
// the source function takes its `#else` arm and the body below is what remains.
fn axiom_pbr_brdf_ggx(
    light_dir: vec3<f32>,
    view_dir: vec3<f32>,
    normal: vec3<f32>,
    f0: vec3<f32>,
    f90: f32,
    roughness: f32,
) -> vec3<f32> {
    let alpha = axiom_pbr_pow2(roughness); // UE4's roughness
    let half_dir = normalize(light_dir + view_dir);
    let dot_nl = axiom_pbr_saturate(dot(normal, light_dir));
    let dot_nv = axiom_pbr_saturate(dot(normal, view_dir));
    let dot_nh = axiom_pbr_saturate(dot(normal, half_dir));
    let dot_vh = axiom_pbr_saturate(dot(view_dir, half_dir));
    let f = axiom_pbr_f_schlick(f0, f90, dot_vh);
    let v = axiom_pbr_v_ggx_smith_correlated(alpha, dot_nl, dot_nv);
    let d = axiom_pbr_d_ggx(alpha, dot_nh);
    return f * (v * d);
}

@group(2) @binding(0) var shadow_map: texture_depth_2d;
@group(2) @binding(1) var shadow_samp: sampler_comparison;
struct ShadowU {
    light_vp: mat4x4<f32>,
    // **The sun.** `xyz` = the normalized direction TOWARD it in world space;
    // `w` = 1.0 when the contact-shadow chain ran for this frame, 0.0 otherwise
    // (`materialpatch.js`'s `owFeat.y`).
    //
    // It rides the shadow uniform rather than the lights uniform because this
    // buffer already IS the shadow-casting directional light's — `light_vp` is
    // that light's view-projection — so "which light is the sun" is answered
    // beside the matrix that answers "where does the sun see from". The frame's
    // FIRST directional light is the one packed; see `crate::scene_renderer`.
    sun: vec4<f32>,
};
@group(2) @binding(2) var<uniform> shadow: ShadowU;
// The resolved screen-space ambient occlusion, half resolution, `.r` = visibility.
// A 1x1 white texture when the device runs no occlusion chain, so the multiply
// below is exactly one and such a frame is byte-identical to one from before this
// existed. In group 2 with the shadow map because both are screen-space lighting
// inputs — and because WebGPU's default `maxBindGroups` is four, so 0..3 is the
// whole budget.
// How much of the occlusion the DIRECT term takes — `materialpatch.js`'s
// `owAoStrength.x * 0.35`, with the strength at one.
const AO_MICRO_SHADOW: f32 = 0.35;
@group(2) @binding(3) var gtao_tex: texture_2d<f32>;
@group(2) @binding(4) var gtao_samp: sampler;
// The resolved screen-space CONTACT SHADOW, **full** resolution, `.r` = the
// multiplier the sun term takes and `.g` the view depth its bilateral needed.
// The same 1x1 white texture as the occlusion above when the chain did not run,
// so the multiply is exactly one.
//
// Full resolution, not half: a contact shadow is the last few centimetres of
// occlusion in the seam where a prop meets the floor — a handful of pixels — and
// half-resolving it removes the only thing it had to say. It shares `gtao_samp`
// because the fetch lands on texel centres at 1:1, where linear and nearest are
// the same sample, and a second sampler for an identical result is a binding
// slot spent on nothing.
@group(2) @binding(5) var contact_tex: texture_2d<f32>;

// `materialpatch.js`'s `owContactShadow`, transcribed:
//
//   float owContactShadow( vec3 lightDirView ) {
//     if ( owFeat.y < 0.5 ) return 1.0;
//     if ( dot( lightDirView, owSunDirView ) < 0.999 ) return 1.0;
//     return texture2D( owContactTex, gl_FragCoord.xy * owScreenTexel ).r;
//   }
//
// **The contact ray runs along ONE direction, so the term is meaningless for any
// other light.** Dropping the `0.999` test would silently darken every point
// light and every second directional in the frame by the sun's contact shadow.
// `crate::contact::contact_shadow_for_light` is the Rust definition of this
// function and `crate::contact`'s own WGSL a second transcription of it; this is
// the copy the main pass calls, written as a multiplier so control flow stays
// uniform for the derivative-dependent work around it.
//
// The dot is taken in WORLD space here and in view space in the source. A dot of
// two unit vectors is invariant under the view rotation, so it is the same test
// with one fewer transform — and the sun this compares against arrives already
// normalized (`shadow.sun.xyz`), or exactly zero when the frame has no
// directional light at all, which fails the test for every light.
const CONTACT_SUN_DOT_THRESHOLD: f32 = 0.999;
fn axiom_contact_shadow(enabled: f32, dot_light_sun: f32, sampled: f32) -> f32 {
    let applies = (enabled >= 0.5) && (dot_light_sun >= CONTACT_SUN_DOT_THRESHOLD);
    // `select` takes the VALUE, so an applying light gets `sampled` bit for bit —
    // a `mix( 1.0, sampled, applies )` would return `1 + (sampled - 1)`, which is
    // a different float.
    return select(1.0, sampled, applies);
}

// Skinning: the joint-matrix palette for the skinned pass (group 3). All skinned
// draws' palettes are concatenated; each draw's per-instance `joint_base` indexes
// the start of its own palette. Bound only by the skinned pipeline.
// The joint palette lives in a TEXTURE, not a storage buffer, and that is a
// portability decision rather than a stylistic one: a vertex-stage storage
// buffer is a WebGPU-class capability that WebGL2 does not have at all, so a
// storage palette means no skinned geometry on the fallback arm - a browser
// showing the rigid half of a scene and silently dropping every character in
// it. Vertex texture fetch, by contrast, is guaranteed by GLES 3.0 (>= 16
// vertex texture units), so this reads on every backend the engine targets.
//
// One matrix is four consecutive RGBA32F texels - its four columns - laid out
// row-major across `PALETTE_ROW_TEXELS` texels per row. `textureLoad` takes an
// exact texel, so no filtering (and no float-linear extension) is involved.
@group(3) @binding(0) var joint_palette: texture_2d<f32>;

// The SURFACE PARAMETER REGION (group 3, binding 1): the tunable numbers an
// authored surface declares, as `crate::surface_program::params` lays them out.
//
// It is a real binding now. It used to be the zero value `SurfaceParams()`
// passed by both stages, which is why a lighting model had to be a value the
// program STATES rather than a lane it reads — a zero lane decodes as `Unlit`
// and would have unlit every frame in the engine. The model stays a stated value
// (it costs nothing and it cannot default to zero by accident); what the binding
// buys is that a `Param`-driven graph finally reads the number its author
// declared, and that animating that number is a `write_buffer` rather than a
// pipeline compile.
//
// **One buffer per program, never one buffer rewritten between draws.**
// `crate::post_chain` records why: a `queue.write_buffer` is ordered against
// *submission*, not against the passes inside an encoder, so N writes to one
// buffer leave every draw in that pass reading the last of them. Each compiled
// program therefore owns its own 512-byte buffer and its own bind group, all
// built against ONE shared layout — which is what keeps groups 1 (`lights`) and
// 2 (`shadow_sample`) set exactly once per pass even when the pipeline changes.
//
// Group 3 is shared with the joint palette above rather than taking a fourth
// group of its own: `wgpu::Limits::downlevel_webgl2_defaults` guarantees only
// four bind groups, and the skinned pass already spends the last one. The two
// bindings are disjoint, the rigid pass uses only this one, and naga resolves
// per entry point — so neither pipeline is asked for a resource it does not read.
@group(3) @binding(1) var<uniform> surface_params: SurfaceParams;

// Joint matrix `index`, unpacked from its four texels.
fn joint_matrix(index: u32) -> mat4x4<f32> {
    let width = textureDimensions(joint_palette).x;
    let base = index * 4u;
    return mat4x4<f32>(
        textureLoad(joint_palette, vec2<u32>((base + 0u) % width, (base + 0u) / width), 0),
        textureLoad(joint_palette, vec2<u32>((base + 1u) % width, (base + 1u) / width), 0),
        textureLoad(joint_palette, vec2<u32>((base + 2u) % width, (base + 2u) / width), 0),
        textureLoad(joint_palette, vec2<u32>((base + 3u) % width, (base + 3u) / width), 0),
    );
}

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
    // The draw's material emissive (linear RGB). Flat across the instance; carried
    // as a varying rather than a uniform because it rides the instance buffer, and
    // the mesh pass batches many materials' instances behind one pipeline.
    @location(4) emissive: vec3<f32>,
    // The draw's specular strength, riding the emissive vec4's fourth lane —
    // which is why that lane stopped being a pad. Same reasoning as emissive: it
    // is per-material, and the mesh pass batches many materials behind one
    // pipeline, so it cannot be a uniform.
    @location(5) specular: f32,
    // The OBJECT-space position and normal — the space a surface program's
    // expressions are evaluated in (`crates/axiom-surface/ARCHITECTURE.md`).
    // `world_pos` and `normal` above are world-space and cannot stand in: a
    // world-space pattern swims when the object moves. Two more interstage
    // locations (7 of the 16 a downlevel target guarantees) and six more
    // components; no new VERTEX ATTRIBUTE, which is the budget that is actually
    // full at 16 of 16.
    @location(6) object_pos: vec3<f32>,
    @location(7) object_normal: vec3<f32>,
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
    // rgb = the material's self-illumination; w = its specular strength.
    @location(13) instance_emissive: vec4<f32>,
) -> VsOut {
    let mvp = mat4x4<f32>(m0, m1, m2, m3);
    let world = mat4x4<f32>(w0, w1, w2, w3);
    // The VERTEX half of the surface program. Object-space offset in object
    // space, added BEFORE the MVP multiply — a displacement is a change to the
    // shape, not to where the shape is looked at from.
    //
    // Every argument is something this stage already had: three of the four
    // per-vertex attributes it has always bound, the frame's surface time from
    // the lighting uniform, and the shared parameter region. No new vertex
    // attribute — the rigid pipeline binds 14 of the 16 a WebGL2 downlevel
    // target guarantees, and a 17th would fail pipeline creation there.
    //
    // `surface_program == 0` runs the DEFAULT program, which returns an exact
    // zero, so the vertex this transforms is the vertex it was handed and every
    // existing frame is unchanged.
    //
    // The NORMAL is deliberately NOT recomputed. A displaced vertex's true
    // normal needs its neighbours' displaced positions, which this stage cannot
    // see; an author who needs a correct shading normal derives one analytically
    // from the same field and binds it to the `Normal` channel
    // (`axiom_surface::SurfaceBuilder::normal_from_height`). What reaches the
    // fragment stage below is the undisplaced surface's normal, which is correct
    // for small displacement and honest for large.
    let displaced = position + axiom_displace(position, normal, uv, lights.camera.w, surface_params);
    var out: VsOut;
    out.clip = mvp * vec4<f32>(displaced, 1.0);
    out.world_pos = (world * vec4<f32>(displaced, 1.0)).xyz;
    out.normal = (world * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv;
    out.color = vertex_color * instance_color;
    // Emissive is NOT multiplied into `out.color`: the fragment stage modulates
    // the colour by N.L, ambient and shadow, and self-illumination must survive
    // all three. It is added after lighting, before fog.
    out.emissive = instance_emissive.rgb;
    out.specular = instance_emissive.w;
    // The DISPLACED object-space position: that is where the surface actually
    // is, so a pattern authored over it rides the deformation instead of sliding
    // through it — the same reasoning `vs_skinned` applies to its post-skin
    // position below.
    out.object_pos = displaced;
    out.object_normal = normal;
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
    let skin = weights.x * joint_matrix(base + u32(joints.x))
             + weights.y * joint_matrix(base + u32(joints.y))
             + weights.z * joint_matrix(base + u32(joints.z))
             + weights.w * joint_matrix(base + u32(joints.w));
    // The skinned stage does NOT call `axiom_displace`, and that is a reported
    // limit rather than an oversight. This pipeline binds all 16 vertex
    // attributes the WebGL2 downlevel target guarantees (6 per-vertex + 10
    // per-instance) — the same ceiling that already costs a skinned material its
    // emissive and specular below — and the vertex is already deformed once,
    // through the joint palette. A surface that displaces is refused for this
    // path by `crate::surface_program::capability::validate` and reported as
    // `axiom_host::FrameFeature::ProceduralSurface`, so a skinned character
    // bound to a wind surface is a stated drop, never a silent no-op.
    let sp = skin * vec4<f32>(position, 1.0);
    let sn = skin * vec4<f32>(normal, 0.0);
    var out: VsOut;
    out.clip = mvp * sp;
    out.world_pos = (world * sp).xyz;
    out.normal = (world * sn).xyz;
    out.uv = uv;
    out.color = vertex_color * instance_color;
    // The SKINNED instance payload carries no emissive lane. Its pipeline already
    // binds 16 vertex attributes (6 vertex + 10 instance), which is exactly the
    // WebGL2 downlevel guarantee for MAX_VERTEX_ATTRIBS, so a 17th attribute would
    // fail pipeline creation on the browser's fallback path. A skinned material's
    // emissive therefore reads zero here — and the Canvas 2D backend reaches the
    // same zero through the same absent field, so the two backends AGREE. Carrying
    // it means repacking the skinned instance (its `joint_base` vec4 has three free
    // lanes), which is a separate change to the skinned draw contract.
    out.emissive = vec3<f32>(0.0, 0.0, 0.0);
    // The skinned instance payload carries no specular lane either, for the same
    // attribute-budget reason; a skinned material reads as fully matte.
    out.specular = 0.0;
    // A skinned vertex's object space is the POST-skin, pre-world one: that is
    // the frame the deformed surface actually lives in, so a pattern authored
    // over it rides the deformation instead of sliding through it.
    out.object_pos = sp.xyz;
    out.object_normal = sn.xyz;
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
"#;

/// The **second half** of the main pass's WGSL: the fragment stage, which calls
/// the spliced-in `axiom_surface` and feeds its result to the unchanged lighting
/// maths.
pub(crate) const SCENE_WGSL_SUFFIX: &str = r#"
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let caps = lights.caps;
    // Textures capability: sample the albedo image, or fall back to flat white (the
    // per-vertex/instance `in.color` still tints it) — the GPU peer of the Canvas 2D
    // flat degrade. `select` evaluates both arms, so the sample stays uniform.
    let sampled = textureSample(albedo_tex, albedo_sampler, in.uv);
    let albedo = select(vec4<f32>(1.0, 1.0, 1.0, 1.0), sampled, (caps & CAP_TEXTURES) != 0u);
    // The alpha cutout does NOT happen here — see the resolved-alpha block after
    // the surface program. Testing the raw texel at this point discarded on the
    // capability alone, which is a backend's "can" being read as a material's
    // "want": every textured draw in a frame got cut, including the ones whose
    // map alpha is DATA.
    // The SURFACE PROGRAM. Six appearance channels from the draw's authored
    // material, evaluated in object space; the lighting below is unchanged and
    // consumes them exactly where it used to read the instance lanes.
    //
    // `surface_program == 0` runs the DEFAULT program, which returns what this
    // pipeline had already resolved — the sampled albedo times the vertex and
    // instance colour, the instance emissive, a flat tangent-space normal. That
    // is what makes every existing frame pixel-identical.
    //
    // `params` is the program's own bound parameter region (group 3, binding 1) —
    // the draw's compiled program and its buffer are selected together, so a
    // program can never read another program's numbers. The time lane is the
    // frame's own surface time (`lights.camera.w`), which is an exact zero on a
    // frame whose surfaces read no clock. The DEFAULT program reads neither, so a
    // draw naming no surface is bit-identical whatever this buffer holds.
    let surface = axiom_surface(
        SurfaceIn(
            in.object_pos,
            in.uv,
            in.object_normal,
            lights.camera.w,
            albedo * in.color,
            in.emissive,
            // World space, for a runtime material shader's world-anchored
            // weathering and triplanar projection. All three are already on
            // hand — this stage interpolates `world_pos` for the fog and the
            // specular term, and `lights.camera` for the same — so no vertex
            // output had to grow.
            in.world_pos,
            normalize(in.normal),
            normalize(lights.camera.xyz - in.world_pos),
            in.color,
        ),
        surface_params,
    );
    // **An opaque material ignores its albedo map's alpha channel.**
    //
    // Three.js computes `diffuseColor.a = opacity * map.a` too, but a material
    // with `transparent: false` (the DEFAULT) renders with blending disabled, so
    // that alpha never reaches the blend equation. This pipeline blends
    // unconditionally, so the map's alpha was silently acting as opacity on every
    // textured draw.
    //
    // That is not hypothetical. A material bake is free to pack DATA into the
    // alpha channel precisely because an opaque material discards it — the
    // convention Three.js's own parallax/POM extensions rely on, and the one the
    // `shmup` port's bake uses (`albedo.a = height`). Honouring it as opacity
    // turned every textured wall in that app see-through by its own height field.
    //
    // So: the MATERIAL decides, and the map does not get a vote. `in.color.w` is
    // `base_color.w * opacity` (the render layer folds them together), which is
    // the same quantity `axiom_render::draw_order` calls `translucent` when it is
    // `< 1`. Deriving it identically here is what keeps a draw's SORTING and its
    // BLENDING from disagreeing — a draw sorted as opaque but blended as
    // translucent is a depth-order bug that only appears from some angles.
    //
    // An alpha-masked material is the documented exception: it opts into reading
    // the map's alpha, `discard`s below the cutoff above, and keeps the soft rim
    // that makes a foliage card's edge look sampled rather than stair-stepped.
    // **The alpha cutout, on the RESOLVED alpha — the material's own answer.**
    //
    // `CAP_ALPHAMASK` is a `BackendCapabilityProfile` bit: it states what the
    // BACKEND can do, not what a material wants. Gating the discard on it alone
    // meant a device that *could* alpha-mask alpha-masked EVERYTHING, so every
    // textured surface in the frame was cut against its own albedo alpha. For a
    // bake that packs the HEIGHT FIELD there — which this port's does, and which
    // is only legal because an opaque material discards that channel — the
    // result is holes through the ground and the walls wherever the height map
    // reads below the threshold.
    //
    // `surface.opacity` is the material's own answer, and it already carries the
    // per-material intent: the runtime material computes
    // `material_opacity * mix(1.0, alb.a, alpha_mask)`, so a material that did
    // NOT ask for masking resolves to its plain opacity and cannot be cut, while
    // a leaf card that did resolves to the texel's alpha and is. The capability
    // stays as the gate it is documented to be — a backend that cannot cut
    // simply does not.
    let cut = ((caps & CAP_ALPHAMASK) != 0u) && (surface.opacity < 0.5);
    if (cut) { discard; }
    // An opaque material ignores its albedo map's alpha channel entirely (see
    // the note on `SurfaceIn.albedo`). Three.js's `transparent: false` — the
    // DEFAULT — renders with blending disabled, so the map's alpha never reaches
    // the blend equation there either.
    let material_alpha = in.color.w;
    let opaque = material_alpha >= 1.0;
    let base = vec4<f32>(surface.base_color.rgb, select(surface.opacity, 1.0, opaque));
    // HOW this surface participates in lighting: `axiom_surface::LightingModel`,
    // stated by the same generated program that supplied the six channels above.
    //
    // All FOUR models are in THIS shader, selected by these two numbers and one
    // `select` at the end of the light loop — never by a second pipeline. That is
    // the same trade the twelve capability bits already make, and it is what
    // keeps four models times N surfaces at N programs instead of 4N. Both gates
    // below are plain multipliers rather than branches, so control flow stays
    // uniform for the derivative-dependent texture work above, exactly as
    // `fog_factor` expresses its capability gate by zeroing a rate.
    //
    // `LambertSpecular` (the default, and what every existing draw carries)
    // makes `diffuse_gate` and `specular_gate` exactly 1.0 and takes the
    // `ambient_lit` arm — an IEEE multiply by one is the identity on every
    // input, so this frame is bit-for-bit the frame this pass drew before the
    // model existed.
    //
    // `Physical` sets `specular_gate` to 0 (so the Blinn-Phong term and the
    // legacy `in.specular` instance lane are both out of the picture for it) and
    // has its whole result **replaced** by the Cook-Torrance sum at the end of
    // the light loop, so the diffuse it accumulated here is discarded rather
    // than added to. That is why the two gates did not have to change shape:
    // whichever value they produce, the final `select` takes the physical one.
    let model = axiom_lighting_model();
    let gathers = model != AXIOM_LIGHT_UNLIT;
    let diffuse_gate = f32(gathers);
    let specular_gate = f32(model == AXIOM_LIGHT_LAMBERT_SPECULAR);
    // Perturb the geometric normal by the material's tangent-space normal map. There is
    // no per-vertex tangent, so build the cotangent frame from screen-space derivatives
    // of world position + uv (Mikkelsen). Normal-mapping capability off → a flat
    // (0,0,1) tangent-space normal, so N stays the geometric normal.
    let geo_n = normalize(in.normal);
    // The two tangent-space normals COMPOSE. They used to `select`, and the
    // result was that `surface.normal` reached the lighting stage on **no path
    // at all**: with `CAP_NORMALMAP` set, `nmap` took the texture and the
    // authored normal was unused; with it clear, `nmap` took the authored normal
    // and `N` below took `geo_n` anyway. The two `select`s read the same bit and
    // took opposite arms, so an authored normal was computed and thrown away.
    // `normal_from_height` was dead on arrival for the same reason.
    //
    // Composition is UDN — sum the xy, keep the base z — with the **texture as
    // the base**. That choice is what keeps every existing frame bit-identical:
    // a surface that authors no normal carries the default `(0, 0, 1)`, so
    // `map_n.xy + (0, 0)` is `map_n.xy` and the z is the texture's own, which is
    // exactly the vector this line produced before.
    let tex_n = textureSample(normal_tex, normal_sampler, in.uv).xyz * 2.0 - 1.0;
    // No map bound -> the tangent-space identity, so the composition below is a
    // no-op rather than a fold against whatever a 1x1 flat texture happened to
    // decode to.
    let map_n = select(vec3<f32>(0.0, 0.0, 1.0), tex_n, (caps & CAP_NORMALMAP) != 0u);
    let nmap = vec3<f32>(map_n.xy + surface.normal.xy, map_n.z);
    let dp1 = dpdx(in.world_pos);
    let dp2 = dpdy(in.world_pos);
    let duv1 = dpdx(in.uv);
    let duv2 = dpdy(in.uv);
    let r1 = cross(dp2, geo_n);
    let r2 = cross(geo_n, dp1);
    let inv_det = 1.0 / max(dot(dp1, r1), 0.0001);
    let tangent = (r1 * duv1.x + r2 * duv2.x) * inv_det;
    let bitangent = (r1 * duv1.y + r2 * duv2.y) * inv_det;
    // The frame can be degenerate, and it must not be allowed to poison N.
    //
    // `inverseSqrt(0)` is `+inf`, and a surface whose four corners share a UV —
    // every quad drawn without explicit texture coordinates — has exactly zero uv
    // derivatives, so both tangent and bitangent are the zero vector and the
    // length is exactly 0. Multiplying that `inf` by a flat normal map's `nmap.x`
    // of `0.0` is `0 * inf`, which is **NaN**, and `normalize` of a NaN vector is
    // NaN: the fragment's whole lighting result is then undefined.
    //
    // Worse, it is undefined *per 2x2 pixel quad*, because `dpdx`/`dpdy` are
    // constant within a quad and vary between them — so the failure rasterizes as
    // a two-pixel-granularity pattern across the whole surface rather than as an
    // obvious hole, and how it resolves depends on how a given GPU handles
    // `0 * inf` and `inverseSqrt(0)`. That is why it can be invisible on one
    // device and a dense static hatch on another.
    //
    // Flooring the length keeps `inv_max` finite for every input.
    let frame_len2 = max(max(dot(tangent, tangent), dot(bitangent, bitangent)), 1.0e-12);
    let inv_max = inverseSqrt(frame_len2);
    let mapped = normalize(tangent * (nmap.x * inv_max) + bitangent * (nmap.y * inv_max) + geo_n * nmap.z);
    // And with no normal map bound, N is the geometric normal *exactly* — chosen,
    // not arrived at by hoping a degenerate frame multiplied by a zero `nmap.xy`
    // cancels itself. `select` keeps control flow uniform so the derivatives above
    // stay valid, and it takes the value rather than the arithmetic, so nothing
    // the unused arm computed can reach the lit result.
    // Gated on whether this fragment has ANY tangent-space tilt, not on the
    // capability bit — because after the composition above the tilt can come
    // from the texture, from the authored normal, or from both.
    //
    // The property the old capability gate protected is preserved exactly: with
    // no map and no authored normal, `nmap.xy` is `(0, 0)`, this takes `geo_n`
    // *itself*, and N is the geometric normal to the bit. It is still chosen
    // rather than arrived at by hoping a degenerate frame times a zero `nmap.xy`
    // cancels — which matters, because a quad whose four corners share a uv has
    // exactly zero uv derivatives.
    let N = select(geo_n, mapped, any(nmap.xy != vec2<f32>(0.0, 0.0)));
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
    // **Ambient occlusion goes on the INDIRECT term, never on the sun.**
    //
    // That is `materialpatch.js`'s central decision and it is not a detail: AO
    // approximates how much of the *sky* a point can see, so multiplying direct
    // sunlight by it darkens surfaces the sun demonstrably reaches, and the frame
    // reads as dirty rather than as occluded. The direct term instead takes a
    // fractional micro-shadow below, at `AO_MICRO_SHADOW` of full strength.
    //
    // Sampled in screen space off the fragment's own position: the chain runs at
    // half resolution over the whole target, so the uv is the fragment's pixel
    // over the target's size, not anything the mesh carries.
    // The target size comes from the AO texture itself — half resolution, so
    // twice its dimensions — rather than from a uniform. One less thing that can
    // disagree with the allocation, and it stays right if the downscale changes.
    //
    // On the 1x1 white neutral this uv runs past 1, and that is harmless: the
    // sampler clamps to edge and every texel of a white texture is white.
    let ao_uv = in.clip.xy / (vec2<f32>(textureDimensions(gtao_tex)) * 2.0);
    let ao = textureSample(gtao_tex, gtao_samp, ao_uv).r;
    // ---- the two-band indirect fill -------------------------------------
    //
    // `hemi` above is a hemisphere AMBIENT: one `mix` between two colours by the
    // normal's up-component. It cannot express that a vertical wall sees half
    // the sky dome, and it has no warm ground bounce at all — so every surface
    // the key light misses was lit by that single mix and collapsed toward
    // black. This adds the two bands the reference actually fills with: a cool
    // skylight band from above and a warm street bounce from below, each with
    // its own normal gate, plus the sun-bounce wrap that lights a shaded wall
    // from the sunlit one across the street.
    //
    // The composition is `crate::indirect_lighting` — the port of
    // `materialpatch.js`, which has been complete and called by nothing. It is
    // invoked here rather than re-derived: the module's doc names this exact
    // site as what would make it live, and its own transcription notes record
    // the divisions and the Horner nesting that a second derivation would drift
    // from.
    //
    // **`irradiance_in` is zero on purpose.** Passing `hemi` would also route it
    // through the module's `multi_bounce`, replacing this pipeline's existing
    // `* ao` on ambient with the source's AO model — a real improvement, and a
    // separate change. Passing zero asks the module for the FILL ALONE, which is
    // additive, so a frame authoring no fill is byte-identical to one from
    // before this existed.
    var fill_u: AxiomIndirectU;
    fill_u.ao_strength = lights.fill_ao_strength;
    fill_u.sky_fill = lights.fill_sky;
    fill_u.ground_fill = lights.fill_ground;
    fill_u.fill_gain = lights.fill_gain;
    fill_u.fill_dir = lights.fill_dir;
    // y = the interior floor, z = the live room count (zero → the interior gate
    // degrades to its AO arm, which is the source's own pre-world behaviour).
    fill_u.indirect = lights.fill_indirect;
    fill_u.sun_dir_world = lights.fill_sun_dir;
    let fill = axiom_indirect_apply(
        fill_u,
        vec3<f32>(0.0, 0.0, 0.0),
        vec3<f32>(0.0, 0.0, 0.0),
        vec3<f32>(0.0, 0.0, 0.0),
        base.rgb,
        surface.roughness,
        in.world_pos,
        N,
        ao,
    );
    let hemi_filled = hemi + fill.irradiance;
    let ambient_lit = base.rgb * hemi_filled * ambient_shade * ao;
    // `mix( 1.0, ao, 0.35 )` — see the direct term in the light loop.
    let ao_micro = mix(1.0, ao, AO_MICRO_SHADOW);
    // **The contact shadow, fetched once and applied to the SUN only** — the
    // choice of light happens per-light in the loop below, through
    // `axiom_contact_shadow`; the fetch has to happen out here, in uniform
    // control flow, so its implicit derivatives stay valid.
    //
    // `gl_FragCoord.xy * owScreenTexel` in the source. Full resolution, so the uv
    // is the fragment's pixel over the contact target's own dimensions — no
    // doubling, unlike the AO above. On the 1x1 white neutral this uv runs past
    // one and clamps to a white texel, which is a multiplier of exactly one.
    let contact_uv = in.clip.xy / vec2<f32>(textureDimensions(contact_tex));
    let contact = textureSample(contact_tex, gtao_samp, contact_uv).r;
    // An UNLIT surface gathers nothing: its base colour is presented as authored,
    // with no ambient, no sun, no shadow and no highlight. `select` takes the
    // VALUE, so nothing the unused arm computed can reach the result, and both
    // arms are evaluated so control flow stays uniform.
    var lit = select(base.rgb, ambient_lit, gathers);
    // Fabric transmission, accumulated alongside the light loop.
    // `crate::material_shader::cloth` ports `OW_CLOTH_LIGHT`, which the source
    // expands once per directional light. Zero for every surface that does not
    // author `transmission`, which is every surface today.
    var trans_sum = vec3<f32>(0.0, 0.0, 0.0);
    // Specular capability off, or a matte material → strength 0, which zeroes the
    // whole term below. Evaluated (not branched around) so control flow stays
    // uniform, exactly as the other capability gates in this shader do.
    // ... and a surface whose model is not `LambertSpecular` zeroes it too: the
    // model gate multiplies the capability gate rather than replacing it, so a
    // highlight needs BOTH the backend able to draw one and the surface asking.
    let gloss = select(0.0, in.specular, (caps & CAP_SPECULAR) != 0u) * specular_gate;
    // Toward the eye. Blinn-Phong uses the half-vector between this and the light,
    // which is why the camera position has to reach the fragment stage at all.
    let V = normalize(lights.camera.xyz - in.world_pos);
    // ---- The PHYSICAL model's material ------------------------------------
    //
    // three's `ShaderChunk/lights_physical_fragment.glsl.js`, transcribed. This
    // is where `SurfaceOut.roughness` and `SurfaceOut.metallic` stop being
    // decorative: under the three models above, specular strength comes from the
    // instance-stream `in.specular` lane (derived from the LEGACY
    // `Material::roughness`) and metalness is read by nothing at all. Under
    // `AXIOM_LIGHT_PHYSICAL` the instance lane is not consulted — `gloss` is
    // already zero for this model, because `specular_gate` is `model ==
    // LAMBERT_SPECULAR` — and the two authored channels drive the BRDF instead.
    //
    // `metalnessFactor` is NOT clamped, because the source does not clamp it.
    // Roughness needs no clamp: `max(., 0.0525)` and `min(., 1.0)` below are the
    // source's own, and between them they bound it on both sides.
    let metalness_factor = surface.metallic;
    let roughness_factor = surface.roughness;
    // `material.diffuseColor = diffuseColor.rgb * ( 1.0 - metalnessFactor );`
    let phys_diffuse_color = base.rgb * (1.0 - metalness_factor);
    // `vec3 dxy = max( abs( dFdx( nonPerturbedNormal ) ), abs( dFdy( nonPerturbedNormal ) ) );`
    // `float geometryRoughness = max( max( dxy.x, dxy.y ), dxy.z );`
    //
    // Specular anti-aliasing: a normal that swings hard across one pixel quad is
    // a rougher surface at that pixel than the material says, and without this a
    // glancing edge boils. `nonPerturbedNormal` is the normal BEFORE normal
    // mapping, which is `geo_n` here. Uniform control flow, so the derivatives
    // are valid — the same discipline the cotangent frame above keeps.
    let dxy = max(abs(dpdx(geo_n)), abs(dpdy(geo_n)));
    let geometry_roughness = max(max(dxy.x, dxy.y), dxy.z);
    // `material.roughness = max( roughnessFactor, 0.0525 );`  // base mip of a 256 cubemap
    // `material.roughness += geometryRoughness;`
    // `material.roughness = min( material.roughness, 1.0 );`
    let phys_roughness = min(max(roughness_factor, 0.0525) + geometry_roughness, 1.0);
    // `material.specularColor = mix( vec3( 0.04 ), diffuseColor.rgb, metalnessFactor );`
    // `material.specularF90 = 1.0;`
    //
    // The non-`IOR` arm, which is the arm `MeshStandardMaterial` compiles: it
    // declares no `ior`, no `specularIntensity` and no `specularColor` uniform.
    // `mix` written out as GLSL defines it, and note it mixes toward the
    // PRE-metalness `diffuseColor.rgb`, not toward `material.diffuseColor`.
    let phys_specular_color =
        vec3<f32>(0.04, 0.04, 0.04) * (1.0 - metalness_factor) + base.rgb * metalness_factor;
    let phys_specular_f90 = 1.0;
    // `RE_IndirectDiffuse_Physical`. The frame's hemisphere ambient IS three's
    // `getHemisphereLightIrradiance` — `mix( groundColor, skyColor, 0.5 * dotNL +
    // 0.5 )`, the identical expression `hemi` computes above — so it enters the
    // physical model as irradiance and takes `BRDF_Lambert`, which is where the
    // `1/PI` the other models lack comes from. `ambient_shade` is the engine's
    // own shadowed-ambient contrast and stays where it is.
    let phys_indirect_diffuse = (hemi_filled * ambient_shade) * axiom_pbr_brdf_lambert(phys_diffuse_color);
    // Two accumulators, not one: three keeps `reflectedLight.directDiffuse` and
    // `.directSpecular` apart across every light and sums them only at the end,
    // and float addition is not associative, so folding them per-light would be a
    // different number.
    var phys_direct_diffuse = vec3<f32>(0.0, 0.0, 0.0);
    var phys_direct_specular = vec3<f32>(0.0, 0.0, 0.0);
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
        // **`directLight.color *= owContactShadow( lightDirView );`** —
        // `materialpatch.js`'s injection into `lights_fragment_begin`, which
        // multiplies the LIGHT, so every term this light feeds (Lambert, the
        // Blinn-Phong highlight and the physical BRDF's irradiance) takes it
        // together. `atten` is exactly that factor here.
        //
        // The gate inside picks the sun out of the loop, so a point light and a
        // second directional both receive an exact 1.0. Deliberately NOT applied
        // to `trans_sum` below, for the reason stated there: cloth transmission
        // gathers the light afresh rather than through the occluded term.
        atten = atten * axiom_contact_shadow(shadow.sun.w, dot(L, shadow.sun.xyz), contact);
        let diffuse = max(dot(N, L), 0.0) * atten * diffuse_gate;
        // Directional lights only, and deliberately NOT shadowed: the source
        // gathers the light afresh here rather than reusing the
        // shadow-attenuated term, because cloth transmits light that reaches its
        // far side. Occlusion arrives instead through the surface's own
        // `transmission` channel. `1.0 - step(0.5, lt.v.w)` is the
        // directional-only gate written as a multiplier rather than a branch, so
        // a point light contributes an exact zero and control flow stays uniform
        // for the derivative-dependent work above.
        let dir_only = 1.0 - step(0.5, lt.v.w);
        trans_sum = trans_sum + axiom_cloth_light(N, V, L, lt.col.rgb * lt.col.w * dir_only);
        // The direct term takes only a FRACTION of the occlusion — the source's
        // `mix( 1.0, owSampleAO(), owAoStrength.x * 0.35 )`. It is a micro-shadow
        // (contact dirt in a crease the sun still reaches), not occlusion: at
        // full strength the sun would be darkened by a term that measures sky
        // visibility, and lit faces would read as grimy.
        lit = lit + base.rgb * lt.col.rgb * lt.col.w * diffuse * ao_micro;
        // The highlight. NOT multiplied by `base.rgb`: a specular reflection is
        // light bouncing off the surface without being absorbed, so it takes the
        // LIGHT's colour, not the surface's — which is what makes a cool moon
        // read as cool on red car paint instead of turning pink. Gated behind
        // N·L so a face turned away from the light cannot glint.
        let H = normalize(L + V);
        let facing = step(0.0, dot(N, L));
        let spec = pow(max(dot(N, H), 0.0), SPECULAR_POWER) * gloss * atten * facing;
        lit = lit + lt.col.rgb * lt.col.w * spec;
        // `void RE_Direct_Physical( ... )`, transcribed:
        //
        //   float dotNL = saturate( dot( geometryNormal, directLight.direction ) );
        //   vec3 irradiance = dotNL * directLight.color;
        //   reflectedLight.directSpecular += irradiance * BRDF_GGX( ... );
        //   reflectedLight.directDiffuse  += irradiance * BRDF_Lambert( material.diffuseColor );
        //
        // In three, `directLight.color` reaches this function with the light's
        // intensity, its distance attenuation (`getPointLightInfo`) and its
        // shadow mask (`lights_fragment_begin`) already multiplied in. Here that
        // is `lt.col.rgb * lt.col.w * atten` — the same three factors, in the
        // same left-to-right order the two legacy terms above already use, so
        // the two models see the same light.
        //
        // Specular before diffuse, because that is the order the source
        // accumulates them in.
        let phys_light_color = lt.col.rgb * lt.col.w * atten;
        let phys_dot_nl = axiom_pbr_saturate(dot(N, L));
        let phys_irradiance = phys_dot_nl * phys_light_color;
        phys_direct_specular = phys_direct_specular
            + phys_irradiance
                * axiom_pbr_brdf_ggx(
                    L,
                    V,
                    N,
                    phys_specular_color,
                    phys_specular_f90,
                    phys_roughness,
                );
        phys_direct_diffuse =
            phys_direct_diffuse + phys_irradiance * axiom_pbr_brdf_lambert(phys_diffuse_color);
    }
    // `meshphysical.glsl.js`'s own combination, in its own order:
    //
    //   vec3 totalDiffuse  = reflectedLight.directDiffuse  + reflectedLight.indirectDiffuse;
    //   vec3 totalSpecular = reflectedLight.directSpecular + reflectedLight.indirectSpecular;
    //   vec3 outgoingLight = totalDiffuse + totalSpecular + totalEmissiveRadiance;
    //
    // `indirectSpecular` is an **exact zero** here, and it is an exact zero in
    // three too whenever there is no environment map: without one, `radiance` and
    // `iblIrradiance` are both `vec3( 0.0 )`, so `RE_IndirectSpecular_Physical`
    // contributes nothing. This pass has no environment probe, so the term is
    // omitted rather than approximated — an image-based specular is its own
    // capability with its own probe, not a line here.
    let phys_total_diffuse = phys_direct_diffuse + phys_indirect_diffuse;
    let phys_total_specular = phys_direct_specular;
    // The fourth model lands the same way the other three do: **one shader, a
    // `select` on a value, never a second pipeline.** `axiom_lighting_model()`
    // returns a literal, so this condition is a per-program compile-time
    // constant — a Lambert program dead-strips the whole physical arm, and a
    // physical one dead-strips the Blinn-Phong arm. Four models across N surfaces
    // is still N programs.
    //
    // `select` takes the VALUE, so nothing the unused arm computed can reach the
    // result, and every legacy line above is untouched: a non-physical fragment
    // is bit-for-bit the fragment this pass drew before the model existed.
    //
    // `outgoingLight`'s emissive third term is `surface.emission`, added below
    // where it always was — it is model-independent, and three adds it in the
    // same place.
    lit = select(lit, phys_total_diffuse + phys_total_specular, model == AXIOM_LIGHT_PHYSICAL);
    // Self-illumination, added after every light term and before fog: it is radiance
    // the surface emits, so no N.L, no ambient and no shadow attenuates it — but the
    // air between it and the camera still does. A non-emissive material contributes
    // exactly zero here, so every existing frame is byte-identical.
    //
    // This line is model-independent on purpose: emission is what the surface
    // RADIATES, and an UNLIT surface radiates the same. With every light term
    // gated to zero above, an unlit fragment is exactly `base_color.rgb +
    // emission` — which is the whole definition of the model.
    // Gated by `diffuse_gate`, not by a new capability bit: an UNLIT surface
    // gathers nothing, and transmission is a gather. `surface.transmission` is
    // 0.0 for every program that does not author it, so this is an exact
    // identity and every existing frame is unchanged to the bit.
    let transmitted = axiom_cloth_transmitted(trans_sum, base.rgb, surface.transmission)
        * diffuse_gate;
    let emitted = lit + transmitted + surface.emission;
    // Atmospheric perspective, last: distance recedes toward the frame's fog colour.
    // This is applied AFTER lighting on purpose — fog replaces the surface's radiance,
    // it does not tint the light — which is also where the Canvas 2D backend's fog
    // post-pass sits (on the composited image), so the two agree.
    //
    // The metres of air are measured, not inferred from depth: this stage already
    // interpolates a world position and already knows where the camera is (it
    // needed both for the specular term), so the true distance is one `length`.
    //
    // **FOG APPLIES TO EVERY LIGHTING MODEL, UNLIT INCLUDED.** That is a decision,
    // not an oversight: fog is a property of the AIR BETWEEN the surface and the
    // camera, not of how the surface responds to light. A flat-shaded unlit
    // marker or a hologram sitting a kilometre away is still seen through a
    // kilometre of atmosphere, and an unfogged unlit object in a fogged scene
    // reads as a hole punched through the world. Emission is treated the same way
    // one line above, for the same reason. An author who wants a surface exempt
    // from the air authors a frame with no fog in it.
    let air_metres = length(in.world_pos - lights.camera.xyz);
    let fogged = mix(emitted, lights.fog_color.rgb, fog_factor(in.clip.z, air_metres));
    return vec4<f32>(fogged, base.a);
}
"#;
