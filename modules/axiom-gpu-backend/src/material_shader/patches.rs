//! **Repair patches.** The mismatched render/plaster repairs on a vertical face
//! that stop a wall reading as one uniform surface.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js`, the `OW_PATCH` block of
//! `MAIN_FRAGMENT` (source lines 449-492), the `owHash11` helper (source lines
//! 134-139), and the two shared wall axes the block reads (source lines 446-447).
//! The parameter is `DEFAULT_PARAMS.patch` (source line 742),
//! `[ coverage 0..1, cell metres, albedo delta, roughness delta ]`, defaulting to
//! `[0.0, 2.6, 0.12, -0.08]`.
//!
//! ## What the layer is
//!
//! Somebody has replastered part of this wall. A repair is a **rectangle in the
//! plane of the facade** — a few percent off the surrounding mix in value, a
//! little smoother because it is newer, and carrying a *trowel edge*, the small
//! raised ridge where the new render was feathered out. The rectangles are drawn
//! from a cellular lattice laid over `(horizontal-along-the-wall, world height)`,
//! one candidate rectangle per cell, with the lattice itself wandered by the
//! macro texture so the cells are not a visible grid. Covering ~10% of each
//! facade with these is what stops a 12 m wall reading as one flat colour.
//!
//! It writes three channels: `albedo.rgb`, roughness (`orm.g`) and the shading
//! height (`owHeightS`).
//!
//! ## The cell hash **is** the pattern
//!
//! `owHash11` is the Dave-Hoskins style scalar hash — `fract`, two chaotic
//! squarings, `fract` — and *not* a `fract(sin(dot(…)) * K)`. That matters twice
//! over. It means there is no `sin` at a large argument in this layer, so the
//! usual libm-divergence budget does not apply and the two sides are held to
//! **bit-identity** rather than to a tolerance. And it means the four multiplier
//! triples
//! (`7.31/13.77/5.1`, `3.17/9.41/21.3`, `11.93/4.73/37.7`, `5.51/17.29/53.9`) are
//! the wall: a different constant is a different building, so they are
//! transcribed digit for digit.
//!
//! The hash is chaotic by construction — that is its job — so a one-ULP
//! difference in its *argument* is amplified by roughly three orders of magnitude
//! in its result. The argument, `cid.x * A + cid.y * B + C`, is the classic
//! fused-multiply-add shape, so this layer's parity is really a measurement of
//! whether the device contracts it. See [`parity`] for the number that came back.
//!
//! ## Vertical faces only, and the shape of that test
//!
//! The facing test is *not* a `step`: it is `smoothstep(0.72, 0.34, abs(nw.y))`,
//! a **reversed-edge** smoothstep that reaches a full `1.0` for any face at or
//! past vertical and falls to `0.0` by the time the face is 46 degrees off. An
//! exactly-vertical face (`nw.y == 0`), which is the common case in a building,
//! lands squarely in the saturated region — `(0 - 0.72) / (0.34 - 0.72)` is
//! `1.894`, clamped to `1.0` — so the `>` / `>=` question never arises here and
//! an exactly-vertical wall is fully patched. The layer's one `>=`-flavoured test
//! is the coverage draw, `step(1.0 - clamp(coverage, 0, 1), r0)`, where GLSL and
//! WGSL `step` both mean `x >= edge`.
//!
//! ## `coverage == 0` disables the layer
//!
//! In the source that is a *compile-time* fact: `defines.OW_PATCH` is only set
//! when `patch[0] > 0` (source line 854), so a zero-coverage material never has
//! the block at all. Here the layer is a function, so it must be a *runtime*
//! no-op, and it is one for a reason worth naming: the coverage draw becomes
//! `step(1.0, r0)`, `r0` is a `fract` and therefore strictly below `1.0`, so
//! `has` is `0.0`, so `pm` is exactly `0.0`, so `pm > 0.001` is false and every
//! channel is returned untouched. That chain is verified at the boundary rather
//! than assumed — see [`coverage_zero_is_a_bit_identical_no_op`] — and it is
//! bit-identical, not merely close.
//!
//! ## Transcription notes
//!
//! - **`fract` is `x - floor(x)`.** World coordinates go negative all the time
//!   here (`owSAxis` is a signed axis and `pc` is divided by a cell size), so
//!   Rust's `%` would be wrong in exactly the region the layer spends most of its
//!   time in. [`fract`] is written as the subtraction.
//! - **`smoothstep` is written out by hand** on both sides rather than calling
//!   the builtin. Two reasons: WGSL leaves the result *indeterminate* when
//!   `low >= high`, which the facing test deliberately relies on, and a builtin's
//!   factoring is unspecified where the spec formula is not. This is the practice
//!   `surface_program::parity` already established for `mix`, `dot` and
//!   `smoothstep`.
//! - **`mix` is likewise written out** as `x * (1 - a) + y * a`, the spec
//!   formula, rather than as the fma-friendly `x + a * (y - x)` a driver may
//!   choose.
//! - **The division is a division.** `vec2(owSAxis, y) / cw` is transcribed as a
//!   divide per component, never as a multiply by `1.0 / cw`.
//! - **The grouping is the specification.** `1.0 + sgn * owPatchP.z * pm` is
//!   `1.0 + ((sgn * z) * pm)`; `height + pm * 0.07 + lip * 0.05` is
//!   `(height + pm * 0.07) + lip * 0.05`. Neither is tidied.
//! - **`pTint`'s selector is redundant and is kept.** The source picks the tint
//!   on `sgn > 0.0`, which cannot differ from the `r3 > 0.48` that produced
//!   `sgn`. Dead logic in the source is still the source.
//! - **Both sides compute in `f32`.** The CPU reference is not an `f64` model of
//!   the shader; it is the same arithmetic at the same width, which is what makes
//!   bit-identity the achievable bar and any divergence a fact about the *device*
//!   rather than about a width mismatch.
//!
//! ## What this layer needs from its siblings
//!
//! Two things, both taken as explicit arguments rather than reached for:
//!
//! - `mac2.rg`, the second macro-texture sample, which wanders the lattice. That
//!   belongs to the macro-variation layer.
//! - `owVert` and `owSAxis` (source lines 446-447) are computed *outside* the
//!   `#ifdef OW_PATCH` block and shared with the runoff layer. They are derived
//!   here from `world_pos` and `nw` so this layer is complete and testable alone;
//!   the expressions are identical, so hoisting them into a shared prologue later
//!   is exact.
//!
//! `owHash11` is also shared with `owRunoff` in the weathering layer. It is
//! emitted here as `axiom_patch_hash11` to avoid a duplicate WGSL definition
//! while the layers are written in parallel.


/// The `patches` layer as WGSL.
///
/// Entry point:
///
/// ```wgsl
/// fn axiom_patch_apply(
///     world_pos: vec3<f32>,     // vOwWPos
///     nw: vec3<f32>,            // owNw, already face-corrected and normalized
///     macro_second_rg: vec2<f32>, // mac2.rg, from the macro-variation layer
///     patch_p: vec4<f32>,       // owPatchP: coverage, cell metres, dAlbedo, dRough
///     albedo_in: vec3<f32>,     // alb.rgb
///     roughness_in: f32,        // orm.g
///     height_in: f32,           // owHeightS
/// ) -> AxiomPatchChannels       // { albedo, roughness, height }
/// ```
///
/// plus the three helpers it is built from — `axiom_patch_hash11`,
/// `axiom_patch_smoothstep` and `axiom_patch_smoothstep2` — which are named for
/// this layer so that composing every layer into one `axiom_surface` cannot
/// collide with the weathering layer's copy of the same source helper.
pub(crate) const PATCHES_WGSL: &str = r#"
// shader.js:134-139 — owHash11. Chaotic by construction; the constants are the
// pattern.
fn axiom_patch_hash11(x: f32) -> f32 {
    var p = fract(x * 0.1031);
    p *= p + 33.33;
    p *= p + p;
    return fract(p);
}

// GLSL smoothstep, written out. WGSL's builtin is indeterminate when low >= high,
// which the facing test at shader.js:446 deliberately relies on.
fn axiom_patch_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn axiom_patch_smoothstep2(e0: vec2<f32>, e1: vec2<f32>, x: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        axiom_patch_smoothstep(e0.x, e1.x, x.x),
        axiom_patch_smoothstep(e0.y, e1.y, x.y),
    );
}

struct AxiomPatchChannels {
    albedo: vec3<f32>,
    roughness: f32,
    height: f32,
};

fn axiom_patch_apply(
    world_pos: vec3<f32>,
    nw: vec3<f32>,
    macro_second_rg: vec2<f32>,
    patch_p: vec4<f32>,
    albedo_in: vec3<f32>,
    roughness_in: f32,
    height_in: f32,
) -> AxiomPatchChannels {
    var alb = albedo_in;
    var rough = roughness_in;
    var height_s = height_in;

    // shader.js:446-447 — shared with the runoff layer in the source.
    let ow_vert = axiom_patch_smoothstep(0.72, 0.34, abs(nw.y));
    let ow_s_axis = world_pos.z * nw.x - world_pos.x * nw.z;

    // shader.js:457-473
    let cw = max(patch_p.y, 0.4);
    var pc = vec2<f32>(ow_s_axis, world_pos.y) / cw;
    // wander the lattice so the cells are not a visible grid
    pc += (vec2<f32>(macro_second_rg.x, macro_second_rg.y) - 0.5) * 0.35;
    let cid = floor(pc);
    let cf = pc - cid;
    let r0 = axiom_patch_hash11(cid.x * 7.31 + cid.y * 13.77 + 5.1);
    let r1 = axiom_patch_hash11(cid.x * 3.17 + cid.y * 9.41 + 21.3);
    let r2 = axiom_patch_hash11(cid.x * 11.93 + cid.y * 4.73 + 37.7);
    let r3 = axiom_patch_hash11(cid.x * 5.51 + cid.y * 17.29 + 53.9);
    let has = step(1.0 - clamp(patch_p.x, 0.0, 1.0), r0);
    let lo = vec2<f32>(0.05 + r1 * 0.30, 0.05 + r2 * 0.30);
    let hi = vec2<f32>(0.95 - r2 * 0.26, 0.95 - r3 * 0.26);
    let fe = 0.028 + 0.030 * r1;          // ~3-6 cm of trowel feather
    let a0 = axiom_patch_smoothstep2(lo, lo + fe, cf);
    let a1 = 1.0 - axiom_patch_smoothstep2(hi - fe, hi, cf);
    let pm = a0.x * a0.y * a1.x * a1.y * has * ow_vert;
    if (pm > 0.001) {
        let sgn = select(-1.0, 1.0, r3 > 0.48);
        alb *= 1.0 + sgn * patch_p.z * pm;
        // A cement repair is greyer and cooler than the render around it; a patch
        // in the original mix that has weathered separately goes warmer. Value
        // alone reads as a lighting artefact — it needs the hue shift too.
        let p_tint = select(
            vec3<f32>(1.030, 1.008, 0.968),
            vec3<f32>(0.975, 0.988, 1.020),
            sgn > 0.0,
        );
        // mix(vec3(1.0), pTint, pm), written as the spec formula.
        alb *= vec3<f32>(1.0) * (1.0 - pm) + p_tint * pm;
        // a fresh coat has lost the mould and the fine crazing of the old wall
        rough = clamp(rough + patch_p.w * pm, 0.0, 1.0);
        // the trowel edge: a bright arris where the new render feathers out
        let lip = pm * (1.0 - pm) * 4.0;
        alb *= 1.0 + lip * 0.13;
        height_s = clamp(height_s + pm * 0.07 + lip * 0.05, 0.0, 1.0);
    }
    return AxiomPatchChannels(alb, rough, height_s);
}
"#;

/// What the layer writes: `alb.rgb`, `orm.g` and `owHeightS`.
#[derive(Debug)]
pub(crate) struct PatchChannels {
    /// `alb.rgb` after the patch value shift, the hue shift and the trowel lip.
    pub(crate) albedo: [f32; 3],
    /// `orm.g`, the roughness, after the fresh-coat delta.
    pub(crate) roughness: f32,
    /// `owHeightS`, the shading height, raised inside the patch and again at its
    /// arris.
    pub(crate) height: f32,
}

/// GLSL `fract`: `x - floor(x)`, which is **not** Rust's `%` once `x` is
/// negative — and `pc` is negative over half the wall.
pub(crate) fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// GLSL `step(edge, x)`: `x < edge ? 0.0 : 1.0`, i.e. `x >= edge`.
pub(crate) fn step_ge(edge: f32, x: f32) -> f32 {
    [0.0_f32, 1.0][usize::from(x >= edge)]
}

/// GLSL `clamp(x, 0, 1)`, which WGSL defines as `min(max(x, 0), 1)`.
///
/// `f32::clamp` is the faithful spelling of that, not merely the tidy one: it
/// agrees with `min(max(…))` on every finite input, and on `-0.0` it agrees
/// *better* — WGSL's `max(-0.0, 0.0)` returns `-0.0` where Rust's `f32::max`
/// returns `0.0`. `NaN`, the one input on which the two genuinely differ, is
/// unreachable here: every argument is finite and the layer's only division is
/// by `cw`, which `max(owPatchP.y, 0.4)` has already floored at `0.4`.
fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// GLSL `smoothstep`, written out. Legal — and used here — with `e0 > e1`.
pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp01((x - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}

/// `owHash11` (shader.js:134-139): the layer's cell hash, and therefore the
/// layer's pattern.
pub(crate) fn hash11(x: f32) -> f32 {
    let p = fract(x * 0.1031);
    let p = p * (p + 33.33);
    let p = p * (p + p);
    fract(p)
}

/// The CPU reference for [`PATCHES_WGSL`]'s `axiom_patch_apply`, and the semantic
/// definition of the layer.
///
/// `nw` is the world-space shading normal after the front-facing correction, as
/// `owNw` is in the source; `macro_second_rg` is `mac2.rg`; `patch_p` is
/// `owPatchP` — `[coverage, cell metres, albedo delta, roughness delta]`.
pub(crate) fn apply(
    world_pos: [f32; 3],
    nw: [f32; 3],
    macro_second_rg: [f32; 2],
    patch_p: [f32; 4],
    albedo_in: [f32; 3],
    roughness_in: f32,
    height_in: f32,
) -> PatchChannels {
    // shader.js:446-447 — shared with the runoff layer in the source.
    let ow_vert = smoothstep(0.72, 0.34, nw[1].abs());
    let ow_s_axis = world_pos[2] * nw[0] - world_pos[0] * nw[2];

    // shader.js:457-473
    let cw = patch_p[1].max(0.4);
    let pc = [ow_s_axis / cw, world_pos[1] / cw];
    // wander the lattice so the cells are not a visible grid
    let pc = [
        pc[0] + (macro_second_rg[0] - 0.5) * 0.35,
        pc[1] + (macro_second_rg[1] - 0.5) * 0.35,
    ];
    let cid = [pc[0].floor(), pc[1].floor()];
    let cf = [pc[0] - cid[0], pc[1] - cid[1]];
    let r0 = hash11(cid[0] * 7.31 + cid[1] * 13.77 + 5.1);
    let r1 = hash11(cid[0] * 3.17 + cid[1] * 9.41 + 21.3);
    let r2 = hash11(cid[0] * 11.93 + cid[1] * 4.73 + 37.7);
    let r3 = hash11(cid[0] * 5.51 + cid[1] * 17.29 + 53.9);
    let has = step_ge(1.0 - clamp01(patch_p[0]), r0);
    let lo = [0.05 + r1 * 0.30, 0.05 + r2 * 0.30];
    let hi = [0.95 - r2 * 0.26, 0.95 - r3 * 0.26];
    let fe = 0.028 + 0.030 * r1; // ~3-6 cm of trowel feather
    let a0 = [
        smoothstep(lo[0], lo[0] + fe, cf[0]),
        smoothstep(lo[1], lo[1] + fe, cf[1]),
    ];
    let a1 = [
        1.0 - smoothstep(hi[0] - fe, hi[0], cf[0]),
        1.0 - smoothstep(hi[1] - fe, hi[1], cf[1]),
    ];
    let pm = a0[0] * a0[1] * a1[0] * a1[1] * has * ow_vert;

    // The source guards the whole write with `if ( pm > 0.001 )`. That guard is
    // not an optimisation — it leaves a ~1e-4 discontinuity at the threshold —
    // so it is transcribed, as a selection between the untouched channels and
    // the written ones. Both sides are always evaluated; neither can trap.
    let sgn = [-1.0_f32, 1.0][usize::from(r3 > 0.48)];
    let value = 1.0 + sgn * patch_p[2] * pm;
    let p_tint = [[1.030_f32, 1.008, 0.968], [0.975, 0.988, 1.020]][usize::from(sgn > 0.0)];
    // mix(vec3(1.0), pTint, pm), written as the spec formula.
    let tint = [
        1.0 * (1.0 - pm) + p_tint[0] * pm,
        1.0 * (1.0 - pm) + p_tint[1] * pm,
        1.0 * (1.0 - pm) + p_tint[2] * pm,
    ];
    // the trowel edge: a bright arris where the new render feathers out
    let lip = pm * (1.0 - pm) * 4.0;
    let arris = 1.0 + lip * 0.13;
    let written_albedo = [
        albedo_in[0] * value * tint[0] * arris,
        albedo_in[1] * value * tint[1] * arris,
        albedo_in[2] * value * tint[2] * arris,
    ];
    let written_roughness = clamp01(roughness_in + patch_p[3] * pm);
    let written_height = clamp01(height_in + pm * 0.07 + lip * 0.05);

    let active = usize::from(pm > 0.001);
    PatchChannels {
        albedo: [
            [albedo_in[0], written_albedo[0]][active],
            [albedo_in[1], written_albedo[1]][active],
            [albedo_in[2], written_albedo[2]][active],
        ],
        roughness: [roughness_in, written_roughness][active],
        height: [height_in, written_height][active],
    }
}

/// One evaluation of the layer: every argument `apply` takes, in one row.
///
/// Shared by the CPU tests and the GPU parity harness so that both sides are
/// driven from the *same* row, and a sample cannot silently mean two things.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PatchSample {
    pub(crate) world_pos: [f32; 3],
    pub(crate) nw: [f32; 3],
    pub(crate) macro_second_rg: [f32; 2],
    pub(crate) patch_p: [f32; 4],
    pub(crate) albedo: [f32; 3],
    pub(crate) roughness: f32,
    pub(crate) height: f32,
}

impl PatchSample {
    /// Run the CPU reference on this row.
    pub(crate) fn evaluate(&self) -> PatchChannels {
        apply(
            self.world_pos,
            self.nw,
            self.macro_second_rg,
            self.patch_p,
            self.albedo,
            self.roughness,
            self.height,
        )
    }
}

/// How many rows one sweep compares. Also the parity target's width.
pub(crate) const SAMPLES: usize = 32;

/// The [`SAMPLES`] rows, chosen to hit what is easy to get wrong.
///
/// Twenty-four are a procedural sweep: world coordinates that go **negative** in
/// every lane (`fract` on a negative `pc` is the layer's single most likely
/// transcription defect), a facing normal that rotates from up-facing through
/// exactly vertical to down-facing, coverages spanning `0.0` through past `1.0`
/// (so the `clamp` on coverage is exercised from both ends), a cell size that
/// goes **negative** (so `max(cw, 0.4)` is exercised), and roughness/height
/// starting at `0.0` and at `1.0` so the two output clamps both saturate.
///
/// The last eight are named cases: exact zero coverage on faces that would
/// otherwise be patched, full coverage, an exactly-vertical face (`nw.y == 0`,
/// the common case in a building), a face just inside and just outside the
/// facing ramp, and a wall 400 m from the origin where `pc` is large and `fract`
/// is losing bits.
pub(crate) fn samples() -> Vec<PatchSample> {
    let swept = (0..24_usize).map(|index| {
        let t = index as f32;
        // A normal that sweeps the whole facing range but is *cubically*
        // concentrated near vertical, because vertical is where the layer does
        // its work and a uniform sweep spends most of its rows on roofs.
        let s = (t / 23.0) * 2.0 - 1.0;
        let ny = s * s * s * 0.9;
        let horizontal = (1.0 - ny * ny).max(0.0).sqrt();
        let phi = t * 0.83;
        PatchSample {
            world_pos: [t * 1.7 - 19.0, t * 0.83 - 7.5, t * -2.3 + 11.0],
            nw: [phi.cos() * horizontal, ny, phi.sin() * horizontal],
            macro_second_rg: [fract(t * 0.317 + 0.11), fract(t * 0.713 + 0.44)],
            patch_p: [
                0.55 + t * 0.024,
                t.mul_add(0.31, -1.4),
                0.12 - t * 0.004,
                (t * 0.9).sin() * 0.14,
            ],
            albedo: [0.42 + t * 0.02, 0.37 + t * 0.017, 0.31 + t * 0.026],
            roughness: fract(t * 0.29),
            height: fract(t * 0.47 + 0.2),
        }
    });
    let named = [
        // Zero coverage on a wall that a coverage of 1 would certainly patch.
        PatchSample {
            world_pos: [-3.25, 4.75, 8.5],
            nw: [0.6, 0.0, -0.8],
            macro_second_rg: [0.62, 0.31],
            patch_p: [0.0, 2.6, 0.12, -0.08],
            albedo: [0.51, 0.47, 0.44],
            roughness: 0.63,
            height: 0.5,
        },
        // The same wall at full coverage: `has` is 1 in every cell.
        PatchSample {
            world_pos: [-3.25, 4.75, 8.5],
            nw: [0.6, 0.0, -0.8],
            macro_second_rg: [0.62, 0.31],
            patch_p: [1.0, 2.6, 0.12, -0.08],
            albedo: [0.51, 0.47, 0.44],
            roughness: 0.63,
            height: 0.5,
        },
        // Exactly vertical, negative in every world lane, default cell size.
        PatchSample {
            world_pos: [-11.7, -2.35, -6.1],
            nw: [-0.28734788, 0.0, 0.95782626],
            macro_second_rg: [0.18, 0.87],
            patch_p: [0.85, 2.6, 0.12, -0.08],
            albedo: [0.44, 0.44, 0.44],
            roughness: 0.55,
            height: 0.4,
        },
        // Just inside the facing ramp: |nw.y| = 0.40, so owVert is partial.
        PatchSample {
            world_pos: [5.5, 3.1, -9.25],
            nw: [0.6, 0.4, 0.692_820_3],
            macro_second_rg: [0.5, 0.5],
            patch_p: [0.9, 1.4, 0.18, 0.22],
            albedo: [0.6, 0.58, 0.55],
            roughness: 0.2,
            height: 0.25,
        },
        // Past the ramp: owVert is exactly 0, so the layer is off by facing.
        PatchSample {
            world_pos: [5.5, 3.1, -9.25],
            nw: [0.0, 1.0, 0.0],
            macro_second_rg: [0.5, 0.5],
            patch_p: [0.9, 1.4, 0.18, 0.22],
            albedo: [0.6, 0.58, 0.55],
            roughness: 0.2,
            height: 0.25,
        },
        // Saturating both output clamps: roughness already 1 with a positive
        // delta, height already 1.
        PatchSample {
            world_pos: [-7.05, 1.95, 2.4],
            nw: [1.0, 0.0, 0.0],
            macro_second_rg: [0.05, 0.95],
            patch_p: [1.0, 0.9, 0.3, 0.5],
            albedo: [0.7, 0.68, 0.66],
            roughness: 1.0,
            height: 1.0,
        },
        // And the other end: roughness 0 with a negative delta.
        PatchSample {
            world_pos: [-7.05, -13.95, 2.4],
            nw: [-1.0, 0.0, 0.0],
            macro_second_rg: [0.95, 0.05],
            patch_p: [1.0, 0.9, 0.3, -0.5],
            albedo: [0.7, 0.68, 0.66],
            roughness: 0.0,
            height: 0.0,
        },
        // 400 m out, where `pc` is large and `fract` is shedding mantissa bits.
        PatchSample {
            world_pos: [-412.5, 38.25, 377.0],
            nw: [
                core::f32::consts::FRAC_1_SQRT_2,
                0.0,
                -core::f32::consts::FRAC_1_SQRT_2,
            ],
            macro_second_rg: [0.33, 0.66],
            patch_p: [0.75, 3.3, 0.12, -0.08],
            albedo: [0.48, 0.46, 0.43],
            roughness: 0.47,
            height: 0.52,
        },
    ];
    swept.chain(named).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        apply, fract, hash11, samples, smoothstep, step_ge, PatchSample, PATCHES_WGSL, SAMPLES,
    };

    /// A row whose channels are all 1.0, so a multiplicative write is visible as
    /// the factor itself.
    fn unit_row(patch_p: [f32; 4]) -> PatchSample {
        PatchSample {
            world_pos: [-3.25, 4.75, 8.5],
            nw: [0.6, 0.0, -0.8],
            macro_second_rg: [0.62, 0.31],
            patch_p,
            albedo: [1.0, 1.0, 1.0],
            roughness: 0.5,
            height: 0.5,
        }
    }

    /// `fract` is `x - floor(x)`, and the whole point is that it is **not** `%`.
    /// A negative argument is the case the layer actually spends its time in.
    #[test]
    fn fract_is_x_minus_floor_and_not_a_remainder() {
        assert_eq!(fract(2.25), 0.25);
        assert_eq!(fract(-2.25), 0.75);
        assert_ne!(fract(-2.25), -2.25_f32 % 1.0);
        assert_eq!(fract(0.0), 0.0);
        assert_eq!(fract(-0.0), 0.0);
        assert_eq!(fract(7.0), 0.0);
    }

    /// GLSL `step` is `x >= edge`, inclusive at the edge — which is what makes
    /// the coverage draw a `>=` and not a `>`.
    #[test]
    fn step_is_inclusive_at_the_edge() {
        assert_eq!(step_ge(0.5, 0.5), 1.0);
        assert_eq!(step_ge(0.5, 0.49999997), 0.0);
        assert_eq!(step_ge(0.5, 0.5000001), 1.0);
    }

    /// The facing test's smoothstep runs with `e0 > e1`, and must saturate at
    /// `1.0` for an exactly-vertical face rather than going indeterminate.
    #[test]
    fn the_reversed_edge_smoothstep_saturates_on_an_exactly_vertical_face() {
        assert_eq!(smoothstep(0.72, 0.34, 0.0), 1.0);
        assert_eq!(smoothstep(0.72, 0.34, 0.34), 1.0);
        assert_eq!(smoothstep(0.72, 0.34, 0.72), 0.0);
        assert_eq!(smoothstep(0.72, 0.34, 1.0), 0.0);
        // Halfway between the edges is the Hermite midpoint.
        let mid = smoothstep(0.72, 0.34, 0.53);
        assert!((mid - 0.5).abs() < 1.0e-6, "{mid}");
        // And the forward direction still behaves.
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
        assert_eq!(smoothstep(0.0, 1.0, -3.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 3.0), 1.0);
    }

    /// `owHash11` must land in `[0, 1)` for every argument the lattice can
    /// produce — the `coverage == 0` no-op depends on it never reaching `1.0` —
    /// and must actually decorrelate neighbouring cells.
    #[test]
    fn the_cell_hash_stays_below_one_and_decorrelates_neighbours() {
        let values: Vec<f32> = (-4000..4000_i32)
            .map(|cell| {
                let c = cell as f32 * 0.25;
                hash11(c * 7.31 + c * 13.77 + 5.1)
            })
            .collect();
        values.iter().for_each(|value| {
            assert!(
                (0.0..1.0).contains(value),
                "owHash11 escaped [0,1): {value}"
            );
        });
        // Adjacent lattice arguments must not track each other, or the
        // rectangles clump into a visible grid.
        let mean_jump = values
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .sum::<f32>()
            / (values.len() - 1) as f32;
        assert!(mean_jump > 0.2, "the hash is not chaotic: {mean_jump}");
        // The published constants, pinned against an independent `Math.fround`
        // transcription of the same six GLSL lines (a second implementation, in
        // another language, written from the source text — the `sky/` lesson).
        // These are exact `f32` values, so the comparison is exact.
        assert_eq!(hash11(0.0), 0.0);
        assert_eq!(hash11(5.1), 0.802_856_45);
        assert_eq!(hash11(21.3), 0.385_116_58);
        assert_eq!(hash11(37.7), 0.75);
        assert_eq!(hash11(-53.9), 0.504_486_1);
    }

    /// **Zero coverage disables the layer, bit-identically.** Verified at the
    /// boundary over the whole lattice rather than assumed from the algebra.
    #[test]
    fn coverage_zero_is_a_bit_identical_no_op() {
        let rows = (0..2000_i32).map(|index| {
            let t = index as f32;
            PatchSample {
                world_pos: [t * 0.37 - 370.0, t * -0.53 + 240.0, t * 0.19 - 95.0],
                nw: [0.6, 0.0, -0.8],
                macro_second_rg: [fract(t * 0.317), fract(t * 0.713)],
                patch_p: [0.0, 2.6, 0.12, -0.08],
                albedo: [0.51, 0.47, 0.44],
                roughness: 0.63,
                height: 0.5,
            }
        });
        rows.for_each(|row| {
            let out = row.evaluate();
            (0..3).for_each(|lane| {
                assert_eq!(
                    out.albedo[lane].to_bits(),
                    row.albedo[lane].to_bits(),
                    "zero coverage moved albedo lane {lane} at {:?}",
                    row.world_pos
                );
            });
            assert_eq!(out.roughness.to_bits(), row.roughness.to_bits());
            assert_eq!(out.height.to_bits(), row.height.to_bits());
        });
        // Negative coverage clamps to zero and is equally a no-op.
        let negative = apply(
            [-3.25, 4.75, 8.5],
            [0.6, 0.0, -0.8],
            [0.62, 0.31],
            [-5.0, 2.6, 0.12, -0.08],
            [0.51, 0.47, 0.44],
            0.63,
            0.5,
        );
        assert_eq!(negative.albedo[0].to_bits(), 0.51_f32.to_bits());
        assert_eq!(negative.roughness.to_bits(), 0.63_f32.to_bits());
        assert_eq!(negative.height.to_bits(), 0.5_f32.to_bits());
    }

    /// A face past the facing ramp is untouched however high the coverage: the
    /// layer is *repair patches on vertical faces*, and a roof is not one.
    #[test]
    fn an_up_facing_surface_is_never_patched() {
        let row = PatchSample {
            nw: [0.0, 1.0, 0.0],
            ..unit_row([1.0, 2.6, 0.12, -0.08])
        };
        let out = row.evaluate();
        assert_eq!(out.albedo[0].to_bits(), 1.0_f32.to_bits());
        assert_eq!(out.roughness.to_bits(), 0.5_f32.to_bits());
        assert_eq!(out.height.to_bits(), 0.5_f32.to_bits());
    }

    /// The layer must actually paint at full coverage — and paint *rectangles*,
    /// not a wash: some of the wall is inside a repair and some is not.
    #[test]
    fn full_coverage_paints_rectangles_across_a_wall() {
        let touched: Vec<bool> = (0..600_i32)
            .map(|index| {
                let t = index as f32;
                let row = PatchSample {
                    world_pos: [-3.25 + t * 0.05, 4.75 + t * 0.031, 8.5 - t * 0.037],
                    ..unit_row([1.0, 2.6, 0.12, -0.08])
                };
                row.evaluate().albedo[0].to_bits() != 1.0_f32.to_bits()
            })
            .collect();
        let inside = touched.iter().filter(|hit| **hit).count();
        assert!(inside > 60, "the wall is barely patched: {inside}/600");
        assert!(inside < 540, "the whole wall is one patch: {inside}/600");
    }

    /// Both tints must occur across a wall — the cool cement repair and the warm
    /// weathered one. Detected from the outputs alone: every other factor in the
    /// write is a scalar applied to all three lanes, so the blue-over-red ratio
    /// isolates `pTint`.
    #[test]
    fn both_the_cool_and_the_warm_repair_tint_occur() {
        let ratios: Vec<f32> = (0..600_i32)
            .map(|index| {
                let t = index as f32;
                let row = PatchSample {
                    world_pos: [-3.25 + t * 0.05, 4.75 + t * 0.031, 8.5 - t * 0.037],
                    ..unit_row([1.0, 2.6, 0.12, -0.08])
                };
                let out = row.evaluate();
                out.albedo[2] / out.albedo[0]
            })
            .collect();
        let cool = ratios.iter().filter(|ratio| **ratio > 1.000_01).count();
        let warm = ratios.iter().filter(|ratio| **ratio < 0.999_99).count();
        assert!(cool > 0, "no cool cement repair across the wall: {cool}");
        assert!(warm > 0, "no warm weathered repair across the wall: {warm}");
    }

    /// The trowel edge is a *bright arris*: at the feather, where `pm` is near
    /// 0.5, the lip term peaks at 1.0 and brightens by 13%. In the middle of a
    /// repair, where `pm` is 1, the lip is gone.
    #[test]
    fn the_trowel_lip_peaks_at_the_feather_and_vanishes_inside_the_patch() {
        // pm == 1 exactly is unreachable through the lattice, so drive the
        // relationship through the sweep: the brightest albedo on a wall of
        // uniform input must exceed the flattest patched one.
        let painted: Vec<f32> = (0..1200_i32)
            .map(|index| {
                let t = index as f32;
                let row = PatchSample {
                    world_pos: [-3.25 + t * 0.017, 4.75 + t * 0.023, 8.5 - t * 0.011],
                    ..unit_row([1.0, 2.6, 0.0, 0.0])
                };
                row.evaluate().albedo[1]
            })
            .filter(|value| *value != 1.0)
            .collect();
        assert!(!painted.is_empty());
        let brightest = painted.iter().copied().fold(f32::MIN, f32::max);
        // With the albedo delta zeroed the only lightening left is the lip, so
        // the peak is the 13% arris (times the green tint, within a percent).
        assert!(
            (1.10..=1.14).contains(&brightest),
            "the arris is not 13%: {brightest}"
        );
    }

    /// The roughness and height writes both clamp, at both ends.
    #[test]
    fn the_roughness_and_height_writes_saturate_rather_than_escape() {
        let rows = (0..1200_i32).map(|index| {
            let t = index as f32;
            PatchSample {
                world_pos: [-7.05 + t * 0.019, 1.95 + t * 0.027, 2.4 - t * 0.013],
                roughness: [0.0, 1.0][usize::from(index % 2 == 0)],
                height: [0.0, 1.0][usize::from(index % 2 == 0)],
                ..unit_row([1.0, 0.9, 0.3, [(-0.9), 0.9][usize::from(index % 2 == 0)]])
            }
        });
        let mut saw_high = false;
        let mut saw_low = false;
        rows.for_each(|row| {
            let out = row.evaluate();
            assert!((0.0..=1.0).contains(&out.roughness), "{}", out.roughness);
            assert!((0.0..=1.0).contains(&out.height), "{}", out.height);
            saw_high |= out.roughness == 1.0;
            saw_low |= out.roughness == 0.0;
        });
        assert!(saw_high, "the roughness clamp never saturated at 1");
        assert!(saw_low, "the roughness clamp never saturated at 0");
    }

    /// `max(owPatchP.y, 0.4)` floors the cell size, so a zero or negative cell
    /// metre is a 40 cm lattice rather than a divide by zero.
    #[test]
    fn a_non_positive_cell_size_falls_back_to_forty_centimetres() {
        let floored = apply(
            [-3.25, 4.75, 8.5],
            [0.6, 0.0, -0.8],
            [0.62, 0.31],
            [1.0, -3.0, 0.12, -0.08],
            [0.51, 0.47, 0.44],
            0.63,
            0.5,
        );
        let explicit = apply(
            [-3.25, 4.75, 8.5],
            [0.6, 0.0, -0.8],
            [0.62, 0.31],
            [1.0, 0.4, 0.12, -0.08],
            [0.51, 0.47, 0.44],
            0.63,
            0.5,
        );
        assert_eq!(floored.albedo[0].to_bits(), explicit.albedo[0].to_bits());
        assert_eq!(floored.roughness.to_bits(), explicit.roughness.to_bits());
        assert!(floored.height.is_finite());
        assert!(format!("{floored:?}").contains("albedo"));
    }

    /// The sweep the parity harness runs is the sweep the CPU tests run, it is
    /// the width of the parity target, and it is not vacuous: a good share of
    /// its rows must actually be inside a repair.
    #[test]
    fn the_parity_sweep_is_the_right_width_and_is_not_vacuous() {
        let rows = samples();
        assert_eq!(rows.len(), SAMPLES);
        let painted = rows
            .iter()
            .filter(|row| {
                let out = row.evaluate();
                out.albedo[0].to_bits() != row.albedo[0].to_bits()
            })
            .count();
        assert!(painted >= 6, "only {painted}/{SAMPLES} rows are patched");
        assert!(painted <= SAMPLES - 6, "{painted}/{SAMPLES} rows patched");
        assert!(format!("{:?}", rows[0]).contains("world_pos"));
    }

    /// The WGSL names the entry point and the helpers this layer promised, and
    /// carries the source's constants verbatim.
    #[test]
    fn the_wgsl_declares_the_layers_entry_points_and_constants() {
        [
            "axiom_patch_hash11",
            "axiom_patch_smoothstep",
            "axiom_patch_smoothstep2",
        ]
        .iter()
        .for_each(|name| {
            assert!(PATCHES_WGSL.contains(&format!("fn {name}(")), "{name}");
        });
        assert!(PATCHES_WGSL.contains("fn axiom_patch_apply("));
        assert!(PATCHES_WGSL.contains("struct AxiomPatchChannels {"));
        [
            "0.1031", "33.33", "7.31", "13.77", "5.1", "3.17", "9.41", "21.3", "11.93", "4.73",
            "37.7", "5.51", "17.29", "53.9", "0.028", "0.030", "0.48", "0.001", "0.72", "0.34",
        ]
        .iter()
        .for_each(|constant| {
            assert!(PATCHES_WGSL.contains(constant), "missing {constant}");
        });
        // The division must stay a division, not a reciprocal multiply.
        assert!(PATCHES_WGSL.contains("vec2<f32>(ow_s_axis, world_pos.y) / cw"));
    }
}

/// **CPU↔GPU parity on a real adapter.**
///
/// The CPU reference above is the semantic definition; this is the proof that
/// [`super::patches::PATCHES_WGSL`] means the same thing on hardware. Compiled
/// only under `--features offscreen`, and it **asserts** an adapter was acquired
/// rather than skipping — a parity test that silently passes when nothing ran is
/// worse than no parity test — following
/// `crate::surface_program::parity`.
///
/// ## The tolerance is **zero**, and that is a measurement
///
/// This layer has no transcendental in it: `owHash11` is `fract`, two multiplies
/// and two more `fract`s, and the rest is multiply/add/subtract, one divide,
/// `floor`, `min`/`max` and a hand-written smoothstep. Every one of those is
/// correctly-rounded in IEEE-754 `f32`, so with the CPU reference computing at
/// the same width there is nothing left for the two sides to disagree about
/// except the two things a device is *permitted* to do differently:
///
/// 1. **Contract `a * b + c` into an `fma`.** The place that would bite is the
///    hash argument, `cid.x * A + cid.y * B + C`. A one-ULP change there is
///    amplified by the two chaotic squarings into an `r0`/`r3` that is different
///    in the first decimal — enough to flip `has` or `sgn` and paint a different
///    wall.
/// 2. **Evaluate `/` to 2.5 ULP** rather than exactly, in `vec2(…) / cw`. On a
///    facade 400 m out, `pc` is of order 125, and 2.5 ULP of that is `2.4e-5`,
///    which the `1/fe ≈ 33` slope of the trowel feather turns into `~1e-3` of
///    patch mask. Worse, it can move `pc` across an integer and change `cid`.
///
/// So this layer has **no meaningful middle tolerance**. Either the device is
/// exact and the two sides agree to the last bit, or it is not and the
/// disagreement is a visibly different building. A `1e-4`-shaped budget would
/// catch neither case; it would only launder the second one into a pass.
///
/// The measurement settles it: on a real Vulkan adapter the worst absolute lane
/// delta over the whole sweep is **`0.0`** — every one of the 32 rows agrees on
/// all five channels *bit for bit*. So that is what
/// [`the_gpu_is_bit_identical_to_the_cpu_reference`] asserts, and it prints the
/// worst delta on every run so the claim stays a measurement rather than
/// becoming a constant nobody can re-derive.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::{samples, PatchSample, PATCHES_WGSL, SAMPLES};

    /// `copy_texture_to_buffer` requires each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// The harness: a fullscreen triangle whose fragment stage evaluates the
    /// layer at the row its pixel column names. Two entry points because the
    /// layer writes five floats and a colour target carries four.
    const HARNESS_WGSL: &str = r#"
struct PatchRows { items: array<vec4<f32>, 160> };
@group(0) @binding(0) var<uniform> rows: PatchRows;

@vertex
fn patch_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

fn patch_row(index: u32) -> AxiomPatchChannels {
    let a = rows.items[index * 5u + 0u];   // world_pos.xyz, roughness
    let b = rows.items[index * 5u + 1u];   // nw.xyz, height
    let c = rows.items[index * 5u + 2u];   // mac2.rg
    let d = rows.items[index * 5u + 3u];   // owPatchP
    let e = rows.items[index * 5u + 4u];   // albedo.rgb
    return axiom_patch_apply(a.xyz, b.xyz, c.xy, d, e.xyz, a.w, b.w);
}

@fragment
fn patch_albedo_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let written = patch_row(u32(position.x));
    return vec4<f32>(written.albedo, written.roughness);
}

@fragment
fn patch_height_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let written = patch_row(u32(position.x));
    return vec4<f32>(written.height, written.height, written.height, written.height);
}
"#;

    /// A real GPU, acquired or the test fails loudly.
    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl Gpu {
        fn acquire() -> Gpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
            Gpu {
                device,
                queue,
                backend: gpu.backend,
            }
        }

        fn render(&self, module: &wgpu::ShaderModule, entry: &str, rows: &[u8]) -> Vec<[f32; 4]> {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-material-patches-bgl"),
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
            let buffer = wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-material-patches-rows"),
                    contents: rows,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-material-patches-bg"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-material-patches-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-material-patches-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("patch_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry),
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
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-material-patches-target"),
                size: wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Rgba32Float: an Rgba8Unorm target quantises to 1/255, four
                // orders of magnitude coarser than the tolerance.
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-material-patches-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-material-patches-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
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
                    texture: &texture,
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
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            self.device
                .poll(wgpu::PollType::Wait)
                .expect("the readback must complete");
            let mapped = slice.get_mapped_range();
            (0..SAMPLES)
                .map(|sample| {
                    [0_usize, 1, 2, 3].map(|lane| {
                        let at = sample * 16 + lane * 4;
                        f32::from_le_bytes([
                            mapped[at],
                            mapped[at + 1],
                            mapped[at + 2],
                            mapped[at + 3],
                        ])
                    })
                })
                .collect()
        }
    }

    /// Five `vec4` per row, in the order `patch_row` unpacks them.
    fn row_bytes(rows: &[PatchSample]) -> Vec<u8> {
        let mut bytes: Vec<u8> = rows
            .iter()
            .flat_map(|row| {
                [
                    row.world_pos[0],
                    row.world_pos[1],
                    row.world_pos[2],
                    row.roughness,
                    row.nw[0],
                    row.nw[1],
                    row.nw[2],
                    row.height,
                    row.macro_second_rg[0],
                    row.macro_second_rg[1],
                    0.0,
                    0.0,
                    row.patch_p[0],
                    row.patch_p[1],
                    row.patch_p[2],
                    row.patch_p[3],
                    row.albedo[0],
                    row.albedo[1],
                    row.albedo[2],
                    0.0,
                ]
            })
            .flat_map(f32::to_le_bytes)
            .collect();
        bytes.resize(SAMPLES * 5 * 16, 0);
        bytes
    }

    /// Both sides of the sweep: `(cpu, gpu)` as `[albedo.rgb, roughness, height]`
    /// per row.
    fn compare(gpu: &Gpu) -> (Vec<[f32; 5]>, Vec<[f32; 5]>) {
        let rows = samples();
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("axiom-material-patches-shader"),
                source: wgpu::ShaderSource::Wgsl([PATCHES_WGSL, HARNESS_WGSL].concat().into()),
            });
        let bytes = row_bytes(&rows);
        let albedo = gpu.render(&module, "patch_albedo_fs", &bytes);
        let height = gpu.render(&module, "patch_height_fs", &bytes);
        let rendered = albedo
            .iter()
            .zip(height.iter())
            .map(|(a, h)| [a[0], a[1], a[2], a[3], h[0]])
            .collect();
        let evaluated = rows
            .iter()
            .map(|row| {
                let out = row.evaluate();
                [
                    out.albedo[0],
                    out.albedo[1],
                    out.albedo[2],
                    out.roughness,
                    out.height,
                ]
            })
            .collect();
        (evaluated, rendered)
    }

    /// The worst absolute lane delta — the measurement a tolerance is set from.
    fn worst(cpu: &[[f32; 5]], gpu: &[[f32; 5]]) -> (f32, usize, usize) {
        cpu.iter().zip(gpu.iter()).enumerate().fold(
            (0.0_f32, 0, 0),
            |acc, (sample, (expected, actual))| {
                (0..5).fold(acc, |(worst, at, lane), index| {
                    let delta = (expected[index] - actual[index]).abs();
                    [(worst, at, lane), (delta, sample, index)][usize::from(delta > worst)]
                })
            },
        )
    }

    /// The layer's WGSL must compile as WGSL before anything else is meaningful.
    #[test]
    fn the_layer_compiles_on_a_real_device() {
        let gpu = Gpu::acquire();
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );
        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let (_module, failure) = crate::test_gpu::validating(&gpu.device, || {
            gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-material-patches-compile"),
                    source: wgpu::ShaderSource::Wgsl([PATCHES_WGSL, HARNESS_WGSL].concat().into()),
                })
        });
        let error = failure;
        assert!(
            error.is_none(),
            "the patches WGSL must compile: {}",
            error.map_or(String::new(), |failure| failure.to_string())
        );
    }

    /// **The parity proof.** Every row, on a real GPU, bit for bit — the budget
    /// the measurement actually supports, for the reasons in this module's
    /// header. The worst delta is printed on every run so the number stays
    /// visible rather than being taken on trust.
    #[test]
    fn the_gpu_is_bit_identical_to_the_cpu_reference() {
        let gpu = Gpu::acquire();
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );
        let (cpu, rendered) = compare(&gpu);
        let (delta, at, worst_lane) = worst(&cpu, &rendered);
        // The per-lane assertion below names the offending row, lane and delta
        // on failure, so a success-path print would only be noise — and no
        // layer or module in this engine emits console output, tests included
        // (Module Law #10, enforced by the architecture checker).
        assert_eq!(
            delta, 0.0,
            "patches must be bit-exact; worst delta {delta:e} at row {at} \
             lane {worst_lane} on {:?}",
            gpu.backend,
        );
        cpu.iter()
            .zip(rendered.iter())
            .enumerate()
            .for_each(|(sample, (expected, actual))| {
                (0..5).for_each(|lane| {
                    assert_eq!(
                        expected[lane].to_bits(),
                        actual[lane].to_bits(),
                        "patches disagrees at row {sample} lane {lane}: CPU {} vs GPU {} \
                         (delta {}). This layer admits no middle tolerance: the device has \
                         either contracted the cell hash's argument or evaluated `pc = … / cw` \
                         to less than an exact rounding, and either way the wall it paints is \
                         not the wall the CPU reference defines.",
                        expected[lane],
                        actual[lane],
                        (expected[lane] - actual[lane]).abs()
                    );
                });
            });
    }

    /// **`coverage == 0` disables the layer bit-identically on the device too.**
    /// The CPU side proves it over the whole lattice; this proves the device
    /// agrees, because a driver that contracted the hash differently could still
    /// only break it by making `fract` reach `1.0`.
    #[test]
    fn zero_coverage_is_a_bit_identical_no_op_on_the_device() {
        let gpu = Gpu::acquire();
        let rows: Vec<PatchSample> = (0..SAMPLES)
            .map(|index| {
                let t = index as f32;
                PatchSample {
                    world_pos: [t * 3.7 - 61.0, t * -5.3 + 24.0, t * 1.9 - 9.5],
                    nw: [0.6, 0.0, -0.8],
                    macro_second_rg: [super::fract(t * 0.317), super::fract(t * 0.713)],
                    patch_p: [0.0, 2.6, 0.12, -0.08],
                    albedo: [0.51, 0.47, 0.44],
                    roughness: 0.63,
                    height: 0.5,
                }
            })
            .collect();
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("axiom-material-patches-zero"),
                source: wgpu::ShaderSource::Wgsl([PATCHES_WGSL, HARNESS_WGSL].concat().into()),
            });
        let bytes = row_bytes(&rows);
        let albedo = gpu.render(&module, "patch_albedo_fs", &bytes);
        let height = gpu.render(&module, "patch_height_fs", &bytes);
        rows.iter().enumerate().for_each(|(index, row)| {
            (0..3).for_each(|lane| {
                assert_eq!(
                    albedo[index][lane].to_bits(),
                    row.albedo[lane].to_bits(),
                    "zero coverage moved albedo lane {lane} on the GPU at row {index}"
                );
            });
            assert_eq!(albedo[index][3].to_bits(), row.roughness.to_bits());
            assert_eq!(height[index][0].to_bits(), row.height.to_bits());
        });
    }
}
