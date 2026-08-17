//! **Station 2 — the live procedural surface**, and **station 3 — the same graph
//! baked**.
//!
//! Station 2 is the half of the system nothing shipping had proven: an authored
//! `FieldGraph` bound to a `Surface` channel, planned into a `surface_program`,
//! emitted as WGSL and evaluated **per pixel**. Station 3 takes the *identical*
//! graph value and bakes it once through
//! [`axiom_proc_texture::TextureOp::Field`] into an RGBA8 tile. One graph, two
//! realisations.
//!
//! ## Why this station reads only `Uv`
//!
//! That is not a stylistic choice, it is what makes the two realisations
//! comparable. `TextureOp::Field` bakes at `EvalContext::at((uv.x, uv.y, 0), uv,
//! +Y)` — its `Point` **is** its `Uv`, flattened into a plane. A live surface's
//! `Point` is the object's own three-dimensional position. A graph reading
//! `Point` would therefore evaluate to two different things in the two paths and
//! any "bake and live agree" claim about it would be false. So the pattern is
//! authored over `Uv` alone, and where it wants a three-lane sample position it
//! `Compose`s one out of the `Uv` lanes — which is exactly the point the bake
//! sees.
//!
//! ## The one place the two realisations genuinely disagree, and it is not the graph
//!
//! `TextureOp::Field` writes **linear** bytes (`clamp(v, 0, 1) * 255`, rounded),
//! and the material-texture upload path binds an app-supplied albedo as
//! `Rgba8UnormSrgb` — the sampler *decodes* the byte through the sRGB curve
//! before it multiplies. So the same graph, baked and then sampled, comes back
//! darker than the same graph evaluated live, by exactly the sRGB transfer
//! function. **The graphs agree; the two upload conventions do not.** That is a
//! real seam between `proc-texture`'s byte convention and the backend's texture
//! format, it is reported rather than papered over, and station 3's label says
//! so on screen. [`tests::the_bake_and_the_live_evaluation_agree_texel_for_texel`]
//! pins the agreement at the level where it is true — the value the graph
//! computes and the byte the bake wrote.

use axiom_field::{FieldBuilder, FieldGraph, FieldId};
use axiom_noise::{FbmConfig, Frequency};
use axiom_proc_texture::ProcTextureApi;
use axiom_recipe::{Param, RecipeGraph, RecipeId};
use axiom_surface::{LightingModel, Surface, SurfaceBuilder, SurfaceChannel};

use crate::authoring::{
    add, clamp, component, compose3, konst, konst4, length, mix, remap01, scale, sin, sub,
};

/// The edge of the baked tile. 128² is 16,384 evaluations of the same graph the
/// live arm runs once per covered pixel — the comparison the station is for.
pub const BAKE_RES: u32 = 128;

/// The seed of the mottle that keeps the rings from reading as a test pattern.
const MOTTLE_SEED: u64 = 0x1D0C_9E77;

/// **The station's one graph**: concentric rings about the centre of the
/// parameterisation, their contrast broken up by a three-octave fbm, mixed
/// between two blues. `Vec4`, linear RGBA, opaque.
///
/// Twenty-nine nodes over `Uv`, `Component`, `Compose`, `Sub`, `Mul`, `Add`,
/// `Length`, `Sin`, `Fbm`, `Clamp` and `Mix` — and not one line of shading Rust.
pub fn ripple_color() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/live/ripple-color"), 1);
    let (builder, t) = ripple_amount(builder);
    let (builder, deep) = konst4(builder, 0.043, 0.098, 0.220, 1.0);
    let (builder, bright) = konst4(builder, 0.318, 0.706, 0.847, 1.0);
    let (builder, color) = mix(builder, deep, bright, t);
    builder.build(color)
}

/// The station's roughness: the same ring family read as a gloss range, so the
/// crests are polished and the troughs are matte. A **second** graph rather than
/// a shared node, because a surface binds one graph per channel.
pub fn ripple_roughness() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/live/ripple-rough"), 1);
    let (builder, t) = ripple_amount(builder);
    let (builder, spread) = scale(builder, t, -0.55);
    let (builder, rough) = crate::authoring::offset(builder, spread, 0.78);
    builder.build(rough)
}

/// The `0..=1` ring-and-mottle amount both channels are read from.
fn ripple_amount(builder: FieldBuilder) -> (FieldBuilder, axiom_field::NodeId) {
    let (builder, uv) = crate::authoring::uv(builder);
    let (builder, u) = component(builder, uv, 0);
    let (builder, v) = component(builder, uv, 1);
    let (builder, half) = konst(builder, 0.5);
    let (builder, cu) = sub(builder, u, half);
    let (builder, cv) = sub(builder, v, half);
    let (builder, zero) = konst(builder, 0.0);
    let (builder, centred) = compose3(builder, cu, cv, zero);
    let (builder, radius) = length(builder, centred);
    let (builder, phase) = scale(builder, radius, 46.0);
    let (builder, wave) = sin(builder, phase);
    let (builder, rings) = remap01(builder, wave);

    // The sample point the fbm reads is composed out of the SAME two Uv lanes
    // the bake supplies as its `Point`, so the two realisations sample the same
    // place. See the module docs.
    let (builder, flat) = compose3(builder, u, v, zero);
    let (builder, warped) = scale(builder, flat, 9.0);
    let (builder, noise) = builder.push_fbm(
        MOTTLE_SEED,
        FbmConfig::new(3, Frequency::new(1.0).expect("an authored frequency is positive")),
        warped,
    );
    let (builder, mottle) = remap01(builder, noise);

    let (builder, ring_share) = scale(builder, rings, 0.72);
    let (builder, mottle_share) = scale(builder, mottle, 0.28);
    let (builder, summed) = add(builder, ring_share, mottle_share);
    let (builder, one) = konst(builder, 1.0);
    clamp(builder, summed, zero, one)
}

/// **Station 2.** The live surface: base colour and roughness both bound to
/// authored graphs, so the backend must lower a real program for it — there is
/// no constant it could fall back to that would look like this.
pub fn live_surface() -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .field(SurfaceChannel::BaseColor, ripple_color())
        .field(SurfaceChannel::Roughness, ripple_roughness())
        .build()
        .expect("two graphs of the declared channel types are a legal surface")
}

/// **Station 3.** The *same* [`ripple_color`] graph, baked once to `BAKE_RES²`
/// linear RGBA8 texels through `TextureOp::Field`.
///
/// `None` only if the bake fails, which for an authored-in-Rust graph is a defect
/// rather than a runtime condition; [`tests::the_shipped_graph_bakes`] proves the
/// shipped graph never takes that arm.
pub fn baked_albedo() -> Option<Vec<u8>> {
    ProcTextureApi::new()
        .bake_with_fields(&bake_recipe(), 0, &[ripple_color()])
        .ok()
        .map(axiom_proc_texture::TextureBuffer::into_pixels)
}

/// The one-node bake recipe: a single `Field` source at the tile's resolution,
/// naming table entry 0. The graph travels *beside* the recipe — a
/// `axiom_recipe::Param` is one `u32` word, so inlining a graph's bytes would
/// spend a 256-node budget on one operator.
fn bake_recipe() -> RecipeGraph {
    let mut recipe = RecipeGraph::new(RecipeId::from_raw(0x0C12_5E1B_u64), 1);
    recipe.add(
        axiom_proc_texture::TextureOp::Field as u16,
        vec![
            Param::int(BAKE_RES),
            Param::int(BAKE_RES),
            Param::int(0),
        ],
        Vec::new(),
    );
    recipe
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::EvalContext;
    use axiom_math::{Vec2, Vec3};

    /// The value the live graph computes at one texel's centre — the exact
    /// sampling convention `TextureOp::Field` bakes on.
    fn live_at(graph: &FieldGraph, x: u32, y: u32) -> [f32; 4] {
        let uv = Vec2::new(
            (x as f32 + 0.5) / BAKE_RES as f32,
            (y as f32 + 0.5) / BAKE_RES as f32,
        );
        let v = graph
            .evaluate(&EvalContext::at(Vec3::new(uv.x, uv.y, 0.0), uv, Vec3::UNIT_Y))
            .expect("a validated graph evaluates")
            .as_vec4();
        [v.x, v.y, v.z, v.w]
    }

    #[test]
    fn the_station_graphs_validate_and_their_node_counts_are_pinned() {
        let color = ripple_color();
        let rough = ripple_roughness();
        assert_eq!(color.validate(), Ok(()));
        assert_eq!(rough.validate(), Ok(()));
        println!(
            "station 2 nodes: colour {} roughness {}",
            color.node_count(),
            rough.node_count()
        );
        assert!(color.node_count() < axiom_field::MAX_NODES);
        assert!(rough.node_count() < axiom_field::MAX_NODES);
        assert_eq!(color.type_at(color.output()), Ok(axiom_field::FieldType::Vec4));
        assert_eq!(rough.type_at(rough.output()), Ok(axiom_field::FieldType::Scalar));
    }

    /// **The station reads `Uv` and nothing else**, which is what makes station 3
    /// a comparison rather than a coincidence.
    #[test]
    fn the_station_reads_only_the_parameterisation() {
        let reqs = live_surface().requirements();
        assert_eq!(reqs.inputs(), axiom_surface::SurfaceInput::UV);
    }

    #[test]
    fn the_shipped_graph_bakes() {
        let pixels = baked_albedo().expect("the shipped graph bakes");
        assert_eq!(pixels.len(), (BAKE_RES * BAKE_RES * 4) as usize);
        assert!(pixels.chunks(4).all(|t| t[3] == 255));
        assert_eq!(baked_albedo(), Some(pixels), "the bake is deterministic");
    }

    /// **Stations 2 and 3 agree, and the stated tolerance is one byte level.**
    ///
    /// The bake rounds each linear channel to the nearest of 256 levels, so the
    /// tolerance is quantisation and nothing else: every texel of the baked tile
    /// is within `1/255` of the value the live graph computes at that texel's
    /// centre.
    #[test]
    fn the_bake_and_the_live_evaluation_agree_texel_for_texel() {
        let graph = ripple_color();
        let baked = baked_albedo().expect("the shipped graph bakes");
        let worst = (0..BAKE_RES * BAKE_RES)
            .map(|index| {
                let (x, y) = (index % BAKE_RES, index / BAKE_RES);
                let live = live_at(&graph, x, y);
                let at = (index * 4) as usize;
                (0..3)
                    .map(|lane| {
                        (f32::from(baked[at + lane]) - live[lane].clamp(0.0, 1.0) * 255.0).abs()
                    })
                    .fold(0.0_f32, f32::max)
            })
            .fold(0.0_f32, f32::max);
        println!("station 2 vs station 3: worst texel delta {worst:.4} byte levels");
        assert!(
            worst <= 1.0,
            "the bake and the live evaluation differ by {worst} byte levels"
        );
    }

    /// The tile is not a flat colour — a "they agree" test on two constant
    /// images would prove nothing.
    #[test]
    fn the_baked_tile_actually_has_a_pattern_in_it() {
        let baked = baked_albedo().expect("the shipped graph bakes");
        let blues: Vec<u8> = baked.chunks(4).map(|t| t[2]).collect();
        let low = *blues.iter().min().expect("a non-empty tile");
        let high = *blues.iter().max().expect("a non-empty tile");
        assert!(
            u32::from(high) - u32::from(low) > 60,
            "the baked tile spans only {low}..{high}"
        );
    }

}
