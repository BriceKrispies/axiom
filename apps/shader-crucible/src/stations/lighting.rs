//! **Station 6 — the three lighting models.** One authored appearance, three
//! `LightingModel` values, **zero extra pipelines**.
//!
//! The model is a closed discriminant carried *inside* a surface, not an axis a
//! program is specialised along. That distinction is the whole reason this
//! station exists: a design that keyed a program on the lighting model would
//! compile 3N programs for N materials, and this one compiles N.
//!
//! What a viewer sees, left to right:
//!
//! * `Unlit` — the base colour, verbatim. No light reaches it, so the sphere
//!   reads as a flat disc of the pattern.
//! * `Lambert` — diffuse only. The pattern is shaded by the light's angle.
//! * `LambertSpecular` — diffuse plus a view-dependent highlight, which is what
//!   the engine's one lit shader has always computed and is therefore the
//!   default.
//!
//! The three surfaces have **three different digests** — a lighting model is
//! part of a surface's identity, so it must be — and they select **two**
//! pipeline markers between them (`UNLIT` for the first, `BASIC_LIT` for the
//! other two), because this backend runs one lit program and the model is a
//! value inside it.

use axiom_field::{FieldBuilder, FieldGraph, FieldId};
use axiom_surface::{LightingModel, Surface, SurfaceBuilder, SurfaceChannel};

use crate::authoring::{
    abs, add, component, konst4, mix, normal, remap01, scale, sin,
};

/// A colour that makes all three models legible at once: a broad latitude ramp
/// (so a diffuse term has something to shade) crossed with a fine banding (so a
/// specular highlight has an edge to catch).
fn model_pattern() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/lighting/pattern"), 1);
    let (builder, n) = normal(builder);
    let (builder, ny) = component(builder, n, 1);
    let (builder, latitude) = remap01(builder, ny);

    let (builder, p) = crate::authoring::point(builder);
    let (builder, py) = component(builder, p, 1);
    let (builder, phase) = scale(builder, py, 26.0);
    let (builder, wave) = sin(builder, phase);
    let (builder, ridges) = abs(builder, wave);

    let (builder, lat_share) = scale(builder, latitude, 0.65);
    let (builder, ridge_share) = scale(builder, ridges, 0.35);
    let (builder, t) = add(builder, lat_share, ridge_share);

    let (builder, cool) = konst4(builder, 0.145, 0.176, 0.318, 1.0);
    let (builder, warm) = konst4(builder, 0.945, 0.816, 0.510, 1.0);
    let (builder, color) = mix(builder, cool, warm, t);
    builder.build(color)
}

/// **Station 6.** The same pattern under each of the three models, in
/// `LightingModel::ALL` order — the order the scene lays them out in.
pub fn lighting_surfaces() -> Vec<Surface> {
    LightingModel::ALL
        .iter()
        .map(|model| {
            SurfaceBuilder::new()
                .lighting(*model)
                .field(SurfaceChannel::BaseColor, model_pattern())
                .constant(
                    SurfaceChannel::Roughness,
                    axiom_field::FieldValue::scalar(axiom_field::Scalar::new(0.22)),
                )
                .build()
                .expect("a vec4 field is a legal base colour")
        })
        .collect()
}

/// The label under each of the three bodies.
pub fn lighting_labels() -> Vec<&'static str> {
    vec!["Unlit", "Lambert", "LambertSpecular"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_three_surfaces_in_the_declared_model_order() {
        let surfaces = lighting_surfaces();
        assert_eq!(surfaces.len(), 3);
        assert_eq!(
            surfaces.iter().map(Surface::lighting).collect::<Vec<_>>(),
            LightingModel::ALL.to_vec()
        );
        assert_eq!(lighting_labels().len(), 3);
    }

    /// **A lighting model is part of a surface's identity.** Three models, three
    /// digests — otherwise a program cache would hand the unlit body the lit
    /// program.
    #[test]
    fn the_three_models_are_three_distinct_surfaces() {
        let digests: std::collections::BTreeSet<u64> = lighting_surfaces()
            .iter()
            .map(|s| s.digest().raw())
            .collect();
        assert_eq!(digests.len(), 3);
    }

    /// ...and the *appearance* underneath them is one graph, byte for byte. If
    /// this ever stopped holding, the station would be comparing three different
    /// materials and proving nothing about lighting.
    #[test]
    fn the_appearance_under_the_three_models_is_one_identical_graph() {
        let graphs: Vec<Vec<u8>> = lighting_surfaces()
            .iter()
            .map(|s| {
                s.binding(SurfaceChannel::BaseColor)
                    .as_field()
                    .expect("a field")
                    .serialize()
            })
            .collect();
        assert_eq!(graphs[0], graphs[1]);
        assert_eq!(graphs[1], graphs[2]);
    }

    #[test]
    fn the_pattern_validates_and_reads_the_normal_and_the_point() {
        let graph = model_pattern();
        assert_eq!(graph.validate(), Ok(()));
        println!("station 6 pattern nodes: {}", graph.node_count());
        let reqs = lighting_surfaces()[0].requirements();
        assert!(reqs.inputs().contains(axiom_surface::SurfaceInput::NORMAL));
        assert!(reqs.inputs().contains(axiom_surface::SurfaceInput::POINT));
        assert!(!reqs.inputs().contains(axiom_surface::SurfaceInput::TIME));
    }

}
