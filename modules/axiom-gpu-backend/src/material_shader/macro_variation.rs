//! **Macro variation** — the large-scale break-up that stops a tiled surface
//! reading as one flat colour across a whole wall.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js`, the "macro variation"
//! section of `MAIN_FRAGMENT` (source lines 402-443) plus the `macro`,
//! `macroBig` and `macroRelief` entries of `DEFAULT_PARAMS` (lines 729-749).
//!
//! ## What the layer is
//!
//! A tile repeats. At 12 m the repeat is the only thing the eye sees, so the
//! source breaks it with a **world-anchored** sample of a shared macro noise map
//! — deliberately *not* uv-anchored, because a break-up that rode the tiling
//! would repeat with it. That one sample drives three separate strengths:
//!
//! - **albedo** (`macro[1]`) — a scalar value multiplier, `0.55 + 0.92 * macro`;
//! - **roughness** (`macro[2]`) — a signed broad patch term, plus a tighter
//!   `mac1.a` term and the detail layer's micro tooth;
//! - **hue** (`macro[3]`) — a *per-channel* multiplier lerped between white and
//!   `(1.05, 1.0, 0.93)`, which is a different operation from the albedo term:
//!   it warms or cools the colour rather than darkening it, and its `t` is
//!   signed, so it extrapolates past white the other way for `mac2.r < 0.5`.
//!
//! `macro[0]` is the world scale those samples are taken at. Four strengths, not
//! one — collapsing any pair of them changes the surface.
//!
//! ## The contrast expansion
//!
//! fbm never spans `0..1`, so averaging two bands collapses toward `0.5` and the
//! anti-tiling multiply becomes a 5% wash. `macroBig[0]` expands the contrast
//! back out — **around the midpoint 0.5**, and in that order:
//!
//! ```text
//! clamp( ( mac1.r * 0.55 + mac2.b * 0.45 - 0.5 ) * contrast + 0.5, 0, 1 )
//! ```
//!
//! Subtract the midpoint, scale, add it back. Scaling first and re-centring
//! afterwards, or re-centring around the *mean of the two bands* instead of the
//! constant `0.5`, both change the surface's overall brightness rather than only
//! its contrast — which is the whole point of the term.
//!
//! ## The second band
//!
//! `macroBig` is `[contrast, bigAmplitude, bigWorldScale, unused]`. `1.0 /
//! bigWorldScale` is the macro texture's period in metres and its coarsest band
//! is a third of that, so the documented `0.028` gives ~12 m features: the
//! difference between the sun-bleached end of a facade and the damp end, the
//! signal that survives at 40 m once everything finer has mipped away. It drives
//! albedo *and* roughness, and it is the one term with its own gate.
//!
//! `macroBig[3]` is marked **unused** in the source and this port reads it
//! nowhere either — recorded here rather than silently dropped, because a
//! parameter word that exists is part of the layout even when nothing consumes
//! it.
//!
//! ## `macroRelief`
//!
//! A macro-gradient normal tilt for ruts, drifts and shallow patches at 1-4 m —
//! the tile cannot carry anything that large, so the *shading* normal is tilted
//! by the finite difference of the macro map's blue channel. It is gated twice:
//! once on the parameter (`macroRelief > 0`, a `#define` in the source), and
//! once per-fragment on **up-facing** surfaces, `owUpFace = step(0.62,
//! abs(owNw.y))`. GLSL `step(edge, x)` is `1.0` when `x >= edge`, so a face at
//! exactly `|n.y| == 0.62` **is** up-facing and does get ruts. That horizon is
//! also where `macroUv` switches projection, and both sites use the same test.
//!
//! ## Defaults of 0 disable, bit-identically
//!
//! `macroBig[1]` and `macroRelief` both default to `0`, and in the source both
//! gates are *compile-time-shaped*: `if (owMacroBig.y > 0.0)` and the
//! `OW_MACRO_RELIEF` `#define`, which `applyOwMaterial` sets iff `macroRelief >
//! 0`. Neither is a multiply-by-zero that happens to vanish:
//!
//! - the relief block re-`normalize`s the shading normal, and normalizing an
//!   already-unit vector is **not** the identity in `f32`;
//! - the relief block's albedo term, `1.0 - (mac1.b - 0.5) * 0.16 * owUpFace`,
//!   does not contain `macroRelief` at all, so a zero amplitude would not
//!   silence it.
//!
//! So both stay real gates here: a runtime `if` in the WGSL on exactly the
//! source's predicate, and a value *selection* on the CPU side (the Branchless
//! Law forbids the `if`, and selecting between two computed values is
//! bit-identical to skipping one of them). `the_disabled_defaults_are_bit_identical`
//! is the test that says so, and it asserts on bits, not on a tolerance.
//!
//! ## The one deliberate divergence: hoisted texture fetches
//!
//! The source takes its four gated samples *inside* the gates. WGSL forbids an
//! implicit-LOD `textureSample` under non-uniform control flow, and whether a
//! parameter is uniform is a property of the *call site* — a contract this layer
//! cannot enforce on the orchestrator that composes it. So the four gated
//! fetches are hoisted above their gates. Texture sampling is pure: every value
//! the gates then consume is identical, and the gates still decide whether it is
//! used. The cost is four fetches a disabled permutation would not have paid.
//!
//! ## The macro texture, and what parity actually pins
//!
//! In the source `owMacroTex` is `shared.macro`, an authored fbm map. There is
//! no such artifact to compare against here, and the layer's content is the
//! *arithmetic*, not the map — so [`MacroNoise::procedural`] fills a
//! `64 x 64` RGBA32F texture from an integer hash, and the same texels are
//! uploaded to the GPU and read by the CPU reference. Sampling is **nearest**
//! with **repeat** addressing, so a texel fetch is exact on both sides and no
//! filter-weight quantisation enters the comparison; what the parity test then
//! pins is every multiply, lerp, clamp and gate between the fetch and the
//! result. A uniform-random texel also drives `clamp` into saturation at both
//! ends, which an authored fbm map would rarely do.
//!
//! Everything is computed in `f32` on both sides, because the GPU has no choice
//! and a `f64` reference would be measuring the reference's own extra precision.

/// The WGSL for the macro-variation layer: one struct and one free function,
/// self-contained down to the texture and sampler, which WGSL permits as
/// function parameters.
///
/// The comments quote the GLSL each line was transcribed from. Every grouping is
/// the source's; float arithmetic is not associative, so the source's grouping is
/// the specification even where it reads clumsily.
pub(crate) const MACRO_VARIATION_WGSL: &str = r#"
// The macro layer's result. `albedo`, `roughness` and `shade_normal` are the
// channels it rewrites; `mac1`/`mac2`/`up_face` escape because later sections of
// the source shader read them directly (`mac2.rg` wanders the repair-patch
// lattice, `mac1.b`/`mac2.g` gate the dust wedge and ground splash, `mac2.a`
// feeds the wear mask, and `owUpFace` shares the macro horizon test).
struct AxiomMacroVariation {
    albedo: vec3<f32>,
    roughness: f32,
    shade_normal: vec3<f32>,
    up_face: f32,
    mac1: vec4<f32>,
    mac2: vec4<f32>,
};

// `world_normal` is the source's `owNw`: the normalized world normal already
// multiplied by `owFaceDir`, so a back face reads as its own front.
// `shade_normal` is `nShade`, in VIEW space, and `view_from_world` is the
// source's `mat3( viewMatrix )` — the relief tilt is built in world space and
// rotated into view space exactly as the source builds it.
fn axiom_macro_variation(
    world_pos: vec3<f32>,
    world_normal: vec3<f32>,
    albedo_in: vec3<f32>,
    roughness_in: f32,
    shade_normal_in: vec3<f32>,
    view_from_world: mat3x3<f32>,
    micro: f32,
    det_fade: f32,
    macro_p: vec4<f32>,
    macro_big: vec4<f32>,
    macro_relief: f32,
    macro_tex: texture_2d<f32>,
    macro_smp: sampler,
) -> AxiomMacroVariation {
    // float owUpFace = step( 0.62, abs( owNw.y ) );
    // GLSL `step(edge, x)` is 1.0 when x >= edge, so |n.y| == 0.62 is up-facing.
    let up_face = step(0.62, abs(world_normal.y));
    // vec2 macroUv = mix( vec2( vOwWPos.x + vOwWPos.z * 0.63, vOwWPos.y ), vOwWPos.xz,
    //                     step( 0.62, abs( owNw.y ) ) );
    // A wall is projected on a sheared xz/y plane; a floor or ceiling on world xz.
    let macro_uv = mix(
        vec2<f32>(world_pos.x + world_pos.z * 0.63, world_pos.y),
        world_pos.xz,
        up_face,
    );
    // vec4 mac1 = texture2D( owMacroTex, macroUv * owMacroP.x );
    let mac1 = textureSample(macro_tex, macro_smp, macro_uv * macro_p.x);
    // vec4 mac2 = texture2D( owMacroTex, macroUv * owMacroP.x * 0.211 + 0.37 );
    let mac2 = textureSample(macro_tex, macro_smp, macro_uv * macro_p.x * 0.211 + 0.37);

    // The four gated fetches, hoisted above their gates. Sampling is pure, so
    // the values the gates consume are unchanged; WGSL forbids an implicit-LOD
    // sample under control flow it cannot prove uniform, and uniformity here is
    // a property of the call site. See this layer's module docs.
    let big_uv = macro_uv * macro_big.z;
    let big_a = textureSample(macro_tex, macro_smp, big_uv);
    let big_b = textureSample(macro_tex, macro_smp, big_uv * 0.37 + 0.61);
    let m_uv = macro_uv * macro_p.x;
    let mhx = textureSample(macro_tex, macro_smp, m_uv + vec2<f32>(0.035, 0.0)).b;
    let mhy = textureSample(macro_tex, macro_smp, m_uv + vec2<f32>(0.0, 0.035)).b;

    // fbm never spans 0..1, so averaging two bands collapses toward 0.5 and the
    // "anti-tiling" multiply becomes a 5% wash. owMacroBig.x expands the contrast
    // back out before it is used, which is what lets a 12 m facade break up.
    // float macro = clamp( ( mac1.r * 0.55 + mac2.b * 0.45 - 0.5 ) * owMacroBig.x + 0.5, 0.0, 1.0 );
    let macro_v = clamp((mac1.r * 0.55 + mac2.b * 0.45 - 0.5) * macro_big.x + 0.5, 0.0, 1.0);
    // alb.rgb *= mix( 1.0, 0.55 + 0.92 * macro, owMacroP.y );
    var albedo = albedo_in * mix(1.0, 0.55 + 0.92 * macro_v, macro_p.y);
    var roughness = roughness_in;

    // A second, much larger band (8-16 m features): the difference between one
    // sun-bleached end of a facade and the damp end, which is the signal that
    // survives at 40 m when everything finer has mipped away.
    // if ( owMacroBig.y > 0.0 ) { ... }
    if (macro_big.y > 0.0) {
        // float big = texture2D( owMacroTex, bigUv ).r * 0.62
        //           + texture2D( owMacroTex, bigUv * 0.37 + 0.61 ).b * 0.38;
        var big = big_a.r * 0.62 + big_b.b * 0.38;
        // big = clamp( ( big - 0.5 ) * 2.3, -1.0, 1.0 );
        big = clamp((big - 0.5) * 2.3, -1.0, 1.0);
        // alb.rgb *= 1.0 + big * owMacroBig.y;
        albedo = albedo * (1.0 + big * macro_big.y);
        // orm.g = clamp( orm.g - big * owMacroBig.y * 0.55, 0.0, 1.0 );
        roughness = clamp(roughness - big * macro_big.y * 0.55, 0.0, 1.0);
    }

    // alb.rgb *= mix( vec3( 1.0 ), vec3( 1.05, 1.0, 0.93 ), ( mac2.r - 0.5 ) * owMacroP.w );
    // A HUE term, not a value term: a per-channel multiplier, signed in t. Note
    // the green lane is mix(1.0, 1.0, t), which is NOT identically 1.0 in f32 and
    // is deliberately left as the source writes it.
    albedo = albedo * mix(
        vec3<f32>(1.0, 1.0, 1.0),
        vec3<f32>(1.05, 1.0, 0.93),
        (mac2.r - 0.5) * macro_p.w,
    );
    // Roughness has to vary or nothing in the frame ever glints: a broad patch
    // term plus a tighter one, both signed, plus the micro tooth.
    // orm.g = clamp( orm.g + ( mac1.g - 0.5 ) * owMacroP.z
    //                      + ( mac1.a - 0.5 ) * 0.16
    //                      - owMicro * 0.07 * owDetFade, 0.0, 1.0 );
    roughness = clamp(
        roughness + (mac1.g - 0.5) * macro_p.z
                  + (mac1.a - 0.5) * 0.16
                  - micro * 0.07 * det_fade,
        0.0,
        1.0,
    );

    // #ifdef OW_MACRO_RELIEF  — defined iff `macroRelief > 0`, so the parameter
    // test IS the source's gate, not a re-derivation of it.
    var shade_normal = shade_normal_in;
    if (macro_relief > 0.0) {
        // Ruts, drifts and shallow patches at 1-4 m. The tile can't carry anything
        // this large, so the shading normal is tilted by the gradient of the macro
        // map — stones and swales then catch the sun instead of reading as dither.
        // vec2 mg = ( vec2( mhx, mhy ) - mac1.b ) * owMacroRelief * owUpFace;
        let mg = (vec2<f32>(mhx, mhy) - vec2<f32>(mac1.b, mac1.b)) * macro_relief * up_face;
        // vec3 tiltW = vec3( -mg.x, 0.0, -mg.y );
        var tilt_w = vec3<f32>(-mg.x, 0.0, -mg.y);
        // tiltW -= owNw * dot( owNw, tiltW );
        tilt_w = tilt_w - world_normal * dot(world_normal, tilt_w);
        // nShade = normalize( nShade + mat3( viewMatrix ) * tiltW );
        shade_normal = normalize(shade_normal + view_from_world * tilt_w);
        // alb.rgb *= 1.0 - ( mac1.b - 0.5 ) * 0.16 * owUpFace;
        albedo = albedo * (1.0 - (mac1.b - 0.5) * 0.16 * up_face);
    }

    return AxiomMacroVariation(albedo, roughness, shade_normal, up_face, mac1, mac2);
}
"#;

/// The macro noise map's edge length in texels. Square, and a power of two so
/// the repeat wrap is exact.
pub(crate) const MACRO_NOISE_SIZE: usize = 64;

/// The macro noise map, as texels. The source's `owMacroTex` is an authored fbm
/// map with no artifact to compare against here; [`MacroNoise::procedural`]
/// fills the same shape from an integer hash so both sides of the parity proof
/// read *identical* texels and what is pinned is the arithmetic around them.
pub(crate) struct MacroNoise {
    /// Row-major `MACRO_NOISE_SIZE * MACRO_NOISE_SIZE` RGBA texels.
    texels: Vec<[f32; 4]>,
}

impl MacroNoise {
    /// A deterministic fill: four independent hashed channels per texel, each in
    /// `0.0..1.0`. Every value is `n / 2^24` for an integer `n < 2^24`, so it is
    /// exact in an `f32` and survives the round trip through an `Rgba32Float`
    /// texture unchanged.
    pub(crate) fn procedural() -> MacroNoise {
        MacroNoise {
            texels: (0..MACRO_NOISE_SIZE * MACRO_NOISE_SIZE)
                .map(|index| {
                    let x = (index % MACRO_NOISE_SIZE) as u32;
                    let y = (index / MACRO_NOISE_SIZE) as u32;
                    [0_u32, 1, 2, 3].map(|lane| hashed_channel(x, y, lane))
                })
                .collect(),
        }
    }

    /// The texels, row-major, ready to upload.
    pub(crate) fn texels(&self) -> &[[f32; 4]] {
        &self.texels
    }

    /// A **nearest**, **repeat** fetch, mirroring what a WebGPU sampler with
    /// `FilterMode::Nearest` and `AddressMode::Repeat` does: the unnormalized
    /// coordinate is floored to a texel index, and the index is reduced modulo
    /// the edge length.
    pub(crate) fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let size = MACRO_NOISE_SIZE as i32;
        let x = ((u * MACRO_NOISE_SIZE as f32).floor() as i32).rem_euclid(size);
        let y = ((v * MACRO_NOISE_SIZE as f32).floor() as i32).rem_euclid(size);
        self.texels[(y * size + x) as usize]
    }
}

/// One channel of one texel: a 32-bit integer avalanche, taken to `0.0..1.0`
/// through its top 24 bits so the quotient is exact.
fn hashed_channel(x: u32, y: u32, lane: u32) -> f32 {
    let mixed = x
        .wrapping_mul(0x2545_F491)
        .wrapping_add(y.wrapping_mul(0x9E37_79B9))
        .wrapping_add(lane.wrapping_mul(0x85EB_CA6B));
    let a = mixed ^ (mixed >> 15);
    let b = a.wrapping_mul(0x2C1B_3C6D);
    let c = b ^ (b >> 12);
    let d = c.wrapping_mul(0x297A_2D39);
    let e = d ^ (d >> 15);
    (e >> 8) as f32 / 16_777_216.0
}

/// Everything the layer reads. Named after the source's own identifiers so a
/// call site stays diffable against `shader.js`.
pub(crate) struct MacroVariationIn {
    /// `vOwWPos` — the world-space fragment position.
    pub(crate) world_pos: [f32; 3],
    /// `owNw` — the normalized world normal, already `* owFaceDir`.
    pub(crate) world_normal: [f32; 3],
    /// `alb.rgb`.
    pub(crate) albedo: [f32; 3],
    /// `orm.g`.
    pub(crate) roughness: f32,
    /// `nShade`, in VIEW space.
    pub(crate) shade_normal: [f32; 3],
    /// `mat3( viewMatrix )`, as three columns.
    pub(crate) view_from_world: [[f32; 3]; 3],
    /// `owMicro` — the detail layer's signed micro tooth.
    pub(crate) micro: f32,
    /// `owDetFade` — the detail layer's distance fade.
    pub(crate) det_fade: f32,
    /// `owMacroP` = `macro`: world scale, albedo strength, roughness strength,
    /// hue strength. Four separate strengths.
    pub(crate) macro_p: [f32; 4],
    /// `owMacroBig` = `macroBig`: contrast, big-band amplitude, big-band world
    /// scale, **unused**. The fourth word is read nowhere, here or in the source.
    pub(crate) macro_big: [f32; 4],
    /// `owMacroRelief` = `macroRelief`; `0` disables the whole relief block.
    pub(crate) macro_relief: f32,
}

/// Everything the layer writes, plus the samples later sections read.
pub(crate) struct MacroVariationOut {
    /// `alb.rgb`.
    pub(crate) albedo: [f32; 3],
    /// `orm.g`.
    pub(crate) roughness: f32,
    /// `nShade`, in VIEW space.
    pub(crate) shade_normal: [f32; 3],
    /// `owUpFace` — `step(0.62, abs(owNw.y))`, shared with later sections.
    pub(crate) up_face: f32,
    /// `mac1` — read later by the dust wedge, the ground splash and the wear mask.
    pub(crate) mac1: [f32; 4],
    /// `mac2` — read later by the repair-patch lattice, the dust wedge, the
    /// ground splash, the wear mask and the grime mask.
    pub(crate) mac2: [f32; 4],
}

/// GLSL `step(edge, x)`: `1.0` when `x >= edge`, `0.0` otherwise. Not
/// `signum`, and the boundary is inclusive — at `|n.y| == 0.62` a face IS
/// up-facing.
fn step(edge: f32, x: f32) -> f32 {
    [0.0, 1.0][usize::from(x >= edge)]
}

/// GLSL/WGSL `mix(x, y, a)`, spelled as both specify it: `x*(1-a) + y*a`, never
/// `x + a*(y-x)`. The two differ in `f32`, and `mix(1.0, 1.0, a)` is not
/// identically `1.0`.
fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// GLSL/WGSL `clamp(x, lo, hi)` = `min(max(x, lo), hi)`. Written out rather than
/// `f32::clamp`, whose contract (a debug assertion on the bounds, and its own
/// NaN rule) is not the shading language's.
fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

/// A `vec3` scaled by a scalar, componentwise.
fn scale3(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

/// `dot` over three lanes, summed left to right.
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `normalize`, as the division the shading languages define it to be. A GPU is
/// free to evaluate the reciprocal square root at its own precision, which is
/// where this layer's parity budget comes from.
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let length = dot3(v, v).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

/// A column-major `mat3 * vec3`, summed left to right over the columns.
fn transform3(columns: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [0_usize, 1, 2]
        .map(|row| columns[0][row] * v[0] + columns[1][row] * v[1] + columns[2][row] * v[2])
}

/// **The CPU reference.** The semantic definition of the macro-variation layer;
/// [`MACRO_VARIATION_WGSL`] is a mirror of it, and the parity test holds the two
/// up to each other on a real adapter.
///
/// Transcribed from the GLSL text of `shader.js` lines 402-443, grouping
/// preserved. The two source gates become value *selections* here — the
/// Branchless Law forbids the `if`, and selecting the untouched value is
/// bit-identical to skipping the block that would have replaced it.
pub(crate) fn macro_variation(input: &MacroVariationIn, noise: &MacroNoise) -> MacroVariationOut {
    // float owUpFace = step( 0.62, abs( owNw.y ) );
    let up_face = step(0.62, input.world_normal[1].abs());
    // vec2 macroUv = mix( vec2( vOwWPos.x + vOwWPos.z * 0.63, vOwWPos.y ), vOwWPos.xz,
    //                     step( 0.62, abs( owNw.y ) ) );
    let macro_uv = [
        mix(
            input.world_pos[0] + input.world_pos[2] * 0.63,
            input.world_pos[0],
            up_face,
        ),
        mix(input.world_pos[1], input.world_pos[2], up_face),
    ];
    // vec4 mac1 = texture2D( owMacroTex, macroUv * owMacroP.x );
    let mac1 = noise.sample(macro_uv[0] * input.macro_p[0], macro_uv[1] * input.macro_p[0]);
    // vec4 mac2 = texture2D( owMacroTex, macroUv * owMacroP.x * 0.211 + 0.37 );
    let mac2 = noise.sample(
        macro_uv[0] * input.macro_p[0] * 0.211 + 0.37,
        macro_uv[1] * input.macro_p[0] * 0.211 + 0.37,
    );

    // float macro = clamp( ( mac1.r * 0.55 + mac2.b * 0.45 - 0.5 ) * owMacroBig.x + 0.5, 0.0, 1.0 );
    let macro_v = clamp(
        (mac1[0] * 0.55 + mac2[2] * 0.45 - 0.5) * input.macro_big[0] + 0.5,
        0.0,
        1.0,
    );
    // alb.rgb *= mix( 1.0, 0.55 + 0.92 * macro, owMacroP.y );
    let albedo_after_value = scale3(
        input.albedo,
        mix(1.0, 0.55 + 0.92 * macro_v, input.macro_p[1]),
    );

    // if ( owMacroBig.y > 0.0 ) { ... }  — computed, then selected.
    // vec2 bigUv = macroUv * owMacroBig.z;
    let big_uv = [
        macro_uv[0] * input.macro_big[2],
        macro_uv[1] * input.macro_big[2],
    ];
    // float big = texture2D( owMacroTex, bigUv ).r * 0.62
    //           + texture2D( owMacroTex, bigUv * 0.37 + 0.61 ).b * 0.38;
    let big_raw = noise.sample(big_uv[0], big_uv[1])[0] * 0.62
        + noise.sample(big_uv[0] * 0.37 + 0.61, big_uv[1] * 0.37 + 0.61)[2] * 0.38;
    // big = clamp( ( big - 0.5 ) * 2.3, -1.0, 1.0 );
    let big = clamp((big_raw - 0.5) * 2.3, -1.0, 1.0);
    let big_on = usize::from(input.macro_big[1] > 0.0);
    // alb.rgb *= 1.0 + big * owMacroBig.y;
    let albedo_after_big = [
        albedo_after_value,
        scale3(albedo_after_value, 1.0 + big * input.macro_big[1]),
    ][big_on];
    // orm.g = clamp( orm.g - big * owMacroBig.y * 0.55, 0.0, 1.0 );
    let roughness_after_big = [
        input.roughness,
        clamp(
            input.roughness - big * input.macro_big[1] * 0.55,
            0.0,
            1.0,
        ),
    ][big_on];

    // alb.rgb *= mix( vec3( 1.0 ), vec3( 1.05, 1.0, 0.93 ), ( mac2.r - 0.5 ) * owMacroP.w );
    let hue_t = (mac2[0] - 0.5) * input.macro_p[3];
    let albedo_after_hue = [
        albedo_after_big[0] * mix(1.0, 1.05, hue_t),
        albedo_after_big[1] * mix(1.0, 1.0, hue_t),
        albedo_after_big[2] * mix(1.0, 0.93, hue_t),
    ];
    // orm.g = clamp( orm.g + ( mac1.g - 0.5 ) * owMacroP.z
    //                      + ( mac1.a - 0.5 ) * 0.16
    //                      - owMicro * 0.07 * owDetFade, 0.0, 1.0 );
    let roughness = clamp(
        roughness_after_big + (mac1[1] - 0.5) * input.macro_p[2] + (mac1[3] - 0.5) * 0.16
            - input.micro * 0.07 * input.det_fade,
        0.0,
        1.0,
    );

    // #ifdef OW_MACRO_RELIEF  — computed, then selected on `macroRelief > 0`.
    // vec2 mUv = macroUv * owMacroP.x;
    let m_uv = [
        macro_uv[0] * input.macro_p[0],
        macro_uv[1] * input.macro_p[0],
    ];
    // float mhx = texture2D( owMacroTex, mUv + vec2( 0.035, 0.0 ) ).b;
    let mhx = noise.sample(m_uv[0] + 0.035, m_uv[1] + 0.0)[2];
    // float mhy = texture2D( owMacroTex, mUv + vec2( 0.0, 0.035 ) ).b;
    let mhy = noise.sample(m_uv[0] + 0.0, m_uv[1] + 0.035)[2];
    // vec2 mg = ( vec2( mhx, mhy ) - mac1.b ) * owMacroRelief * owUpFace;
    let mg = [
        (mhx - mac1[2]) * input.macro_relief * up_face,
        (mhy - mac1[2]) * input.macro_relief * up_face,
    ];
    // vec3 tiltW = vec3( -mg.x, 0.0, -mg.y );
    let tilt_raw = [-mg[0], 0.0, -mg[1]];
    // tiltW -= owNw * dot( owNw, tiltW );
    let projected = scale3(input.world_normal, dot3(input.world_normal, tilt_raw));
    let tilt_w = [
        tilt_raw[0] - projected[0],
        tilt_raw[1] - projected[1],
        tilt_raw[2] - projected[2],
    ];
    // nShade = normalize( nShade + mat3( viewMatrix ) * tiltW );
    let tilt_v = transform3(input.view_from_world, tilt_w);
    let tilted = normalize3([
        input.shade_normal[0] + tilt_v[0],
        input.shade_normal[1] + tilt_v[1],
        input.shade_normal[2] + tilt_v[2],
    ]);
    // alb.rgb *= 1.0 - ( mac1.b - 0.5 ) * 0.16 * owUpFace;
    let relief_on = usize::from(input.macro_relief > 0.0);
    let albedo = [
        albedo_after_hue,
        scale3(albedo_after_hue, 1.0 - (mac1[2] - 0.5) * 0.16 * up_face),
    ][relief_on];
    let shade_normal = [input.shade_normal, tilted][relief_on];

    MacroVariationOut {
        albedo,
        roughness,
        shade_normal,
        up_face,
        mac1,
        mac2,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        macro_variation, MacroNoise, MacroVariationIn, MACRO_NOISE_SIZE, MACRO_VARIATION_WGSL,
    };

    /// How many cases one parity run compares.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    const SAMPLES: usize = 16;

    /// The identity view basis: the relief tilt then reaches `nShade` unrotated,
    /// which is the case whose arithmetic is easiest to reason about by hand.
    const IDENTITY_VIEW: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    /// A deliberately non-axis-aligned view basis, so a transposed `mat3` or a
    /// row/column swap in either transcription shows up.
    const SKEW_VIEW: [[f32; 3]; 3] = [
        [0.6, -0.8, 0.0],
        [0.48, 0.36, -0.8],
        [0.64, 0.48, 0.6],
    ];

    /// The source's `DEFAULT_PARAMS.macro`.
    const DEFAULT_MACRO: [f32; 4] = [0.045, 0.35, 0.1, 0.35];
    /// The source's `DEFAULT_PARAMS.macroBig` — amplitude 0, i.e. band off.
    const DEFAULT_BIG: [f32; 4] = [1.0, 0.0, 0.03, 0.0];

    /// A base case every other one varies from.
    fn base() -> MacroVariationIn {
        MacroVariationIn {
            world_pos: [2.317, 1.733, -3.109],
            world_normal: [0.0, 1.0, 0.0],
            albedo: [0.62, 0.55, 0.48],
            roughness: 0.5,
            shade_normal: [0.0, 0.0, 1.0],
            view_from_world: IDENTITY_VIEW,
            micro: 0.31,
            det_fade: 0.8,
            macro_p: DEFAULT_MACRO,
            macro_big: DEFAULT_BIG,
            macro_relief: 0.0,
        }
    }

    /// A unit normal at a given `|n.y|`, tilted in x.
    fn normal_at(ny: f32) -> [f32; 3] {
        [(1.0 - ny * ny).sqrt(), ny, 0.0]
    }

    /// The [`SAMPLES`] cases, chosen to drive every gate on and off, both sides
    /// of the `0.62` horizon (including exactly on it), the clamps into
    /// saturation at both ends, and a signed hue term.
    fn cases() -> Vec<MacroVariationIn> {
        vec![
            // 0: the documented defaults on a floor. Both gates off.
            base(),
            // 1: a ceiling — |n.y| == 1, still up-facing.
            MacroVariationIn {
                world_normal: [0.0, -1.0, 0.0],
                ..base()
            },
            // 2: a wall. up_face 0, and macroUv takes the sheared projection.
            MacroVariationIn {
                world_normal: [1.0, 0.0, 0.0],
                ..base()
            },
            // 3: EXACTLY on the horizon. GLSL step is >=, so this is up-facing.
            MacroVariationIn {
                world_normal: normal_at(0.62),
                macro_relief: 0.6,
                ..base()
            },
            // 4: one ULP below the horizon: not up-facing, relief silenced by it.
            MacroVariationIn {
                world_normal: normal_at(f32::from_bits(0.62_f32.to_bits() - 1)),
                macro_relief: 0.6,
                ..base()
            },
            // 5: the big band on, at the documented ~12 m scale.
            MacroVariationIn {
                macro_big: [1.0, 0.35, 0.028, 0.0],
                ..base()
            },
            // 6: relief on, on a floor, identity view.
            MacroVariationIn {
                macro_relief: 0.85,
                ..base()
            },
            // 7: both gates on, skewed view basis.
            MacroVariationIn {
                macro_big: [1.6, 0.5, 0.028, 0.0],
                macro_relief: 1.2,
                view_from_world: SKEW_VIEW,
                shade_normal: [0.267_261_2, 0.534_522_5, 0.801_783_7],
                ..base()
            },
            // 8: relief on but on a wall — the facing gate, not the amplitude one.
            MacroVariationIn {
                world_normal: [0.6, 0.0, -0.8],
                macro_relief: 1.2,
                view_from_world: SKEW_VIEW,
                ..base()
            },
            // 9: negative world coordinates, so the repeat wrap sign-extends.
            MacroVariationIn {
                world_pos: [-17.41, -5.23, -9.87],
                macro_big: [1.0, 0.4, 0.028, 0.0],
                macro_relief: 0.7,
                ..base()
            },
            // 10: albedo strength 0 — the value term must vanish, the hue must not.
            MacroVariationIn {
                macro_p: [0.045, 0.0, 0.1, 0.9],
                ..base()
            },
            // 11: a negative hue strength, so `t` extrapolates the other way.
            MacroVariationIn {
                macro_p: [0.045, 0.35, 0.1, -1.4],
                ..base()
            },
            // 12 and 13: the same fragment with the roughness strength's sign
            // flipped, so whichever way `mac1.g` leans one of the two saturates
            // the top clamp and the other the bottom one. Pinned by
            // `roughness_saturates_at_both_ends`.
            MacroVariationIn {
                roughness: 0.5,
                macro_p: [0.045, 0.35, 20.0, 0.35],
                micro: -0.9,
                det_fade: 1.0,
                ..base()
            },
            MacroVariationIn {
                roughness: 0.5,
                macro_p: [0.045, 0.35, -20.0, 0.35],
                macro_big: [1.0, 0.9, 0.028, 0.0],
                micro: 0.95,
                det_fade: 1.0,
                ..base()
            },
            // 14: a large contrast expansion, so `macro` saturates at both ends.
            MacroVariationIn {
                macro_p: [0.31, 0.9, 0.1, 0.35],
                macro_big: [6.0, 0.0, 0.03, 0.0],
                ..base()
            },
            // 15: a NON-UNIT shading normal with relief off. If the disabled path
            // re-normalized, this case would come back unit and the bit-identity
            // test below would fail.
            MacroVariationIn {
                shade_normal: [0.0, 0.0, 2.0],
                macro_relief: 0.0,
                ..base()
            },
        ]
    }

    /// The macro noise is a real, varied signal, and its channels are
    /// independent: a texture whose lanes agreed would make three of the four
    /// strengths untestable.
    #[test]
    fn the_procedural_macro_noise_varies_and_its_channels_are_independent() {
        let noise = MacroNoise::procedural();
        assert_eq!(noise.texels().len(), MACRO_NOISE_SIZE * MACRO_NOISE_SIZE);
        let (low, high) = noise.texels().iter().flatten().fold(
            (f32::MAX, f32::MIN),
            |(low, high), value| (low.min(*value), high.max(*value)),
        );
        assert!(low < 0.02, "the noise must reach near 0, got {low}");
        assert!(high > 0.98, "the noise must reach near 1, got {high}");
        let agreeing = noise
            .texels()
            .iter()
            .filter(|texel| [texel[1], texel[2], texel[3]].contains(&texel[0]))
            .count();
        assert_eq!(agreeing, 0, "the four channels must be independent");
    }

    /// The CPU sampler is nearest + repeat, exactly as the parity sampler is
    /// configured: a coordinate one period away lands on the same texel, and a
    /// negative coordinate wraps rather than clamping.
    #[test]
    fn the_cpu_sampler_wraps_like_a_repeat_sampler_and_snaps_like_a_nearest_one() {
        let noise = MacroNoise::procedural();
        assert_eq!(noise.sample(0.3, 0.7), noise.sample(1.3, 2.7));
        assert_eq!(noise.sample(0.3, 0.7), noise.sample(-0.7, -0.3));
        // Two coordinates inside one texel snap to the same value; the next
        // texel over does not.
        let texel = 1.0 / MACRO_NOISE_SIZE as f32;
        assert_eq!(
            noise.sample(0.25 + texel * 0.1, 0.5),
            noise.sample(0.25 + texel * 0.9, 0.5)
        );
        assert_ne!(noise.sample(0.25, 0.5), noise.sample(0.25 + texel, 0.5));
    }

    /// **The `0` defaults disable, bit-identically.** Not "to a tolerance" — the
    /// source's gates are a `#define` and a uniform test, so a disabled term must
    /// leave no trace at all.
    #[test]
    fn the_disabled_defaults_are_bit_identical() {
        let noise = MacroNoise::procedural();
        // macroBig[1] == 0: changing the big band's WORLD SCALE, which nothing
        // else reads, must not move a single bit.
        let off = macro_variation(
            &MacroVariationIn {
                macro_big: [1.0, 0.0, 0.03, 0.0],
                ..base()
            },
            &noise,
        );
        let rescaled = macro_variation(
            &MacroVariationIn {
                macro_big: [1.0, 0.0, 5.75, 0.0],
                ..base()
            },
            &noise,
        );
        assert_eq!(off.albedo.map(f32::to_bits), rescaled.albedo.map(f32::to_bits));
        assert_eq!(off.roughness.to_bits(), rescaled.roughness.to_bits());
        // And with the amplitude raised it DOES move, so the check above is not
        // passing because the band is inert.
        let on = macro_variation(
            &MacroVariationIn {
                macro_big: [1.0, 0.4, 5.75, 0.0],
                ..base()
            },
            &noise,
        );
        assert_ne!(off.albedo[0].to_bits(), on.albedo[0].to_bits());
        assert_ne!(off.roughness.to_bits(), on.roughness.to_bits());

        // macroRelief == 0: the shading normal must come back UNTOUCHED. The
        // input here is non-unit, so a stray `normalize` cannot hide.
        let relief_off = macro_variation(
            &MacroVariationIn {
                shade_normal: [0.0, 0.0, 2.0],
                macro_relief: 0.0,
                ..base()
            },
            &noise,
        );
        assert_eq!(relief_off.shade_normal.map(f32::to_bits), [0.0_f32, 0.0, 2.0].map(f32::to_bits));
        // ...and the relief block's albedo term, which contains no `macroRelief`
        // factor at all, must also be absent.
        let relief_on = macro_variation(
            &MacroVariationIn {
                shade_normal: [0.0, 0.0, 2.0],
                macro_relief: 0.9,
                ..base()
            },
            &noise,
        );
        assert_ne!(relief_off.albedo[0].to_bits(), relief_on.albedo[0].to_bits());
        assert_ne!(
            relief_on.shade_normal.map(f32::to_bits),
            [0.0_f32, 0.0, 2.0].map(f32::to_bits)
        );
        // The relief albedo term is exactly `1 - (mac1.b - 0.5) * 0.16 * upFace`
        // applied on top of the disabled result — transcribed independently here
        // so a change to the gate cannot quietly move the factor too.
        let factor = 1.0 - (relief_off.mac1[2] - 0.5) * 0.16 * relief_off.up_face;
        assert_eq!(
            relief_on.albedo[0].to_bits(),
            (relief_off.albedo[0] * factor).to_bits()
        );
    }

    /// The `0.62` horizon is inclusive, because GLSL `step(edge, x)` is `x >=
    /// edge`. One ULP either side of it decides which faces get ruts and which
    /// projection `macroUv` takes, so it is pinned at the bit.
    #[test]
    fn the_up_face_horizon_is_inclusive_at_exactly_0_62() {
        let noise = MacroNoise::procedural();
        let at = macro_variation(
            &MacroVariationIn {
                world_normal: normal_at(0.62),
                ..base()
            },
            &noise,
        );
        let below = macro_variation(
            &MacroVariationIn {
                world_normal: normal_at(f32::from_bits(0.62_f32.to_bits() - 1)),
                ..base()
            },
            &noise,
        );
        assert_eq!(at.up_face, 1.0, "|n.y| == 0.62 is up-facing");
        assert_eq!(below.up_face, 0.0, "one ULP below it is not");
        // And the two took different projections, so the horizon is load-bearing.
        assert_ne!(at.mac1, below.mac1);
    }

    /// The contrast expansion is around the midpoint `0.5`, in the source's
    /// order. A midpoint-preserving remap leaves a mid-grey band alone whatever
    /// the contrast; getting the order wrong makes the whole surface brighter or
    /// darker as contrast rises, which is the failure this pins.
    #[test]
    fn the_contrast_expansion_is_centred_on_the_midpoint() {
        let noise = MacroNoise::procedural();
        // Two contrasts, everything else equal. Recover `macro` from the albedo
        // multiplier and check it moved AWAY from 0.5 rather than shifting.
        let recover = |contrast: f32| {
            let input = MacroVariationIn {
                macro_p: [0.045, 1.0, 0.0, 0.0],
                macro_big: [contrast, 0.0, 0.03, 0.0],
                ..base()
            };
            let out = macro_variation(&input, &noise);
            // albedo *= mix(1, 0.55 + 0.92*macro, 1.0) = 0.55 + 0.92*macro,
            // then the hue term with strength 0. Solve for `macro`.
            let hue = 1.0 - 0.0;
            ((out.albedo[0] / (input.albedo[0] * hue)) - 0.55) / 0.92
        };
        let flat = recover(0.0);
        let unity = recover(1.0);
        let wide = recover(4.0);
        // contrast 0 collapses everything onto the midpoint exactly.
        assert!(
            (flat - 0.5).abs() < 1.0e-6,
            "contrast 0 must give exactly the midpoint, got {flat}"
        );
        // Raising the contrast moves the same sample further from the midpoint,
        // on the SAME side of it.
        assert!((unity - 0.5).abs() > 1.0e-4, "the sample must be off-midpoint");
        assert!((wide - 0.5).abs() > (unity - 0.5).abs());
        assert_eq!(
            (unity - 0.5).is_sign_negative(),
            (wide - 0.5).is_sign_negative(),
            "expanding contrast must not cross the midpoint"
        );
    }

    /// The four `macro` strengths are four separate knobs, and the hue term is
    /// not the albedo term: zeroing one must leave the others working, and the
    /// hue term must change the CHANNEL RATIOS while the albedo term does not.
    #[test]
    fn the_four_macro_strengths_are_independent_and_the_hue_term_is_chromatic() {
        let noise = MacroNoise::procedural();
        let run = |macro_p: [f32; 4]| {
            macro_variation(
                &MacroVariationIn {
                    macro_p,
                    ..base()
                },
                &noise,
            )
        };
        let neutral = run([0.045, 0.0, 0.0, 0.0]);
        // Albedo strength alone: a SCALAR multiplier, so the ratios are preserved.
        let value = run([0.045, 0.7, 0.0, 0.0]);
        assert_ne!(value.albedo[0], neutral.albedo[0]);
        assert_eq!(value.roughness, neutral.roughness);
        let ratio_neutral = neutral.albedo[0] / neutral.albedo[2];
        let ratio_value = value.albedo[0] / value.albedo[2];
        assert!(
            (ratio_value - ratio_neutral).abs() < 1.0e-6,
            "the albedo term is achromatic: {ratio_value} vs {ratio_neutral}"
        );
        // Hue strength alone: the ratios MUST move.
        let hue = run([0.045, 0.0, 0.0, 0.9]);
        let ratio_hue = hue.albedo[0] / hue.albedo[2];
        assert!(
            (ratio_hue - ratio_neutral).abs() > 1.0e-3,
            "the hue term must be chromatic: {ratio_hue} vs {ratio_neutral}"
        );
        assert_eq!(hue.roughness, neutral.roughness);
        // Roughness strength alone touches roughness and nothing else.
        let rough = run([0.045, 0.0, 0.8, 0.0]);
        assert_ne!(rough.roughness, neutral.roughness);
        assert_eq!(rough.albedo, neutral.albedo);
        // World scale alone moves the samples, hence everything downstream.
        let scaled = run([0.31, 0.0, 0.0, 0.0]);
        assert_ne!(scaled.mac1, neutral.mac1);
        assert_ne!(scaled.mac2, neutral.mac2);
    }

    /// The big band drives albedo *up* and roughness *down* for a positive
    /// signal, at the source's `0.55` roughness coupling — the sun-bleached end
    /// of a facade is lighter and smoother, not lighter and rougher.
    #[test]
    fn the_second_band_couples_albedo_up_to_roughness_down() {
        let noise = MacroNoise::procedural();
        // A world position whose big-band signal is positive, found by scanning
        // rather than asserted, so the test cannot be quietly inverted.
        let with = |amplitude: f32, x: f32| {
            macro_variation(
                &MacroVariationIn {
                    world_pos: [x, 1.733, -3.109],
                    macro_p: [0.045, 0.0, 0.0, 0.0],
                    macro_big: [1.0, amplitude, 0.028, 0.0],
                    ..base()
                },
                &noise,
            )
        };
        let positive = (0..400)
            .map(|step| step as f32 * 0.25)
            .find(|x| with(0.5, *x).albedo[0] > with(0.0, *x).albedo[0])
            .expect("some position must have a positive big-band signal");
        let off = with(0.0, positive);
        let on = with(0.5, positive);
        assert!(on.albedo[0] > off.albedo[0]);
        assert!(
            on.roughness < off.roughness,
            "a positive band must smooth as it lightens: {} vs {}",
            on.roughness,
            off.roughness
        );
        // The coupling is 0.55 of the amplitude, and the same `big` drives both.
        let big = (on.albedo[0] / off.albedo[0] - 1.0) / 0.5;
        assert!(
            (off.roughness - big * 0.5 * 0.55 - on.roughness).abs() < 1.0e-6,
            "the roughness coupling must be 0.55"
        );
    }

    /// The relief tilt is applied through the supplied view basis, in world space
    /// first. A transposed matrix, or a tilt built in view space, moves the
    /// result — so the basis is exercised with a non-identity rotation.
    #[test]
    fn the_relief_tilt_goes_through_the_view_basis() {
        let noise = MacroNoise::procedural();
        let run = |view: [[f32; 3]; 3]| {
            macro_variation(
                &MacroVariationIn {
                    macro_relief: 1.2,
                    view_from_world: view,
                    shade_normal: [0.267_261_2, 0.534_522_5, 0.801_783_7],
                    ..base()
                },
                &noise,
            )
            .shade_normal
        };
        let identity = run(IDENTITY_VIEW);
        let skewed = run(SKEW_VIEW);
        assert_ne!(identity, skewed);
        let transposed = [
            [SKEW_VIEW[0][0], SKEW_VIEW[1][0], SKEW_VIEW[2][0]],
            [SKEW_VIEW[0][1], SKEW_VIEW[1][1], SKEW_VIEW[2][1]],
            [SKEW_VIEW[0][2], SKEW_VIEW[1][2], SKEW_VIEW[2][2]],
        ];
        assert_ne!(
            skewed, run(transposed),
            "a transposed basis must not produce the same tilt"
        );
        // The tilt is orthogonalized against the world normal before rotation, so
        // on a floor (n = +y) the world-space tilt has no y component and the
        // identity-basis result keeps the input's y up to the renormalize.
        assert!(identity[1] > 0.0);
        // And the result is unit-length, because the source renormalizes.
        let length = (identity[0] * identity[0]
            + identity[1] * identity[1]
            + identity[2] * identity[2])
            .sqrt();
        assert!((length - 1.0).abs() < 1.0e-6, "got {length}");
    }

    /// Roughness is clamped to `0..=1`, and BOTH ends are reachable — a clamp
    /// written with the wrong bound only shows where it bites. Cases 12 and 13
    /// are the same fragment with the roughness strength's sign flipped, so one
    /// of them saturates each end whichever way `mac1.g` leans.
    #[test]
    fn roughness_saturates_at_both_ends() {
        let noise = MacroNoise::procedural();
        let mut ends = [
            macro_variation(&cases()[12], &noise).roughness,
            macro_variation(&cases()[13], &noise).roughness,
        ];
        ends.sort_by(f32::total_cmp);
        assert_eq!(
            ends,
            [0.0, 1.0],
            "cases 12 and 13 must saturate opposite ends of the roughness clamp"
        );
    }

    /// The WGSL declares exactly the entry points and the parameter list the
    /// orchestrator will splice against, including the texture and sampler
    /// parameters that make the layer self-contained.
    #[test]
    fn the_wgsl_declares_the_layers_entry_point_and_result_struct() {
        [
            "struct AxiomMacroVariation {",
            "fn axiom_macro_variation(",
            "    macro_tex: texture_2d<f32>,",
            "    macro_smp: sampler,",
            ") -> AxiomMacroVariation {",
            "if (macro_big.y > 0.0) {",
            "if (macro_relief > 0.0) {",
        ]
        .iter()
        .for_each(|fragment| {
            assert!(
                MACRO_VARIATION_WGSL.contains(fragment),
                "the WGSL must contain {fragment}"
            );
        });
        // `macroBig[3]` is unused in the source and must stay unused here — a
        // `.w` read on that uniform would be an invention.
        assert!(!MACRO_VARIATION_WGSL.contains("macro_big.w"));
    }

    // ------------------------------------------------------------------
    // CPU <-> GPU parity, on a real adapter.
    // ------------------------------------------------------------------

    /// **The parity budget for this layer.** Derived from [`MEASURED_WORST`],
    /// never fitted to a miss: 5.6x the worst delta this sweep has been measured
    /// at, which is inside [`SLACK_LIMIT`] and leaves room for an adapter whose
    /// `mix`, `mat3 * vec3` or `normalize` factor differently from the one the
    /// record was taken on.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    const TOLERANCE: f32 = 1.0e-6;

    /// **The measurement, committed as data.** The worst absolute lane delta the
    /// sweep showed on the recording adapter (Windows, `wgpu` default backend —
    /// the failure message names the live one). Output lanes here live in
    /// `0..=1.6`, so this is about `1.5e-7` relative: the last mantissa bit or
    /// two, which is what `mix`, a three-column `mat3 * vec3` and a `normalize`
    /// reciprocal are each free to cost.
    ///
    /// A number in a test log is read once and rots; a number here is diffable
    /// and re-checked every run.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    const MEASURED_WORST: f32 = 1.79e-7;

    /// How far above the measured worst case a declared tolerance may sit before
    /// it stops being a measurement and starts being a hiding place.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    const SLACK_LIMIT: f32 = 10.0;

    /// How far the live measurement may drift above the committed one before the
    /// record is stale and has to be retaken.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    const DRIFT_LIMIT: f32 = 2.0;

    /// The harness: a fullscreen triangle whose fragment stage evaluates one
    /// case's macro layer and emits one of its four result vectors, chosen by the
    /// pixel column. Four columns per case, so a single `SAMPLES * 4` wide target
    /// carries all sixteen output lanes.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    const HARNESS_WGSL: &str = r#"
struct MacroCases { items: array<vec4<f32>, 144> };
@group(0) @binding(0) var<uniform> cases: MacroCases;
@group(0) @binding(1) var macro_tex: texture_2d<f32>;
@group(0) @binding(2) var macro_smp: sampler;

@vertex
fn macro_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn macro_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let column = u32(position.x);
    let base = (column / 4u) * 9u;
    let v0 = cases.items[base + 0u];
    let v1 = cases.items[base + 1u];
    let v2 = cases.items[base + 2u];
    let v3 = cases.items[base + 3u];
    let v4 = cases.items[base + 4u];
    let v5 = cases.items[base + 5u];
    let v6 = cases.items[base + 6u];
    let v7 = cases.items[base + 7u];
    let v8 = cases.items[base + 8u];
    let result = axiom_macro_variation(
        v0.xyz, v1.xyz, v2.xyz, v0.w, v3.xyz,
        mat3x3<f32>(v6.xyz, v7.xyz, v8.xyz),
        v1.w, v2.w, v4, v5, v3.w,
        macro_tex, macro_smp,
    );
    var lanes = array<vec4<f32>, 4>(
        vec4<f32>(result.albedo, result.roughness),
        vec4<f32>(result.shade_normal, result.up_face),
        result.mac1,
        result.mac2,
    );
    return lanes[column % 4u];
}
"#;

    /// One case's nine uniform vectors, in the order `macro_fs` unpacks them.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    fn case_words(input: &MacroVariationIn) -> [f32; 36] {
        [
            input.world_pos[0],
            input.world_pos[1],
            input.world_pos[2],
            input.roughness,
            input.world_normal[0],
            input.world_normal[1],
            input.world_normal[2],
            input.micro,
            input.albedo[0],
            input.albedo[1],
            input.albedo[2],
            input.det_fade,
            input.shade_normal[0],
            input.shade_normal[1],
            input.shade_normal[2],
            input.macro_relief,
            input.macro_p[0],
            input.macro_p[1],
            input.macro_p[2],
            input.macro_p[3],
            input.macro_big[0],
            input.macro_big[1],
            input.macro_big[2],
            input.macro_big[3],
            input.view_from_world[0][0],
            input.view_from_world[0][1],
            input.view_from_world[0][2],
            0.0,
            input.view_from_world[1][0],
            input.view_from_world[1][1],
            input.view_from_world[1][2],
            0.0,
            input.view_from_world[2][0],
            input.view_from_world[2][1],
            input.view_from_world[2][2],
            0.0,
        ]
    }

    /// The four result vectors the CPU reference produces for one case, in the
    /// same order the harness emits them.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    fn expected_lanes(input: &MacroVariationIn, noise: &MacroNoise) -> [[f32; 4]; 4] {
        let out = macro_variation(input, noise);
        [
            [out.albedo[0], out.albedo[1], out.albedo[2], out.roughness],
            [
                out.shade_normal[0],
                out.shade_normal[1],
                out.shade_normal[2],
                out.up_face,
            ],
            out.mac1,
            out.mac2,
        ]
    }

    /// Acquire a real adapter, or fail loudly. A parity test that silently passes
    /// when nothing ran proves nothing, so this asserts rather than skipping.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    fn render_on_a_real_adapter(
        noise: &MacroNoise,
        words: &[f32],
    ) -> (Vec<[f32; 4]>, wgpu::Backend) {
        // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
        // ~50 tests each opening their own is what crashes the driver.
        let gpu = crate::test_gpu::TestGpu::shared();
        let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
        let backend = gpu.backend;
        assert_ne!(
            backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );

        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let (module, failure) = crate::test_gpu::validating(&device, || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("axiom-macro-variation-shader"),
                source: wgpu::ShaderSource::Wgsl(
                    [MACRO_VARIATION_WGSL, HARNESS_WGSL].concat().into(),
                ),
            })
        });
        let compile_error = failure;
        assert!(
            compile_error.is_none(),
            "the macro layer's WGSL must compile: {}",
            compile_error.map_or(String::new(), |error| error.to_string())
        );

        // The macro noise, as an Rgba32Float texture. A float format because an
        // 8-bit one would quantise the texel and put the CPU reference's
        // `n / 255` against the GPU's, which is a comparison of two decoders
        // rather than of this layer's arithmetic.
        let size = MACRO_NOISE_SIZE as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-macro-noise"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texel_bytes: Vec<u8> = noise
            .texels()
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texel_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 16),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Nearest + repeat: the fetch is exact on both sides, so no filter-weight
        // quantisation enters the comparison. `Rgba32Float` is unfilterable
        // without an optional feature, which this sampler honours.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-macro-noise-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-macro-variation-bgl"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });
        let mut uniform_bytes: Vec<u8> =
            words.iter().flat_map(|value| value.to_le_bytes()).collect();
        uniform_bytes.resize(144 * 16, 0);
        let uniform = wgpu::util::DeviceExt::create_buffer_init(
            &device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("axiom-macro-variation-cases"),
                contents: &uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-macro-variation-bg"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-macro-variation-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("axiom-macro-variation-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("macro_vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("macro_fs"),
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

        let width = (SAMPLES * 4) as u32;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-macro-variation-target"),
            size: wgpu::Extent3d {
                width,
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
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let row_bytes = (width * 16).div_ceil(256) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-macro-variation-readback"),
            size: u64::from(row_bytes),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-macro-variation-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
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
                texture: &target,
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
                width,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait)
            .expect("the readback must complete");
        let mapped = slice.get_mapped_range();
        (
            (0..width as usize)
                .map(|column| {
                    [0_usize, 1, 2, 3].map(|lane| {
                        let at = column * 16 + lane * 4;
                        f32::from_le_bytes([
                            mapped[at],
                            mapped[at + 1],
                            mapped[at + 2],
                            mapped[at + 3],
                        ])
                    })
                })
                .collect(),
            backend,
        )
    }

    /// **The parity proof, and the measurement it is set from.** Every case, on
    /// a real adapter, against the CPU reference — and the worst delta re-taken
    /// every run so a drift in either transcription surfaces as a number rather
    /// than as a pass.
    ///
    /// It holds three relations at once, so neither the record nor the budget can
    /// quietly stop describing the hardware:
    ///
    /// 1. every lane is inside [`TOLERANCE`] — the layer agrees;
    /// 2. the live worst delta is within [`DRIFT_LIMIT`] of [`MEASURED_WORST`] —
    ///    the committed record is still true;
    /// 3. [`TOLERANCE`] is no more than [`SLACK_LIMIT`] above the live delta —
    ///    the budget is not a hiding place. Being *too generous* fails here.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    #[test]
    fn the_macro_layer_agrees_with_its_cpu_reference_on_a_real_adapter() {
        let noise = MacroNoise::procedural();
        let all = cases();
        assert_eq!(all.len(), SAMPLES, "one uniform slot per case");
        let words: Vec<f32> = all.iter().flat_map(|input| case_words(input)).collect();
        let (rendered, backend) = render_on_a_real_adapter(&noise, &words);
        let worst = all
            .iter()
            .enumerate()
            .fold(0.0_f32, |worst, (index, input)| {
                let expected = expected_lanes(input, &noise);
                (0..4).fold(worst, |worst, vector| {
                    let actual = rendered[index * 4 + vector];
                    (0..4).fold(worst, |worst, lane| {
                        let delta = (expected[vector][lane] - actual[lane]).abs();
                        assert!(
                            delta <= TOLERANCE,
                            "macro_variation disagrees at case {index} vector {vector}                              lane {lane} on {backend:?}: CPU {} vs GPU {}                              (delta {delta:e}, tolerance {TOLERANCE:e})",
                            expected[vector][lane],
                            actual[lane]
                        );
                        worst.max(delta)
                    })
                })
            });
        // The sweep must not be vacuous: a shader that returned a constant, or a
        // reference that ignored its input, would otherwise pass every lane.
        let first = rendered[0];
        assert!(
            rendered.iter().any(|lanes| *lanes != first),
            "the parity sweep must exercise a varying signal"
        );
        // ...and it must have driven BOTH gates, or the sweep proves the enabled
        // path only. Cases 5 and 6 turn on the big band and the relief in turn.
        assert_ne!(
            rendered[0 * 4],
            rendered[5 * 4],
            "case 5 must differ from case 0: the big band did nothing"
        );
        assert_ne!(
            rendered[0 * 4 + 1],
            rendered[6 * 4 + 1],
            "case 6 must differ from case 0: the relief tilt did nothing"
        );
        // **The GPU honours the disabled default too, at the bit.** Case 15 feeds
        // a NON-UNIT shading normal with `macroRelief == 0`. If the shader's
        // relief gate were a multiply-by-zero rather than a gate, the trailing
        // `normalize` would hand back a unit vector and this would read 1.0.
        assert_eq!(
            rendered[15 * 4 + 1].map(f32::to_bits),
            [0.0_f32, 0.0, 2.0, 1.0].map(f32::to_bits),
            "a zero macroRelief must leave the shading normal untouched on {backend:?}"
        );
        assert!(
            worst <= MEASURED_WORST * DRIFT_LIMIT,
            "the worst CPU/GPU delta is now {worst:e} against a committed measurement              of {MEASURED_WORST:e} on {backend:?}. Re-measure and re-record it rather              than widening the tolerance."
        );
        assert!(
            TOLERANCE <= worst * SLACK_LIMIT,
            "TOLERANCE {TOLERANCE:e} is more than {SLACK_LIMIT}x the live worst delta              {worst:e} on {backend:?}: that is a hiding place, not a budget."
        );
    }

    /// The committed record and the declared budget have to stay in the relation
    /// the brief demands even before a GPU is touched: a tolerance more than
    /// [`SLACK_LIMIT`] looser than the measurement is itself a failure, and a
    /// record of zero is not a measurement.
    #[cfg(all(feature = "offscreen", not(target_arch = "wasm32")))]
    #[test]
    fn the_tolerance_is_not_loose_against_the_recorded_measurement() {
        assert!(MEASURED_WORST > 0.0, "a measurement of zero is not a measurement");
        assert!(
            MEASURED_WORST <= TOLERANCE,
            "the recorded measurement {MEASURED_WORST:e} is outside the declared              tolerance {TOLERANCE:e}"
        );
        assert!(
            TOLERANCE <= MEASURED_WORST * SLACK_LIMIT,
            "TOLERANCE {TOLERANCE:e} is more than {SLACK_LIMIT}x the recorded worst              delta {MEASURED_WORST:e}"
        );
    }
}
