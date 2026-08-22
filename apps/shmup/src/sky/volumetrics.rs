//! Ported from Claude-of-Duty `src/sky/volumetrics.js:1-527` — volumetric
//! fog, light shafts and aerial perspective. The source is three WebGL2
//! fragment shaders (`MARCH_FRAG`, `RESOLVE_FRAG`, `COMPOSITE_FRAG`, built
//! on a `SHARED` + `CSM_GLSL` prelude) plus a `Volumetrics` class that wires
//! them to render targets. No JavaScript form of any shader exists anywhere,
//! so every function here is hand-transcribed the same way
//! `dome`/`clouds`/`stars` are, pinned against a second, independent
//! hand-transcription in `tests/sky_volumetrics/capture.mjs`.
//!
//! ## What is here
//!
//! | source                                    | here |
//! |-------------------------------------------|------|
//! | `skFogAmbient` (70-79)                    | [`fog_ambient`] |
//! | `skFogPhase` (83-85)                      | [`fog_phase`] |
//! | `skFogInscatterPhase` (103-107)           | [`fog_inscatter_phase`] |
//! | `skFogNearRamp` (118-120)                 | [`fog_near_ramp`] |
//! | `skFogDensity` (123-129)                  | [`fog_density`] |
//! | `skHeightIntegral` (136-141)              | [`height_integral`] |
//! | `skRayFor` (144-151)                      | [`ray_for`] |
//! | `skVogel` (163-167)                       | [`vogel`] |
//! | `skSunVisibility` (174-199)               | [`sun_visibility`] |
//! | `MARCH_FRAG` `main` (211-274)             | [`march_frag`] + [`raymarch_fog`] |
//! | `RESOLVE_FRAG` `main` (287-309)           | [`resolve_frag`] |
//! | `skUpsample` (325-341)                    | [`upsample`] |
//! | `COMPOSITE_FRAG` `main`, marched (344-388)| [`composite_marched`] |
//! | `COMPOSITE_FRAG` `main`, `VOL_ANALYTIC`   | [`composite_analytic`] |
//! | `Volumetrics.resize` sizing (469-470)     | [`half_res_size`] |
//! | `Volumetrics.render` bookkeeping (485-506)| [`TemporalState`] |
//!
//! ## What is *not* here, and an honest account of why
//!
//! An earlier revision of this module claimed four things were "deliberately
//! not ported". Re-audited against the source, exactly **one** of those four
//! was a real GPU boundary; the other three were unfinished work wearing a
//! justification, and two further functions (`skUpsample`, and
//! `COMPOSITE_FRAG`'s marched branch) were not ported and were not mentioned
//! at all. The verdict, function by function:
//!
//! * **`SkyPass` / `hdrTarget` / uniform binding / render-target allocation
//!   and disposal (the `Volumetrics` class body, 392-527) — a genuine
//!   boundary.** Allocating a framebuffer and binding a sampler is not
//!   arithmetic and there is nothing to reference-implement. *But* the class
//!   is not uniformly plumbing: `resize`'s half-resolution sizing, `render`'s
//!   `frame % 64` dither phase, and the history ping-pong / first-frame
//!   `uBlend = 0` are ordinary arithmetic and state that decide what the
//!   shaders compute. Those are ported ([`half_res_size`],
//!   [`TemporalState`]); only the GPU object lifetimes are left out.
//! * **`skRayFor` (144-151) — not a boundary.** It is one `mat4 * vec4`, a
//!   perspective divide, an upper-3x3 rotate and a normalise. The matrices
//!   are *inputs*, exactly as the atmosphere LUTs are inputs to
//!   [`super::atmosphere::raymarch_sky`]. "This crate has no camera-matrix
//!   type yet" is a reason to add [`Mat4`], not to skip the function. Ported.
//! * **`RESOLVE_FRAG` (277-310) — not a boundary.** A fragment shader is a
//!   pure function of its samplers; the temporal *state* lives in the render
//!   targets outside it, and the neighbourhood min/max, the widened clamp and
//!   the off-screen `w = 0` reject are all plain arithmetic. Ported as
//!   [`resolve_frag`], with the three `texture()` fetches as closures.
//! * **`skSunVisibility` / `CSM_GLSL` (154-200) — almost entirely not a
//!   boundary.** Cascade selection, the shadow-matrix transform, the
//!   projective divide, the border reject, the depth bias and the four Vogel
//!   taps are CPU maths. Only `texture(owCsmMaps, vec3(uv, layer)).r` — one
//!   texel fetch — needs a GPU. Ported as [`sun_visibility`], with that fetch
//!   as a closure.
//!
//! ## GPU-only inputs, made explicit
//!
//! Where a function genuinely needs something only a GPU has, it takes it as
//! a parameter or a closure rather than faking it — the shape
//! [`super::atmosphere::raymarch_sky`] set for `uTransmittanceLut` and
//! [`super::dome::sun_disc`] set for `fwidth`. In this module that is:
//!
//! * every `texture()` fetch — `tDepth`, `tVolume`, `tCurrent`, `tHistory`,
//!   `tVelocity`, `owCsmMaps` (closures), and `uSkyAmbientLut` (already the
//!   `ambient: [Vec3; 2]` two-texel probe, since it is a 2x1 texture read at
//!   its two exact texel centres);
//! * `gl_FragCoord.xy` and `vUv` — rasteriser-provided, so parameters.
//!
//! What those closures do **not** model, and a GPU would: bilinear filtering,
//! wrap/clamp addressing outside `[0,1]`, and the fp16 storage of the HDR
//! targets. A caller supplying nearest-neighbour `f64` lookups gets the
//! algorithm, not the sampler.
//!
//! ## Scattering vs extinction
//!
//! Deliberately separate uniforms, not tied by a single-scattering albedo —
//! see `volumetrics.js:29-35`. Extinction sets ground-level visibility;
//! inscatter gain sets shaft readability. Tying them either hides the shafts
//! outdoors or turns 200 m of street to milk.

use super::atmosphere::{gl_mix, hg_phase, Vec3};
use super::clouds::{cloud_shadow, CloudParams};
use super::noise::{ign, val3, Vec2};

/// GLSL `step(edge, x)`: `0.0` when `x < edge`, `1.0` otherwise. Note the
/// argument order — the *edge* comes first, unlike every `smoothstep`-shaped
/// helper. Used by [`sun_visibility`]'s shadow compare.
fn gl_step(edge: f64, x: f64) -> f64 {
    (x >= edge) as u8 as f64
}

/// A minimal `f64` 4-vector. This module's own vocabulary, for the same
/// reason [`super::noise::Vec2`] and [`super::atmosphere::Vec3`] are theirs
/// (see `crate::materials::noise`'s module doc): `RESOLVE_FRAG` carries
/// radiance *and* transmittance in one `vec4` and clamps/mixes all four
/// channels together, and nothing else in this crate needed a `vec4` before.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Vec4 {
    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Vec4 { x, y, z, w }
    }

    /// GLSL `vec4( v, w )`.
    pub const fn from_vec3(v: Vec3, w: f64) -> Self {
        Vec4::new(v.x, v.y, v.z, w)
    }

    /// GLSL swizzle `.xyz`.
    pub const fn xyz(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    pub fn add(self, o: Vec4) -> Vec4 {
        Vec4::new(self.x + o.x, self.y + o.y, self.z + o.z, self.w + o.w)
    }

    pub fn sub(self, o: Vec4) -> Vec4 {
        Vec4::new(self.x - o.x, self.y - o.y, self.z - o.z, self.w - o.w)
    }

    /// `vec4 * float`.
    pub fn scale(self, s: f64) -> Vec4 {
        Vec4::new(self.x * s, self.y * s, self.z * s, self.w * s)
    }

    /// `vec4 + float` (GLSL broadcasts a scalar across every component).
    pub fn add_scalar(self, s: f64) -> Vec4 {
        Vec4::new(self.x + s, self.y + s, self.z + s, self.w + s)
    }

    /// Componentwise `min`.
    pub fn min(self, o: Vec4) -> Vec4 {
        Vec4::new(
            self.x.min(o.x),
            self.y.min(o.y),
            self.z.min(o.z),
            self.w.min(o.w),
        )
    }

    /// Componentwise `max`.
    pub fn max(self, o: Vec4) -> Vec4 {
        Vec4::new(
            self.x.max(o.x),
            self.y.max(o.y),
            self.z.max(o.z),
            self.w.max(o.w),
        )
    }

    /// Componentwise `clamp(v, lo, hi)`.
    pub fn clamp(self, lo: Vec4, hi: Vec4) -> Vec4 {
        self.max(lo).min(hi)
    }

    /// GLSL `mix(a, b, t)` with a scalar `t`, written `a + (b - a) * t` — the
    /// same convention as [`super::atmosphere::gl_mix`].
    pub fn mix(self, o: Vec4, t: f64) -> Vec4 {
        self.add(o.sub(self).scale(t))
    }
}

/// A 4x4 matrix, stored **row-major**: `self.0[row][col]`, matching
/// [`super::celestial::Mat3`]'s convention in this crate.
///
/// THREE stores `Matrix4.elements` **column-major**, so a uniform lifted
/// straight out of a `THREE.Matrix4` must go through
/// [`Mat4::from_three_elements`] rather than being poured into `Mat4([..])`
/// row by row. Getting this backwards transposes the matrix, which compiles
/// and silently produces a plausible-but-wrong camera ray (the
/// "matrix storage order" trap in `docs/work-manifests/shmup-port`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    pub const fn identity() -> Self {
        Mat4([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    /// Build from THREE's column-major `Matrix4.elements` (`e[col * 4 + row]`).
    pub fn from_three_elements(e: &[f64; 16]) -> Self {
        let mut m = [[0.0; 4]; 4];
        for (row, out_row) in m.iter_mut().enumerate() {
            for (col, cell) in out_row.iter_mut().enumerate() {
                *cell = e[col * 4 + row];
            }
        }
        Mat4(m)
    }

    /// GLSL `mat4 * vec4`.
    pub fn mul_vec4(self, v: Vec4) -> Vec4 {
        let r = |i: usize| self.0[i][0] * v.x + self.0[i][1] * v.y + self.0[i][2] * v.z + self.0[i][3] * v.w;
        Vec4::new(r(0), r(1), r(2), r(3))
    }

    /// GLSL `mat3( m ) * vec3` — the upper-left 3x3 (the rotation/scale part
    /// of a world matrix, translation dropped), exactly what `skRayFor`'s
    /// `mat3( uCamWorld ) * vd` does.
    pub fn mul_vec3_upper3x3(self, v: Vec3) -> Vec3 {
        let r = |i: usize| self.0[i][0] * v.x + self.0[i][1] * v.y + self.0[i][2] * v.z;
        Vec3::new(r(0), r(1), r(2))
    }
}

/* ==================================================================== */
/* SHARED — volumetrics.js:38-152                                        */
/* ==================================================================== */

/// Colour of the haze as a function of angle to the key light — the two
/// texels of the ambient probe (cool whole-sky average, warm horizon-band
/// average) split the haze hue by direction instead of averaging it to grey.
/// `skFogAmbient`, `volumetrics.js:70-79`.
pub fn fog_ambient(cos_key: f64, ambient: [Vec3; 2], key_irr: Vec3) -> Vec3 {
    let cool = ambient[0];
    let hor = ambient[1];
    let max_c = key_irr.x.max(key_irr.y.max(key_irr.z)).max(1.0e-4);
    let key_hue = key_irr.scale(1.0 / max_c);
    let f = 0.5 + 0.5 * cos_key.clamp(-1.0, 1.0);
    let warm = hor.mul(Vec3::splat(1.0).mix(key_hue, 0.55)).scale(1.3);
    cool.mix(warm, f * f)
}

/// Dual-lobe Henyey-Greenstein: a forward peak for the shafts, a broad back
/// lobe so the fog is visible with the sun behind you. `skFogPhase`,
/// `volumetrics.js:83-85`.
pub fn fog_phase(cos_theta: f64, g_fwd: f64, g_back: f64, back_weight: f64) -> f64 {
    gl_mix(hg_phase(cos_theta, g_fwd), hg_phase(cos_theta, g_back), back_weight)
}

/// The shaft gain applied to the *anisotropic excess only*
/// (`p + max(0, p - iso) * (gain - 1)`), not the whole phase function —
/// multiplying the whole phase also multiplies its isotropic floor, which
/// covers every pixel regardless of where the sun is. See `volumetrics.js:87-107`
/// for the full derivation, including why the excess form (rather than
/// `mix(iso, p, gain)`, which goes negative in the side lobes) is used.
/// `skFogInscatterPhase`, `volumetrics.js:103-107`.
pub fn fog_inscatter_phase(cos_theta: f64, g_fwd: f64, g_back: f64, back_weight: f64, shaft_gain: f64) -> f64 {
    const ISO: f64 = 1.0 / (4.0 * std::f64::consts::PI);
    let p = fog_phase(cos_theta, g_fwd, g_back, back_weight);
    p + (p - ISO).max(0.0) * (shaft_gain - 1.0)
}

/// Near-field scattering ramp: fades the first 12 m of fog in, so an
/// exponential-height density tuned for 200 m of street doesn't wash the
/// weapon/hands/near geometry. `skFogNearRamp`, `volumetrics.js:118-120`.
pub fn fog_near_ramp(t: f64) -> f64 {
    super::atmosphere::smoothstep(0.0, 12.0, t)
}

/// Normalised density: 1 at the fog base, exponential above, optionally
/// wind-torn by a two-octave value-noise field. `skFogDensity`,
/// `volumetrics.js:123-129`.
pub fn fog_density(p: Vec3, base_y: f64, inv_height_scale: f64, noise_scale: f64, drift: Vec3, noise_amount: f64) -> f64 {
    let h = (-(p.y - base_y) * inv_height_scale).exp();
    if noise_amount <= 0.001 {
        return h;
    }
    let q = p.scale(noise_scale).add(drift);
    let n = val3(q) * 0.63 + val3(q.scale(2.71).add_scalar(5.1)) * 0.37;
    h * gl_mix(1.0, 0.30 + 1.55 * n, noise_amount)
}

/// Closed form of `integral(0..t) exp(-(y-b)/H) ds` along a ray — exact, so
/// the transmittance applied to geometry is smooth at full resolution.
/// `skHeightIntegral`, `volumetrics.js:136-141`.
pub fn height_integral(y0: f64, dy: f64, t: f64, base_y: f64, inv_height_scale: f64) -> f64 {
    let d0 = (-(y0 - base_y) * inv_height_scale).exp();
    let x = dy * inv_height_scale * t;
    if x.abs() < 1.0e-4 {
        return d0 * t;
    }
    d0 * (1.0 - (-x).exp()) / (dy * inv_height_scale)
}

/// World ray through a `uv`, normalised onto the `z = -1` plane before
/// rotation. Returns `(dir, ray_len)` — the source's two `out` parameters —
/// where `dir` is unit and `ray_len` is the length of the un-normalised
/// z = -1 ray, so `t / ray_len` recovers a view-space depth (which is exactly
/// what [`march_frag`] hands [`sun_visibility`]).
///
/// `inv_proj`/`cam_world` are `uInvProj`/`uCamWorld`; see [`Mat4`] on storage
/// order before building them from a THREE matrix. `skRayFor`,
/// `volumetrics.js:144-151`.
pub fn ray_for(uv: Vec2, inv_proj: Mat4, cam_world: Mat4) -> (Vec3, f64) {
    let h = inv_proj.mul_vec4(Vec4::new(uv.x * 2.0 - 1.0, uv.y * 2.0 - 1.0, 1.0, 1.0));
    // `h.xyz / h.w` — a true divide, not a reciprocal multiply. `x / w` and
    // `x * (1/w)` differ in the last bit and this port is pinned bit-for-bit
    // against a second transcription; the same applies to every `/` below.
    let vd = h.xyz().div(Vec3::splat(h.w));
    let vd = vd.div(Vec3::splat((-vd.z).max(1.0e-6)));
    let w = cam_world.mul_vec3_upper3x3(vd);
    let ray_len = w.length();
    (w.div(Vec3::splat(ray_len)), ray_len)
}

/* ==================================================================== */
/* CSM_GLSL — volumetrics.js:154-200                                     */
/* ==================================================================== */

/// A single Vogel-disc tap. `skVogel`, `volumetrics.js:163-167`.
pub fn vogel(i: i32, n: i32, phi: f64) -> Vec2 {
    let r = ((f64::from(i) + 0.5) / f64::from(n)).sqrt();
    let theta = f64::from(i) * 2.399_963_23 + phi;
    Vec2::new(theta.cos(), theta.sin()).scale(r)
}

/// The cascade-shadow-map uniforms `skSunVisibility` reads (`CSM_GLSL`,
/// `volumetrics.js:155-161`; declared in `src/render/csm.js:67-79`).
///
/// `owCsmMaps` — the `sampler2DArray` itself — is not here: it is the one
/// genuinely GPU-only input, and [`sun_visibility`] takes it as a closure.
#[derive(Debug, Clone, Copy)]
pub struct CsmUniforms<'a> {
    /// `OW_CASCADES`, the shader define. `matrix` must have this length, and
    /// `split`/`texel`/`range` are indexed only below it.
    pub cascades: usize,
    /// `owCsmMatrix[ OW_CASCADES ]` — world -> cascade clip.
    pub matrix: &'a [Mat4],
    /// `owCsmSplit` — per-cascade far view-depth. A `vec4` in the source even
    /// when `OW_CASCADES < 4`.
    pub split: [f64; 4],
    /// `owCsmTexel` — per-cascade world size of one shadow texel.
    pub texel: [f64; 4],
    /// `owCsmRange` — per-cascade depth range, the bias denominator.
    pub range: [f64; 4],
    /// `owCsmMapSize` — `(mapSize, 1 / mapSize)`. Only `.y` is read here.
    pub map_size: Vec2,
    /// `owCsmParams` — `(strength, tan(sun angular radius), max filter
    /// radius in texels, temporal rotation)`. `skSunVisibility` reads only
    /// `.x`; the whole vec4 is carried so the struct matches the uniform.
    pub params: [f64; 4],
}

/// Sun/moon visibility at a world point. Four Vogel taps, not a full PCSS
/// lookup: a volumetric sample is already averaged along the ray and then
/// temporally accumulated, so the extra taps would buy nothing.
/// `skSunVisibility`, `volumetrics.js:174-199`.
///
/// `sample_csm(uv, layer)` stands in for
/// `texture( owCsmMaps, vec3( uv, float( c ) ) ).r` — the single texel fetch
/// that needs a GPU. Everything else (cascade selection, the projective
/// divide, the border reject, the depth bias, the taps) is computed here.
///
/// Source quirk, ported as-is: the early `viewDepth >= owCsmSplit[cascades-1]`
/// return makes the loop's `c = OW_CASCADES - 1` fallback unreachable — the
/// loop always breaks. Kept, because the judgement that it is dead can be
/// wrong and preserving it costs nothing.
pub fn sun_visibility(
    w_pos: Vec3,
    view_depth: f64,
    rot: f64,
    csm: &CsmUniforms<'_>,
    sample_csm: impl Fn(Vec2, usize) -> f64,
) -> f64 {
    if csm.params[0] <= 0.0 {
        return 1.0;
    }
    if view_depth >= csm.split[csm.cascades - 1] {
        return 1.0;
    }

    let mut c = csm.cascades - 1;
    for i in 0..csm.cascades {
        if view_depth < csm.split[i] {
            c = i;
            break;
        }
    }

    let sc = csm.matrix[c].mul_vec4(Vec4::from_vec3(w_pos, 1.0));
    let proj = sc.xyz().div(Vec3::splat(sc.w)).scale(0.5).add_scalar(0.5);
    if proj.z >= 1.0 || proj.z <= 0.0 {
        return 1.0;
    }
    let edge = Vec2::new(proj.x.min(1.0 - proj.x), proj.y.min(1.0 - proj.y));
    if edge.x.min(edge.y) <= 0.0 {
        return 1.0;
    }

    // No surface normal out here, so the bias is purely depth based; two
    // texels of the cascade's own range is enough to stop shafts
    // self-shadowing.
    let recv = proj.z - (csm.texel[c] * 2.2) / csm.range[c];
    let r = csm.map_size.y * 1.6;
    let mut s = 0.0;
    for i in 0..4 {
        let o = vogel(i, 4, rot * 6.2831853).scale(r);
        s += gl_step(recv, sample_csm(Vec2::new(proj.x + o.x, proj.y + o.y), c));
    }
    gl_mix(1.0, s * 0.25, csm.params[0])
}

/* ==================================================================== */
/* MARCH_FRAG — volumetrics.js:202-275                                   */
/* ==================================================================== */

/// The `uFog`/`uFog2`/`uPhase`/... uniforms the march and composite passes
/// share, unpacked. See each field's comment for the uniform it replaces.
#[derive(Debug, Clone, Copy)]
pub struct FogUniforms {
    /// `uFog.x` — scattering coefficient.
    pub sigma_s: f64,
    /// `uFog.y` — `1 / height scale`.
    pub inv_height_scale: f64,
    /// `uFog.z` — base Y.
    pub base_y: f64,
    /// `uFog.w` — maximum march/haze distance, in metres.
    pub max_distance: f64,
    /// `uFog2.x` — extinction coefficient (monochrome; see this module's
    /// scattering-vs-extinction note).
    pub sigma_e: f64,
    /// `uFog2.y` — shaft gain, applied via [`fog_inscatter_phase`].
    pub shaft_gain: f64,
    /// `uFog2.z` — ambient boost.
    pub ambient_boost: f64,
    /// `uFog2.w` — wind-torn noise amount.
    pub noise_amount: f64,
    /// `uFogExt` — per-channel extinction, for the analytic transmittance.
    /// Separate from `sigma_e` on purpose (`volumetrics.js:49`): the marched
    /// pass integrates one monochrome coefficient, the composite applies
    /// three.
    pub fog_ext: Vec3,
    /// `uPhase.x` — forward HG `g`.
    pub g_fwd: f64,
    /// `uPhase.y` — back HG `g`.
    pub g_back: f64,
    /// `uPhase.z` — back-lobe weight.
    pub back_weight: f64,
    /// `uPhase.w` — density-noise spatial scale.
    pub noise_scale: f64,
    pub key_dir: Vec3,
    pub key_irr: Vec3,
    pub fog_drift: Vec3,
    /// `uSkyAmbientLut` read at its two texel centres — the two-texel ambient
    /// probe (`super::luts::bake_ambient`'s output).
    pub ambient: [Vec3; 2],
}

/// The distance both the march and the composite integrate over:
/// `sky ? uFog.w : min( depth * rayLen, uFog.w )`, where `sky` is
/// `depth <= 0.0`. `volumetrics.js:217-218` and `351-352` — the same three
/// lines in both shaders, factored once here so they cannot drift apart.
pub fn ray_max_distance(depth: f64, ray_len: f64, max_distance: f64) -> f64 {
    let sky = depth <= 0.0;
    if sky {
        max_distance
    } else {
        (depth * ray_len).min(max_distance)
    }
}

/// The interleaved-gradient dither offset that decorrelates the march's start
/// point per pixel and per frame: `skIGN( gl_FragCoord.xy + uFrame * 5.588238 )`,
/// `volumetrics.js:221`. `frame` is `uFrame`, already reduced by
/// [`march_frame_phase`].
pub fn march_dither(frag_coord: Vec2, frame: f64) -> f64 {
    ign(frag_coord.add_scalar(frame * 5.588238))
}

/// `uFrame`'s value: `r.frame % 64` (`volumetrics.js:491`). The dither cycles
/// with a period of 64 frames so the temporal resolve has a bounded sample
/// set to converge over.
pub fn march_frame_phase(frame: u64) -> f64 {
    (frame % 64) as f64
}

/// `MARCH_FRAG`'s `main()` in full: reconstruct the ray, clip it to the depth
/// buffer, dither the start offset, march, and write `vec4( L, T )`.
/// `volumetrics.js:211-274`.
///
/// `depth` is `texture( tDepth, vUv ).r`; `frag_coord` is `gl_FragCoord.xy`;
/// `frame` is `uFrame`. Returns the fragment colour, including the source's
/// `maxT <= 0.02` early-out value of `vec4( 0, 0, 0, 1 )`.
#[allow(clippy::too_many_arguments)]
pub fn march_frag(
    uv: Vec2,
    frag_coord: Vec2,
    frame: f64,
    inv_proj: Mat4,
    cam_world: Mat4,
    depth: f64,
    cam_pos: Vec3,
    u: &FogUniforms,
    cloud: &CloudParams,
    steps: u32,
    sun_visibility: impl Fn(Vec3, f64, f64) -> f64,
) -> Vec4 {
    let (dir, ray_len) = ray_for(uv, inv_proj, cam_world);

    let max_t = ray_max_distance(depth, ray_len, u.max_distance);
    if max_t <= 0.02 {
        return Vec4::new(0.0, 0.0, 0.0, 1.0);
    }

    let dith = march_dither(frag_coord, frame);
    let (l, t) = raymarch_fog(dir, ray_len, max_t, dith, cam_pos, u, cloud, steps, sun_visibility);
    Vec4::from_vec3(l, t)
}

/// The march loop itself — `MARCH_FRAG`'s `for` body plus the per-ray
/// constants it hoists (`volumetrics.js:222-273`). Kept separate from
/// [`march_frag`] so a caller that already has a ray (a debug probe, a
/// single-pixel test) does not have to fabricate a projection matrix.
///
/// Returns `(L, T)` — inscattered radiance and residual transmittance,
/// exactly `fragColor`'s `.rgb`/`.a`. Steps are distributed exponentially
/// (`f*f*(3-2f)*0.35 + f*f*f*0.65`, `volumetrics.js:242`): centimetres near
/// the camera, tens of metres at the far plane.
///
/// `sun_visibility(world_pos, view_depth, rot)` stands in for
/// `skSunVisibility` — see the module doc; wrap [`sun_visibility`] to supply
/// real cascade data.
#[allow(clippy::too_many_arguments)]
pub fn raymarch_fog(
    dir: Vec3,
    ray_len: f64,
    max_t: f64,
    dith: f64,
    cam_pos: Vec3,
    u: &FogUniforms,
    cloud: &CloudParams,
    steps: u32,
    sun_visibility: impl Fn(Vec3, f64, f64) -> f64,
) -> (Vec3, f64) {
    let cos_key = dir.dot(u.key_dir);
    let phase = fog_inscatter_phase(cos_key, u.g_fwd, u.g_back, u.back_weight, u.shaft_gain);
    let ambient = fog_ambient(cos_key, u.ambient, u.key_irr).scale(u.ambient_boost);

    // Cloud shadow, twice per ray rather than once per step (~25x cheaper,
    // visually indistinguishable — see volumetrics.js:226-230).
    let cloud_near = cloud_shadow(Vec2::new(cam_pos.x, cam_pos.z), u.key_dir, cloud);
    let far_pos = cam_pos.add(dir.scale(max_t));
    let cloud_far = cloud_shadow(Vec2::new(far_pos.x, far_pos.z), u.key_dir, cloud);

    let mut l = Vec3::splat(0.0);
    let mut t_trans = 1.0;
    let mut prev = 0.0;

    for i in 0..steps {
        let f = (f64::from(i) + dith) / f64::from(steps);
        let t = max_t * f * f * (3.0 - 2.0 * f) * 0.35 + max_t * f * f * f * 0.65;
        let dt = t - prev;
        prev = t;
        if dt <= 1.0e-5 {
            continue;
        }

        let wp = cam_pos.add(dir.scale(t));
        let dens = fog_density(wp, u.base_y, u.inv_height_scale, u.noise_scale, u.fog_drift, u.noise_amount);
        if dens <= 1.0e-4 {
            continue;
        }

        let sigma_s = u.sigma_s * dens * fog_near_ramp(t);
        let sigma_e = (u.sigma_e * dens).max(1.0e-7);

        let mut vis = sun_visibility(wp, t / ray_len, dith);
        vis *= gl_mix(cloud_near, cloud_far, f);

        // Ambient-occlusion proxy: a shadowed sample sees far less sky.
        let amb_occ = 0.42 + 0.58 * vis;

        // `phase` already carries the shaft gain on its anisotropic part
        // only, so this is the forward lobe lifted and nothing else.
        let j = u.key_irr.scale(vis * phase).add(ambient.scale(amb_occ));

        let a_t = (-sigma_e * dt).exp();
        // `T * j * sigmaS * ( 1 - aT ) / sigmaE`, left to right and with a
        // real divide at the end — not `* (1 / sigmaE)`, which rounds
        // differently. Do not tidy this accumulation.
        let contrib = j
            .scale(t_trans)
            .scale(sigma_s)
            .scale(1.0 - a_t)
            .div(Vec3::splat(sigma_e));
        l = l.add(contrib);
        t_trans *= a_t;
        if t_trans < 0.004 {
            break;
        }
    }

    (l, t_trans)
}

/* ==================================================================== */
/* RESOLVE_FRAG — volumetrics.js:277-310                                 */
/* ==================================================================== */

/// The temporal resolve: reproject the history by the velocity buffer, clamp
/// it to a (slightly widened) 3x3 neighbourhood of the current frame, and
/// blend. This is what turns the march's dithered samples into a clean shaft
/// instead of a noise field. `RESOLVE_FRAG`'s `main()`,
/// `volumetrics.js:287-309`.
///
/// The three samplers are closures (`tCurrent`, `tHistory`, `tVelocity`); the
/// frame-to-frame *state* they read from lives in the render targets outside
/// this function, which is itself pure. `blend` is `uBlend` — see
/// [`TemporalState`], which produces the `0` on the first frame after a
/// reset that stops the resolve from reading an undefined history.
pub fn resolve_frag(
    uv: Vec2,
    texel: Vec2,
    blend: f64,
    sample_current: impl Fn(Vec2) -> Vec4,
    sample_history: impl Fn(Vec2) -> Vec4,
    sample_velocity: impl Fn(Vec2) -> Vec2,
) -> Vec4 {
    let cur = sample_current(uv);
    let vel = sample_velocity(uv);
    let huv = Vec2::new(uv.x - vel.x, uv.y - vel.y);

    let mut lo = cur;
    let mut hi = cur;
    for i in 0..9 {
        if i == 4 {
            continue;
        }
        // GLSL integer `%` and `/` both truncate: i/3 is 0,0,0,1,1,1,2,2,2.
        let o = Vec2::new(f64::from(i % 3) - 1.0, f64::from(i / 3) - 1.0);
        let n = sample_current(Vec2::new(uv.x + o.x * texel.x, uv.y + o.y * texel.y));
        lo = lo.min(n);
        hi = hi.max(n);
    }
    // Widen slightly: clamping hard to the 3x3 range throws away the very
    // convergence the accumulation exists to buy.
    let c = lo.add(hi).scale(0.5);
    let e = hi.sub(lo).scale(0.5).scale(1.6).add_scalar(1.0e-5);
    let his = sample_history(huv).clamp(c.sub(e), c.add(e));

    let mut w = blend;
    if huv.x < 0.0 || huv.x > 1.0 || huv.y < 0.0 || huv.y > 1.0 {
        w = 0.0;
    }
    cur.mix(his, w)
}

/* ==================================================================== */
/* COMPOSITE_FRAG — volumetrics.js:312-390                               */
/* ==================================================================== */

/// Depth-aware 4-tap upsample of the half-resolution inscatter: bilinear
/// weights times a depth-similarity weight, which stops a bright shaft
/// bleeding across a foreground silhouette. `skUpsample`,
/// `volumetrics.js:325-341`.
///
/// `texel_half` is `uTexelHalf` (`1/mw, 1/mh` — see [`half_res_size`]);
/// `sample_depth`/`sample_volume` are `tDepth`/`tVolume`.
pub fn upsample(
    uv: Vec2,
    depth: f64,
    texel_half: Vec2,
    sample_depth: impl Fn(Vec2) -> f64,
    sample_volume: impl Fn(Vec2) -> Vec3,
) -> Vec3 {
    let hp = Vec2::new(uv.x / texel_half.x - 0.5, uv.y / texel_half.y - 0.5);
    let base = Vec2::new(hp.x.floor(), hp.y.floor());
    let f = Vec2::new(hp.x - base.x, hp.y - base.y);
    let mut sum = Vec3::splat(0.0);
    let mut wsum = 0.0;
    for i in 0..4 {
        let o = Vec2::new(f64::from(i & 1), f64::from(i >> 1));
        let tuv = Vec2::new(
            (base.x + o.x + 0.5) * texel_half.x,
            (base.y + o.y + 0.5) * texel_half.y,
        );
        let bw = (if o.x < 0.5 { 1.0 - f.x } else { f.x }) * (if o.y < 0.5 { 1.0 - f.y } else { f.y });
        let d = sample_depth(tuv);
        let w = bw / (0.05 + (d - depth).abs() * 0.35) + 1.0e-5;
        sum = sum.add(sample_volume(tuv).scale(w));
        wsum += w;
    }
    sum.div(Vec3::splat(wsum))
}

/// The per-channel analytic transmittance the composite applies to *every*
/// pixel, sky included: `exp( -uFogExt * skHeightIntegral(...) )`.
/// `volumetrics.js:367-368`.
///
/// It IS applied to sky pixels, deliberately — this layer is the ground haze
/// (dust, exhaust, the bottom 40 m of a hot street) and it sits between the
/// camera and the sky just as much as between the camera and a wall. Adding
/// the inscatter of a 900 m column while skipping the extinction made the fog
/// a pure emitter over the sky; see `volumetrics.js:354-366` for the full
/// account of the "cream void" that produced.
pub fn composite_transmittance(dir_y: f64, dist: f64, cam_pos_y: f64, u: &FogUniforms) -> (f64, Vec3) {
    let od = height_integral(cam_pos_y, dir_y, dist, u.base_y, u.inv_height_scale);
    (od, u.fog_ext.scale(-od).exp())
}

/// `COMPOSITE_FRAG` compiled **without** `VOL_ANALYTIC`: the marched path.
/// Analytic per-channel transmittance on the scene colour, plus a depth-aware
/// bilateral upsample of the marched inscatter. `volumetrics.js:344-388`.
///
/// `dir`/`dist` are the source's own `skRayFor( vUv, ... )` and
/// `sky ? uFog.w : min( depth * rayLen, uFog.w )`, passed in rather than
/// recomputed: they are byte-for-byte the same two expressions
/// [`march_frag`] evaluates, so [`ray_for`] and [`ray_max_distance`] stay the
/// single definition of each and the two passes cannot drift apart.
#[allow(clippy::too_many_arguments)]
pub fn composite_marched(
    color: Vec3,
    uv: Vec2,
    dir: Vec3,
    depth: f64,
    dist: f64,
    cam_pos_y: f64,
    u: &FogUniforms,
    texel_half: Vec2,
    sample_depth: impl Fn(Vec2) -> f64,
    sample_volume: impl Fn(Vec2) -> Vec3,
) -> Vec3 {
    let (_od, trans) = composite_transmittance(dir.y, dist, cam_pos_y, u);
    let inscatter = upsample(uv, depth, texel_half, sample_depth, sample_volume);
    color.mul(trans).add(inscatter)
}

/// `COMPOSITE_FRAG` compiled **with** `VOL_ANALYTIC`: the fallback used when
/// the marched pass is disabled (`config.q.volumetrics` off, or no CSM). No
/// shafts, but the same phase split, the same near-field ramp and the same
/// aerial perspective, so a low-end machine gets the correct distance falloff
/// rather than a differently-graded scene. `volumetrics.js:370-383`.
///
/// The ramp is folded in analytically: `smoothstep(0,12,t)` averages 0.5 over
/// `[0,12]` and 1 past it, so subtracting half the optical depth of the first
/// twelve metres reproduces the marched integral to within a few percent.
///
/// `dir`/`dist` come from [`ray_for`] and [`ray_max_distance`], for the
/// reason given on [`composite_marched`].
pub fn composite_analytic(color: Vec3, dir: Vec3, dist: f64, cam_pos_y: f64, u: &FogUniforms) -> Vec3 {
    let (od, trans) = composite_transmittance(dir.y, dist, cam_pos_y, u);

    let od_near = height_integral(cam_pos_y, dir.y, dist.min(12.0), u.base_y, u.inv_height_scale);
    let od_s = (od - od_near * 0.5).max(0.0);
    let mono = 1.0 - (-u.sigma_e * od_s).exp();
    let cos_key = dir.dot(u.key_dir);
    // Two separate scales, matching the source's `expr * ratio * mono`
    // grouping — folding them into one `ratio * mono` factor rounds
    // differently.
    let inscatter = u
        .key_irr
        .scale(fog_inscatter_phase(cos_key, u.g_fwd, u.g_back, u.back_weight, u.shaft_gain) * 0.55)
        .add(fog_ambient(cos_key, u.ambient, u.key_irr).scale(u.ambient_boost))
        .scale(u.sigma_s / u.sigma_e.max(1.0e-6))
        .scale(mono);

    color.mul(trans).add(inscatter)
}

/* ==================================================================== */
/* The Volumetrics class — volumetrics.js:392-527                        */
/* ==================================================================== */

/// `Volumetrics`' default march step count (`opts.steps ?? 40`) and
/// resolution scale (`opts.scale ?? 0.5`), and the resolve's steady-state
/// blend (`0.9`). `volumetrics.js:399,405,445,501`.
pub const DEFAULT_STEPS: u32 = 40;
/// See [`DEFAULT_STEPS`].
pub const DEFAULT_SCALE: f64 = 0.5;
/// See [`DEFAULT_STEPS`].
pub const RESOLVE_BLEND: f64 = 0.9;

/// The march target's dimensions: `max(1, round(w * scale))` per axis.
/// `Volumetrics.resize`, `volumetrics.js:469-470`.
///
/// JS `Math.round` breaks ties toward `+Infinity` and Rust's `f64::round`
/// breaks them away from zero; the two agree here because `w * scale` is
/// never negative. `uTexel`/`uTexelHalf` are `(1/mw, 1/mh)`
/// (`volumetrics.js:480-481`).
pub fn half_res_size(w: u32, h: u32, scale: f64) -> (u32, u32) {
    let round = |v: f64| (v.round() as i64).max(1) as u32;
    (round(f64::from(w) * scale), round(f64::from(h) * scale))
}

/// What one frame of `Volumetrics.render` resolves to, once the render-target
/// objects are stripped out: which history slot to read, which to write, and
/// what `uBlend` to use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameTargets {
    /// `this.rtHistory[ this._flip ^ 1 ]` — bound to `tHistory`.
    pub history_prev: usize,
    /// `this.rtHistory[ this._flip ]` — the resolve's render target, and the
    /// texture the composite then reads as `tVolume`.
    pub history_next: usize,
    /// `uBlend`: `0` on the first frame after a reset, `0.9` thereafter.
    pub blend: f64,
}

/// The history ping-pong and reset latch from `Volumetrics.render`
/// (`volumetrics.js:485-506`), with the GPU objects removed.
///
/// This is the part of the class that is arithmetic rather than plumbing, and
/// it is load-bearing: without the `_reset` latch the first frame after a
/// resize blends against an undefined history buffer, and without the flip
/// the resolve reads the target it is writing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TemporalState {
    flip: usize,
    reset: bool,
}

impl Default for TemporalState {
    fn default() -> Self {
        TemporalState::new()
    }
}

impl TemporalState {
    /// `this._flip = 0; this._reset = true;` — `volumetrics.js:407-408`.
    pub const fn new() -> Self {
        TemporalState { flip: 0, reset: true }
    }

    /// `reset()`, `volumetrics.js:515-517`; also what `resize` does when the
    /// march target is reallocated (`volumetrics.js:482`).
    pub fn reset(&mut self) {
        self.reset = true;
    }

    /// One frame: pick the slots and the blend, then clear the reset latch
    /// and flip. `volumetrics.js:496-504`.
    pub fn begin_frame(&mut self) -> FrameTargets {
        let out = FrameTargets {
            history_prev: self.flip ^ 1,
            history_next: self.flip,
            blend: if self.reset { 0.0 } else { RESOLVE_BLEND },
        };
        self.reset = false;
        self.flip ^= 1;
        out
    }
}
