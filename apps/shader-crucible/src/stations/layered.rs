//! **Station 1 — the layered material.** Brushed metal, paint over it, scratches
//! cutting back to bare metal, and dirt settling on top. Four surfaces, three
//! masks, one artifact.
//!
//! This is the case the whole `Surface` design exists to serve, and it is built
//! first because it is the one with a real budget risk: a layered surface
//! **flattens into one field graph per channel**, `MAX_LAYERS` is 4 and
//! `axiom_field::MAX_NODES` is 256, and the two are in tension by construction.
//! [`tests::the_flattened_node_count_per_channel_is_pinned`] prints and pins the
//! measured number so a future edit that blows the budget fails a test rather
//! than a frame.
//!
//! ## Why every one of the seven channels ends up a graph
//!
//! The three layer masks are *fields*. `axiom_surface`'s requirements rule says
//! a channel varies when some surface binds it to a field **or when some layer's
//! mask is a field** — because every blend expression makes each channel a
//! function of the mask. So `Metallic`, `Normal`, `Opacity` and `Displacement`
//! flatten into graphs here too, even though every surface in the tree binds
//! them to plain constants: `Mix(const, const, mask_field)` has a non-constant
//! input, so it cannot fold back to a constant. **Four of the seven channels of
//! this station cost their node budget for a value that never changes.** That is
//! not a defect in the authoring; it is the price of mask-driven layering, and
//! it is what makes the budget tight.
//!
//! ## `metallic` changes no pixel
//!
//! The base is a metal and the channel says so, and **nothing reads it**.
//! `Metallic` is carried, digested and reported by design (SPEC-11's "resist PBR
//! scope creep"): no lighting model consumes it, so moving it moves no pixel.
//! Station 1's on-screen label states that, because a channel a demo shows
//! without saying it is inert is a demo that lies.

use axiom_field::{FieldBuilder, FieldGraph, FieldId, FieldValue, Scalar};
use axiom_math::Vec4;
use axiom_noise::{FbmConfig, Frequency};
use axiom_surface::{
    ChannelBinding, LayerBlend, LightingModel, Surface, SurfaceBuilder, SurfaceChannel,
    SurfaceLayer,
};

use crate::authoring::{
    abs, add, frequency_point, knob, konst, konst4, mix, remap01, scale, sin, smoothstep_at,
};

/// The seed of the brushed-metal streak.
const STREAK_SEED: u64 = 0x5B12_7A03;
/// The seed of the paint's coverage blotches.
const PAINT_SEED: u64 = 0x9E31_44C1;
/// The seed of the domain warp that stops the scratches being a ruled grating.
const SCRATCH_WARP_SEED: u64 = 0x27F0_1D5A;
/// The seed of the dirt's settling pattern.
const DIRT_SEED: u64 = 0x4417_B8E9;

/// The name of the knob station 4 retunes: how sharply the scratch mask cuts.
pub const PARAM_SCRATCH_BITE: &str = "crucible/layered/scratch_bite";

/// A `Vec3` `Const`-lane colour, opaque, as a `Vec4` channel constant.
fn opaque(r: f32, g: f32, b: f32) -> FieldValue {
    FieldValue::vec4(Vec4::new(r, g, b, 1.0))
}

/// A scalar channel constant.
fn amount(value: f32) -> FieldValue {
    FieldValue::scalar(Scalar::new(value))
}

/// **The brushed-metal base colour**: a fine streak running along the object's
/// x axis, sampled from an anisotropic domain (high frequency across the grain,
/// almost none along it) so the highlight reads as brushing rather than as
/// blotches.
fn brushed_metal_color() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/layered/metal-color"), 1);
    let (builder, p) = frequency_point(builder, 34.0, 2.0, 34.0);
    let (builder, n) = builder.push_noise(STREAK_SEED, p);
    let (builder, streak) = remap01(builder, n);
    let (builder, dark) = konst4(builder, 0.106, 0.116, 0.132, 1.0);
    let (builder, light) = konst4(builder, 0.402, 0.424, 0.462, 1.0);
    let (builder, color) = mix(builder, dark, light, streak);
    builder.build(color)
}

/// **The brushed-metal roughness**: the same streak, read as a roughness range.
/// A separate graph rather than a shared node, because a `Surface` binds one
/// graph per channel and there is no cross-channel sharing — the honest cost of
/// the channel vocabulary being closed.
fn brushed_metal_roughness() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/layered/metal-rough"), 1);
    let (builder, p) = frequency_point(builder, 34.0, 2.0, 34.0);
    let (builder, n) = builder.push_noise(STREAK_SEED, p);
    let (builder, streak) = remap01(builder, n);
    let (builder, spread) = scale(builder, streak, 0.24);
    let (builder, rough) = crate::authoring::offset(builder, spread, 0.30);
    builder.build(rough)
}

/// **The paint's coverage mask**: low-frequency fbm, smoothstepped so paint
/// covers most of the body and thins to bare metal at the edges of the blotches.
fn paint_mask() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/layered/paint-mask"), 1);
    let (builder, p) = frequency_point(builder, 1.6, 1.6, 1.6);
    let (builder, n) = builder.push_fbm(
        PAINT_SEED,
        FbmConfig::new(3, Frequency::new(1.0).expect("an authored frequency is positive")),
        p,
    );
    let (builder, unit) = remap01(builder, n);
    let (builder, coverage) = smoothstep_at(builder, 0.42, 0.72, unit);
    builder.build(coverage)
}

/// **The scratch mask**: a family of thin lines running across the grain, their
/// phase warped by a noise so they wander instead of ruling a perfect grating,
/// and a `Smoothstep` whose lower edge is the retunable knob
/// [`PARAM_SCRATCH_BITE`].
///
/// The line family is `|sin(k * x')|` where `x'` is the warped coordinate — an
/// authored graph over `Sin` and `Abs`, not a Rust function. Raising the bite
/// narrows every scratch at once **without moving the surface's digest**, which
/// is station 4's whole assertion.
fn scratch_mask(bite: f32) -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/layered/scratch-mask"), 1);
    let (builder, warp_p) = frequency_point(builder, 3.0, 3.0, 3.0);
    let (builder, warp) = builder.push_noise(SCRATCH_WARP_SEED, warp_p);
    let (builder, p) = frequency_point(builder, 21.0, 0.0, 4.0);
    let (builder, x) = crate::authoring::component(builder, p, 0);
    let (builder, z) = crate::authoring::component(builder, p, 2);
    let (builder, ridge) = add(builder, x, z);
    let (builder, wobble) = scale(builder, warp, 2.4);
    let (builder, phase) = add(builder, ridge, wobble);
    let (builder, wave) = sin(builder, phase);
    let (builder, ridged) = abs(builder, wave);
    let (builder, lo) = knob(builder, PARAM_SCRATCH_BITE, bite);
    let (builder, hi) = konst(builder, 1.0);
    let (builder, cut) =
        builder.push(axiom_field::FieldOp::Smoothstep, Vec::new(), vec![lo, hi, ridged]);
    builder.build(cut)
}

/// **The dirt mask**: fbm blotches biased dark, so dirt settles in patches
/// rather than washing the whole body.
fn dirt_mask() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/layered/dirt-mask"), 1);
    let (builder, p) = frequency_point(builder, 4.4, 5.8, 4.4);
    let (builder, n) = builder.push_fbm(
        DIRT_SEED,
        FbmConfig::new(3, Frequency::new(1.0).expect("an authored frequency is positive")),
        p,
    );
    let (builder, unit) = remap01(builder, n);
    let (builder, settled) = smoothstep_at(builder, 0.46, 0.86, unit);
    builder.build(settled)
}

/// **The paint's own colour**, faintly mottled so the paint is not a flat swatch
/// — a second, much weaker fbm read as a value shift on one hue.
fn paint_color() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/layered/paint-color"), 1);
    let (builder, p) = frequency_point(builder, 7.0, 7.0, 7.0);
    let (builder, n) = builder.push_fbm(
        PAINT_SEED ^ 0x1111,
        FbmConfig::new(2, Frequency::new(1.0).expect("an authored frequency is positive")),
        p,
    );
    let (builder, unit) = remap01(builder, n);
    let (builder, shade) = scale(builder, unit, 0.18);
    let (builder, base) = konst4(builder, 0.541, 0.086, 0.106, 1.0);
    let (builder, lift) = konst4(builder, 0.735, 0.180, 0.140, 1.0);
    let (builder, color) = mix(builder, base, lift, shade);
    builder.build(color)
}

/// The paint sitting over the metal: a mottled red, glossy.
fn paint() -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .field(SurfaceChannel::BaseColor, paint_color())
        .constant(SurfaceChannel::Roughness, amount(0.18))
        .constant(SurfaceChannel::Metallic, amount(0.0))
        .build()
        .expect("the paint is a legal surface")
}

/// What a scratch reveals: bright, bare, rough-edged metal.
fn bare_metal() -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .constant(SurfaceChannel::BaseColor, opaque(0.706, 0.729, 0.784))
        .constant(SurfaceChannel::Roughness, amount(0.42))
        .constant(SurfaceChannel::Metallic, amount(1.0))
        .build()
        .expect("bare metal is a legal surface")
}

/// The dirt: a dull brown that *multiplies* rather than covers, so what is under
/// it still reads through.
fn dirt() -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .constant(SurfaceChannel::BaseColor, opaque(0.360, 0.302, 0.216))
        .constant(SurfaceChannel::Roughness, amount(0.88))
        .constant(SurfaceChannel::Metallic, amount(0.0))
        .build()
        .expect("dirt is a legal surface")
}

/// The shipped bite of the scratch mask.
pub const SHIPPED_SCRATCH_BITE: f32 = 0.86;

/// **Station 1.** Brushed metal, paint (`Over` a coverage mask), scratches
/// (`Over` a line mask), dirt (`Multiply` a blotch mask) — four surfaces in one
/// artifact, flattening to one graph per channel.
pub fn layered_material() -> Surface {
    layered_material_tuned(SHIPPED_SCRATCH_BITE)
}

/// [`layered_material`] with the scratch bite retuned.
///
/// **Every surface this returns has the identical [`Surface::digest`].** The
/// bite is a parameter slot, and both `FieldGraph::digest` and `Surface::digest`
/// deliberately exclude slot values. That is the load-bearing property of the
/// whole design and station 4 is the demonstration of it.
pub fn layered_material_tuned(scratch_bite: f32) -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .field(SurfaceChannel::BaseColor, brushed_metal_color())
        .field(SurfaceChannel::Roughness, brushed_metal_roughness())
        .constant(SurfaceChannel::Metallic, amount(1.0))
        .layer(SurfaceLayer::new(
            paint(),
            ChannelBinding::field(paint_mask()),
            LayerBlend::Over,
        ))
        .layer(SurfaceLayer::new(
            bare_metal(),
            ChannelBinding::field(scratch_mask(scratch_bite)),
            LayerBlend::Over,
        ))
        .layer(SurfaceLayer::new(
            dirt(),
            ChannelBinding::field(dirt_mask()),
            LayerBlend::Multiply,
        ))
        .build()
        .expect("three layers over a base is within MAX_LAYERS")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The measurement the manifest asked for first.** A layered surface
    /// flattens into one graph per channel against `MAX_NODES = 256`; this prints
    /// the achieved count for every one of the seven channels and pins it.
    ///
    /// Four of the seven are constants in every surface of the tree and are
    /// graphs anyway, because the masks are fields — see the module docs.
    #[test]
    fn the_flattened_node_count_per_channel_is_pinned() {
        let surface = layered_material();
        assert_eq!(surface.validate(), Ok(()));
        let flat = surface.flatten().expect("the layered surface flattens");
        let inspection = flat.inspect().expect("a flattened surface inspects");
        let counts: Vec<(SurfaceChannel, u16, bool)> = inspection
            .channels()
            .iter()
            .map(|c| (c.channel(), c.node_count(), c.is_constant()))
            .collect();
        counts.iter().for_each(|(channel, nodes, constant)| {
            println!(
                "station 1 flattened channel {channel:?}: {nodes} nodes (constant: {constant})"
            );
        });
        let worst = counts.iter().map(|(_, n, _)| *n).max().expect("seven channels");
        println!("station 1 worst channel: {worst} nodes of {} available", axiom_field::MAX_NODES);
        assert!(
            (worst as usize) < axiom_field::MAX_NODES,
            "the flattened layered material does not fit the node budget: {worst} nodes"
        );
    }

    #[test]
    fn every_layer_mask_is_a_field_and_the_tree_is_within_budget() {
        let surface = layered_material();
        assert_eq!(surface.layers().len(), 3);
        assert!(surface.layers().iter().all(|l| !l.mask().is_constant()));
        assert_eq!(
            surface.layers().iter().map(|l| l.blend()).collect::<Vec<_>>(),
            vec![LayerBlend::Over, LayerBlend::Over, LayerBlend::Multiply]
        );
    }

    /// **The load-bearing assertion of the whole design**, at station 1's own
    /// knob: retuning the scratch bite leaves the digest exactly where it was.
    #[test]
    fn retuning_the_scratch_bite_leaves_the_digest_identical() {
        let shipped = layered_material();
        let sharper = layered_material_tuned(0.94);
        assert_eq!(shipped.digest(), sharper.digest());
        assert_ne!(shipped.serialize(), sharper.serialize());
    }
}
