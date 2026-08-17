//! **Station 5 — time-varying displacement**, and the first of the four
//! limitations this app is required not to hide.
//!
//! Two authored `Vec3` displacement graphs over `Point` and `Time`: a **wind**
//! that leans a body along `+x` with an amplitude that grows with height, and a
//! **ripple** that travels outward as a raised ring. Both are vertex-stage
//! fields — the one channel a vertex stage reads — and both read the frame's
//! **engine** clock, never a wall clock, so tick *N* replayed twice deforms
//! identically and tick *N* and tick *N + 60* differ.
//!
//! ## Limitation 1: a displaced vertex casts an undisplaced shadow
//!
//! The shadow depth pre-pass is a **separate WGSL module**. It runs no
//! `axiom_displace`, so the depth it writes is the depth of the *undeformed*
//! mesh. A body leaning hard in the wind therefore casts a shadow that is still
//! standing upright, and the further the displacement pushes a vertex the further
//! its shadow is from where it should be.
//!
//! This station is deliberately lit by a directional light at a low angle onto a
//! bright ground plane, and its amplitude is deliberately large, **so that the
//! discrepancy is visible in the frame rather than argued about in a comment**.
//! Its on-screen label states it. It is not fixed here: fixing it means teaching
//! the shadow pass to run the vertex program, which is an engine change and out
//! of scope for a demonstration app.

use axiom_field::{FieldBuilder, FieldGraph, FieldId};
use axiom_math::Vec4;
use axiom_surface::{LightingModel, Surface, SurfaceBuilder, SurfaceChannel};

use crate::authoring::{
    add, clamp, component, compose3, konst, konst4, length, mix, mul, remap01, scale, sin, sub,
    time,
};

/// How far, in object units, the wind leans the top of a body.
pub const WIND_AMPLITUDE: f32 = 0.42;
/// How far, in object units, the ripple lifts a crest.
pub const RIPPLE_AMPLITUDE: f32 = 0.30;

/// **Wind.** `x` is displaced by `sin(time * rate + y * lean) * amplitude *
/// height`, where `height` is the object-space `y` clamped into `0..=1` — so the
/// base of the body stays planted and the top swings. `y` and `z` are untouched.
///
/// The `height` weighting is what makes the shadow discrepancy legible: the
/// vertices that move most are the ones furthest from the ground, so the gap
/// between a body and its shadow opens with height.
pub fn wind_displacement() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/displace/wind"), 1);
    let (builder, p) = crate::authoring::point(builder);
    let (builder, height_raw) = component(builder, p, 1);
    let (builder, zero) = konst(builder, 0.0);
    let (builder, one) = konst(builder, 1.0);
    let (builder, height) = clamp(builder, height_raw, zero, one);

    let (builder, clock) = time(builder);
    let (builder, rate) = scale(builder, clock, 1.9);
    let (builder, lean) = scale(builder, height_raw, 1.35);
    let (builder, phase) = add(builder, rate, lean);
    let (builder, wave) = sin(builder, phase);

    let (builder, amplitude) = scale(builder, wave, WIND_AMPLITUDE);
    let (builder, dx) = mul(builder, amplitude, height);
    let (builder, offset) = compose3(builder, dx, zero, zero);
    builder.build(offset)
}

/// **Ripple.** A ring travelling outward across the object's `xz` plane:
/// `y` is displaced by `sin(radius * 7 - time * 3.4) * amplitude`, tapered to
/// zero past the body's own radius so the edge does not tear.
pub fn ripple_displacement() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/displace/ripple"), 1);
    let (builder, p) = crate::authoring::point(builder);
    let (builder, x) = component(builder, p, 0);
    let (builder, z) = component(builder, p, 2);
    let (builder, zero) = konst(builder, 0.0);
    let (builder, planar) = compose3(builder, x, zero, z);
    let (builder, radius) = length(builder, planar);

    let (builder, clock) = time(builder);
    let (builder, travel) = scale(builder, clock, 3.4);
    let (builder, spatial) = scale(builder, radius, 7.0);
    let (builder, phase) = sub(builder, spatial, travel);
    let (builder, wave) = sin(builder, phase);

    // Taper: full amplitude at the centre, none past radius 1.
    let (builder, taper) = crate::authoring::smoothstep_at(builder, 1.05, 0.15, radius);
    let (builder, tapered) = mul(builder, wave, taper);
    let (builder, dy) = scale(builder, tapered, RIPPLE_AMPLITUDE);
    let (builder, offset) = compose3(builder, zero, dy, zero);
    builder.build(offset)
}

/// The wind body: a warm base colour so the leaning silhouette is legible
/// against its own upright shadow.
pub fn wind_surface() -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .field(SurfaceChannel::Displacement, wind_displacement())
        .constant(
            SurfaceChannel::BaseColor,
            axiom_field::FieldValue::vec4(Vec4::new(0.878, 0.510, 0.165, 1.0)),
        )
        .constant(
            SurfaceChannel::Roughness,
            axiom_field::FieldValue::scalar(axiom_field::Scalar::new(0.55)),
        )
        .build()
        .expect("a vec3 field is a legal displacement")
}

/// The ripple body, with a field-authored colour riding the same ring family so
/// the crests read even when the silhouette is edge-on.
pub fn ripple_surface() -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .field(SurfaceChannel::Displacement, ripple_displacement())
        .field(SurfaceChannel::BaseColor, ripple_crest_color())
        .build()
        .expect("a vec3 displacement and a vec4 base colour are legal")
}

/// The ripple's colour: the same travelling ring, read as a mix between a trough
/// and a crest colour. A *fragment*-stage graph reading the same `Time` the
/// vertex-stage one does — one surface, two stages, **one program and one
/// digest**.
fn ripple_crest_color() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/displace/ripple-color"), 1);
    let (builder, p) = crate::authoring::point(builder);
    let (builder, x) = component(builder, p, 0);
    let (builder, z) = component(builder, p, 2);
    let (builder, zero) = konst(builder, 0.0);
    let (builder, planar) = compose3(builder, x, zero, z);
    let (builder, radius) = length(builder, planar);
    let (builder, clock) = time(builder);
    let (builder, travel) = scale(builder, clock, 3.4);
    let (builder, spatial) = scale(builder, radius, 7.0);
    let (builder, phase) = sub(builder, spatial, travel);
    let (builder, wave) = sin(builder, phase);
    let (builder, unit) = remap01(builder, wave);
    let (builder, trough) = konst4(builder, 0.086, 0.267, 0.310, 1.0);
    let (builder, crest) = konst4(builder, 0.541, 0.898, 0.855, 1.0);
    let (builder, color) = mix(builder, trough, crest, unit);
    builder.build(color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::EvalContext;
    use axiom_kernel::Seconds;
    use axiom_math::{Vec2, Vec3};

    fn displace(graph: &FieldGraph, point: Vec3, seconds: f32) -> Vec3 {
        graph
            .evaluate(&EvalContext::new(
                point,
                Vec2::new(0.0, 0.0),
                Vec3::UNIT_Y,
                Seconds::finite_or_zero(seconds),
            ))
            .expect("a validated graph evaluates")
            .as_vec3()
    }

    #[test]
    fn both_displacement_graphs_validate_and_are_vec3() {
        [wind_displacement(), ripple_displacement()]
            .iter()
            .for_each(|graph| {
                assert_eq!(graph.validate(), Ok(()));
                assert_eq!(
                    graph.type_at(graph.output()),
                    Ok(axiom_field::FieldType::Vec3)
                );
                println!("station 5 displacement nodes: {}", graph.node_count());
                assert!(graph.node_count() < axiom_field::MAX_NODES);
            });
    }

    /// **Deterministic engine time, never a wall clock.** The same second
    /// replays to the same offset, and a different second is a different offset.
    #[test]
    fn the_same_second_replays_to_the_same_offset() {
        let wind = wind_displacement();
        let at = Vec3::new(0.2, 0.9, -0.1);
        assert_eq!(displace(&wind, at, 3.25), displace(&wind, at, 3.25));
        assert_ne!(displace(&wind, at, 3.25), displace(&wind, at, 4.25));
    }

    /// The wind's amplitude grows with height and is exactly zero at the base —
    /// the property that makes limitation 1 legible in the frame.
    #[test]
    fn the_wind_plants_the_base_and_swings_the_top() {
        let wind = wind_displacement();
        let base = displace(&wind, Vec3::new(0.0, 0.0, 0.0), 1.1);
        let top = displace(&wind, Vec3::new(0.0, 1.0, 0.0), 1.1);
        assert_eq!(base.x, 0.0, "the planted base must not move");
        assert!(top.x.abs() > 0.05, "the top barely moved: {}", top.x);
        // Only x is displaced.
        assert_eq!((top.y, top.z), (0.0, 0.0));
    }

    /// The ripple travels: a fixed point on the body rises and falls over time,
    /// and the taper holds the rim still.
    #[test]
    fn the_ripple_travels_and_the_rim_stays_put() {
        let ripple = ripple_displacement();
        let inner = Vec3::new(0.25, 0.0, 0.0);
        let heights: Vec<f32> = (0..8)
            .map(|t| displace(&ripple, inner, t as f32 * 0.12).y)
            .collect();
        let span = heights.iter().fold(f32::MIN, |a, b| a.max(*b))
            - heights.iter().fold(f32::MAX, |a, b| a.min(*b));
        assert!(span > 0.05, "the ripple is static: span {span}");
        let rim = displace(&ripple, Vec3::new(1.4, 0.0, 0.0), 0.4);
        assert_eq!(rim.y, 0.0, "the taper must hold the rim still");
    }

    /// **One surface, two stages, one program.** The ripple surface binds a
    /// vertex-stage channel and a fragment-stage one and is still a single
    /// digest — a displacing surface must never force a second pipeline.
    #[test]
    fn a_displacing_surface_is_one_artifact_with_one_digest() {
        let surface = ripple_surface();
        let reqs = surface.requirements();
        assert!(reqs.has_displacement());
        assert!(reqs.varies(SurfaceChannel::BaseColor));
        assert!(reqs.inputs().contains(axiom_surface::SurfaceInput::TIME));
        assert!(reqs.inputs().contains(axiom_surface::SurfaceInput::POINT));
        assert_eq!(surface.digest(), ripple_surface().digest());
    }
}
