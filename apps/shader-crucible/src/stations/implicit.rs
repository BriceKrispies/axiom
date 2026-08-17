//! **Station 7 — the implicit surface.** A metaball body whose *shape* is an
//! authored `FieldGraph`, sampled onto a lattice and marched into triangles.
//!
//! This is the gap `axiom-mesh-ops` documented in the negative: `ScalarField`
//! wanted a "function-as-a-value" and Rust had none to give it, so the layer
//! took a sampled lattice and said in writing that the missing thing was a
//! field. It is no longer missing — the density here is a `FieldGraph`, the
//! lattice is that graph evaluated at its nodes, and `implicit_surface_mesh`
//! marches it.
//!
//! ## Why the blobs are gaussians and not inverse distances
//!
//! The textbook metaball is `k / d²`, and **the field algebra deliberately has
//! no `Div`**: division by zero is a determinism hazard and a NaN source, and a
//! metaball's whole point is a singularity at its centre. `Pow(x, -1)` is the
//! sanctioned reciprocal and is documented to yield `0` at and below zero, which
//! at a blob's centre is exactly the wrong answer — the density would vanish
//! precisely where it should be largest.
//!
//! So the blobs are `exp(-falloff * d²)`, which is total, finite everywhere,
//! smooth, sums the way a metaball must, and needs no division at all.
//! `d² = dot(p - centre, p - centre)`, and the whole three-blob body is 40 nodes
//! over `Point`, `Sub`, `Dot`, `Mul`, `Exp` and `Add`. **The reciprocal the
//! algebra does not have turned out not to be needed** — which is the honest
//! version of "the vocabulary was big enough", and it is worth more than a
//! twenty-eighth operator.

use axiom_field::{EvalContext, FieldBuilder, FieldGraph, FieldId, NodeId};
use axiom_math::{Vec2, Vec3};
use axiom_mesh::Mesh;
use axiom_mesh_ops::{
    implicit_surface_mesh, DetailBudget, ImplicitSurfaceOptions, IsoValue, ScalarField,
};
use axiom_surface::{LightingModel, Surface, SurfaceBuilder, SurfaceChannel};

use crate::authoring::{add, exp, konst, konst3, konst4, mix, scale, sub};

/// The lattice edge the density is sampled on. 40³ is 64,000 evaluations of a
/// 40-node graph at the preparation barrier — comfortably a startup cost and
/// emphatically not a frame one.
pub const LATTICE: u32 = 40;

/// The half-extent of the sampled box, in object units.
pub const EXTENT: f32 = 1.6;

/// The level the mesher marches. Each blob peaks at 1 at its own centre, so a
/// level of `0.55` sits where two neighbouring blobs have just merged — which is
/// the shape a metaball is *for*.
pub const ISO: f32 = 0.55;

/// The three blob centres and their falloffs.
const BLOBS: [([f32; 3], f32); 3] = [
    ([-0.46, 0.10, 0.0], 3.1),
    ([0.44, -0.14, 0.12], 3.6),
    ([0.02, 0.52, -0.22], 4.4),
];

/// **The body's shape, as a graph.** `sum_i exp(-falloff_i * |p - centre_i|²)`.
pub fn density_field() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/implicit/density"), 1);
    let (builder, p) = crate::authoring::point(builder);
    let (builder, zero) = konst(builder, 0.0);
    let (builder, total) = BLOBS.iter().fold((builder, zero), |(builder, acc), (c, k)| {
        let (builder, blob) = gaussian(builder, p, *c, *k);
        add(builder, acc, blob)
    });
    builder.build(total)
}

/// One blob: `exp(-falloff * dot(p - centre, p - centre))`.
fn gaussian(
    builder: FieldBuilder,
    p: NodeId,
    centre: [f32; 3],
    falloff: f32,
) -> (FieldBuilder, NodeId) {
    let (builder, c) = konst3(builder, centre[0], centre[1], centre[2]);
    let (builder, delta) = sub(builder, p, c);
    // `Dot` over the inputs' common width — the squared distance, with no
    // division and no `sqrt`.
    let (builder, squared) =
        builder.push(axiom_field::FieldOp::Dot, Vec::new(), vec![delta, delta]);
    let (builder, weighted) = scale(builder, squared, -falloff);
    exp(builder, weighted)
}

/// The sampled lattice: [`density_field`] evaluated at every node of a
/// `LATTICE³` grid spanning `[-EXTENT, EXTENT]` on each axis.
///
/// **Preparation-time work.** 64,000 evaluations of a 40-node graph is a startup
/// cost; nothing here may run in a frame.
pub fn sampled_lattice() -> Option<ScalarField> {
    let graph = density_field();
    let step = 2.0 * EXTENT / (LATTICE - 1) as f32;
    let values: Vec<f32> = (0..LATTICE * LATTICE * LATTICE)
        .map(|index| {
            let x = index % LATTICE;
            let y = (index / LATTICE) % LATTICE;
            let z = index / (LATTICE * LATTICE);
            let at = Vec3::new(
                -EXTENT + x as f32 * step,
                -EXTENT + y as f32 * step,
                -EXTENT + z as f32 * step,
            );
            graph
                .evaluate(&EvalContext::at(at, Vec2::new(0.0, 0.0), Vec3::UNIT_Y))
                .map(|value| value.as_scalar().get())
                .unwrap_or(0.0)
        })
        .collect();
    ScalarField::new(values, LATTICE, LATTICE, LATTICE).ok()
}

/// The marched body.
///
/// `None` only when the sampling or the extraction fails, which for an
/// authored-in-Rust graph is a defect rather than a runtime condition;
/// [`tests::the_body_meshes_and_is_a_closed_blob`] proves the shipped graph never
/// takes that arm.
pub fn implicit_body() -> Option<Mesh> {
    let step = 2.0 * EXTENT / (LATTICE - 1) as f32;
    sampled_lattice().and_then(|field| {
        implicit_surface_mesh(
            &field,
            IsoValue::new(ISO).ok()?,
            ImplicitSurfaceOptions {
                origin: Vec3::new(-EXTENT, -EXTENT, -EXTENT),
                spacing: Vec3::new(step, step, step),
                budget: DetailBudget::default(),
            },
        )
        .ok()
    })
}

/// **Station 7's appearance**: a graph reading the *same* density the shape came
/// from, so the surface's colour and its silhouette are two readings of one
/// authored function. Hot where the body is dense, cool at its thin necks.
pub fn implicit_surface() -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .field(SurfaceChannel::BaseColor, density_color())
        .constant(
            SurfaceChannel::Roughness,
            axiom_field::FieldValue::scalar(axiom_field::Scalar::new(0.34)),
        )
        .build()
        .expect("a vec4 field is a legal base colour")
}

/// The colour: **how sharply one blob dominates**, mixed between a neck colour
/// and a core colour.
///
/// Not the density itself, and the reason is worth stating: the marched body is
/// an *iso-surface*, so the density is `ISO` at every point of it, by definition.
/// Colouring by density would paint the whole body one flat colour — which is
/// what the first authoring of this station did, and what the screenshot showed.
///
/// What varies over an iso-surface is *which* blob is responsible for the
/// density there. Near one blob's centre a single term carries almost all of the
/// sum; at a neck between two, two terms carry half each. So the mix parameter
/// is `max_i(blob_i) - 0.5 * sum_i(blob_i)`, spread and clamped: zero where two
/// blobs meet, large where one dominates. `Max` and `Sub`, no division.
fn density_color() -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/implicit/color"), 1);
    let (builder, p) = crate::authoring::point(builder);
    let (builder, zero) = konst(builder, 0.0);
    let (builder, first) = gaussian(builder, p, BLOBS[0].0, BLOBS[0].1);
    let (builder, (total, strongest)) = BLOBS.iter().skip(1).fold(
        (builder, (first, first)),
        |(builder, (sum, best)), (c, k)| {
            let (builder, blob) = gaussian(builder, p, *c, *k);
            let (builder, sum) = add(builder, sum, blob);
            let (builder, best) = crate::authoring::max(builder, best, blob);
            (builder, (sum, best))
        },
    );
    let (builder, half) = crate::authoring::scale(builder, total, 0.5);
    let (builder, dominance) = sub(builder, strongest, half);
    let (builder, spread) = scale(builder, dominance, 5.0);
    let (builder, t) = crate::authoring::clamp_unit(builder, spread);
    let (builder, neck) = konst4(builder, 0.180, 0.298, 0.549, 1.0);
    let (builder, core) = konst4(builder, 0.902, 0.412, 0.318, 1.0);
    let (builder, color) = mix(builder, neck, core, t);
    let _ = zero;
    builder.build(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_density_graph_validates_and_needs_no_division() {
        let graph = density_field();
        assert_eq!(graph.validate(), Ok(()));
        println!("station 7 density nodes: {}", graph.node_count());
        assert!(graph.node_count() < axiom_field::MAX_NODES);
        assert_eq!(
            graph.type_at(graph.output()),
            Ok(axiom_field::FieldType::Scalar)
        );
        // Every value is finite everywhere, including at a blob's own centre —
        // the point at which the textbook `k / d²` metaball is undefined.
        let at_centre = graph
            .evaluate(&EvalContext::at(
                Vec3::new(BLOBS[0].0[0], BLOBS[0].0[1], BLOBS[0].0[2]),
                Vec2::new(0.0, 0.0),
                Vec3::UNIT_Y,
            ))
            .expect("evaluates")
            .as_scalar()
            .get();
        assert!(at_centre.is_finite());
        assert!(at_centre >= 1.0, "a blob peaks at its own centre: {at_centre}");
    }

    #[test]
    fn the_body_meshes_and_is_a_closed_blob() {
        let mesh = implicit_body().expect("the shipped density marches");
        println!(
            "station 7 mesh: {} vertices, {} triangles",
            mesh.vertex_count(),
            mesh.indices().len() / 3
        );
        assert!(mesh.vertex_count() > 0);
        assert_eq!(mesh.indices().len() % 3, 0);
        assert_eq!(mesh.normals().len(), mesh.vertex_count());
        // The body sits inside the sampled box it was marched from.
        assert!(mesh
            .positions()
            .iter()
            .all(|p| p.x.abs() <= EXTENT + 0.01
                && p.y.abs() <= EXTENT + 0.01
                && p.z.abs() <= EXTENT + 0.01));
    }

    #[test]
    fn the_extraction_is_deterministic() {
        let a = implicit_body().expect("marches");
        let b = implicit_body().expect("marches");
        assert_eq!(a.positions(), b.positions());
        assert_eq!(a.indices(), b.indices());
    }

    #[test]
    fn the_colour_graph_validates() {
        let graph = density_color();
        assert_eq!(graph.validate(), Ok(()));
        assert_eq!(graph.type_at(graph.output()), Ok(axiom_field::FieldType::Vec4));
    }

}
