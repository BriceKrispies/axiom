//! **The pyramid's WGSL**, transcribed from the GLSL in `bloom.js`.
//!
//! Split in two so the parity harness can compile the *same* functions the real
//! pass runs, with its own entry points spliced after them:
//!
//! - [`BLOOM_PYRAMID_WGSL`] — the tap tables and the pure filter functions. No
//!   bindings, no entry points, nothing about textures.
//! - [`BLOOM_PASSES_WGSL`] — the bindings, the fullscreen-triangle vertex stage,
//!   and the two fragment entry points that fetch thirteen (or nine) taps and
//!   call into the above.
//!
//! # Why the filters take a tap array rather than a sampler
//!
//! Because that is the boundary between what this port is responsible for and
//! what the hardware is. The *arithmetic over thirteen colours* is transcribed
//! GLSL and must match a CPU reference bit-for-bit-ish; the *bilinear fetch that
//! produced those colours* is a sampler whose subtexel precision is
//! implementation-defined. Keeping them apart means the tight parity number
//! measures the transcription and the loose one measures the filter, instead of
//! one number hiding inside the other.
//!
//! # The branch in `bloom_down_fs` is the source's branch
//!
//! `uParams.x > 0.5` selects between two genuinely different algorithms, and the
//! source writes it as an `if`. The Branchless Law is a rule about **Rust**: the
//! `engine_no_branching` dylint reads Rust HIR, and a shader is a `&str`. So the
//! WGSL says what the GLSL says.

/// The tap tables and the pure filters. Shared verbatim by the pass and by the
/// parity harness.
pub(crate) const BLOOM_PYRAMID_WGSL: &str = r#"
// The 13 downsample offsets in SOURCE texels, in the source's a..m order:
//   a( -2, +2 )  b( 0, +2 )  c( +2, +2 )
//   d( -2,  0 )  e( 0,  0 )  f( +2,  0 )
//   g( -2, -2 )  h( 0, -2 )  i( +2, -2 )
//          j( -1, +1 )  k( +1, +1 )  l( -1, -1 )  m( +1, -1 )
fn bloom_down_tap(index: u32) -> vec2<f32> {
    var taps = array<vec2<f32>, 13>(
        vec2<f32>(-2.0,  2.0),
        vec2<f32>( 0.0,  2.0),
        vec2<f32>( 2.0,  2.0),
        vec2<f32>(-2.0,  0.0),
        vec2<f32>( 0.0,  0.0),
        vec2<f32>( 2.0,  0.0),
        vec2<f32>(-2.0, -2.0),
        vec2<f32>( 0.0, -2.0),
        vec2<f32>( 2.0, -2.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
    );
    return taps[index];
}

// The 9 tent offsets, in source texels scaled by uRadius, row-major from the
// top-left so the centre is index 4.
fn bloom_up_tap(index: u32) -> vec2<f32> {
    var taps = array<vec2<f32>, 9>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 0.0,  1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0,  0.0),
        vec2<f32>( 0.0,  0.0),
        vec2<f32>( 1.0,  0.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 0.0, -1.0),
        vec2<f32>( 1.0, -1.0),
    );
    return taps[index];
}

// `owLum` from glsl.js — Rec.709, written out rather than as `dot`, whose three
// products a compiler may factor however it likes.
fn bloom_luminance(c: vec3<f32>) -> f32 {
    return c.r * 0.2126 + c.g * 0.7152 + c.b * 0.0722;
}

// `karisWeight( c ) = 1.0 / ( 1.0 + owLum( c ) )`.
fn bloom_karis_weight(c: vec3<f32>) -> f32 {
    return 1.0 / (1.0 + bloom_luminance(c));
}

// `max( uParams.z, 1e-4 )`, applied once per pass by the caller in the source.
fn bloom_knee_floor(knee: f32) -> f32 {
    return max(knee, 1e-4);
}

// `owBloomPrefilter`. Max-channel driven, so a saturated light blooms as readily
// as a white one. `clamp` written out as `min(max(...))`, which is what GLSL's
// clamp is, because WGSL's builtin is permitted to factor differently.
fn bloom_prefilter(c: vec3<f32>, thr: f32, knee: f32) -> vec3<f32> {
    let l = max(max(c.r, c.g), c.b);
    let surplus = l - thr;
    let soft_in = min(max(surplus + knee, 0.0), 2.0 * knee);
    let soft = soft_in * soft_in / (4.0 * knee + 1e-5);
    return c * (max(soft, surplus) / max(l, 1e-4));
}

// `fetch`'s `max( ..., vec3( 0.0 ) )`, componentwise.
fn bloom_floor_at_zero(c: vec3<f32>) -> vec3<f32> {
    return max(c, vec3<f32>(0.0));
}

// The level-0 downsample: exposure, soft-knee threshold, Karis average, and the
// min(24) firefly clamp.
fn bloom_downsample_karis(
    taps: array<vec3<f32>, 13>,
    exposure: f32,
    threshold: f32,
    knee_in: f32,
) -> vec3<f32> {
    var t = taps;
    let knee = bloom_knee_floor(knee_in);
    // Exposure first, so the firefly clamp AND the threshold are both in
    // display-referred terms.
    for (var n: i32 = 0; n < 13; n = n + 1) {
        t[n] = bloom_prefilter(bloom_floor_at_zero(t[n]) * exposure, threshold, knee);
    }
    let g0 = (t[0] + t[1] + t[3] + t[4]) * 0.25;
    let g1 = (t[1] + t[2] + t[4] + t[5]) * 0.25;
    let g2 = (t[3] + t[4] + t[6] + t[7]) * 0.25;
    let g3 = (t[4] + t[5] + t[7] + t[8]) * 0.25;
    let g4 = (t[9] + t[10] + t[11] + t[12]) * 0.25;
    let w0 = bloom_karis_weight(g0) * 0.125;
    let w1 = bloom_karis_weight(g1) * 0.125;
    let w2 = bloom_karis_weight(g2) * 0.125;
    let w3 = bloom_karis_weight(g3) * 0.125;
    let w4 = bloom_karis_weight(g4) * 0.5;
    // A DIVISION, not a reciprocal multiplied three times.
    let result = (g0 * w0 + g1 * w1 + g2 * w2 + g3 * w3 + g4 * w4)
        / max(w0 + w1 + w2 + w3 + w4, 1e-5);
    return min(result, vec3<f32>(24.0));
}

// Every downsample below level 0: fixed weights, no exposure, no threshold, no
// clamp. Accumulated in the source's order.
fn bloom_downsample_plain(taps: array<vec3<f32>, 13>) -> vec3<f32> {
    var t = taps;
    for (var n: i32 = 0; n < 13; n = n + 1) {
        t[n] = bloom_floor_at_zero(t[n]);
    }
    var result = t[4] * 0.125;
    result = result + (t[0] + t[2] + t[6] + t[8]) * 0.03125;
    result = result + (t[1] + t[3] + t[5] + t[7]) * 0.0625;
    result = result + (t[9] + t[10] + t[11] + t[12]) * 0.125;
    return result;
}

// The 9-tap tent. No re-flooring: its source is a downsample's output.
fn bloom_upsample_tent(taps: array<vec3<f32>, 9>) -> vec3<f32> {
    let sum = taps[4] * 4.0
        + (taps[1] + taps[3] + taps[5] + taps[7]) * 2.0
        + (taps[0] + taps[2] + taps[6] + taps[8]);
    return sum * 0.0625;
}

// `composite.js`'s one bloom line: an ADD into HDR, ahead of the tone map.
fn bloom_combine(hdr: vec3<f32>, bloom: vec3<f32>, strength: f32) -> vec3<f32> {
    return hdr + bloom_floor_at_zero(bloom) * max(strength, 0.0);
}
"#;

/// The bindings, the vertex stage and the two fragment entry points.
///
/// Concatenated after [`BLOOM_PYRAMID_WGSL`] at pipeline build.
pub(crate) const BLOOM_PASSES_WGSL: &str = r#"
struct BloomVsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct BloomParams {
    // xy = the SOURCE's texel size, already multiplied by uRadius for the
    // upsample (`t = uTexel * uRadius`, reciprocal first — see reference.rs).
    texel: vec4<f32>,
    // x = the karis flag (`uParams.x`), y = threshold, z = knee,
    // w = the metered exposure scalar.
    //
    // The source reads exposure from a 1x1 `FloatType` target with
    // `texture2D( tExposure, vec2( 0.5 ) ).r`. A full-float 1x1 fetch and a
    // uniform `f32` are the same number, so the texture buys nothing here and
    // costs a binding; `exposure.js` owns the metering that produces it.
    tune: vec4<f32>,
    // x = uWeight, written to alpha so the fixed-function blender performs the
    // source's `NormalBlending` lerp.
    blend: vec4<f32>,
};

@group(0) @binding(0) var bloom_src: texture_2d<f32>;
@group(0) @binding(1) var bloom_sampler: sampler;
@group(0) @binding(2) var<uniform> bloom_params: BloomParams;

@vertex
fn bloom_vs(@builtin(vertex_index) vi: u32) -> BloomVsOut {
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
    var out: BloomVsOut;
    out.clip = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uv[vi];
    return out;
}

@fragment
fn bloom_down_fs(in: BloomVsOut) -> @location(0) vec4<f32> {
    let t = bloom_params.texel.xy;
    var taps: array<vec3<f32>, 13>;
    for (var n: i32 = 0; n < 13; n = n + 1) {
        let o = bloom_down_tap(u32(n));
        let uv = in.uv + vec2<f32>(o.x * t.x, o.y * t.y);
        taps[n] = textureSample(bloom_src, bloom_sampler, uv).rgb;
    }
    // The source's own `if ( uParams.x > 0.5 )`. Two different algorithms, not
    // one parameterised one.
    if (bloom_params.tune.x > 0.5) {
        return vec4<f32>(
            bloom_downsample_karis(
                taps,
                bloom_params.tune.w,
                bloom_params.tune.y,
                bloom_params.tune.z,
            ),
            1.0,
        );
    }
    return vec4<f32>(bloom_downsample_plain(taps), 1.0);
}

@fragment
fn bloom_up_fs(in: BloomVsOut) -> @location(0) vec4<f32> {
    let t = bloom_params.texel.xy;
    var taps: array<vec3<f32>, 9>;
    for (var n: i32 = 0; n < 9; n = n + 1) {
        let o = bloom_up_tap(u32(n));
        let uv = in.uv + vec2<f32>(o.x * t.x, o.y * t.y);
        taps[n] = textureSample(bloom_src, bloom_sampler, uv).rgb;
    }
    // Alpha carries uWeight; `SrcAlpha`/`OneMinusSrcAlpha` blending turns that
    // into `lerp( dst, src, weight )`, the energy-preserving accumulation.
    return vec4<f32>(bloom_upsample_tent(taps), bloom_params.blend.x);
}
"#;
