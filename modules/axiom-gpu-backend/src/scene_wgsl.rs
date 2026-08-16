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
//! **Nothing about how a fragment is lit moved.** The Blinn-Phong model, the 5x5
//! PCF shadow lookup, the hemisphere ambient, the distance fog and the capability
//! gates are byte-for-byte what they were; the only change is where the six
//! channel values come from. A program supplies values, never a way of being lit.

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
    // view-projection; w unused. Specular is view-dependent — that is the whole
    // difference between it and the Lambert term — so the fragment stage cannot
    // compute one without knowing where the frame is being watched from.
    camera: vec4<f32>,
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

@group(2) @binding(0) var shadow_map: texture_depth_2d;
@group(2) @binding(1) var shadow_samp: sampler_comparison;
struct ShadowU { light_vp: mat4x4<f32> };
@group(2) @binding(2) var<uniform> shadow: ShadowU;

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
    var out: VsOut;
    out.clip = mvp * vec4<f32>(position, 1.0);
    out.world_pos = (world * vec4<f32>(position, 1.0)).xyz;
    out.normal = (world * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv;
    out.color = vertex_color * instance_color;
    // Emissive is NOT multiplied into `out.color`: the fragment stage modulates
    // the colour by N.L, ambient and shadow, and self-illumination must survive
    // all three. It is added after lighting, before fog.
    out.emissive = instance_emissive.rgb;
    out.specular = instance_emissive.w;
    out.object_pos = position;
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
    // Alpha cutout capability: drop fully-transparent texels (foliage leaf-alpha cards)
    // so they neither shade nor write depth; the soft 0.5..1 rim still alpha-blends.
    // Gated on the AlphaMask bit; off → the quad renders opaque.
    let cut = ((caps & CAP_ALPHAMASK) != 0u) && (albedo.a < 0.5);
    if (cut) { discard; }
    // The SURFACE PROGRAM. Six appearance channels from the draw's authored
    // material, evaluated in object space; the lighting below is unchanged and
    // consumes them exactly where it used to read the instance lanes.
    //
    // `surface_program == 0` runs the DEFAULT program, which returns what this
    // pipeline had already resolved — the sampled albedo times the vertex and
    // instance colour, the instance emissive, a flat tangent-space normal. That
    // is what makes every existing frame pixel-identical.
    //
    // `params` is the zero value: this pass binds no parameter buffer yet, and
    // the only program it runs reads none.
    let surface = axiom_surface(
        SurfaceIn(in.object_pos, in.uv, in.object_normal, 0.0, albedo * in.color, in.emissive),
        SurfaceParams(),
    );
    let base = vec4<f32>(surface.base_color.rgb, surface.opacity);
    // Perturb the geometric normal by the material's tangent-space normal map. There is
    // no per-vertex tangent, so build the cotangent frame from screen-space derivatives
    // of world position + uv (Mikkelsen). Normal-mapping capability off → a flat
    // (0,0,1) tangent-space normal, so N stays the geometric normal.
    let geo_n = normalize(in.normal);
    let nmap = select(surface.normal, textureSample(normal_tex, normal_sampler, in.uv).xyz * 2.0 - 1.0, (caps & CAP_NORMALMAP) != 0u);
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
    let N = select(geo_n, mapped, (caps & CAP_NORMALMAP) != 0u);
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
    // Specular capability off, or a matte material → strength 0, which zeroes the
    // whole term below. Evaluated (not branched around) so control flow stays
    // uniform, exactly as the other capability gates in this shader do.
    let gloss = select(0.0, in.specular, (caps & CAP_SPECULAR) != 0u);
    // Toward the eye. Blinn-Phong uses the half-vector between this and the light,
    // which is why the camera position has to reach the fragment stage at all.
    let V = normalize(lights.camera.xyz - in.world_pos);
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
        // The highlight. NOT multiplied by `base.rgb`: a specular reflection is
        // light bouncing off the surface without being absorbed, so it takes the
        // LIGHT's colour, not the surface's — which is what makes a cool moon
        // read as cool on red car paint instead of turning pink. Gated behind
        // N·L so a face turned away from the light cannot glint.
        let H = normalize(L + V);
        let facing = step(0.0, dot(N, L));
        let spec = pow(max(dot(N, H), 0.0), SPECULAR_POWER) * gloss * atten * facing;
        lit = lit + lt.col.rgb * lt.col.w * spec;
    }
    // Self-illumination, added after every light term and before fog: it is radiance
    // the surface emits, so no N.L, no ambient and no shadow attenuates it — but the
    // air between it and the camera still does. A non-emissive material contributes
    // exactly zero here, so every existing frame is byte-identical.
    let emitted = lit + surface.emission;
    // Atmospheric perspective, last: distance recedes toward the frame's fog colour.
    // This is applied AFTER lighting on purpose — fog replaces the surface's radiance,
    // it does not tint the light — which is also where the Canvas 2D backend's fog
    // post-pass sits (on the composited image), so the two agree.
    //
    // The metres of air are measured, not inferred from depth: this stage already
    // interpolates a world position and already knows where the camera is (it
    // needed both for the specular term), so the true distance is one `length`.
    let air_metres = length(in.world_pos - lights.camera.xyz);
    let fogged = mix(emitted, lights.fog_color.rgb, fog_factor(in.clip.z, air_metres));
    return vec4<f32>(fogged, base.a);
}
"#;
