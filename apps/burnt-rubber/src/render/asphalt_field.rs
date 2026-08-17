//! The tarmac's aggregate grain, **authored as a field graph** rather than
//! hand-written as a pixel loop.
//!
//! [`super::asphalt_texture`] states what the grain *is* — three octaves, why
//! each exists, what each is measured against, and why its four tuning constants
//! hold the values they hold. Every one of those arguments still stands and none
//! of it is restated here. What this module changes is only *how the pixels are
//! produced*: the same three-octave structure is expressed as a value — an
//! [`axiom_field::FieldGraph`] of 58 nodes over the field algebra's 23 operators
//! — and baked to RGBA8 through [`axiom_proc_texture::TextureOp::Field`].
//!
//! ## Why a graph, and what it buys that a loop does not
//!
//! `asphalt_texture.rs` is 759 lines of Rust that only a Rust compiler can read.
//! Its three sibling generators (`verge_texture`, `foliage_texture`) carry
//! *byte-identical copies* of `hash_unit`, `smoothstep`, `lerp` and
//! `byte_for_multiplier`. A graph is a value: it can be digested, diffed against
//! another graph, canonicalised, serialized, explained one line per node, and —
//! the property this module is built to demonstrate — **retuned without
//! recompiling anything**, because a parameter's value is deliberately outside
//! [`axiom_field::FieldGraph::digest`].
//!
//! ## The four tuning constants are PARAMETERS, not `Const` nodes
//!
//! `SMOOTH_SHARE`, `CROSS_SHARE`, `CONTRAST` and `MIN_MULTIPLIER` are declared
//! parameter slots reading their shipped values from
//! [`super::asphalt_texture`] — one definition, so the two paths cannot drift.
//! Because they are slots and not literals, [`asphalt_field_tuned`] retunes the
//! grain while producing a graph with the **same structural digest**: on a live
//! surface that is a uniform write, not a shader recompile. That is asserted by
//! [`tests::retuning_the_grain_moves_the_pixels_and_not_the_digest`].
//!
//! ## The noise basis is different, and that is the honest part
//!
//! `asphalt_texture`'s octaves are built on `hash_unit`, an integer bit-mixer
//! (`wrapping_mul`, `xor`, `>>`). **The field algebra has no integer operators
//! and deliberately no `Div`, `Pow` or transcendental**, so `hash_unit` is not
//! expressible and cannot be made expressible without a 24th operator — which
//! would fail the admission test in `crates/axiom-field/ARCHITECTURE.md` (one
//! consumer, and composable-from-existing is not the issue; a bit-mixer is a
//! different *kind* of thing from a pointwise real expression).
//!
//! So the field path re-authors the same three octaves over the algebra's own
//! stochastic operator, [`axiom_field::FieldOp::Noise`] — `axiom_noise`'s
//! quintic-faded gradient noise. **The two paths are therefore not texel-equal
//! and cannot be.** What they *are* is statistically equivalent, and the
//! equivalence is pinned rather than asserted by hand-waving: each octave is
//! remapped to `0..1` by a gain (`CROSS_GAIN`, `SMOOTH_GAIN`, `FINE_GAIN`)
//! chosen so its standard deviation over the tile matches the octave it
//! replaces, and the baked tile is then held to **every assertion
//! `asphalt_texture`'s own test module makes** — strength, magnified step,
//! spectrum, anisotropic survival, scale, band.
//!
//! ## The sRGB transfer function is a fitted cubic, because the algebra has no `Pow`
//!
//! The backend uploads this texture as `Rgba8UnormSrgb`, so a byte here is
//! *decoded* before it multiplies the tarmac colour, and `asphalt_texture`
//! inverts the transfer function with `powf(1.0 / 2.4)`. The field algebra
//! excludes `Pow` on purpose (CPU/GPU `f32` disagreement past the parity
//! tolerance), so the encode is authored as a **cubic least-squares fit over
//! `m ∈ [0.5, 1.0]`** — [`SRGB_FIT`] — whose worst error over that whole domain
//! is 0.035 of one byte level, pinned by
//! [`tests::the_srgb_fit_is_within_a_twentieth_of_a_byte_of_the_real_encode`].
//! `[0.5, 1.0]` rather than the shipped `[0.86, 1.0]` band precisely so
//! `min_multiplier` stays a *free* parameter: the fit does not have to move when
//! the knob does.
//!
//! ## What the render actually shows, and the vocabulary limit behind it
//!
//! Captured at `burnt-rubber-straight`, tick 0, GPU backend, the field road and
//! the hand-written road differ by an RMS of **2.2 display levels** with a worst
//! pixel of **9**, over 22.5% of the frame — all of it tarmac. Both are within
//! the amplitude the module's own tests bound.
//!
//! They do not look identical, and the difference is *directional structure*:
//! the field road shows visibly stronger longitudinal streaking — the wheel-track
//! ripple `CROSS_SHARE` exists to produce — where the hand-written road is nearly
//! a smooth plane at the same depth. Measured on the column profile (every column
//! averaged down the tile) the field tile's worst column-to-column step is
//! **2.0 byte levels against the hand-written tile's 0.93**, at an identical
//! octave standard deviation.
//!
//! **Matching the standard deviation does not match the look, and the reason is a
//! limit of the vocabulary rather than a tuning mistake.** The hand-written
//! `cross_octave` is *value* noise: random values at integer band positions,
//! smoothstep-interpolated, so it has plateaus and gentle ramps.
//! `FieldOp::Noise` is *gradient* (Perlin) noise — pinned to exactly zero at
//! every integer lattice node, extremal mid-cell — so at equal variance it has a
//! higher local slope and reads as a more regular ripple. The algebra offers
//! **one** noise character and no way to author another: reproducing value noise
//! needs a floor or a fract, and there is no `Floor`, no `Fract`, no `Div` and no
//! integer arithmetic in the 23 operators.
//!
//! That is a genuine finding about the algebra's expressive range, and it is
//! reported rather than worked around. Whether the field road's stronger ripple
//! is better or worse than the hand-written road's near-flat plane is a judgement
//! about the game's look, not a test result — which is why
//! `asphalt_texture.rs` stays where it is until a human has judged the two.
//!
//! ## The cost, measured — and the engine defect it exposed
//!
//! The bake is **211 ms** against the hand-written generator's **2.3 ms** for
//! the same 128² tile (release, this machine). `PreparedTextures::generate`, the
//! whole texture half of the startup barrier, goes from 4.9 ms to 211 ms.
//!
//! That is not the graph being 90× more work than the loop. It is a defect in
//! the field layer's evaluator, and the measurement isolates it: evaluating this
//! graph up to node *k*, for k = 1, 8, 16, 32 and 57, costs a **flat ~135 ns per
//! node** — the same for node 1, a `Component` lane read, as for a `Noise`
//! sample. A lane read is a handful of instructions; 135 ns is a memory copy.
//!
//! `crates/axiom-field/src/eval.rs::evaluate` folds its `[FieldValue; MAX_NODES]`
//! register file **by value**:
//!
//! ```ignore
//! (0..=last).fold([FieldValue::ZERO; MAX_NODES], |mut registers, index| { … })
//! ```
//!
//! `MAX_NODES` is 256 and a `FieldValue` is 20 bytes, so the accumulator is
//! 5,120 bytes and the fold moves it once per node. The layer's
//! `ARCHITECTURE.md` claims the per-call cost is `O(nodes)` and that the
//! evaluator allocates nothing; the second is true and the first is not — in
//! memory traffic it is `O(nodes × MAX_NODES)`. 58 nodes × 5,120 bytes × 16,384
//! texels is 4.7 GB of `memcpy` for one tile, which is what the 211 ms is.
//!
//! **This is reported, not fixed here.** It is an engine change in a layer this
//! slice is not permitted to touch, and it is a small one — a `for_each` over a
//! `&mut` local register array instead of a by-value `fold` accumulator, which
//! stays branchless.
//!
//! ## Baked, not live
//!
//! The graph is evaluated once per texel at the startup barrier and uploaded as
//! an ordinary texture, exactly as the hand-written generator was. Asphalt is
//! the largest surface in any frame; making it a per-pixel surface program would
//! move its cost from a one-off 16,384 evaluations to the whole road's fill
//! rate, which is a different change with a different risk and is not what this
//! slice claims.

use axiom_field::{
    FieldBuilder, FieldGraph, FieldId, FieldOp, FieldParamSlot, FieldType, FieldValue, NodeId,
    Param, Scalar,
};
use axiom_proc_texture::{ProcTextureApi, TextureOp};
use axiom_recipe::{RecipeGraph, RecipeId};

use super::asphalt_texture::{
    CONTRAST, CROSS_BANDS, CROSS_SALT, FINE_SALT, LATTICE, MIN_MULTIPLIER, RES, SMOOTH_SALT,
    SMOOTH_SHARE,
};
use super::asphalt_texture::CROSS_SHARE;

/// The seed of the cross-road octave. It is `asphalt_texture`'s own salt for the
/// same octave, widened to the 64-bit seed `FieldOp::Noise` carries — the two
/// octaves are the same feature and there is no reason for them to disagree
/// about which stream they are drawn from.
const CROSS_SEED: u64 = CROSS_SALT as u64;

/// The seed of the smooth (isotropic lattice) octave. See [`CROSS_SEED`].
const SMOOTH_SEED: u64 = SMOOTH_SALT as u64;

/// The seed of the fine (texel-scale) octave. See [`CROSS_SEED`].
const FINE_SEED: u64 = FINE_SALT as u64;

/// Gain applied to the cross-road octave's `[-1, 1]` gradient noise before it is
/// re-centred on `0.5`, so the remapped octave carries the **same standard
/// deviation over the tile** as `asphalt_texture::cross_octave` does.
///
/// This is the whole of the basis conversion, and it is what lets
/// [`CROSS_SHARE`] keep its authored value and its authored *meaning*: a share
/// of the amplitude is only a share if the things being shared are the same
/// size. The number is measured, not guessed —
/// [`tests::each_octave_carries_the_amplitude_of_the_octave_it_replaces`] recomputes
/// the ratio from both implementations and fails if it has drifted.
const CROSS_GAIN: f32 = 1.1815;

/// Gain for the smooth octave. See [`CROSS_GAIN`].
const SMOOTH_GAIN: f32 = 0.8783;

/// Gain for the fine, texel-scale octave. See [`CROSS_GAIN`].
///
/// This one is the largest because it is standing in for a **full-width
/// uniform** — `hash_unit` is flat on `0..1` with a standard deviation of
/// `1/sqrt(12) = 0.2887` — where gradient noise sampled at cell centres is a
/// blend of four corner dot products and piles up near zero. That is the same
/// distinction `asphalt_texture::CONTRAST` is documented against.
const FINE_GAIN: f32 = 1.6784;

/// The sRGB encode `1.055 * m^(1/2.4) - 0.055`, as a cubic in `m` in ascending
/// coefficient order, least-squares fitted over `m ∈ [0.5, 1.0]`.
///
/// See the module docs for why a fit rather than the real transfer function.
/// Worst error over the whole fitted domain is `1.4e-4` linear = **0.035 of one
/// byte level**, which is below the rounding the bake performs anyway.
const SRGB_FIT: [f32; 4] = [
    0.309_718_17,
    1.088_208_6,
    -0.548_796_43,
    0.150_974_68,
];

/// The name of the parameter slot carrying `asphalt_texture::SMOOTH_SHARE`.
pub const PARAM_SMOOTH_SHARE: &str = "asphalt/smooth_share";
/// The name of the parameter slot carrying `asphalt_texture::CROSS_SHARE`.
pub const PARAM_CROSS_SHARE: &str = "asphalt/cross_share";
/// The name of the parameter slot carrying `asphalt_texture::CONTRAST`.
pub const PARAM_CONTRAST: &str = "asphalt/contrast";
/// The name of the parameter slot carrying `asphalt_texture::MIN_MULTIPLIER`.
pub const PARAM_MIN_MULTIPLIER: &str = "asphalt/min_multiplier";

/// The four knobs a caller may retune without moving the graph's digest.
///
/// A plain record rather than four positional arguments, because
/// [`asphalt_field_tuned`] is the demonstration that a *value* change is a
/// uniform write: naming the knobs at the call site is the point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AsphaltTuning {
    /// Share of the amplitude carried by the smooth octave.
    pub smooth_share: f32,
    /// Share of the amplitude carried by the cross-road octave.
    pub cross_share: f32,
    /// Contrast expansion about the field's midpoint.
    pub contrast: f32,
    /// The darkest linear multiplier a texel may apply.
    pub min_multiplier: f32,
}

impl AsphaltTuning {
    /// The shipped tuning — read straight off [`super::asphalt_texture`]'s own
    /// constants, so the two paths have exactly one set of numbers between them.
    pub const SHIPPED: AsphaltTuning = AsphaltTuning {
        smooth_share: SMOOTH_SHARE,
        cross_share: CROSS_SHARE,
        contrast: CONTRAST,
        min_multiplier: MIN_MULTIPLIER,
    };
}

/// The asphalt grain as an authored field graph at the shipped tuning.
pub fn asphalt_field() -> FieldGraph {
    asphalt_field_tuned(AsphaltTuning::SHIPPED)
}

/// The asphalt grain as an authored field graph at an arbitrary tuning.
///
/// **Every graph this returns has the identical `digest()`**, whatever the
/// tuning: the four knobs are parameter-table slots and `FieldGraph::digest`
/// folds each slot's declared *type* and not its value. That is the property a
/// program cache keys on, and it is why retuning a material cannot invalidate a
/// compiled shader.
pub fn asphalt_field_tuned(tuning: AsphaltTuning) -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("burnt-rubber/asphalt"), 1);

    // --- the four knobs, as slots -----------------------------------------
    let (builder, smooth_share) = declare(builder, PARAM_SMOOTH_SHARE, tuning.smooth_share);
    let (builder, cross_share) = declare(builder, PARAM_CROSS_SHARE, tuning.cross_share);
    let (builder, contrast) = declare(builder, PARAM_CONTRAST, tuning.contrast);
    let (builder, min_multiplier) = declare(builder, PARAM_MIN_MULTIPLIER, tuning.min_multiplier);

    // --- the domain --------------------------------------------------------
    let (builder, uv) = builder.push(FieldOp::Uv, Vec::new(), Vec::new());
    let (builder, u) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
    let (builder, v) = builder.push(FieldOp::Component, vec![Param::int(1)], vec![uv]);
    let (builder, zero) = konst(builder, 0.0);
    let (builder, one) = konst(builder, 1.0);
    let (builder, half) = konst(builder, 0.5);

    // --- the cross-road octave: anisotropic, x only ------------------------
    //
    // THE construction the manifest asked to be verified: the lateral
    // coordinate is scaled to the band count and composed into a Vec3 whose
    // other two lanes are held at zero, so the sampled field is a function of
    // `uv.x` alone and is constant down the course. That is what makes it the
    // one octave a 16x anisotropic sampler at a grazing angle cannot average
    // away — see `asphalt_texture::CROSS_SHARE`.
    let (builder, bands) = konst(builder, CROSS_BANDS as f32);
    let (builder, cross_x) = builder.push(FieldOp::Mul, Vec::new(), vec![u, bands]);
    let (builder, cross_p) = builder.push(
        FieldOp::Compose,
        vec![Param::int(3)],
        vec![cross_x, zero, zero],
    );
    let (builder, cross_n) = builder.push_noise(CROSS_SEED, cross_p);
    let (builder, cross01) = remap(builder, cross_n, CROSS_GAIN, half);

    // --- the smooth octave: isotropic, LATTICE cells across the tile -------
    let (builder, lattice) = konst(builder, LATTICE as f32);
    let (builder, smooth_x) = builder.push(FieldOp::Mul, Vec::new(), vec![u, lattice]);
    let (builder, smooth_y) = builder.push(FieldOp::Mul, Vec::new(), vec![v, lattice]);
    let (builder, smooth_p) = builder.push(
        FieldOp::Compose,
        vec![Param::int(3)],
        vec![smooth_x, smooth_y, zero],
    );
    let (builder, smooth_n) = builder.push_noise(SMOOTH_SEED, smooth_p);
    let (builder, smooth01) = remap(builder, smooth_n, SMOOTH_GAIN, half);

    // --- the fine octave: one cell per texel -------------------------------
    let (builder, texels) = konst(builder, RES as f32);
    let (builder, fine_x) = builder.push(FieldOp::Mul, Vec::new(), vec![u, texels]);
    let (builder, fine_y) = builder.push(FieldOp::Mul, Vec::new(), vec![v, texels]);
    let (builder, fine_p) = builder.push(
        FieldOp::Compose,
        vec![Param::int(3)],
        vec![fine_x, fine_y, zero],
    );
    let (builder, fine_n) = builder.push_noise(FINE_SEED, fine_p);
    let (builder, fine01) = remap(builder, fine_n, FINE_GAIN, half);

    // --- the weighted mix --------------------------------------------------
    let (builder, p_smooth) = builder.push_param(smooth_share, FieldType::Scalar);
    let (builder, p_cross) = builder.push_param(cross_share, FieldType::Scalar);
    let (builder, rest) = builder.push(FieldOp::Sub, Vec::new(), vec![one, p_smooth]);
    let (builder, p_fine) = builder.push(FieldOp::Sub, Vec::new(), vec![rest, p_cross]);
    let (builder, w_smooth) = builder.push(FieldOp::Mul, Vec::new(), vec![smooth01, p_smooth]);
    let (builder, w_cross) = builder.push(FieldOp::Mul, Vec::new(), vec![cross01, p_cross]);
    let (builder, w_fine) = builder.push(FieldOp::Mul, Vec::new(), vec![fine01, p_fine]);
    let (builder, sum2) = builder.push(FieldOp::Add, Vec::new(), vec![w_smooth, w_cross]);
    let (builder, mixed) = builder.push(FieldOp::Add, Vec::new(), vec![sum2, w_fine]);

    // --- contrast about the midpoint, clamped into 0..1 --------------------
    let (builder, p_contrast) = builder.push_param(contrast, FieldType::Scalar);
    let (builder, centred) = builder.push(FieldOp::Sub, Vec::new(), vec![mixed, half]);
    let (builder, gained) = builder.push(FieldOp::Mul, Vec::new(), vec![centred, p_contrast]);
    let (builder, expanded) = builder.push(FieldOp::Add, Vec::new(), vec![gained, half]);
    let (builder, value) = builder.push(FieldOp::Clamp, Vec::new(), vec![expanded, zero, one]);

    // --- the linear multiplier the grain applies to the tarmac colour ------
    let (builder, p_min) = builder.push_param(min_multiplier, FieldType::Scalar);
    let (builder, span) = builder.push(FieldOp::Sub, Vec::new(), vec![one, p_min]);
    let (builder, scaled) = builder.push(FieldOp::Mul, Vec::new(), vec![value, span]);
    let (builder, multiplier) = builder.push(FieldOp::Add, Vec::new(), vec![scaled, p_min]);

    // --- the sRGB encode, as a cubic in Horner form ------------------------
    let (builder, encoded) = srgb_encode(builder, multiplier);

    // --- neutral grey, opaque ---------------------------------------------
    let (builder, rgba) = builder.push(
        FieldOp::Compose,
        vec![Param::int(4)],
        vec![encoded, encoded, encoded, one],
    );
    builder.build(rgba)
}

/// The tiling asphalt albedo baked from [`asphalt_field`], as `RES * RES` RGBA8
/// texels ready for `RunningApp::add_texture_data`.
///
/// `None` when the bake fails, which for an authored-in-Rust graph is a defect
/// rather than a runtime condition — [`tests::the_shipped_graph_bakes_and_is_the_buffer_add_texture_data_accepts`] proves
/// the shipped graph never takes that arm, and the caller
/// (`preparation::textures`) keeps the hand-written generator as the fallback so
/// a defect here is a texture that looks slightly different, never a missing
/// road.
pub fn asphalt_field_albedo() -> Option<Vec<u8>> {
    asphalt_field_albedo_tuned(AsphaltTuning::SHIPPED)
}

/// [`asphalt_field_albedo`] at an arbitrary tuning.
pub fn asphalt_field_albedo_tuned(tuning: AsphaltTuning) -> Option<Vec<u8>> {
    let graph = asphalt_field_tuned(tuning);
    ProcTextureApi::new()
        .bake_with_fields(&bake_recipe(), 0, &[graph])
        .ok()
        .map(axiom_proc_texture::TextureBuffer::into_pixels)
}

/// The one-node bake recipe: a single `Field` source at the texture's own
/// resolution, naming table entry 0.
///
/// The graph travels *beside* the recipe rather than inside it — a
/// `axiom_recipe::Param` is one `u32` word, so inlining a 55-node graph's bytes
/// would spend a 256-node budget on one operator.
fn bake_recipe() -> RecipeGraph {
    let mut recipe = RecipeGraph::new(RecipeId::from_raw(0xA5B4_A17_u64), 1);
    recipe.add(
        TextureOp::Field as u16,
        vec![Param::int(RES), Param::int(RES), Param::int(0)],
        Vec::new(),
    );
    recipe
}

/// Declare a scalar parameter slot named `name` holding `value`.
fn declare(builder: FieldBuilder, name: &str, value: f32) -> (FieldBuilder, FieldParamSlot) {
    builder.declare(name, FieldValue::scalar(Scalar::new(value)))
}

/// Append a scalar `Const` node.
fn konst(builder: FieldBuilder, value: f32) -> (FieldBuilder, NodeId) {
    builder.push_const(FieldValue::scalar(Scalar::new(value)))
}

/// `noise * gain + half` — one octave's `[-1, 1]` signal remapped onto `0..1`
/// with the amplitude of the octave it replaces. See [`CROSS_GAIN`].
fn remap(
    builder: FieldBuilder,
    noise: NodeId,
    gain: f32,
    half: NodeId,
) -> (FieldBuilder, NodeId) {
    let (builder, g) = konst(builder, gain);
    let (builder, scaled) = builder.push(FieldOp::Mul, Vec::new(), vec![noise, g]);
    builder.push(FieldOp::Add, Vec::new(), vec![scaled, half])
}

/// The [`SRGB_FIT`] cubic evaluated in Horner form: three multiplies and three
/// adds, no `Pow` and no division.
fn srgb_encode(builder: FieldBuilder, m: NodeId) -> (FieldBuilder, NodeId) {
    let (builder, c3) = konst(builder, SRGB_FIT[3]);
    SRGB_FIT
        .iter()
        .rev()
        .skip(1)
        .fold((builder, c3), |(builder, acc), coefficient| {
            let (builder, product) = builder.push(FieldOp::Mul, Vec::new(), vec![acc, m]);
            let (builder, c) = konst(builder, *coefficient);
            builder.push(FieldOp::Add, Vec::new(), vec![product, c])
        })
}

#[cfg(test)]
mod tests {
    use super::super::asphalt_texture::{
        asphalt_albedo, cross_octave, hash_unit, smooth_octave, TILE_METRES,
    };
    use super::*;
    use axiom_field::EvalContext;
    use axiom_math::{Vec2, Vec3};

    /// The tarmac's linear base colour, green channel — the same mirror
    /// `asphalt_texture`'s own tests keep, for the same reason: every claim about
    /// how the grain *looks* is a claim about the value after it multiplies this.
    const TARMAC: f32 = 0.0910;

    fn decoded(byte: u8) -> f32 {
        let e = byte as f32 / 255.0;
        [e / 12.92, ((e + 0.055) / 1.055).powf(2.4)][usize::from(e > 0.040_45)]
    }

    fn displayed(linear: f32) -> f32 {
        255.0 * (1.055 * linear.powf(1.0 / 2.4) - 0.055)
    }

    /// Every texel's displayed tarmac level, row-major, for a given albedo.
    fn tarmac_levels(pixels: &[u8]) -> Vec<f32> {
        pixels
            .chunks(4)
            .map(|t| displayed(TARMAC * decoded(t[0])))
            .collect()
    }

    fn stats(v: &[f32]) -> (f32, f32) {
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
        (mean, sd)
    }

    fn baked() -> Vec<u8> {
        asphalt_field_albedo().expect("the shipped graph bakes")
    }

    /// The value of one node of the graph at one texel centre — the same
    /// sampling convention `TextureOp::Field` bakes on.
    fn at(graph: &FieldGraph, node: NodeId, x: u32, y: u32) -> f32 {
        let uv = Vec2::new((x as f32 + 0.5) / RES as f32, (y as f32 + 0.5) / RES as f32);
        graph
            .evaluate_at(
                &EvalContext::at(Vec3::new(uv.x, uv.y, 0.0), uv, Vec3::UNIT_Y),
                node,
            )
            .expect("every node of a validated graph evaluates")
            .as_vec4()
            .x
    }

    /// The three `Noise` nodes, in authoring order: cross, smooth, fine. Found by
    /// operator rather than by number, so inserting a node upstream does not
    /// silently repoint a test at a different octave.
    fn noise_nodes(graph: &FieldGraph) -> Vec<NodeId> {
        (0..graph.node_count() as u32)
            .map(NodeId::from_raw)
            .filter(|n| graph.op_at(*n) == Ok(FieldOp::Noise))
            .collect()
    }

    #[test]
    fn the_graph_validates_and_sits_well_inside_the_node_budget() {
        let graph = asphalt_field();
        assert_eq!(graph.validate(), Ok(()));
        assert_eq!(graph.node_count(), 58);
        assert!(graph.node_count() < axiom_field::MAX_NODES);
        // Three octaves, three `Noise` nodes, and the output is an opaque RGBA.
        assert_eq!(noise_nodes(&graph).len(), 3);
        assert_eq!(graph.type_at(graph.output()), Ok(FieldType::Vec4));
    }

    /// **The structural digest is a committed value.** It is what a program cache
    /// keys on, so a change to it is a change to the identity of the material —
    /// never something that should happen by accident.
    #[test]
    fn the_graph_digest_is_the_committed_value() {
        assert_eq!(asphalt_field().digest().raw(), 0xA1F7_7B72_373D_941E);
        // The canonical form's digest differs only because canonicalisation
        // sorts the operands of commuted nodes (`a + b` and `b + a` are one
        // node). Both are committed, because a program cache may legitimately
        // key on either and neither may move by accident.
        assert_eq!(
            asphalt_field()
                .canonicalize()
                .expect("a validated graph canonicalises")
                .digest()
                .raw(),
            0xE31F_68EB_778D_334E
        );
    }

    /// **Requirement 8, the half that matters at runtime.** Retuning any of the
    /// four knobs moves the pixels and leaves the digest exactly where it was —
    /// so on a live surface a retune is a uniform write, not a recompile.
    ///
    /// **Requirement 6 rides on the same assertion from the other side**: one
    /// changed constant produces different bytes, which is what moves the
    /// committed `agent_*_resources.bin` fingerprint.
    #[test]
    fn retuning_the_grain_moves_the_pixels_and_not_the_digest() {
        let shipped = asphalt_field();
        let stronger = asphalt_field_tuned(AsphaltTuning {
            min_multiplier: 0.62,
            ..AsphaltTuning::SHIPPED
        });
        assert_eq!(shipped.digest(), stronger.digest());
        // ...and the *bytes* of the graph do differ, because the slot values are
        // part of the serialized state even though they are outside the digest.
        assert_ne!(shipped.serialize(), stronger.serialize());

        let quiet = baked();
        let loud = asphalt_field_albedo_tuned(AsphaltTuning {
            min_multiplier: 0.62,
            ..AsphaltTuning::SHIPPED
        })
        .expect("the retuned graph bakes");
        assert_ne!(quiet, loud);
        // And it is a change in the direction the knob names: a lower floor is a
        // wider band, so the darkest texel gets darker.
        let darkest = |p: &[u8]| p.chunks(4).map(|t| t[0]).min().expect("a non-empty tile");
        assert!(darkest(&loud) < darkest(&quiet));
    }

    /// **The authored graph carries no waste.** Canonicalisation drops dead
    /// nodes, merges duplicated subexpressions and folds constant subtrees; if it
    /// removes nothing, the authoring did none of those things. (It still
    /// *rewrites* the graph — commuted operands are sorted — which is why the
    /// two digests above differ; what it does not do is shrink it.)
    #[test]
    fn the_authored_graph_carries_no_dead_or_duplicated_nodes() {
        let graph = asphalt_field();
        let canonical = graph.canonicalize().expect("a validated graph canonicalises");
        assert_eq!(canonical.node_count(), graph.node_count());
        // Canonicalisation is idempotent, so the committed canonical digest is a
        // fixed point and not a way-station.
        assert_eq!(
            canonical
                .canonicalize()
                .expect("a canonical graph canonicalises")
                .digest(),
            canonical.digest()
        );
    }

    /// **Requirement 8, the authoring half: two authoring orders canonicalise
    /// equal.** Written on a small representative graph rather than on all 58
    /// nodes, because the property is the field layer's and what an app needs to
    /// know is that it holds for the shapes an app actually writes: a commuted
    /// `Add`, a duplicated subexpression, and a dead branch.
    #[test]
    fn two_authoring_orders_of_the_same_field_canonicalise_equal() {
        let tidy = {
            let (b, uv) = FieldBuilder::new(FieldId::of_name("burnt-rubber/order"), 1).push(
                FieldOp::Uv,
                Vec::new(),
                Vec::new(),
            );
            let (b, u) = b.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
            let (b, v) = b.push(FieldOp::Component, vec![Param::int(1)], vec![uv]);
            let (b, sum) = b.push(FieldOp::Add, Vec::new(), vec![u, v]);
            b.build(sum)
        };
        let untidy = {
            let (b, uv) = FieldBuilder::new(FieldId::of_name("burnt-rubber/order"), 1).push(
                FieldOp::Uv,
                Vec::new(),
                Vec::new(),
            );
            let (b, u) = b.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
            let (b, v) = b.push(FieldOp::Component, vec![Param::int(1)], vec![uv]);
            // A duplicate of `u` (CSE), a dead branch (DCE), and the operands the
            // other way round (commuted-input sorting).
            let (b, _dup) = b.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
            let (b, _dead) = b.push(FieldOp::Length, Vec::new(), vec![uv]);
            let (b, sum) = b.push(FieldOp::Add, Vec::new(), vec![v, u]);
            b.build(sum)
        };
        assert_ne!(tidy.digest(), untidy.digest());
        assert_eq!(
            tidy.canonicalize().expect("valid").digest(),
            untidy.canonicalize().expect("valid").digest()
        );
    }

    /// **The manifest's first vocabulary question, answered by measurement.**
    ///
    /// The directional (`cross_octave`) term is anisotropic and x-only, and it is
    /// the octave that survives 16x anisotropic filtering at depth — the visually
    /// load-bearing one. It is expressed as `Noise` sampled at
    /// `Compose(Mul(Component(Uv, 0), CROSS_BANDS), Const(0), Const(0))`, and the
    /// claim that construction makes is exactly this: **the field is constant
    /// down the course and varies across it.**
    #[test]
    fn the_directional_octave_is_constant_down_the_course_and_varies_across_it() {
        let graph = asphalt_field();
        let cross = noise_nodes(&graph)[0];
        // Constant along v, bit-exactly: the composed point's y and z lanes are
        // `Const(0)`, so `uv.y` never reaches the sample.
        let down: Vec<f32> = (0..RES).map(|y| at(&graph, cross, 37, y)).collect();
        assert!(down.iter().all(|value| *value == down[0]));
        // ...and genuinely varies across, over the full range of a noise octave.
        let across: Vec<f32> = (0..RES).map(|x| at(&graph, cross, x, 0)).collect();
        let (_, sd) = stats(&across);
        assert!(sd > 0.15, "the cross-road octave is flat: sd {sd}");
    }

    /// **The manifest's second vocabulary question, answered by measurement, not
    /// by argument.**
    ///
    /// `asphalt_texture`'s octaves wrap *by construction* — cell indices are
    /// taken `% LATTICE`. `axiom_noise`'s gradient noise has no such wrap, so the
    /// honest question was whether a seam is visible, and the honest options were
    /// to accept it, to `Mix` two samples at offset domains, or to report that
    /// the algebra needs a domain-wrap facility.
    ///
    /// **It is accepted, and the reason is a property of the noise rather than
    /// luck.** Gradient noise is exactly zero at every integer lattice point, and
    /// `uv` in `[0, 1)` scaled by an *integer* band/cell count lands both tile
    /// edges within half a texel of the same lattice node. So the wrap
    /// discontinuity is bounded by how far the field moves in half a texel next
    /// to a node, which is the smallest step anywhere in the octave.
    ///
    /// Measured on the low-frequency structure — every column averaged down the
    /// whole tile, which strips the per-texel octave and leaves exactly what a
    /// seam would draw as a stripe down the road — the wrap step is **0.13 of a
    /// byte level against a worst interior column step of 2.0**. The hand-written
    /// toroidal tile measures 0.27 against 0.93 on the same statistic. The field
    /// tile's seam is *smaller in absolute terms* than the one the toroidal
    /// construction leaves.
    #[test]
    fn the_tile_wraps_without_a_seam_a_road_could_show() {
        let pixels = baked();
        let byte = |x: u32, y: u32| pixels[((y * RES + x) * 4) as usize] as f32;
        let column = |x: u32| (0..RES).map(|y| byte(x, y)).sum::<f32>() / RES as f32;
        let columns: Vec<f32> = (0..RES).map(column).collect();
        let step = |i: usize| (columns[i] - columns[(i + 1) % RES as usize]).abs();
        let wrap = step(RES as usize - 1);
        let interior = (0..RES as usize - 1).map(step).fold(0.0f32, f32::max);
        assert!(
            wrap < interior,
            "the tile wrap ({wrap:.3} levels) is a larger step than the worst \
             interior column step ({interior:.3}); it would draw as a stripe down \
             the road every {TILE_METRES} m"
        );

        let row = |y: u32| (0..RES).map(|x| byte(x, y)).sum::<f32>() / RES as f32;
        let rows: Vec<f32> = (0..RES).map(row).collect();
        let rstep = |i: usize| (rows[i] - rows[(i + 1) % RES as usize]).abs();
        let rwrap = rstep(RES as usize - 1);
        let rinterior = (0..RES as usize - 1).map(rstep).fold(0.0f32, f32::max);
        assert!(rwrap <= rinterior, "horizontal seam: {rwrap} vs {rinterior}");
    }

    /// **The basis conversion, recomputed from both implementations.**
    ///
    /// `CROSS_GAIN` / `SMOOTH_GAIN` / `FINE_GAIN` exist so each field octave
    /// carries the standard deviation of the hand-written octave it replaces —
    /// which is what lets `SMOOTH_SHARE`, `CROSS_SHARE` and `CONTRAST` keep both
    /// their authored values and their authored *meanings*. A share of the
    /// amplitude is only a share if the things being shared are the same size.
    #[test]
    fn each_octave_carries_the_amplitude_of_the_octave_it_replaces() {
        let graph = asphalt_field();
        let nodes = noise_nodes(&graph);
        let sampled = |node: NodeId| -> f32 {
            let v: Vec<f32> = (0..RES * RES)
                .map(|i| at(&graph, node, i % RES, i / RES))
                .collect();
            stats(&v).1
        };
        let source = |f: &dyn Fn(u32, u32) -> f32| -> f32 {
            let v: Vec<f32> = (0..RES * RES).map(|i| f(i % RES, i / RES)).collect();
            stats(&v).1
        };
        let pairs = [
            (
                sampled(nodes[0]) * CROSS_GAIN,
                source(&|x, _| cross_octave(x)),
                "cross",
                CROSS_GAIN,
            ),
            (
                sampled(nodes[1]) * SMOOTH_GAIN,
                source(&|x, y| smooth_octave(x, y)),
                "smooth",
                SMOOTH_GAIN,
            ),
            (
                sampled(nodes[2]) * FINE_GAIN,
                source(&|x, y| hash_unit(x, y, FINE_SALT)),
                "fine",
                FINE_GAIN,
            ),
        ];
        pairs.iter().for_each(|(field, hand, name, gain)| {
            let ratio = field / hand;
            assert!(
                (0.99..1.01).contains(&ratio),
                "the {name} octave's gain has drifted: field sd {field:.5} vs \
                 hand-written sd {hand:.5} (ratio {ratio:.4}); set the gain to \
                 {:.4}",
                gain / ratio,
            );
        });
    }

    /// **The sRGB transfer function, as a polynomial, because the algebra has no
    /// `Pow`.** Pinned over the whole fitted domain rather than at the shipped
    /// band, so `min_multiplier` stays a free parameter.
    #[test]
    fn the_srgb_fit_is_within_a_twentieth_of_a_byte_of_the_real_encode() {
        let real = |m: f32| 1.055 * m.powf(1.0 / 2.4) - 0.055;
        let fitted = |m: f32| {
            SRGB_FIT
                .iter()
                .rev()
                .fold(0.0f32, |acc, coefficient| acc * m + coefficient)
        };
        let worst = (0..=2000)
            .map(|i| 0.5 + 0.5 * i as f32 / 2000.0)
            .map(|m| (real(m) - fitted(m)).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst * 255.0 < 0.05,
            "the sRGB cubic is off by {:.4} byte levels",
            worst * 255.0
        );
    }

    #[test]
    fn the_shipped_graph_bakes_and_is_the_buffer_add_texture_data_accepts() {
        let pixels = baked();
        assert_eq!(pixels.len(), (RES * RES * 4) as usize);
        assert!(pixels.chunks(4).all(|t| t[3] == 255));
        assert!(pixels.chunks(4).all(|t| t[0] == t[1] && t[1] == t[2]));
    }

    #[test]
    fn the_bake_is_deterministic() {
        assert_eq!(baked(), baked());
    }

    /// **The equivalence test, and its stated tolerance.**
    ///
    /// The two paths are *not* texel-equal and cannot be: `hash_unit` is an
    /// integer bit-mixer and the field algebra has no integer operators (see the
    /// module docs). So the comparison that means something over the full 128x128
    /// tile is between the two tiles' **order statistics** — sort both byte
    /// arrays and compare element-wise. That is a genuine per-channel comparison
    /// of all 16,384 texels, and it is the right one for two samples of the same
    /// authored distribution.
    ///
    /// **Achieved tolerance: every quantile within 2 byte levels, means within 1
    /// byte level, standard deviations within 10%, and identical extremes.**
    #[test]
    fn the_field_tile_matches_the_hand_written_tile_within_tolerance() {
        let mut field: Vec<u8> = baked().chunks(4).map(|t| t[0]).collect();
        let mut hand: Vec<u8> = asphalt_albedo().chunks(4).map(|t| t[0]).collect();
        assert_eq!(field.len(), hand.len());
        field.sort_unstable();
        hand.sort_unstable();

        let worst_quantile = field
            .iter()
            .zip(hand.iter())
            .map(|(f, h)| (i32::from(*f) - i32::from(*h)).abs())
            .max()
            .expect("a non-empty tile");
        assert!(
            worst_quantile <= 2,
            "the two tiles' distributions differ by {worst_quantile} byte levels \
             at their worst quantile"
        );

        assert_eq!(
            (field[0], field[field.len() - 1]),
            (hand[0], hand[hand.len() - 1]),
            "the two tiles span different byte ranges"
        );

        let fs = stats(&field.iter().map(|b| f32::from(*b)).collect::<Vec<f32>>());
        let hs = stats(&hand.iter().map(|b| f32::from(*b)).collect::<Vec<f32>>());
        assert!(
            (fs.0 - hs.0).abs() <= 1.0,
            "mean byte moved from {:.3} to {:.3}",
            hs.0,
            fs.0
        );
        assert!(
            (fs.1 / hs.1 - 1.0).abs() <= 0.10,
            "byte standard deviation moved from {:.3} to {:.3}",
            hs.1,
            fs.1
        );
    }

    /// **The field tile is held to `asphalt_texture`'s own strength assertion.**
    /// Displayed spread as a fraction of displayed value, the exposure-invariant
    /// statistic the era-C reference was measured in: 1.6-2.3% on the reference,
    /// widened for byte quantisation.
    #[test]
    fn the_grain_is_as_strong_as_the_reference_asphalt_and_no_stronger() {
        let levels = tarmac_levels(&baked());
        let (mean, sd) = stats(&levels);
        let relative = sd / mean;
        assert!(
            (0.012..0.030).contains(&relative),
            "displayed variation is {:.2}% of the tarmac's value",
            relative * 100.0
        );
    }

    /// **The field tile is held to `asphalt_texture`'s magnified-step budget.**
    #[test]
    fn adjacent_texels_stay_inside_the_magnified_step_budget() {
        let levels = tarmac_levels(&baked());
        let level = |x: u32, y: u32| levels[(y * RES + x) as usize];
        let worst = (0..RES)
            .flat_map(|y| {
                (0..RES).map(move |x| {
                    (level(x, y) - level((x + 1) % RES, y))
                        .abs()
                        .max((level(x, y) - level(x, (y + 1) % RES)).abs())
                })
            })
            .fold(0.0f32, f32::max);
        assert!(worst <= 8.0, "adjacent texels differ by {worst:.1} levels");
    }

    /// **The field tile is held to `asphalt_texture`'s anisotropic-survival
    /// assertion** — the one the whole cross-road octave exists to satisfy, and
    /// the one that proves the directional term was expressed correctly. If the
    /// `Compose(Mul(Component(Uv, 0), k), 0, 0)` construction were wrong in any
    /// way that let `uv.y` in, this is the test that would catch it.
    #[test]
    fn the_grain_survives_the_anisotropic_filter_it_is_sampled_with() {
        let owned = tarmac_levels(&baked());
        let levels: &[f32] = &owned;
        [8_u32, 16, 32, 64].iter().for_each(|taps| {
            let filtered: Vec<f32> = (0..RES / taps)
                .flat_map(|band| {
                    (0..RES).map(move |x| {
                        (0..*taps)
                            .map(|j| levels[((band * taps + j) * RES + x) as usize])
                            .sum::<f32>()
                            / *taps as f32
                    })
                })
                .collect();
            let (mean, sd) = stats(&filtered);
            let relative = sd / mean;
            assert!(
                (0.012..0.030).contains(&relative),
                "after averaging {taps} texels along the road the tarmac varies \
                 by {:.2}% of its own value",
                relative * 100.0
            );
        });
    }

    /// **The field tile is held to `asphalt_texture`'s spectrum assertion** — the
    /// grain lives at texel scale, not at cell scale, or the near road renders as
    /// embossed leather rather than as aggregate.
    #[test]
    fn most_of_the_grain_lives_at_texel_scale_not_at_cell_scale() {
        let levels = tarmac_levels(&baked());
        let per_cell = (RES / LATTICE) as usize;
        let cells_across = RES as usize / per_cell;
        let cell = |cx: usize, cy: usize| {
            let sum: f32 = (0..per_cell)
                .flat_map(|j| {
                    (0..per_cell)
                        .map(move |i| ((cy * per_cell + j) * RES as usize) + cx * per_cell + i)
                })
                .map(|idx| levels[idx])
                .sum();
            sum / (per_cell * per_cell) as f32
        };
        let along: Vec<f32> = (0..cells_across)
            .map(|cx| {
                let column: Vec<f32> = (0..cells_across).map(|cy| cell(cx, cy)).collect();
                stats(&column).1
            })
            .collect();
        let rms = (along.iter().map(|s| s * s).sum::<f32>() / along.len() as f32).sqrt();
        let share = rms / stats(&levels).1;
        assert!(
            share < 0.45,
            "{:.0}% of the grain's amplitude is cell-scale blobs wobbling down \
             the course",
            share * 100.0
        );
    }
}
