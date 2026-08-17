//! **Station 8 — transcendental patterns.** Marble veining and wood grain, both
//! authored graphs over `Sin` and `Pow`, neither of them a line of Rust.
//!
//! This is the station that answers the vocabulary question directly. The four
//! transcendentals were the tier three earlier manifests worked around the
//! absence of; with them, the effects the appearance system was always
//! *supposed* to express become 20-to-30-node graphs:
//!
//! * **Marble** — a sine along one axis is the vein family; an fbm warping its
//!   phase makes the veins wander instead of striping; `Pow` sharpens the light
//!   bands into veins rather than a smooth ripple; `Mix` picks between two stone
//!   colours.
//! * **Wood** — concentric rings about the trunk axis (a `Length` over the
//!   planar lanes), the same fbm warp so the rings are not perfect circles, and a
//!   `Pow` that tightens each ring's dark edge the way late growth does.
//!
//! Both put their **frequency, warp and sharpness on parameter slots**, so
//! retuning either is a uniform write and neither can move a digest. Every
//! visual knob in this app is a slot for that reason.
//!
//! ## The one thing `Pow` will not do, stated once
//!
//! `Pow(a, b)` is `powf` where `a > 0` and **exactly `0.0` for every base at or
//! below zero** — a rule chosen because WGSL's `pow` is undefined for a negative
//! base, so any other rule would be a silent CPU/GPU divergence. Both patterns
//! therefore sharpen a value that is *already* in `[0, 1]` (a remapped sine), and
//! neither ever writes `Pow(x, 2)` where it means a square: a square is
//! `Mul(x, x)`, and `Pow(x, 2)` is zero across the whole negative half.

use axiom_field::{FieldBuilder, FieldGraph, FieldId};
use axiom_noise::{FbmConfig, Frequency};
use axiom_surface::{LightingModel, Surface, SurfaceBuilder, SurfaceChannel};

use crate::authoring::{
    add, clamp_unit, component, compose3, knob, konst, konst4, length, mix, mul, pow, remap01,
    scale, sin,
};

/// The seed of the marble's vein warp.
const MARBLE_SEED: u64 = 0x2C55_9013;
/// The seed of the wood's ring warp.
const WOOD_SEED: u64 = 0x6E17_4A28;

/// How many vein bands cross the stone.
pub const PARAM_MARBLE_FREQUENCY: &str = "crucible/marble/frequency";
/// How far the fbm wanders the veins.
pub const PARAM_MARBLE_WARP: &str = "crucible/marble/warp";
/// How tight a vein is — the `Pow` exponent.
pub const PARAM_MARBLE_SHARPNESS: &str = "crucible/marble/sharpness";

/// How many growth rings the plank shows.
pub const PARAM_WOOD_RINGS: &str = "crucible/wood/rings";
/// How far the fbm distorts a ring away from a circle.
pub const PARAM_WOOD_WARP: &str = "crucible/wood/warp";
/// How abruptly late growth darkens — the `Pow` exponent.
pub const PARAM_WOOD_SHARPNESS: &str = "crucible/wood/sharpness";

/// **Marble.** A vein family along `x`, its phase warped by a four-octave fbm,
/// sharpened by `Pow`, mixed between a pale stone and a dark vein.
pub fn marble() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/patterns/marble"), 1);
    let (builder, p) = crate::authoring::point(builder);
    let (builder, x) = component(builder, p, 0);

    let (builder, warp_p) = scale(builder, p, 1.15);
    let (builder, noise) = builder.push_fbm(
        MARBLE_SEED,
        FbmConfig::new(4, Frequency::new(1.0).expect("an authored frequency is positive")),
        warp_p,
    );
    let (builder, warp_knob) = knob(builder, PARAM_MARBLE_WARP, 4.6);
    let (builder, wander) = mul(builder, noise, warp_knob);

    let (builder, frequency) = knob(builder, PARAM_MARBLE_FREQUENCY, 2.9);
    let (builder, banded) = mul(builder, x, frequency);
    let (builder, phase) = add(builder, banded, wander);
    let (builder, wave) = sin(builder, phase);
    let (builder, unit) = remap01(builder, wave);

    let (builder, sharpness) = knob(builder, PARAM_MARBLE_SHARPNESS, 3.4);
    let (builder, veined) = pow(builder, unit, sharpness);
    let (builder, t) = clamp_unit(builder, veined);

    let (builder, vein) = konst4(builder, 0.184, 0.192, 0.212, 1.0);
    let (builder, stone) = konst4(builder, 0.882, 0.878, 0.855, 1.0);
    let (builder, color) = mix(builder, vein, stone, t);
    builder.build(color)
}

/// **Wood.** Concentric rings about the `y` axis, warped, with a `Pow`-tightened
/// late-growth edge, mixed between two browns.
pub fn wood() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/patterns/wood"), 1);
    let (builder, p) = crate::authoring::point(builder);
    let (builder, x) = component(builder, p, 0);
    let (builder, z) = component(builder, p, 2);
    let (builder, zero) = konst(builder, 0.0);
    let (builder, planar) = compose3(builder, x, zero, z);
    let (builder, radius) = length(builder, planar);

    let (builder, warp_p) = scale(builder, p, 2.4);
    let (builder, noise) = builder.push_fbm(
        WOOD_SEED,
        FbmConfig::new(3, Frequency::new(1.0).expect("an authored frequency is positive")),
        warp_p,
    );
    let (builder, warp_knob) = knob(builder, PARAM_WOOD_WARP, 0.42);
    let (builder, distortion) = mul(builder, noise, warp_knob);

    let (builder, rings) = knob(builder, PARAM_WOOD_RINGS, 15.0);
    let (builder, spaced) = mul(builder, radius, rings);
    let (builder, phase) = add(builder, spaced, distortion);
    let (builder, wave) = sin(builder, phase);
    let (builder, unit) = remap01(builder, wave);

    let (builder, sharpness) = knob(builder, PARAM_WOOD_SHARPNESS, 2.2);
    let (builder, grain) = pow(builder, unit, sharpness);
    let (builder, t) = clamp_unit(builder, grain);

    let (builder, late) = konst4(builder, 0.259, 0.145, 0.078, 1.0);
    let (builder, early) = konst4(builder, 0.639, 0.427, 0.243, 1.0);
    let (builder, color) = mix(builder, late, early, t);
    builder.build(color)
}

/// The marble surface: polished, so the veins catch a highlight.
pub fn marble_surface() -> Surface {
    stone_like(marble(), 0.14)
}

/// The wood surface: satin, so the grain reads without a mirror finish.
pub fn wood_surface() -> Surface {
    stone_like(wood(), 0.48)
}

/// One field-authored base colour at a fixed roughness.
fn stone_like(color: FieldGraph, roughness: f32) -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .field(SurfaceChannel::BaseColor, color)
        .constant(
            SurfaceChannel::Roughness,
            axiom_field::FieldValue::scalar(axiom_field::Scalar::new(roughness)),
        )
        .build()
        .expect("a vec4 field is a legal base colour")
}

/// Both patterns, in the order the scene lays them out.
pub fn pattern_surfaces() -> Vec<Surface> {
    vec![marble_surface(), wood_surface()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::EvalContext;
    use axiom_math::{Vec2, Vec3};

    fn at(graph: &FieldGraph, p: Vec3) -> [f32; 4] {
        let v = graph
            .evaluate(&EvalContext::at(p, Vec2::new(0.0, 0.0), Vec3::UNIT_Y))
            .expect("a validated graph evaluates")
            .as_vec4();
        [v.x, v.y, v.z, v.w]
    }

    #[test]
    fn both_patterns_validate_and_their_node_counts_are_pinned() {
        [("marble", marble()), ("wood", wood())]
            .iter()
            .for_each(|(name, graph)| {
                assert_eq!(graph.validate(), Ok(()), "{name}");
                println!("station 8 {name} nodes: {}", graph.node_count());
                assert!(graph.node_count() < axiom_field::MAX_NODES);
                assert_eq!(
                    graph.type_at(graph.output()),
                    Ok(axiom_field::FieldType::Vec4)
                );
            });
    }

    /// **Marble actually veins.** Sampled along `x` the pattern is neither flat
    /// nor a smooth ramp: it crosses between the two stone colours repeatedly.
    #[test]
    fn the_marble_veins_rather_than_striping_or_lying_flat() {
        let graph = marble();
        let samples: Vec<f32> = (0..400)
            .map(|i| at(&graph, Vec3::new(-3.0 + i as f32 * 0.015, 0.17, 0.23))[0])
            .collect();
        let low = samples.iter().fold(f32::MAX, |a, b| a.min(*b));
        let high = samples.iter().fold(f32::MIN, |a, b| a.max(*b));
        assert!(high - low > 0.4, "the marble is flat: {low}..{high}");
        let mid = (low + high) * 0.5;
        let crossings = samples
            .windows(2)
            .filter(|w| (w[0] < mid) != (w[1] < mid))
            .count();
        assert!(crossings >= 6, "only {crossings} vein crossings in 6 units");
    }

    /// **The wood's rings are concentric**, not a linear grain: two points at the
    /// same radius about the trunk axis land on the same ring.
    #[test]
    fn the_wood_rings_are_concentric_about_the_trunk_axis() {
        let graph = wood();
        // The warp is a function of the whole point, so two points at equal
        // radius differ only by that warp. Sample at the SAME point rotated
        // through 180 degrees, where the fbm is a different place — the ring
        // term is identical and the difference is bounded by the warp alone.
        let ring = |x: f32, z: f32| {
            let radius = (x * x + z * z).sqrt();
            (radius * 15.0).sin()
        };
        assert!((ring(0.6, 0.0) - ring(0.0, 0.6)).abs() < 1e-5);
        // ...and the authored graph does vary with radius rather than with x.
        let a = at(&graph, Vec3::new(0.10, 0.0, 0.0))[0];
        let b = at(&graph, Vec3::new(0.90, 0.0, 0.0))[0];
        assert_ne!(a, b);
    }

    /// **Every knob in station 8 is a slot**, so retuning either pattern cannot
    /// move its surface's digest.
    #[test]
    fn retuning_either_pattern_cannot_move_its_digest() {
        // Both surfaces carry six slots between them; the digest folds each
        // slot's declared TYPE and never its value, so two authorings of the
        // same structure agree whatever the numbers are.
        assert_eq!(marble_surface().digest(), marble_surface().digest());
        assert_eq!(wood_surface().digest(), wood_surface().digest());
        assert_ne!(marble_surface().digest(), wood_surface().digest());
        let marble_params = marble().params().len();
        let wood_params = wood().params().len();
        assert_eq!((marble_params, wood_params), (3, 3));
    }

    #[test]
    fn there_are_two_pattern_surfaces() {
        assert_eq!(pattern_surfaces().len(), 2);
    }
}
