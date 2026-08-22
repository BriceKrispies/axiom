//! **Cavity and vertex-colour masks** — `materials/shader.js:568-596`, plus the
//! `aoStrength` consumer at `shader.js:678`.
//!
//! This is where the per-vertex masks the geometry side bakes actually reach the
//! shading. Two independent things live here, and the source runs them in this
//! order:
//!
//! 1. **Cavity.** `cav = 1 - owHeightS` — the *complement of the height field*,
//!    not a screen-space derivative. There is no `dpdx`/`dpdy` and no curvature
//!    estimate in this section, so no `fwidth`-shaped parameter is needed (the
//!    divergence `apps/shmup/src/sky/dome.rs` had to make for `skSunDisc`). The
//!    only implicit input is `owHeightS` itself, which the POM/detail/patch
//!    layers write earlier in `MAIN_FRAGMENT`; here it is an explicit argument.
//!    Cavity darkens albedo toward the grime colour by `cav²·owWeatherP.w` and
//!    scales AO by `1 - cav·owWeatherP.w·0.5`. It is **always on** — it is not
//!    inside the `OW_VCOL_MASKS` define.
//! 2. **The three vertex-colour masks**, inside `#ifdef OW_VCOL_MASKS`.
//!    `materials/masks.js:11` fixes the channel meanings: `r = wear`,
//!    `g = grime`, `b = extra AO`, all defaulting to 0. Each rides its **own**
//!    strength out of the `wear` uniform (`owWearP`), and the three are not
//!    interchangeable:
//!
//!    | channel | strength | what it does |
//!    |---|---|---|
//!    | `vColor.r` | `wear[0]` | mixes albedo toward `wearColor`, and *lerps* roughness/metalness to `wearMaterial.xy` |
//!    | `vColor.g` | `wear[1]` | mixes albedo toward `grimeColor`, adds roughness, cuts metalness |
//!    | `vColor.b` | `wear[2]` | multiplies AO down — the only one that touches AO |
//!
//! ## `wear[3]` is dead, and stays dead
//!
//! `shader.js:91` declares `owWearP` as `x wear amt, y grime amt, z vcol AO amt,
//! w curvature`, but **nothing in the file ever reads `owWearP.w`** — grep the
//! source and the only sites are the declaration, the upload
//! (`shader.js:826`) and this layer's three reads of `.x`/`.y`/`.z`.
//! `DEFAULT_PARAMS.wear` agrees with reality rather than with the declaration:
//! `[0.5, 0.7, 0.5, 0]`, commented "wear, grime, extra AO, **unused**". The
//! "curvature" the stale comment means is baked per-vertex by `masks.js`, not
//! passed as a scalar. So [`MaskInputs::wear_params`] carries the whole `vec4`
//! and the lane is named and unread, exactly as the source has it. Dead
//! computation in the source is still part of the source; inventing a use for it
//! would be a bigger lie than keeping it.
//!
//! ## `aoStrength` lerps toward 1 — it does not multiply
//!
//! `shader.js:678` is `ambientOcclusion = ( owORM.r - 1.0 ) * owAoAmt + 1.0`,
//! i.e. `mix(1, ao, aoStrength)`. A direct `ao * aoStrength` agrees only where
//! `aoStrength == 1` or `ao == 1`; everywhere else it is darker, and at
//! `aoStrength == 0` it blacks the frame's indirect diffuse out instead of
//! disabling occlusion. [`ambient_occlusion`] is the site, transcribed in the
//! source's own grouping (`(ao - 1) * s + 1`, **not** `1 + s*(ao - 1)` — float
//! addition is not associative and this is the specification).
//!
//! ## `vertexMasks: false` is the default
//!
//! `DEFAULT_PARAMS.vertexMasks` is `false`, and `OW_VCOL_MASKS` is a *compile
//! time* define in the source: with masks off the whole block is not in the
//! program. A runtime port cannot have two programs behind one function, so the
//! WGSL selects with `select()` and the Rust with a two-element table index —
//! both of which return one of two values *unchanged*, never a blend. That makes
//! the disabled path **bit-identical**, which
//! [`tests::disabling_the_vertex_masks_is_bit_identical_whatever_the_mask_params_say`]
//! proves at the boundary by varying every mask input under a disabled flag and
//! comparing `to_bits`.
//!
//! ## Storage width
//!
//! Everything here is `f32`, on both sides. The GPU has no choice, and a `f64`
//! CPU reference would make the parity tolerance measure the *width difference*
//! rather than the hardware. The independent Python transcription used to pin
//! [`tests::the_pinned_case_matches_an_independent_transcription_of_the_glsl`]
//! is `f64`, and that test carries the one tolerance that exists for that reason.
//!
//! ## Transcription notes
//!
//! `mix` and `smoothstep` are written out by hand in the WGSL, from their GLSL
//! spec expressions, rather than calling the WGSL builtins — the same choice
//! `surface_program::emit` documents: a builtin's internal factoring is
//! unspecified, so calling it would put an unmeasurable difference between the
//! shader and the CPU reference. `clamp` *is* the builtin: GLSL and WGSL both
//! define it as `min(max(e, low), high)` with no factoring freedom, and the Rust
//! side spells that out rather than using `f32::clamp` (which panics on
//! `low > high` instead of returning `high`). No division in this layer became a
//! reciprocal-multiply; the only division is inside `smoothstep`, where it is
//! written as a division on both sides.

/// The WGSL for the cavity + vertex-mask layer.
///
/// Two entry points, both free functions over explicit arguments — no globals,
/// no assumed binding index, no `params.slots`:
///
/// ```wgsl
/// fn axiom_masks_apply(
///     albedo: vec3<f32>, orm: vec3<f32>, height_s: f32, vertex_color: vec3<f32>,
///     mac1: vec4<f32>, mac2: vec4<f32>, grime_color: vec3<f32>, wear_color: vec3<f32>,
///     wear_material: vec4<f32>, wear_params: vec4<f32>, weather_params: vec4<f32>,
///     vertex_masks: bool,
/// ) -> AxiomMasksOut
///
/// fn axiom_masks_ambient_occlusion(occlusion: f32, ao_strength: f32) -> f32
/// ```
///
/// `orm` is the source's `owORM`: `x = ao`, `y = roughness`, `z = metalness`.
/// `weather_params` is the whole `owWeatherP` even though only `.w` (cavity
/// grime) is read here, and `wear_params` the whole `owWearP` even though `.w`
/// is read nowhere at all — the uniforms keep their identity across the layers
/// that share them.
pub(crate) const MASKS_WGSL: &str = r#"
// The two channels this layer rewrites. `orm` is `owORM`: ao, roughness,
// metalness.
struct AxiomMasksOut {
    albedo: vec3<f32>,
    orm: vec3<f32>,
};

// GLSL `mix(x, y, a)`, spelled out as the spec's `x*(1-a) + y*a`. Hand-written
// rather than calling the builtin because a builtin's factoring is unspecified
// and the CPU reference has to match THIS expression.
fn axiom_masks_mix(x: f32, y: f32, a: f32) -> f32 {
    return x * (1.0 - a) + y * a;
}

fn axiom_masks_mix3(x: vec3<f32>, y: vec3<f32>, a: f32) -> vec3<f32> {
    return vec3<f32>(
        axiom_masks_mix(x.x, y.x, a),
        axiom_masks_mix(x.y, y.y, a),
        axiom_masks_mix(x.z, y.z, a),
    );
}

// GLSL `smoothstep(edge0, edge1, x)`, spelled out, for the same reason.
fn axiom_masks_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// `shader.js:678`, the `aomap_fragment` override. A LERP TOWARD 1, not a
// multiply: `aoStrength = 0` must mean "no occlusion", not "black".
fn axiom_masks_ambient_occlusion(occlusion: f32, ao_strength: f32) -> f32 {
    return (occlusion - 1.0) * ao_strength + 1.0;
}

// `shader.js:568-596`. `vertex_masks` is the runtime stand-in for the source's
// `#ifdef OW_VCOL_MASKS`; `select` returns one branch's value UNCHANGED, so the
// disabled path is bit-identical to a program compiled without the define.
fn axiom_masks_apply(
    albedo: vec3<f32>,
    orm: vec3<f32>,
    height_s: f32,
    vertex_color: vec3<f32>,
    mac1: vec4<f32>,
    mac2: vec4<f32>,
    grime_color: vec3<f32>,
    wear_color: vec3<f32>,
    wear_material: vec4<f32>,
    wear_params: vec4<f32>,
    weather_params: vec4<f32>,
    vertex_masks: bool,
) -> AxiomMasksOut {
    // float cav = 1.0 - owHeightS;
    let cav = 1.0 - height_s;
    // alb.rgb = mix( alb.rgb, owGrimeCol, cav * cav * owWeatherP.w );
    let cav_albedo = axiom_masks_mix3(albedo, grime_color, cav * cav * weather_params.w);
    // orm.r *= 1.0 - cav * owWeatherP.w * 0.5;
    let cav_orm = vec3<f32>(orm.x * (1.0 - cav * weather_params.w * 0.5), orm.y, orm.z);

    // #ifdef OW_VCOL_MASKS
    // float wearN = smoothstep( 0.25, 0.85, mac1.b * 0.65 + mac2.a * 0.55 );
    let wear_n = axiom_masks_smoothstep(0.25, 0.85, mac1.b * 0.65 + mac2.a * 0.55);
    // float wearM = vColor.r * owWearP.x * ( 0.55 + 0.45 * smoothstep( 0.30, 0.80, owHeightS ) )
    //             * ( 0.25 + 1.15 * wearN );
    let wear_raw = vertex_color.r * wear_params.x
        * (0.55 + 0.45 * axiom_masks_smoothstep(0.30, 0.80, height_s))
        * (0.25 + 1.15 * wear_n);
    // wearM = clamp( wearM, 0.0, 1.0 );
    let wear_m = clamp(wear_raw, 0.0, 1.0);
    // alb.rgb = mix( alb.rgb, owWearCol, wearM * owWearMat.w );
    let wear_albedo = axiom_masks_mix3(cav_albedo, wear_color, wear_m * wear_material.w);
    // orm.g = mix( orm.g, owWearMat.x, wearM );
    let wear_g = axiom_masks_mix(cav_orm.y, wear_material.x, wear_m);
    // orm.b = mix( orm.b, owWearMat.y, wearM );
    let wear_b = axiom_masks_mix(cav_orm.z, wear_material.y, wear_m);
    // float grimeM = vColor.g * owWearP.y * ( 0.35 + 0.65 * cav ) * ( 0.45 + 0.9 * mac2.g );
    // NOT clamped in the source: grimeM > 1 extrapolates the mixes below.
    let grime_m = vertex_color.g * wear_params.y * (0.35 + 0.65 * cav) * (0.45 + 0.9 * mac2.g);
    // alb.rgb = mix( alb.rgb, owGrimeCol, grimeM * 0.8 );
    let masked_albedo = axiom_masks_mix3(wear_albedo, grime_color, grime_m * 0.8);
    // orm.g = clamp( orm.g + grimeM * 0.22, 0.0, 1.0 );
    let masked_g = clamp(wear_g + grime_m * 0.22, 0.0, 1.0);
    // orm.b *= 1.0 - grimeM * 0.8;
    let masked_b = wear_b * (1.0 - grime_m * 0.8);
    // orm.r *= 1.0 - vColor.b * owWearP.z;   (on the CAVITY-modified ao)
    let masked_r = cav_orm.x * (1.0 - vertex_color.b * wear_params.z);
    // #endif

    return AxiomMasksOut(
        select(cav_albedo, masked_albedo, vertex_masks),
        select(cav_orm, vec3<f32>(masked_r, masked_g, masked_b), vertex_masks),
    );
}
"#;

/// GLSL `mix(x, y, a)`: the spec's `x*(1-a) + y*a`, in that grouping.
fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// GLSL `mix` over a `vec3` by a scalar — three independent scalar mixes, which
/// is what the componentwise definition is.
fn mix3(x: [f32; 3], y: [f32; 3], a: f32) -> [f32; 3] {
    [
        mix(x[0], y[0], a),
        mix(x[1], y[1], a),
        mix(x[2], y[2], a),
    ]
}

/// GLSL `clamp(x, low, high)` = `min(max(x, low), high)`. Deliberately not
/// `f32::clamp`, which panics when `low > high` where GLSL returns `high`.
fn clamp(x: f32, low: f32, high: f32) -> f32 {
    x.max(low).min(high)
}

/// GLSL `smoothstep(edge0, edge1, x)`, written as the spec writes it. The
/// division stays a division: turning it into a reciprocal-multiply is the
/// single most common defect this port has found.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Everything `axiom_masks_apply` reads, with the source's uniform names on each
/// field. Every one is an explicit input: this layer reads no global and assumes
/// no binding.
pub(crate) struct MaskInputs {
    /// `alb.rgb` as it stands after the weathering layer.
    pub(crate) albedo: [f32; 3],
    /// `owORM`: `[ao, roughness, metalness]`.
    pub(crate) orm: [f32; 3],
    /// `owHeightS`, the height field after POM/detail/patches. Cavity is its
    /// complement — there is no derivative in this layer.
    pub(crate) height_s: f32,
    /// `vColor`: `r = wear`, `g = grime`, `b = extra AO` (`masks.js:11`).
    pub(crate) vertex_color: [f32; 3],
    /// `mac1`, the first macro-noise sample. Only `.b` is read here.
    pub(crate) mac1: [f32; 4],
    /// `mac2`, the second macro-noise sample. `.g` and `.a` are read here.
    pub(crate) mac2: [f32; 4],
    /// `owGrimeCol`.
    pub(crate) grime_color: [f32; 3],
    /// `owWearCol`.
    pub(crate) wear_color: [f32; 3],
    /// `owWearMat`: `[roughness, metalness, reserved, tint amount]`.
    pub(crate) wear_material: [f32; 4],
    /// `owWearP` = `DEFAULT_PARAMS.wear`: `[wear, grime, extra AO, unused]`.
    /// Lane 3 is read nowhere in `shader.js`; see this module's header.
    pub(crate) wear_params: [f32; 4],
    /// `owWeatherP`. Only `.w`, cavity grime, is read here.
    pub(crate) weather_params: [f32; 4],
    /// `DEFAULT_PARAMS.vertexMasks` — the source's `#ifdef OW_VCOL_MASKS`.
    /// **Defaults to `false`**, and `false` must be bit-identical to a program
    /// compiled without the define.
    pub(crate) vertex_masks: bool,
}

/// The two channels the layer rewrites.
pub(crate) struct MaskLayer {
    /// `alb.rgb`.
    pub(crate) albedo: [f32; 3],
    /// `owORM`: `[ao, roughness, metalness]`.
    pub(crate) orm: [f32; 3],
}

/// **The CPU reference.** `shader.js:568-596`, in the source's own order and
/// grouping — the semantic definition [`MASKS_WGSL`] is held against.
pub(crate) fn apply(input: &MaskInputs) -> MaskLayer {
    // float cav = 1.0 - owHeightS;
    let cav = 1.0 - input.height_s;
    // alb.rgb = mix( alb.rgb, owGrimeCol, cav * cav * owWeatherP.w );
    let cav_albedo = mix3(input.albedo, input.grime_color, cav * cav * input.weather_params[3]);
    // orm.r *= 1.0 - cav * owWeatherP.w * 0.5;
    let cav_orm = [
        input.orm[0] * (1.0 - cav * input.weather_params[3] * 0.5),
        input.orm[1],
        input.orm[2],
    ];

    let (masked_albedo, masked_orm) = vertex_colour_masks(input, cav, cav_albedo, cav_orm);

    // The source's `#ifdef` is compile-time; a runtime port picks one of two
    // finished values, never a blend, so `false` reproduces the undefined build
    // bit for bit.
    let (albedo, orm) = [(cav_albedo, cav_orm), (masked_albedo, masked_orm)]
        [usize::from(input.vertex_masks)];
    MaskLayer { albedo, orm }
}

/// The `#ifdef OW_VCOL_MASKS` block, `shader.js:573-595`, over the
/// cavity-modified albedo/ORM.
fn vertex_colour_masks(
    input: &MaskInputs,
    cav: f32,
    cav_albedo: [f32; 3],
    cav_orm: [f32; 3],
) -> ([f32; 3], [f32; 3]) {
    // float wearN = smoothstep( 0.25, 0.85, mac1.b * 0.65 + mac2.a * 0.55 );
    let wear_n = smoothstep(0.25, 0.85, input.mac1[2] * 0.65 + input.mac2[3] * 0.55);
    // float wearM = vColor.r * owWearP.x * ( 0.55 + 0.45 * smoothstep( 0.30, 0.80, owHeightS ) )
    //             * ( 0.25 + 1.15 * wearN );
    let wear_raw = input.vertex_color[0]
        * input.wear_params[0]
        * (0.55 + 0.45 * smoothstep(0.30, 0.80, input.height_s))
        * (0.25 + 1.15 * wear_n);
    // wearM = clamp( wearM, 0.0, 1.0 );
    let wear_m = clamp(wear_raw, 0.0, 1.0);
    // alb.rgb = mix( alb.rgb, owWearCol, wearM * owWearMat.w );
    let wear_albedo = mix3(cav_albedo, input.wear_color, wear_m * input.wear_material[3]);
    // orm.g = mix( orm.g, owWearMat.x, wearM );
    let wear_g = mix(cav_orm[1], input.wear_material[0], wear_m);
    // orm.b = mix( orm.b, owWearMat.y, wearM );
    let wear_b = mix(cav_orm[2], input.wear_material[1], wear_m);
    // float grimeM = vColor.g * owWearP.y * ( 0.35 + 0.65 * cav ) * ( 0.45 + 0.9 * mac2.g );
    // Unclamped in the source: above 1 it extrapolates the mixes that follow.
    let grime_m = input.vertex_color[1]
        * input.wear_params[1]
        * (0.35 + 0.65 * cav)
        * (0.45 + 0.9 * input.mac2[1]);
    // alb.rgb = mix( alb.rgb, owGrimeCol, grimeM * 0.8 );
    let masked_albedo = mix3(wear_albedo, input.grime_color, grime_m * 0.8);
    // orm.g = clamp( orm.g + grimeM * 0.22, 0.0, 1.0 );
    let masked_g = clamp(wear_g + grime_m * 0.22, 0.0, 1.0);
    // orm.b *= 1.0 - grimeM * 0.8;
    let masked_b = wear_b * (1.0 - grime_m * 0.8);
    // orm.r *= 1.0 - vColor.b * owWearP.z;
    let masked_r = cav_orm[0] * (1.0 - input.vertex_color[2] * input.wear_params[2]);
    (masked_albedo, [masked_r, masked_g, masked_b])
}

/// **`aoStrength`.** `shader.js:678`: `( owORM.r - 1.0 ) * owAoAmt + 1.0`.
///
/// A lerp of the occlusion toward **1**, in the source's grouping. Not
/// `occlusion * ao_strength`: the two agree only at `ao_strength == 1` or
/// `occlusion == 1`, and at `ao_strength == 0` the multiply would black out the
/// indirect diffuse where the source disables occlusion entirely.
pub(crate) fn ambient_occlusion(occlusion: f32, ao_strength: f32) -> f32 {
    (occlusion - 1.0) * ao_strength + 1.0
}

#[cfg(test)]
mod tests {
    use super::{ambient_occlusion, apply, clamp, mix, mix3, smoothstep, MaskInputs, MASKS_WGSL};

    /// The parameter set the source ships: `DEFAULT_PARAMS.weather`,
    /// `.wear`, `.wearMaterial`, `.vertexMasks`, over a plausible surface.
    /// Every other test perturbs one field of this with `..base()`.
    pub(super) fn base() -> MaskInputs {
        MaskInputs {
            albedo: [0.42, 0.31, 0.26],
            orm: [0.87, 0.55, 0.10],
            height_s: 0.62,
            vertex_color: [0.80, 0.45, 0.65],
            mac1: [0.51, 0.47, 0.72, 0.39],
            mac2: [0.44, 0.58, 0.61, 0.66],
            grime_color: [0.0620, 0.0545, 0.0430],
            wear_color: [0.5530, 0.5450, 0.5250],
            wear_material: [0.42, 0.0, 0.0, 0.5],
            wear_params: [0.5, 0.7, 0.5, 0.0],
            weather_params: [0.35, 0.3, 0.55, 0.4],
            vertex_masks: false,
        }
    }

    /// The sample set both the CPU tests and the GPU parity test drive, chosen
    /// to reach every shape in the layer: `cav` at both ends, `wearM` clamped to
    /// 1 and pinned at 0, `grimeM` above 1 (where the mixes extrapolate), the
    /// smoothstep inputs below `edge0` and above `edge1`, cavity grime off, and
    /// the mask flag both ways over identical inputs.
    pub(super) fn samples() -> Vec<MaskInputs> {
        vec![
            base(),
            MaskInputs { vertex_masks: true, ..base() },
            // cav = 1: a full cavity. Also drives smoothstep(0.30, 0.80, 0) below
            // edge0, so the height bias sits at its 0.55 floor.
            MaskInputs { height_s: 0.0, vertex_masks: true, ..base() },
            // cav = 0: no cavity at all, and smoothstep above edge1.
            MaskInputs { height_s: 1.0, vertex_masks: true, ..base() },
            // wearM saturates: the clamp's upper arm.
            MaskInputs {
                vertex_color: [1.0, 0.2, 0.1],
                wear_params: [4.0, 0.7, 0.5, 0.0],
                vertex_masks: true,
                ..base()
            },
            // wearM = 0: nothing rubs through, and wearN's smoothstep is below
            // edge0 (mac1.b, mac2.a both dark).
            MaskInputs {
                vertex_color: [0.0, 0.6, 0.3],
                mac1: [0.10, 0.20, 0.05, 0.30],
                mac2: [0.15, 0.25, 0.35, 0.08],
                vertex_masks: true,
                ..base()
            },
            // wearN's smoothstep above edge1, and a metal wearMaterial — the one
            // library case the source says may raise metalness.
            MaskInputs {
                mac1: [0.90, 0.80, 0.95, 0.85],
                mac2: [0.88, 0.92, 0.75, 0.99],
                wear_material: [0.18, 1.0, 0.0, 0.85],
                vertex_masks: true,
                ..base()
            },
            // grimeM > 1: the two mixes extrapolate and `1 - grimeM*0.8` goes
            // negative, which the source does not guard.
            MaskInputs {
                vertex_color: [0.1, 1.0, 0.2],
                wear_params: [0.5, 3.0, 0.5, 0.0],
                vertex_masks: true,
                ..base()
            },
            // Cavity grime off: the always-on half must be an identity.
            MaskInputs { weather_params: [0.35, 0.3, 0.55, 0.0], ..base() },
            MaskInputs {
                weather_params: [0.35, 0.3, 0.55, 0.0],
                vertex_masks: true,
                ..base()
            },
            // Cavity grime at full strength over a deep cavity.
            MaskInputs {
                height_s: 0.05,
                weather_params: [0.35, 0.3, 0.55, 1.0],
                ..base()
            },
            // Every vertex channel at zero: the masks are painted, and an
            // unpainted mesh must come out of the block unchanged.
            MaskInputs { vertex_color: [0.0, 0.0, 0.0], vertex_masks: true, ..base() },
            // The AO channel alone, at full strength: `orm.r` halves.
            MaskInputs {
                vertex_color: [0.0, 0.0, 1.0],
                wear_params: [0.5, 0.7, 0.5, 0.0],
                vertex_masks: true,
                ..base()
            },
            // A bright, rough, non-metal surface — plaster.
            MaskInputs {
                albedo: [0.78, 0.74, 0.69],
                orm: [1.0, 0.92, 0.0],
                height_s: 0.31,
                vertex_color: [0.35, 0.90, 0.20],
                vertex_masks: true,
                ..base()
            },
            // A dark, glossy, fully metallic surface.
            MaskInputs {
                albedo: [0.09, 0.10, 0.11],
                orm: [0.60, 0.14, 1.0],
                height_s: 0.88,
                vertex_color: [0.55, 0.05, 0.95],
                wear_material: [0.30, 1.0, 0.0, 0.20],
                vertex_masks: true,
                ..base()
            },
            // The default parameter set with masks on, over a mid surface: the
            // case an app that flips `vertexMasks` actually hits.
            MaskInputs { height_s: 0.50, vertex_masks: true, ..base() },
        ]
    }

    /// The GLSL primitives, at the definitions the source's dialect gives them.
    #[test]
    fn the_glsl_primitives_are_the_glsl_definitions_not_rusts() {
        // mix extrapolates outside 0..=1, and is `x*(1-a) + y*a`, not a lerp
        // written the other way.
        assert_eq!(mix(2.0, 6.0, 0.25), 3.0);
        assert_eq!(mix(2.0, 6.0, 2.0), 10.0);
        assert_eq!(mix3([0.0, 1.0, 2.0], [4.0, 5.0, 6.0], 0.5), [2.0, 3.0, 4.0]);
        // GLSL clamp is min(max(x, low), high) — including the degenerate
        // low > high, where it returns high rather than panicking as
        // `f32::clamp` would.
        assert_eq!(clamp(-1.0, 0.0, 1.0), 0.0);
        assert_eq!(clamp(3.0, 0.0, 1.0), 1.0);
        assert_eq!(clamp(0.25, 0.0, 1.0), 0.25);
        assert_eq!(clamp(0.5, 1.0, 0.0), 0.0);
        // smoothstep clamps first, then applies the cubic.
        assert_eq!(smoothstep(0.25, 0.85, 0.0), 0.0);
        assert_eq!(smoothstep(0.25, 0.85, 1.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
        assert!((smoothstep(0.0, 1.0, 0.25) - 0.15625).abs() < 1e-7);
    }

    /// Pinned against an independent transcription of the GLSL text into Python
    /// (`f64`), written from `shader.js` rather than from this file. The
    /// tolerance is `1e-6`, which is the storage-width difference between that
    /// `f64` oracle and this `f32` reference — nothing else.
    #[test]
    fn the_pinned_case_matches_an_independent_transcription_of_the_glsl() {
        let off = apply(&base());
        let on = apply(&MaskInputs { vertex_masks: true, ..base() });
        let expect = |actual: [f32; 3], want: [f64; 3], what: &str| {
            (0..3).for_each(|lane| {
                let delta = (f64::from(actual[lane]) - want[lane]).abs();
                assert!(
                    delta < 1e-6,
                    "{what} lane {lane}: {} vs oracle {} (delta {delta})",
                    actual[lane],
                    want[lane]
                );
            });
        };
        expect(off.albedo, [0.399_321_92, 0.295_242_32, 0.247_466_08], "cavity albedo");
        expect(off.orm, [0.803_88, 0.55, 0.10], "cavity orm");
        expect(on.albedo, [0.381_770_319, 0.311_679_768_9, 0.274_951_462_3], "masked albedo");
        expect(on.orm, [0.542_619, 0.527_246_552_5, 0.044_023_499_6], "masked orm");
        // shader.js:678, from the same oracle.
        assert!((f64::from(ambient_occlusion(off.orm[0], 1.0)) - 0.803_88).abs() < 1e-6);
        assert!((f64::from(ambient_occlusion(off.orm[0], 0.35)) - 0.931_358).abs() < 1e-6);
        assert!((f64::from(ambient_occlusion(on.orm[0], 0.35)) - 0.839_916_65).abs() < 1e-6);
    }

    /// **The boundary.** `vertexMasks` defaults to `false`, and off must mean a
    /// program that never had the block: bit-identical output however loudly the
    /// mask parameters are set.
    #[test]
    fn disabling_the_vertex_masks_is_bit_identical_whatever_the_mask_params_say() {
        let quiet = apply(&MaskInputs {
            vertex_color: [0.0, 0.0, 0.0],
            wear_params: [0.0, 0.0, 0.0, 0.0],
            wear_material: [0.0, 0.0, 0.0, 0.0],
            wear_color: [0.0, 0.0, 0.0],
            ..base()
        });
        let loud = apply(&MaskInputs {
            vertex_color: [1.0, 1.0, 1.0],
            wear_params: [9.0, 9.0, 9.0, 9.0],
            wear_material: [1.0, 1.0, 1.0, 1.0],
            wear_color: [1.0, 1.0, 1.0],
            ..base()
        });
        (0..3).for_each(|lane| {
            assert_eq!(
                quiet.albedo[lane].to_bits(),
                loud.albedo[lane].to_bits(),
                "albedo lane {lane} moved with the masks disabled"
            );
            assert_eq!(
                quiet.orm[lane].to_bits(),
                loud.orm[lane].to_bits(),
                "orm lane {lane} moved with the masks disabled"
            );
        });
        // And the disabled output really is the cavity half alone.
        let cav = 1.0_f32 - base().height_s;
        assert_eq!(
            quiet.albedo[0].to_bits(),
            mix(base().albedo[0], base().grime_color[0], cav * cav * base().weather_params[3])
                .to_bits()
        );
        assert_eq!(
            quiet.orm[0].to_bits(),
            (base().orm[0] * (1.0 - cav * base().weather_params[3] * 0.5)).to_bits()
        );
    }

    /// `wear[3]` is read nowhere in `shader.js`. It is carried so the uniform
    /// keeps its shape, and moving it must change nothing — on either side of
    /// the mask flag.
    #[test]
    fn the_fourth_wear_lane_is_unused_by_the_source_and_moving_it_changes_nothing() {
        [false, true].iter().for_each(|&flag| {
            let zero = apply(&MaskInputs {
                wear_params: [0.5, 0.7, 0.5, 0.0],
                vertex_masks: flag,
                ..base()
            });
            let loud = apply(&MaskInputs {
                wear_params: [0.5, 0.7, 0.5, 7.5],
                vertex_masks: flag,
                ..base()
            });
            (0..3).for_each(|lane| {
                assert_eq!(zero.albedo[lane].to_bits(), loud.albedo[lane].to_bits());
                assert_eq!(zero.orm[lane].to_bits(), loud.orm[lane].to_bits());
            });
        });
    }

    /// The three `wear` channels are three separate masks with three separate
    /// strengths, and collapsing any pair of them is a visible defect. Each is
    /// driven alone and must move only what it owns.
    #[test]
    fn the_three_wear_channels_are_separate_masks_with_separate_strengths() {
        let neutral = MaskInputs {
            vertex_color: [0.0, 0.0, 0.0],
            weather_params: [0.35, 0.3, 0.55, 0.0],
            vertex_masks: true,
            ..base()
        };
        let none = apply(&neutral);
        // r: albedo toward wearColor, roughness/metalness toward wearMaterial,
        // AO untouched.
        let wear = apply(&MaskInputs { vertex_color: [1.0, 0.0, 0.0], ..neutral });
        assert_ne!(wear.albedo[0], none.albedo[0]);
        assert_ne!(wear.orm[1], none.orm[1]);
        assert_ne!(wear.orm[2], none.orm[2]);
        assert_eq!(wear.orm[0].to_bits(), none.orm[0].to_bits());
        // g: albedo toward grimeColor, roughness up, metalness down, AO
        // untouched.
        let grime = apply(&MaskInputs { vertex_color: [0.0, 1.0, 0.0], ..neutral });
        assert_ne!(grime.albedo[0], none.albedo[0]);
        assert!(grime.orm[1] > none.orm[1], "grime roughens");
        assert!(grime.orm[2] < none.orm[2], "grime kills metalness");
        assert_eq!(grime.orm[0].to_bits(), none.orm[0].to_bits());
        // b: AO alone, and nothing else in the frame.
        let ao = apply(&MaskInputs { vertex_color: [0.0, 0.0, 1.0], ..neutral });
        assert!(ao.orm[0] < none.orm[0], "the AO channel darkens occlusion");
        (0..3).for_each(|lane| assert_eq!(ao.albedo[lane].to_bits(), none.albedo[lane].to_bits()));
        assert_eq!(ao.orm[1].to_bits(), none.orm[1].to_bits());
        assert_eq!(ao.orm[2].to_bits(), none.orm[2].to_bits());
        // Each rides its OWN strength: zeroing wear[0] leaves grime[1] working.
        let only_grime = apply(&MaskInputs {
            vertex_color: [1.0, 1.0, 0.0],
            wear_params: [0.0, 0.7, 0.5, 0.0],
            ..neutral
        });
        assert_ne!(only_grime.albedo[0], none.albedo[0]);
        assert_eq!(only_grime.orm[2].to_bits(), grime.orm[2].to_bits());
    }

    /// Cavity is the complement of the height field, always on, and independent
    /// of the vertex masks.
    #[test]
    fn cavity_darkens_with_depth_and_is_not_gated_by_the_vertex_masks() {
        let deep = apply(&MaskInputs { height_s: 0.05, ..base() });
        let high = apply(&MaskInputs { height_s: 0.95, ..base() });
        assert!(deep.albedo[0] < high.albedo[0], "a deeper cavity is grimier");
        assert!(deep.orm[0] < high.orm[0], "a deeper cavity occludes more");
        // With cavity grime off, the layer is the identity when masks are off.
        let off = apply(&MaskInputs {
            weather_params: [0.35, 0.3, 0.55, 0.0],
            ..base()
        });
        (0..3).for_each(|lane| {
            assert_eq!(off.albedo[lane].to_bits(), base().albedo[lane].to_bits());
            assert_eq!(off.orm[lane].to_bits(), base().orm[lane].to_bits());
        });
    }

    /// `aoStrength` lerps toward 1. The multiply it is not agrees only at the
    /// endpoints, and disagrees hardest exactly where it matters.
    #[test]
    fn ao_strength_lerps_the_occlusion_toward_one_rather_than_multiplying_it() {
        // Off means "no occlusion", not "black".
        assert_eq!(ambient_occlusion(0.25, 0.0), 1.0);
        // Full strength passes the occlusion through.
        assert_eq!(ambient_occlusion(0.25, 1.0), 0.25);
        // Unoccluded stays unoccluded at every strength.
        assert_eq!(ambient_occlusion(1.0, 0.4), 1.0);
        // And in between it is NOT the multiply.
        let lerped = ambient_occlusion(0.25, 0.5);
        assert!((lerped - 0.625).abs() < 1e-7, "got {lerped}");
        assert!((lerped - 0.25 * 0.5).abs() > 0.4, "a multiply would be far darker");
        // The source overdrives it too — aoStrength above 1 extrapolates.
        assert!((ambient_occlusion(0.5, 2.0) - 0.0).abs() < 1e-7);
    }

    /// Every sample the parity test drives is exercised on the CPU too, so the
    /// table is covered whether or not the `offscreen` feature is on, and so a
    /// sample that produced a non-finite lane could never reach the GPU
    /// unnoticed.
    #[test]
    fn every_parity_sample_produces_finite_channels_on_the_cpu() {
        let all = samples();
        assert_eq!(all.len(), 16, "the parity target is SAMPLES wide");
        all.iter().enumerate().for_each(|(index, input)| {
            let out = apply(input);
            let ao = ambient_occlusion(out.orm[0], 0.35);
            (0..3).for_each(|lane| {
                assert!(out.albedo[lane].is_finite(), "sample {index} albedo {lane}");
                assert!(out.orm[lane].is_finite(), "sample {index} orm {lane}");
            });
            assert!(ao.is_finite(), "sample {index} ao");
        });
        // The table must actually move the output, or the parity it feeds is
        // vacuous: the albedo red lane spreads across the set.
        let reds: Vec<f32> = all.iter().map(|input| apply(input).albedo[0]).collect();
        let low = reds.iter().fold(f32::MAX, |a, b| a.min(*b));
        let high = reds.iter().fold(f32::MIN, |a, b| a.max(*b));
        assert!(high - low > 0.3, "the sample table must span the layer's range");
    }

    /// The WGSL says what this module says: both entry points, and no stray
    /// reach for a global or a binding.
    #[test]
    fn the_wgsl_declares_the_two_entry_points_and_reads_no_global() {
        assert!(MASKS_WGSL.contains("fn axiom_masks_apply("));
        assert!(MASKS_WGSL.contains("fn axiom_masks_ambient_occlusion(occlusion: f32, ao_strength: f32) -> f32"));
        assert!(!MASKS_WGSL.contains("@group"));
        assert!(!MASKS_WGSL.contains("@binding"));
        assert!(!MASKS_WGSL.contains("var<uniform>"));
    }
}

/// **CPU↔GPU parity on a real adapter**, in the shape
/// `crate::surface_program::parity` establishes: acquire an adapter and
/// **assert** on it rather than skipping, render one fragment per sample into an
/// `Rgba32Float` target, read the lanes back, and compare against the CPU
/// reference at a tolerance derived from the measurement.
///
/// This module carries its own device harness rather than borrowing
/// `surface_program`'s: that one is `pub(super)` inside `surface_program`, and
/// reaching across for it would mean editing a file this layer does not own.
#[cfg(all(test, feature = "offscreen"))]
mod parity {
    use super::tests::samples;
    use super::{ambient_occlusion, apply, MaskInputs, MASKS_WGSL};

    /// How many samples one run compares; also the target's width.
    const SAMPLES: usize = 16;

    /// `vec4`s of uniform per sample. Must match `MASK_HARNESS_WGSL`'s unpack.
    const LANES: usize = 10;

    /// The tolerance, **derived from a measurement**: the worst lane delta over
    /// the sample table measures `5.96e-8` — exactly `2^-24`, one `f32` ULP at
    /// 1.0 — on a Vulkan adapter. That is as close as two `f32` evaluations can
    /// be without being equal, and it is what the layer costs: no
    /// transcendentals, one division, and every `mix`/`smoothstep` written out
    /// so the only remaining difference is the hardware's freedom to contract
    /// `a*b + c` into an `fma`.
    ///
    /// `4e-7` is **6.7x** that measurement and 3.4x the ULP floor — inside the
    /// brief's 10x rule with room for a driver that contracts differently, and
    /// three orders of magnitude tighter than the `1e-4` the field algebra's
    /// operators need. [`the_tolerance_is_within_ten_times_the_measured_hardware_delta`]
    /// re-measures every run and fails if this drifts loose.
    const TOLERANCE: f32 = 4.0e-7;

    /// `copy_texture_to_buffer` wants each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// The harness: a fullscreen triangle whose fragment stage evaluates the
    /// layer at the sample its pixel column names.
    const MASK_HARNESS_WGSL: &str = r#"
struct MaskSamples { items: array<vec4<f32>, 160> };
@group(0) @binding(0) var<uniform> mask_samples: MaskSamples;

@vertex
fn masks_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

fn masks_at(index: u32) -> AxiomMasksOut {
    let base = index * 10u;
    let v0 = mask_samples.items[base + 0u];
    let v1 = mask_samples.items[base + 1u];
    let v2 = mask_samples.items[base + 2u];
    return axiom_masks_apply(
        v0.xyz, v1.xyz, v0.w, v2.xyz,
        mask_samples.items[base + 3u],
        mask_samples.items[base + 4u],
        mask_samples.items[base + 5u].xyz,
        mask_samples.items[base + 6u].xyz,
        mask_samples.items[base + 7u],
        mask_samples.items[base + 8u],
        mask_samples.items[base + 9u],
        v2.w > 0.5,
    );
}

// rgb = the layer's albedo, a = the ambient occlusion the frame applies, so
// `axiom_masks_ambient_occlusion` is proven over this layer's OWN occlusion
// rather than at an invented value.
@fragment
fn masks_albedo_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let out = masks_at(index);
    let ao_strength = mask_samples.items[index * 10u + 1u].w;
    return vec4<f32>(out.albedo, axiom_masks_ambient_occlusion(out.orm.x, ao_strength));
}

@fragment
fn masks_orm_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    return vec4<f32>(masks_at(index).orm, 0.0);
}
"#;

    /// The `aoStrength` each sample is rendered at — deliberately not 1, so the
    /// lerp-toward-1 is exercised rather than the pass-through endpoint.
    const AO_STRENGTH: f32 = 0.35;

    /// One sample's ten `vec4` lanes, in the order `masks_at` unpacks them.
    fn lanes_of(input: &MaskInputs) -> [f32; LANES * 4] {
        [
            input.albedo[0], input.albedo[1], input.albedo[2], input.height_s,
            input.orm[0], input.orm[1], input.orm[2], AO_STRENGTH,
            input.vertex_color[0], input.vertex_color[1], input.vertex_color[2],
            f32::from(u8::from(input.vertex_masks)),
            input.mac1[0], input.mac1[1], input.mac1[2], input.mac1[3],
            input.mac2[0], input.mac2[1], input.mac2[2], input.mac2[3],
            input.grime_color[0], input.grime_color[1], input.grime_color[2], 0.0,
            input.wear_color[0], input.wear_color[1], input.wear_color[2], 0.0,
            input.wear_material[0], input.wear_material[1], input.wear_material[2],
            input.wear_material[3],
            input.wear_params[0], input.wear_params[1], input.wear_params[2],
            input.wear_params[3],
            input.weather_params[0], input.weather_params[1], input.weather_params[2],
            input.weather_params[3],
        ]
    }

    /// A real GPU, or a loud failure. A parity test that silently passes when
    /// nothing ran is worse than no parity test.
    struct MaskGpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl MaskGpu {
        fn acquire() -> MaskGpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
            MaskGpu {
                device,
                queue,
                backend: gpu.backend,
            }
        }

        /// Compile the layer's WGSL with the harness spliced after it, failing
        /// loudly with the driver's own message.
        fn compile(&self) -> wgpu::ShaderModule {
            // The error scope is the SHARED device's, so it is entered exclusively;
            // see `crate::test_gpu::validating`.
            let (module, failure) = crate::test_gpu::validating(&self.device, || {
                self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("axiom-material-masks-shader"),
                        source: wgpu::ShaderSource::Wgsl(
                            [MASKS_WGSL, MASK_HARNESS_WGSL].concat().into(),
                        ),
                    })
            });
            assert!(
                failure.is_none(),
                "the masks WGSL must compile: {}",
                failure.map_or(String::new(), |error| error.to_string())
            );
            module
        }

        /// Render `entry_point` over a `SAMPLES x 1` `Rgba32Float` target — a
        /// float target because an `Rgba8Unorm` one quantises to 1/255, four
        /// orders of magnitude coarser than the tolerance.
        fn render(&self, module: &wgpu::ShaderModule, entry_point: &str, uniform: &[u8])
            -> Vec<[f32; 4]>
        {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-material-masks-bgl"),
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
                    label: Some("axiom-material-masks-uniform"),
                    contents: uniform,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-material-masks-bg"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-material-masks-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-material-masks-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("masks_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry_point),
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
                label: Some("axiom-material-masks-target"),
                size: wgpu::Extent3d {
                    width: SAMPLES as u32,
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
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-material-masks-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-material-masks-pass"),
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

    /// The sample table's uniform bytes, padded to the harness's declared array.
    fn uniform_bytes(all: &[MaskInputs]) -> Vec<u8> {
        let mut bytes: Vec<u8> = all
            .iter()
            .flat_map(|input| lanes_of(input))
            .flat_map(f32::to_le_bytes)
            .collect();
        bytes.resize(SAMPLES * LANES * 16, 0);
        bytes
    }

    /// Both entry points on both sides: `(cpu, gpu)` lane sets for the albedo
    /// pass (rgb + the applied AO) and the ORM pass.
    fn compare(gpu: &MaskGpu) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
        let all = samples();
        let module = gpu.compile();
        let bytes = uniform_bytes(&all);
        let albedo = gpu.render(&module, "masks_albedo_fs", &bytes);
        let orm = gpu.render(&module, "masks_orm_fs", &bytes);
        let rendered: Vec<[f32; 4]> = albedo.into_iter().chain(orm).collect();
        let evaluated: Vec<[f32; 4]> = all
            .iter()
            .map(|input| {
                let out = apply(input);
                [
                    out.albedo[0],
                    out.albedo[1],
                    out.albedo[2],
                    ambient_occlusion(out.orm[0], AO_STRENGTH),
                ]
            })
            .chain(all.iter().map(|input| {
                let out = apply(input);
                [out.orm[0], out.orm[1], out.orm[2], 0.0]
            }))
            .collect();
        (evaluated, rendered)
    }

    /// The worst absolute lane delta — the measurement a tolerance is set from.
    fn worst_delta(cpu: &[[f32; 4]], gpu: &[[f32; 4]]) -> f32 {
        cpu.iter()
            .zip(gpu.iter())
            .flat_map(|(expected, actual)| {
                [0_usize, 1, 2, 3].map(|lane| (expected[lane] - actual[lane]).abs())
            })
            .fold(0.0_f32, f32::max)
    }

    /// **The parity proof.** Every sample, both entry points, on a real adapter.
    #[test]
    fn the_masks_wgsl_agrees_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = MaskGpu::acquire();
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );
        let (cpu, rendered) = compare(&gpu);
        cpu.iter()
            .zip(rendered.iter())
            .enumerate()
            .for_each(|(sample, (expected, actual))| {
                (0..4).for_each(|lane| {
                    let delta = (expected[lane] - actual[lane]).abs();
                    assert!(
                        delta <= TOLERANCE,
                        "masks disagree at sample {sample} lane {lane}: \
                         CPU {} vs GPU {} (delta {delta}, tolerance {TOLERANCE})",
                        expected[lane],
                        actual[lane]
                    );
                });
            });
    }

    /// The tolerance is derived from the hardware, not fitted to a miss: it must
    /// stay within ten times what the adapter actually costs. A budget looser
    /// than that is itself a failure.
    ///
    /// The measurement is floored at [`f32::EPSILON`] because a tolerance cannot
    /// honestly be tighter than the representation: an adapter that happened to
    /// agree bit-for-bit would otherwise force every positive budget to fail.
    /// The floor is not what carries this test — [`TOLERANCE`] is inside 10x the
    /// *raw* `5.96e-8` measurement too.
    #[test]
    fn the_tolerance_is_within_ten_times_the_measured_hardware_delta() {
        let gpu = MaskGpu::acquire();
        let (cpu, rendered) = compare(&gpu);
        let measured = worst_delta(&cpu, &rendered);
        assert!(
            measured <= TOLERANCE,
            "the measured worst delta {measured} exceeds the tolerance {TOLERANCE}"
        );
        assert!(
            TOLERANCE <= measured.max(f32::EPSILON) * 10.0,
            "the tolerance {TOLERANCE} is more than 10x the measured worst delta \
             {measured}; derive it from the measurement"
        );
    }

    /// **The boundary, on the GPU.** `select` must return the disabled value
    /// untouched: two samples differing only in the flag, and the disabled one
    /// must be bit-equal to the same sample rendered with every mask parameter
    /// screaming.
    #[test]
    fn the_disabled_path_is_bit_identical_on_the_gpu() {
        let gpu = MaskGpu::acquire();
        let module = gpu.compile();
        let quiet = MaskInputs {
            vertex_color: [0.0, 0.0, 0.0],
            wear_params: [0.0, 0.0, 0.0, 0.0],
            wear_material: [0.0, 0.0, 0.0, 0.0],
            vertex_masks: false,
            ..super::tests::base()
        };
        let loud = MaskInputs {
            vertex_color: [1.0, 1.0, 1.0],
            wear_params: [9.0, 9.0, 9.0, 9.0],
            wear_material: [1.0, 1.0, 1.0, 1.0],
            vertex_masks: false,
            ..super::tests::base()
        };
        let on = MaskInputs { vertex_masks: true, ..super::tests::base() };
        let table: Vec<MaskInputs> = [quiet, loud, on]
            .into_iter()
            .chain(samples().into_iter().take(SAMPLES - 3))
            .collect();
        let bytes = uniform_bytes(&table);
        let albedo = gpu.render(&module, "masks_albedo_fs", &bytes);
        let orm = gpu.render(&module, "masks_orm_fs", &bytes);
        (0..3).for_each(|lane| {
            assert_eq!(
                albedo[0][lane].to_bits(),
                albedo[1][lane].to_bits(),
                "GPU albedo lane {lane} moved with the masks disabled"
            );
            assert_eq!(
                orm[0][lane].to_bits(),
                orm[1][lane].to_bits(),
                "GPU orm lane {lane} moved with the masks disabled"
            );
        });
        // And the flag is not inert: turning it on over the base sample moves
        // the frame, so the bit-identity above is a disable and not a no-op.
        assert_ne!(albedo[2][0].to_bits(), albedo[0][0].to_bits());
    }
}
