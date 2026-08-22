//! **The GTAO WGSL**, transcribed from the GLSL in `src/render/gtao.js`.
//!
//! Split the way [`crate::bloom_pyramid::wgsl`] is split, and for the same
//! reason:
//!
//! - [`GTAO_WGSL`] — the pure functions. No bindings, no entry points, nothing
//!   about textures. This is the text the parity harness compiles beside its own
//!   entry points, so what a tolerance measures is **the transcription**.
//! - [`GTAO_CORE_PASS_WGSL`], [`GTAO_TEMPORAL_PASS_WGSL`],
//!   [`GTAO_BLUR_PASS_WGSL`] — the bindings, the fullscreen-triangle vertex
//!   stage, and the fragment entry points that fetch texels and call into the
//!   above. Each is `GTAO_WGSL ++ <pass>`; none is bound by anything yet.
//!
//! The boundary between them is the boundary between what this port is
//! responsible for and what the hardware is. The *arithmetic over 48 reconstructed
//! view positions* is transcribed GLSL and must match a CPU reference; the
//! *texel fetch that produced those depths* is a sampler. Keeping them apart
//! means the tight parity number measures the port and the loose one measures the
//! filter, instead of one hiding inside the other.
//!
//! # The loops and the `if`s here are the source's, and that is not a loophole
//!
//! The Branchless Law is a rule about **Rust**: `engine_no_branching` reads Rust
//! HIR, and shader text is a `&str`. `AO_CORE` is a nested loop with four guards
//! per tap and it stays one — a horizon search is a horizon search. What the CPU
//! reference next door does *is* branchless, because that is Rust.
//!
//! # Three deliberate divergences from the GLSL, all forced by the target
//!
//! Each is stated at its site as well as here.
//!
//! 1. **`v` is flipped before `axiom_gtao_view_pos`.** `owViewPos` reconstructs
//!    from `uv * 2 - 1`, which is NDC only where the framebuffer's `v` runs up.
//!    See [`crate::gtao::NDC_UV_V_SIGN`].
//! 2. **The horizon step negates `dir2.y`.** `sliceDir` is a *view-space* vector.
//!    See [`crate::gtao::SCREEN_STEP_V_SIGN`] — and note that getting this wrong
//!    exchanges the two horizons and collapses the arc on every grazing surface,
//!    exactly as the source's own comment warns.
//! 3. **The velocity's `y` is negated before reprojection**, per
//!    [`crate::gbuffer::VELOCITY_TEXTURE_V_SIGN`].
//!
//! # Fragment outputs are `vec2<f32>`, because the targets are `RG16Float`
//!
//! The source declares `gl_FragColor` a `vec4` and lets three.js's `RGFormat`
//! discard `ba`. WGSL's fragment outputs must match the attachment's component
//! count, so each pass writes `vec2<f32>`. The numbers written are the same, and
//! the discarded `0.0, 1.0` were never read.

/// The transcribed functions, with no bindings and no entry points, so the parity
/// harness and the three passes compile *the same text*.
///
/// `clamp`, `mix`, `sign`, `fract` and every `dot`/`length`/`normalize` are
/// written out: WGSL's builtins are permitted to factor differently from GLSL's,
/// and this text has to mean exactly what `gtao.js` means. `inverseSqrt` is
/// kept — the source says `inversesqrt`, and it is the one place the port is
/// deliberately using an approximation rather than a division (see the note file
/// for what that costs in the parity budget).
pub(crate) const GTAO_WGSL: &str = r#"
// GTAO, from Claude-of-Duty `src/render/gtao.js` and the `COMMON` chunk of
// `src/render/glsl.js`. Jimenez et al. 2016 -- the visibility-arc integral.

const AXIOM_GTAO_PI: f32 = 3.141592653589793;
const AXIOM_GTAO_HALF_PI: f32 = 1.5707963267948966;
// `#define OW_SLICES 3` -- the file header's "two slices" is stale prose.
const AXIOM_GTAO_SLICES: i32 = 3;
const AXIOM_GTAO_STEPS: i32 = 8;
// WebGPU's framebuffer v runs DOWN; the source's WebGL one runs UP. See
// `gtao.rs`: this is the same fact as `gbuffer::VELOCITY_TEXTURE_V_SIGN`.
const AXIOM_GTAO_SCREEN_STEP_V_SIGN: f32 = -1.0;
const AXIOM_GTAO_VELOCITY_V_SIGN: f32 = -1.0;

fn axiom_gtao_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    return min(max(x, lo), hi);
}

// GLSL `mix( x, y, a )` = `x * ( 1 - a ) + y * a`. Not `x + a * ( y - x )`.
fn axiom_gtao_mix(x: f32, y: f32, a: f32) -> f32 {
    return x * (1.0 - a) + y * a;
}

// `fract( x )` = `x - floor( x )`.
fn axiom_gtao_fract(x: f32) -> f32 {
    return x - floor(x);
}

// GLSL `sign`, which returns 0.0 for ANY zero. Written out rather than calling
// the builtin, exactly as `material_shader/frames.rs` does.
fn axiom_gtao_sign(x: f32) -> f32 {
    return f32(x > 0.0) - f32(x < 0.0);
}

// `owIGN` -- interleaved gradient noise. Two nested fracts, in that order.
fn axiom_gtao_ign(p: vec2<f32>) -> f32 {
    return axiom_gtao_fract(52.9829189 * axiom_gtao_fract(p.x * 0.06711056 + p.y * 0.00583715));
}

// `owHash12`. Note `p.xyx`: the x component appears twice.
fn axiom_gtao_hash12(p: vec2<f32>) -> f32 {
    let q = vec3<f32>(
        axiom_gtao_fract(p.x * 0.1031),
        axiom_gtao_fract(p.y * 0.1031),
        axiom_gtao_fract(p.x * 0.1031),
    );
    let d = q.x * (q.y + 33.33) + q.y * (q.z + 33.33) + q.z * (q.x + 33.33);
    let r = vec3<f32>(q.x + d, q.y + d, q.z + d);
    return axiom_gtao_fract((r.x + r.y) * r.z);
}

// `owViewPos( uv, depth, projInv )`. `uv` is the SOURCE's -- v running up -- so
// a WebGPU caller flips first. `proj_inv[c][r]` is column c, row r, matching
// GLSL's `m * v = c0*v.x + c1*v.y + c2*v.z + c3*v.w`.
fn axiom_gtao_view_pos(uv: vec2<f32>, depth: f32, proj_inv: mat4x4<f32>) -> vec3<f32> {
    let c = vec4<f32>(uv.x * 2.0 - 1.0, uv.y * 2.0 - 1.0, 1.0, 1.0);
    let h = vec4<f32>(
        proj_inv[0][0] * c.x + proj_inv[1][0] * c.y + proj_inv[2][0] * c.z + proj_inv[3][0] * c.w,
        proj_inv[0][1] * c.x + proj_inv[1][1] * c.y + proj_inv[2][1] * c.z + proj_inv[3][1] * c.w,
        proj_inv[0][2] * c.x + proj_inv[1][2] * c.y + proj_inv[2][2] * c.z + proj_inv[3][2] * c.w,
        proj_inv[0][3] * c.x + proj_inv[1][3] * c.y + proj_inv[2][3] * c.z + proj_inv[3][3] * c.w,
    );
    let dir = vec3<f32>(h.x / h.w, h.y / h.w, h.z / h.w);
    let s = max(1e-6, -dir.z);
    let unit = vec3<f32>(dir.x / s, dir.y / s, dir.z / s);
    return vec3<f32>(unit.x * depth, unit.y * depth, unit.z * depth);
}

// `owArc( h, n, cosN, sinN )` -- the closed-form cosine-weighted visible arc.
fn axiom_gtao_arc(h: f32, n: f32, cos_n: f32, sin_n: f32) -> f32 {
    return 0.25 * (-cos(2.0 * h - n) + cos_n + 2.0 * h * sin_n);
}

// World radius -> pixels, then clamped. Left-associated, and the depth is a
// DIVISION.
fn axiom_gtao_radius_px(radius: f32, p11: f32, res_y: f32, depth: f32) -> f32 {
    let r = radius * p11 * 0.5 * res_y / max(0.2, depth);
    return axiom_gtao_clamp(r, 6.0, 128.0);
}

// The QUADRATIC step distribution: `radiusPx * ft * ft + 1.0`, grouped
// `(radiusPx * ft) * ft`. The +1 px floor keeps a tap off the centre texel,
// whose zero-length ds gives a garbage horizon and closes the arc completely.
fn axiom_gtao_step_offset(step_index: f32, noise2: f32, radius_px: f32) -> f32 {
    let ft = (step_index + noise2) / f32(AXIOM_GTAO_STEPS);
    return radius_px * ft * ft + 1.0;
}

// One tap folded into a running horizon cosine. THIS IS THE THICKNESS MODEL:
// `fall` is `clamp(len2/r2, 0, 1)` SQUARED and is the mix's third argument, so a
// tap at the full radius returns the incumbent horizon unchanged. `uParams.w`,
// labelled `thickness`, is never read by the source.
fn axiom_gtao_horizon_update(cos_h: f32, ds: vec3<f32>, v: vec3<f32>, inv_r2: f32) -> f32 {
    let len2 = ds.x * ds.x + ds.y * ds.y + ds.z * ds.z;
    let inv = inverseSqrt(len2);
    let c = (ds.x * v.x + ds.y * v.y + ds.z * v.z) * inv;
    var fall = axiom_gtao_clamp(len2 * inv_r2, 0.0, 1.0);
    fall = fall * fall;
    let updated = max(cos_h, axiom_gtao_mix(c, cos_h, fall));
    // `if ( len2 > 2e-5 )` -- below it the tap is skipped outright, which also
    // discards the NaN `inverseSqrt( 0 )` produces.
    return select(cos_h, updated, len2 > 2e-5);
}

struct AxiomGtaoSlice {
    // `length( projN )` -- also the slice's weight in the sum.
    proj_len: f32,
    cos_n: f32,
    n: f32,
    sin_n: f32,
};

// The slice frame, from the view normal, the view vector and the azimuth.
// `sliceDir = vec3( dir2, 0.0 )` is a VIEW-SPACE vector, which is why the pass
// negates dir2.y when it converts the step into texture space.
fn axiom_gtao_slice_frame(nrm: vec3<f32>, v: vec3<f32>, dir2: vec2<f32>) -> AxiomGtaoSlice {
    let slice_dir = vec3<f32>(dir2.x, dir2.y, 0.0);
    let cr = vec3<f32>(
        slice_dir.y * v.z - slice_dir.z * v.y,
        slice_dir.z * v.x - slice_dir.x * v.z,
        slice_dir.x * v.y - slice_dir.y * v.x,
    );
    let cr_len = sqrt(cr.x * cr.x + cr.y * cr.y + cr.z * cr.z);
    let axis = vec3<f32>(cr.x / cr_len, cr.y / cr_len, cr.z / cr_len);
    let n_dot_axis = nrm.x * axis.x + nrm.y * axis.y + nrm.z * axis.z;
    let proj_n = vec3<f32>(
        nrm.x - axis.x * n_dot_axis,
        nrm.y - axis.y * n_dot_axis,
        nrm.z - axis.z * n_dot_axis,
    );
    let proj_len = sqrt(proj_n.x * proj_n.x + proj_n.y * proj_n.y + proj_n.z * proj_n.z);
    let proj_nn = vec3<f32>(proj_n.x / proj_len, proj_n.y / proj_len, proj_n.z / proj_len);
    let sd_dot_v = slice_dir.x * v.x + slice_dir.y * v.y + slice_dir.z * v.z;
    let ortho = vec3<f32>(
        slice_dir.x - v.x * sd_dot_v,
        slice_dir.y - v.y * sd_dot_v,
        slice_dir.z - v.z * sd_dot_v,
    );
    let ortho_len = sqrt(ortho.x * ortho.x + ortho.y * ortho.y + ortho.z * ortho.z);
    let ortho_dir = vec3<f32>(ortho.x / ortho_len, ortho.y / ortho_len, ortho.z / ortho_len);
    let cos_n = axiom_gtao_clamp(proj_nn.x * v.x + proj_nn.y * v.y + proj_nn.z * v.z, -1.0, 1.0);
    let n = axiom_gtao_sign(
        ortho_dir.x * proj_nn.x + ortho_dir.y * proj_nn.y + ortho_dir.z * proj_nn.z
    ) * acos(cos_n);
    return AxiomGtaoSlice(proj_len, cos_n, n, sin(n));
}

// `phi = ( float( s ) + noise ) * ( OW_PI / float( OW_SLICES ) )` -- note the
// parenthesised division, evaluated as a group before the multiply.
fn axiom_gtao_slice_direction(slice_index: f32, noise: f32) -> vec2<f32> {
    let phi = (slice_index + noise) * (AXIOM_GTAO_PI / f32(AXIOM_GTAO_SLICES));
    return vec2<f32>(cos(phi), sin(phi));
}

// One slice's contribution. The +-pi/2 clamp is about `n`, NOT about zero, and
// there is deliberately NO per-slice clamp on the result: a tilted surface's
// single slice legitimately integrates past 1, and the excess compensates the
// slices whose projected normal is short. Clamping here is the classic "my SSAO
// looks like dirt" bug.
fn axiom_gtao_slice_visibility(cos_h_neg: f32, cos_h_pos: f32, s: AxiomGtaoSlice) -> f32 {
    var h1 = -acos(axiom_gtao_clamp(cos_h_neg, -1.0, 1.0));
    var h2 = acos(axiom_gtao_clamp(cos_h_pos, -1.0, 1.0));
    h1 = s.n + max(h1 - s.n, -AXIOM_GTAO_HALF_PI);
    h2 = s.n + min(h2 - s.n, AXIOM_GTAO_HALF_PI);
    let contribution = s.proj_len
        * (axiom_gtao_arc(h1, s.n, s.cos_n, s.sin_n) + axiom_gtao_arc(h2, s.n, s.cos_n, s.sin_n));
    // `if ( projLen < 1e-4 ) continue;`
    return select(contribution, 0.0, s.proj_len < 1e-4);
}

// `clamp( visibility / float( OW_SLICES ), 0.0, 4.0 )` -- four, not one.
fn axiom_gtao_resolve(sum: f32) -> f32 {
    return axiom_gtao_clamp(sum / f32(AXIOM_GTAO_SLICES), 0.0, 4.0);
}

// The temporal accumulator's history weight: off-screen history is discarded
// outright, and disagreeing depth fades out on a 30x RELATIVE exponential.
fn axiom_gtao_temporal_weight(
    feedback: f32,
    huv: vec2<f32>,
    hist_depth: f32,
    cur_depth: f32,
) -> f32 {
    let outside = (huv.x < 0.0) | (huv.x > 1.0) | (huv.y < 0.0) | (huv.y > 1.0);
    let w = select(feedback, 0.0, outside);
    let rel = abs(hist_depth - cur_depth) / max(0.05, cur_depth);
    return w * exp(-rel * 30.0);
}

// The WIDE neighbourhood clamp. +-0.45, because the per-frame signal is three
// slices of a stochastic integral and a tight clamp would re-inject its variance.
fn axiom_gtao_temporal_clamp(hist_ao: f32, cur_ao: f32, n0: f32, n1: f32, n2: f32, n3: f32) -> f32 {
    var mn = cur_ao;
    var mx = cur_ao;
    mn = min(mn, n0); mx = max(mx, n0);
    mn = min(mn, n1); mx = max(mx, n1);
    mn = min(mn, n2); mx = max(mx, n2);
    mn = min(mn, n3); mx = max(mx, n3);
    return axiom_gtao_clamp(hist_ao, mn - 0.45, mx + 0.45);
}

// `w0 = 0.4 / float( i + 1 )` -- a reciprocal ramp, not a Gaussian.
fn axiom_gtao_blur_distance_weight(tap: f32) -> f32 {
    return 0.4 / (tap + 1.0);
}

// The edge-stopping weight. The exponent groups `((-|dd|) * 22.0) / max(0.1, d)`.
fn axiom_gtao_blur_tap_weight(w0: f32, tap_depth: f32, centre_depth: f32) -> f32 {
    return w0 * exp(-abs(tap_depth - centre_depth) * 22.0 / max(0.1, centre_depth));
}

// `ao = sum / wsum`, then -- on the LAST stage only -- clamp and the intensity
// curve. Doing it on both stages would square the exponent and would throw away
// the above-one visibility the horizontal pass is still carrying.
fn axiom_gtao_blur_output(sum: f32, wsum: f32, apply_curve: f32, intensity: f32) -> f32 {
    let ao = sum / wsum;
    let curved = pow(axiom_gtao_clamp(ao, 0.0, 1.0), intensity);
    return select(ao, curved, apply_curve > 0.5);
}

// `owDecodeNormal` from glsl.js -- the VIEW-space octahedral normal the G-buffer
// stores in slot 0's xy. Identical to `gbuffer::decode_normal`; kept here so the
// three passes are one self-contained text.
fn axiom_gtao_decode_normal(f: vec2<f32>) -> vec3<f32> {
    let nz = 1.0 - abs(f.x) - abs(f.y);
    let t = max(-nz, 0.0);
    let nx = f.x + select(t, -t, f.x >= 0.0);
    let ny = f.y + select(t, -t, f.y >= 0.0);
    let len = sqrt(nx * nx + ny * ny + nz * nz);
    return vec3<f32>(nx / len, ny / len, nz / len);
}

// The fullscreen TRIANGLE every pass draws -- one primitive, no diagonal seam,
// exactly as `pass.js` does it.
@vertex
fn axiom_gtao_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}
"#;

/// `AO_CORE`: three slices x eight steps x two sides, into an `RG16Float`
/// `(visibility, linear depth)`.
///
/// Concatenate after [`GTAO_WGSL`]. Bind group 0 is `{ uniform, depth texture,
/// normal texture, sampler }`; the sampler must be **nearest**, because every
/// fetch here is a point sample of a G-buffer channel and a linear filter across
/// a depth discontinuity invents geometry that is not there.
pub(crate) const GTAO_CORE_PASS_WGSL: &str = r#"
struct AxiomGtaoCoreU {
    proj_inv: mat4x4<f32>,
    // 1 / resolution.
    texel: vec2<f32>,
    resolution: vec2<f32>,
    // x radius(m)  y intensity (NEVER READ -- the blur owns intensity)
    // z frame      w thickness (NEVER READ -- see `horizon_update`)
    params: vec4<f32>,
    // x = uP11 = camera.projectionMatrix.elements[5].
    p11: vec4<f32>,
};

@group(0) @binding(0) var<uniform> gtao_core_u: AxiomGtaoCoreU;
@group(0) @binding(1) var gtao_core_depth: texture_2d<f32>;
@group(0) @binding(2) var gtao_core_normal: texture_2d<f32>;
@group(0) @binding(3) var gtao_core_samp: sampler;

@fragment
fn axiom_gtao_core_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    let uv = position.xy * gtao_core_u.texel;
    let nrm = textureSampleLevel(gtao_core_normal, gtao_core_samp, uv, 0.0);

    // `if ( nrm.z < 0.5 ) { gl_FragColor = vec4( 1.0, 1e4, 0.0, 1.0 ); return; }`
    // The 1e4 depth sentinel is what makes every downstream depth-aware weight
    // reject this pixel; a 0.0 sentinel would have done the opposite.
    if (nrm.z < 0.5) {
        return vec2<f32>(1.0, 1e4);
    }

    let depth = textureSampleLevel(gtao_core_depth, gtao_core_samp, uv, 0.0).r;
    // `owViewPos` wants the source's v-UP uv; see `gtao::NDC_UV_V_SIGN`.
    let source_uv = vec2<f32>(uv.x, 1.0 - uv.y);
    let p = axiom_gtao_view_pos(source_uv, depth, gtao_core_u.proj_inv);
    let n = axiom_gtao_decode_normal(nrm.xy);
    let p_len = sqrt(p.x * p.x + p.y * p.y + p.z * p.z);
    let v = vec3<f32>(-p.x / p_len, -p.y / p_len, -p.z / p_len);

    let radius = gtao_core_u.params.x;
    let radius_px = axiom_gtao_radius_px(radius, gtao_core_u.p11.x, gtao_core_u.resolution.y, depth);

    // `gl_FragCoord.y` is measured from the BOTTOM of the framebuffer;
    // `@builtin(position).y` from the top. The third v correction, and the same
    // fact as the other two -- without it the dither is mirrored, which is
    // harmless to look at and is still not what the source computes.
    let frag = vec2<f32>(position.x, gtao_core_u.resolution.y - position.y);

    // `owIGN( gl_FragCoord.xy + uParams.z * 5.588238 )` -- a vec2 + float
    // broadcast, so the SAME scalar is added to both components.
    let phase = gtao_core_u.params.z;
    let noise = axiom_gtao_ign(frag + vec2<f32>(phase * 5.588238, phase * 5.588238));
    let noise2 = axiom_gtao_hash12(frag * 0.371 + vec2<f32>(phase, phase));

    let inv_r2 = 1.0 / (radius * radius);
    var visibility = 0.0;

    for (var s = 0; s < AXIOM_GTAO_SLICES; s = s + 1) {
        let dir2 = axiom_gtao_slice_direction(f32(s), noise);
        let frame = axiom_gtao_slice_frame(n, v, dir2);
        if (frame.proj_len < 1e-4) { continue; }

        // A view-space +dir2.y is UP the screen, which is a DECREASE in a WebGPU
        // framebuffer's v. Get this wrong and the +dir/-dir horizons swap
        // relative to orthoDir -- the source's own comment says that collapses
        // the visibility arc on every grazing surface.
        let step_dir = vec2<f32>(dir2.x, dir2.y * AXIOM_GTAO_SCREEN_STEP_V_SIGN);

        var cos_h_pos = -1.0;
        var cos_h_neg = -1.0;

        for (var t = 0; t < AXIOM_GTAO_STEPS; t = t + 1) {
            let off = axiom_gtao_step_offset(f32(t), noise2, radius_px);
            let duv = step_dir * off * gtao_core_u.texel;

            // +dir. The bounds test is STRICT on both ends, as the source writes it.
            let uv1 = uv + duv;
            if (uv1.x > 0.0 && uv1.x < 1.0 && uv1.y > 0.0 && uv1.y < 1.0) {
                let cov1 = textureSampleLevel(gtao_core_normal, gtao_core_samp, uv1, 0.0).z;
                if (cov1 > 0.5) {
                    let d1 = textureSampleLevel(gtao_core_depth, gtao_core_samp, uv1, 0.0).r;
                    let q = axiom_gtao_view_pos(vec2<f32>(uv1.x, 1.0 - uv1.y), d1, gtao_core_u.proj_inv);
                    cos_h_pos = axiom_gtao_horizon_update(
                        cos_h_pos, vec3<f32>(q.x - p.x, q.y - p.y, q.z - p.z), v, inv_r2);
                }
            }

            // -dir
            let uv2 = uv - duv;
            if (uv2.x > 0.0 && uv2.x < 1.0 && uv2.y > 0.0 && uv2.y < 1.0) {
                let cov2 = textureSampleLevel(gtao_core_normal, gtao_core_samp, uv2, 0.0).z;
                if (cov2 > 0.5) {
                    let d2 = textureSampleLevel(gtao_core_depth, gtao_core_samp, uv2, 0.0).r;
                    let q = axiom_gtao_view_pos(vec2<f32>(uv2.x, 1.0 - uv2.y), d2, gtao_core_u.proj_inv);
                    cos_h_neg = axiom_gtao_horizon_update(
                        cos_h_neg, vec3<f32>(q.x - p.x, q.y - p.y, q.z - p.z), v, inv_r2);
                }
            }
        }

        visibility = visibility + axiom_gtao_slice_visibility(cos_h_neg, cos_h_pos, frame);
    }

    return vec2<f32>(axiom_gtao_resolve(visibility), depth);
}
"#;

/// `AO_TEMPORAL`: velocity-reprojected accumulation, `RG16Float` in and out.
///
/// The history target must be a **second** buffer, ping-ponged, and the blur must
/// write somewhere else entirely — `render()`'s comment is explicit: *"the
/// history must stay un-blurred or the accumulator smears more every frame."*
/// Concatenate after [`GTAO_WGSL`].
pub(crate) const GTAO_TEMPORAL_PASS_WGSL: &str = r#"
struct AxiomGtaoTemporalU {
    texel: vec2<f32>,
    // x = uFeedback (0.92).
    params: vec2<f32>,
};

@group(0) @binding(0) var<uniform> gtao_temporal_u: AxiomGtaoTemporalU;
@group(0) @binding(1) var gtao_temporal_current: texture_2d<f32>;
@group(0) @binding(2) var gtao_temporal_history: texture_2d<f32>;
@group(0) @binding(3) var gtao_temporal_velocity: texture_2d<f32>;
@group(0) @binding(4) var gtao_temporal_samp: sampler;

@fragment
fn axiom_gtao_temporal_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    let uv = position.xy * gtao_temporal_u.texel;
    let cur = textureSampleLevel(gtao_temporal_current, gtao_temporal_samp, uv, 0.0).rg;

    // The G-buffer stores half the NDC delta in a y-UP clip space; this
    // framebuffer's v runs down. See `gbuffer::VELOCITY_TEXTURE_V_SIGN`.
    let vel_raw = textureSampleLevel(gtao_temporal_velocity, gtao_temporal_samp, uv, 0.0).rg;
    let vel = vec2<f32>(vel_raw.x, vel_raw.y * AXIOM_GTAO_VELOCITY_V_SIGN);
    // `huv = vUv - vel`. `glsl.js`'s header says velocity can be ADDED; both
    // this pass and `taa.js` subtract, and `taa.js`'s fallback
    // (`vel = vUv - prevUv`) settles which is right. The header is wrong.
    let huv = uv - vel;

    let hist = textureSampleLevel(gtao_temporal_history, gtao_temporal_samp, huv, 0.0).rg;
    let w = axiom_gtao_temporal_weight(gtao_temporal_u.params.x, huv, hist.g, cur.g);

    // A WIDE window at TWO texels, in the source's i = 0..4 order.
    let o = gtao_temporal_u.texel * 2.0;
    let n0 = textureSampleLevel(gtao_temporal_current, gtao_temporal_samp, uv + vec2<f32>(o.x, 0.0), 0.0).r;
    let n1 = textureSampleLevel(gtao_temporal_current, gtao_temporal_samp, uv - vec2<f32>(o.x, 0.0), 0.0).r;
    let n2 = textureSampleLevel(gtao_temporal_current, gtao_temporal_samp, uv + vec2<f32>(0.0, o.y), 0.0).r;
    let n3 = textureSampleLevel(gtao_temporal_current, gtao_temporal_samp, uv - vec2<f32>(0.0, o.y), 0.0).r;
    let h = axiom_gtao_temporal_clamp(hist.r, cur.r, n0, n1, n2, n3);

    // `g` carries the CURRENT depth, never the history's -- the next frame's
    // rejection test compares against the frame this value was stored for.
    return vec2<f32>(axiom_gtao_mix(cur.r, h, w), cur.g);
}
"#;

/// `AO_BLUR`: the separable depth-aware bilateral, run twice.
///
/// `uDirection` is already in **uv** units (`(texel.x, 0)` then `(0, texel.y)`),
/// and `uParams.x` is `0` on the horizontal pass and `1` on the vertical —
/// *"clamp + intensity curve on the last stage only"*. Concatenate after
/// [`GTAO_WGSL`].
pub(crate) const GTAO_BLUR_PASS_WGSL: &str = r#"
struct AxiomGtaoBlurU {
    texel: vec2<f32>,
    // The blur axis, in uv units: (texel.x, 0) then (0, texel.y).
    direction: vec2<f32>,
    // x: apply the clamp + intensity curve on this pass.  y: intensity (1.1).
    // A vec4 rather than the vec2 it needs: a uniform struct's size must be a
    // multiple of 16, and 8 + 8 + 8 is not.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> gtao_blur_u: AxiomGtaoBlurU;
@group(0) @binding(1) var gtao_blur_ao: texture_2d<f32>;
@group(0) @binding(2) var gtao_blur_samp: sampler;

@fragment
fn axiom_gtao_blur_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    let uv = position.xy * gtao_blur_u.texel;
    let c = textureSampleLevel(gtao_blur_ao, gtao_blur_samp, uv, 0.0).rg;
    var sum = c.r * 0.4;
    var wsum = 0.4;

    for (var i = 1; i <= 3; i = i + 1) {
        let w0 = axiom_gtao_blur_distance_weight(f32(i));
        let o = gtao_blur_u.direction * f32(i);
        let a = textureSampleLevel(gtao_blur_ao, gtao_blur_samp, uv + o, 0.0).rg;
        let b = textureSampleLevel(gtao_blur_ao, gtao_blur_samp, uv - o, 0.0).rg;
        let wa = axiom_gtao_blur_tap_weight(w0, a.g, c.g);
        let wb = axiom_gtao_blur_tap_weight(w0, b.g, c.g);
        // The pair is summed FIRST and then added, which is not the same
        // rounding as two separate `+=`.
        sum = sum + (a.r * wa + b.r * wb);
        wsum = wsum + (wa + wb);
    }

    return vec2<f32>(
        axiom_gtao_blur_output(sum, wsum, gtao_blur_u.params.x, gtao_blur_u.params.y),
        c.g,
    );
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wgsl_declares_every_function_the_cpu_reference_mirrors() {
        [
            "fn axiom_gtao_clamp(",
            "fn axiom_gtao_mix(",
            "fn axiom_gtao_fract(",
            "fn axiom_gtao_sign(",
            "fn axiom_gtao_ign(",
            "fn axiom_gtao_hash12(",
            "fn axiom_gtao_view_pos(",
            "fn axiom_gtao_arc(",
            "fn axiom_gtao_radius_px(",
            "fn axiom_gtao_step_offset(",
            "fn axiom_gtao_horizon_update(",
            "fn axiom_gtao_slice_frame(",
            "fn axiom_gtao_slice_direction(",
            "fn axiom_gtao_slice_visibility(",
            "fn axiom_gtao_resolve(",
            "fn axiom_gtao_temporal_weight(",
            "fn axiom_gtao_temporal_clamp(",
            "fn axiom_gtao_blur_distance_weight(",
            "fn axiom_gtao_blur_tap_weight(",
            "fn axiom_gtao_blur_output(",
            "fn axiom_gtao_decode_normal(",
        ]
        .iter()
        .for_each(|declaration| {
            assert!(
                GTAO_WGSL.contains(declaration),
                "GTAO_WGSL no longer declares `{declaration}`; the CPU reference \
                 next door still mirrors it and the parity harness calls it"
            );
        });
    }

    #[test]
    fn the_wgsl_calls_no_builtin_whose_factoring_is_unspecified() {
        // `clamp`, `mix`, `sign`, `fract`, `dot`, `length`, `normalize` and
        // `cross` are all written out. A bare call to one is the regression.
        ["\tclamp(", " clamp(", " mix(", " sign(", " dot(", " length(", " normalize(", " cross(", " fract("]
            .iter()
            .for_each(|call| {
                assert!(
                    !GTAO_WGSL.contains(call),
                    "GTAO_WGSL calls the builtin `{}`, whose factoring WGSL leaves \
                     open; write it out as the GLSL means it",
                    call.trim()
                );
            });
        // `inverseSqrt` IS kept: the source says `inversesqrt`.
        assert!(GTAO_WGSL.contains("inverseSqrt(len2)"));
    }

    #[test]
    fn the_three_constants_that_decide_the_look_are_in_the_text() {
        assert!(
            GTAO_WGSL.contains("const AXIOM_GTAO_SLICES: i32 = 3;"),
            "three slices, not the header's two"
        );
        assert!(GTAO_WGSL.contains("const AXIOM_GTAO_STEPS: i32 = 8;"));
        // The quadratic distribution and its +1 px floor.
        assert!(GTAO_WGSL.contains("return radius_px * ft * ft + 1.0;"));
        // The quartic falloff.
        assert!(GTAO_WGSL.contains("fall = fall * fall;"));
        // The clamp about `n`, not about zero.
        assert!(GTAO_WGSL.contains("h1 = s.n + max(h1 - s.n, -AXIOM_GTAO_HALF_PI);"));
        // Four, not one.
        assert!(GTAO_WGSL.contains("axiom_gtao_clamp(sum / f32(AXIOM_GTAO_SLICES), 0.0, 4.0)"));
        // The wide temporal window.
        assert!(GTAO_WGSL.contains("mn - 0.45, mx + 0.45"));
        // The blur's edge-stopping scale.
        assert!(GTAO_WGSL.contains("* 22.0 / max(0.1, centre_depth)"));
    }

    #[test]
    fn each_pass_writes_two_channels_because_the_targets_are_rg16f() {
        [
            ("core", GTAO_CORE_PASS_WGSL),
            ("temporal", GTAO_TEMPORAL_PASS_WGSL),
            ("blur", GTAO_BLUR_PASS_WGSL),
        ]
        .iter()
        .for_each(|(name, source)| {
            assert!(
                source.contains("-> @location(0) vec2<f32>"),
                "the {name} pass must write an RG pair, not a vec4"
            );
        });
    }

    #[test]
    fn both_v_conventions_are_corrected_exactly_once_each() {
        // The reconstruction flip, at the centre and at both tap sites.
        assert_eq!(
            GTAO_CORE_PASS_WGSL.matches("1.0 - uv").count(),
            3,
            "every one of the three owViewPos call sites must flip v"
        );
        assert_eq!(
            GTAO_CORE_PASS_WGSL.matches("axiom_gtao_view_pos(").count(),
            3,
            "if a fourth reconstruction appears it needs a flip too"
        );
        // And the frag-coord origin, which is the third v fact.
        assert!(
            GTAO_CORE_PASS_WGSL.contains("gtao_core_u.resolution.y - position.y"),
            "gl_FragCoord.y is measured from the bottom; position.y is not"
        );
        assert!(
            GTAO_CORE_PASS_WGSL.contains("dir2.y * AXIOM_GTAO_SCREEN_STEP_V_SIGN"),
            "the horizon step must negate the view-space y"
        );
        assert!(
            GTAO_TEMPORAL_PASS_WGSL.contains("vel_raw.y * AXIOM_GTAO_VELOCITY_V_SIGN"),
            "the reprojection must negate the velocity's y"
        );
        assert!(
            GTAO_TEMPORAL_PASS_WGSL.contains("let huv = uv - vel;"),
            "the reprojection SUBTRACTS the current-minus-previous delta"
        );
    }

    #[test]
    fn the_two_dead_core_uniform_lanes_are_ported_and_labelled_dead() {
        assert!(
            GTAO_CORE_PASS_WGSL.contains("NEVER READ -- the blur owns intensity"),
            "uParams.y is a dead lane and must stay named as one"
        );
        assert!(
            GTAO_CORE_PASS_WGSL.contains("NEVER READ -- see `horizon_update`"),
            "uParams.w (thickness) is a dead lane and must stay named as one"
        );
        // And they really are unread: the core reads only .x and .z.
        assert!(!GTAO_CORE_PASS_WGSL.contains("params.y"));
        assert!(!GTAO_CORE_PASS_WGSL.contains("params.w"));
        assert!(GTAO_CORE_PASS_WGSL.contains("gtao_core_u.params.x"));
        assert!(GTAO_CORE_PASS_WGSL.contains("gtao_core_u.params.z"));
    }
}
