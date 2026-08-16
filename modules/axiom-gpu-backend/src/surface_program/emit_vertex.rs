//! The **vertex-stage** emitter: an authored surface's `Displacement` channel in,
//! one WGSL `axiom_displace` function out.
//!
//! ## Deformation is a general field consumer, not a material concept
//!
//! Read this before concluding that a GPU executing something makes it an
//! appearance feature. A displacement is a `Vec3` field of position and time; it
//! has no more to do with materials than a heightfield does, and the engine
//! already proves it — `axiom_proc_mesh`'s `MeshOp::Displace` is bake-time
//! deformation with no material anywhere near it, and `axiom_mesh_ops`
//! transforms geometry with no material either.
//!
//! `axiom_surface::Surface` carries a `Displacement` channel **only because that
//! is the binding site for the vertex stage of the program its fragment channels
//! already compile into**. It is a wiring convenience — one authored artifact,
//! one digest, one pipeline — and not a claim that deformation is appearance.
//!
//! ## Why this is a different file and not a different compiler
//!
//! It is the same fold over the same operator table as
//! [`crate::surface_program::emit`] — one `let` per node, in node id order,
//! through [`crate::surface_program::emit_ops`]. What differs is three facts, and
//! only three: the channel index (displacement is `SurfaceChannel::ALL[6]`, so
//! its registers are `c6_n*`), the parameter base (its slots follow every
//! fragment channel's, because that is the order `params::pack` writes them), and
//! the result type (`vec3<f32>`, the object-space offset, not a `SurfaceOut`).
//! So this file owns the *signature and the splice*, and borrows the body.
//!
//! ## The calling convention, and why it needs no new vertex attribute
//!
//! ```wgsl
//! fn axiom_displace(pos: vec3<f32>, nrm: vec3<f32>, uv: vec2<f32>, t: f32,
//!                   params: SurfaceParams) -> vec3<f32>
//! ```
//!
//! Every argument is something the rigid vertex stage already has in hand:
//! `position`, `normal` and `uv` are three of the four per-vertex attributes it
//! has always bound, `t` is the frame's presentation time from the lighting
//! uniform, and `params` is the shared surface-parameter region. The rigid
//! pipeline binds 14 of the 16 vertex attributes a WebGL2 downlevel target
//! guarantees, and this adds **none** — a seventeenth attribute would fail
//! pipeline creation on the browser fallback path, which is why the graph may
//! read only these.
//!
//! It is emitted as **two** functions: `axiom_displace_at`, which carries the
//! graph and takes a `SurfaceIn` — the struct `emit_ops` writes every context
//! read against (`in.object_pos`, `in.uv`, `in.object_normal`, `in.time`) — and
//! the named entry above, which packs the vertex stage's loose arguments into
//! one and calls it. That split is why the emitter needs no second spelling of
//! the context: a parameter named `in` is exactly what the fragment side already
//! has. The two lanes a vertex stage has no answer for — the resolved albedo and
//! the instance emissive — are filled with the identity, and no context operator
//! reads them.
//!
//! ## Normals are **not** recomputed, and that is a decision
//!
//! Displacing a vertex invalidates its normal. Recomputing one needs the
//! displaced positions of that vertex's *neighbours*, which a vertex stage cannot
//! see — it is handed one vertex, with no topology and no adjacency, and WebGL2
//! has neither tessellation nor geometry shaders to give it any. So the geometric
//! normal that reaches the fragment stage after a displacement is the *undisplaced*
//! surface's normal, and this backend does not pretend otherwise.
//!
//! The honest way to get a correct normal is to derive one analytically from the
//! same field and bind it to the `Normal` channel — which is exactly what
//! `axiom_surface::SurfaceBuilder::normal_from_height` exists for. An author who
//! displaces by a height field and binds that field's analytic normal gets a
//! correct shading normal; an author who displaces and binds nothing gets the
//! flat one, which is correct only for small displacement. Both are stated, and
//! neither is guessed at.

use axiom_surface::{Surface, SurfaceChannel};

use crate::surface_program::emit::{channel_text, vertex_param_base};
use crate::surface_program::program_error::{SurfaceProgramError, SurfaceProgramFault};

/// The channel index displacement occupies in `SurfaceChannel::ALL` — the last
/// one, which is what makes the six fragment channels a prefix and this one's
/// parameter base a plain sum.
const DISPLACEMENT_CHANNEL: usize = 6;

/// The WGSL `axiom_displace` function `surface`'s displacement channel lowers to.
///
/// Deterministic, for the same reason the fragment emitter is: every step is an
/// ordered fold and nothing here iterates a map, so the same surface always
/// yields byte-identical text. That is what lets the vertex and fragment halves
/// of one surface share **one** program digest and therefore one pipeline — a
/// displacing surface must never force a second pipeline for the same material.
///
/// Emitted from the surface's **flattened** form, because flattening is what
/// composes a layered surface's per-channel graphs, and therefore its parameter
/// table, into the one program a backend runs.
///
/// A surface with no displacement still emits a function: the constant
/// `SurfaceChannel::Displacement` default, which is the zero vector, so the
/// vertex stage adds exactly nothing. It is [`DEFAULT_DISPLACE_WGSL`]'s twin by
/// value rather than by text — but the pass splices the *default* for such a
/// surface anyway, so the arithmetic never happens.
///
/// [`DEFAULT_DISPLACE_WGSL`]: crate::surface_program::wgsl_template::DEFAULT_DISPLACE_WGSL
pub(crate) fn displace_function(surface: &Surface) -> Result<String, SurfaceProgramError> {
    let program_id = surface.digest().raw();
    surface
        .flatten()
        .map_err(|error| {
            SurfaceProgramError::new(
                program_id,
                SurfaceChannel::Displacement.bit(),
                SurfaceProgramFault::Flatten,
                String::from(error.message()),
            )
        })
        .map(|flat| function_text(&flat))
}

/// The function text for an already-flattened surface.
fn function_text(flat: &Surface) -> String {
    let graph = flat
        .binding(SurfaceChannel::ALL[DISPLACEMENT_CHANNEL])
        .as_graph();
    let (body, output) = channel_text(&graph, DISPLACEMENT_CHANNEL, vertex_param_base(flat));
    format!(
        "fn axiom_displace_at(in: SurfaceIn, params: SurfaceParams) -> vec3<f32> {{\n\
         {body}    return {output}.xyz;\n\
         }}\n\
         {DISPLACE_ENTRY}"
    )
}

/// The named entry the vertex stage calls, in the fixed signature every
/// generated program shares. It packs the stage's loose arguments into the
/// `SurfaceIn` the graph body reads, and it is the same text for every surface —
/// so it is a constant, not something the fold assembles.
const DISPLACE_ENTRY: &str = "fn axiom_displace(pos: vec3<f32>, nrm: vec3<f32>, uv: vec2<f32>, \
     t: f32, params: SurfaceParams) -> vec3<f32> {\n\
     \x20   return axiom_displace_at(\n\
     \x20       SurfaceIn(pos, uv, nrm, t, vec4<f32>(1.0, 1.0, 1.0, 1.0), \
     vec3<f32>(0.0, 0.0, 0.0)),\n\
     \x20       params,\n\
     \x20   );\n\
     }\n";

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldGraph, FieldId, FieldOp, FieldType, FieldValue};
    use axiom_math::{Vec3, Vec4};
    use axiom_recipe::Scalar;
    use axiom_surface::{LayerBlend, SurfaceBuilder, SurfaceLayer};

    /// A vec3 displacement that is the object-space normal scaled by a declared
    /// amplitude — the shape every ripple and wind graph ends in.
    fn normal_offset() -> FieldGraph {
        let (builder, normal) = FieldBuilder::new(FieldId::of_name("gpu/vtx/n"), 1).push(
            FieldOp::Normal,
            Vec::new(),
            Vec::new(),
        );
        let (builder, slot) =
            builder.declare("amp", FieldValue::scalar(Scalar::new(0.25)));
        let (builder, amp) = builder.push_param(slot, FieldType::Scalar);
        let (builder, scaled) = builder.push(FieldOp::Mul, Vec::new(), vec![normal, amp]);
        builder.build(scaled)
    }

    #[test]
    fn an_undisplaced_surface_emits_the_zero_offset() {
        let surface = SurfaceBuilder::new().build().expect("legal");
        let text = displace_function(&surface).expect("a flat surface flattens");
        assert!(text.starts_with("fn axiom_displace_at(in: SurfaceIn, params: SurfaceParams) -> vec3<f32> {\n"));
        assert!(text.contains("let c6_n0 = vec4<f32>(0.0, 0.0, 0.0, 0.0);"));
        assert!(text.contains("    return c6_n0.xyz;\n"));
        // The named entry is the fixed signature the vertex stage calls, and it
        // is the same text for every surface.
        assert!(text.ends_with(DISPLACE_ENTRY));
        assert!(DISPLACE_ENTRY.contains(
            "fn axiom_displace(pos: vec3<f32>, nrm: vec3<f32>, uv: vec2<f32>, t: f32, \
             params: SurfaceParams) -> vec3<f32>"
        ));
        assert!(DISPLACE_ENTRY
            .contains("SurfaceIn(pos, uv, nrm, t, vec4<f32>(1.0, 1.0, 1.0, 1.0), vec3<f32>(0.0, 0.0, 0.0))"));
    }

    #[test]
    fn a_displacement_graph_emits_one_ssa_line_per_node_in_the_vertex_register_namespace() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Displacement, normal_offset())
            .build()
            .expect("a vec3 field is a legal displacement");
        let text = displace_function(&surface).expect("flattens");
        let normal_at = text
            .find("let c6_n0 = vec4<f32>(in.object_normal, 0.0);")
            .expect("the normal reads the vertex normal the caller packed in");
        let param_at = text
            .find("let c6_n1 = params.slots[0u];")
            .expect("the amplitude reads slot 0 — no fragment channel declares one");
        assert!(normal_at < param_at, "emission is in node id order");
        assert!(text.contains("let c6_n2 = "));
        assert!(text.contains("    return c6_n2.xyz;\n"));
        // Generation is a pure function: one digest, one program, one pipeline.
        assert_eq!(text, displace_function(&surface).expect("flattens"));
    }

    /// The vertex stage's parameters sit **after** every fragment channel's,
    /// because `params::pack` writes the seven channels end to end in channel
    /// order. Get this wrong and a displacing, tinted surface reads its
    /// amplitude out of the base colour's slot.
    #[test]
    fn the_vertex_stages_parameters_follow_every_fragment_channels() {
        let (builder, slot) = FieldBuilder::new(FieldId::of_name("gpu/vtx/tint"), 1)
            .declare("tint", FieldValue::vec4(Vec4::new(1.0, 0.0, 0.0, 1.0)));
        let (builder, node) = builder.push_param(slot, FieldType::Vec4);
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, builder.build(node))
            .field(SurfaceChannel::Displacement, normal_offset())
            .build()
            .expect("legal");
        let flat = surface.flatten().expect("flattens");
        assert_eq!(vertex_param_base(&flat), 1);
        let text = displace_function(&surface).expect("flattens");
        assert!(
            text.contains("let c6_n1 = params.slots[1u];"),
            "the amplitude reads the slot after the tint's: {text}"
        );
    }

    #[test]
    fn a_layer_tree_that_will_not_flatten_is_a_named_displacement_failure() {
        let over = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, chain("gpu/vtx/under", 65))
            .layer(SurfaceLayer::new(
                SurfaceBuilder::new()
                    .field(SurfaceChannel::Opacity, chain("gpu/vtx/over", 65))
                    .build()
                    .expect("legal"),
                SurfaceLayer::opaque_mask(),
                LayerBlend::Over,
            ))
            .build()
            .expect("one layer is within budget");
        let error = displace_function(&over).expect_err("the composition is over budget");
        assert_eq!(error.program_id(), over.digest().raw());
        assert_eq!(error.fault(), SurfaceProgramFault::Flatten);
        assert_eq!(error.channel_names(), vec!["displacement"]);
    }

    /// A scalar chain of `steps` `Add`s over fresh constants.
    fn chain(name: &str, steps: u16) -> FieldGraph {
        let (builder, node) = (0..steps).fold(
            FieldBuilder::new(FieldId::of_name(name), 1)
                .push_const(FieldValue::scalar(Scalar::new(1.0))),
            |(builder, acc), _| {
                let (builder, one) = builder.push_const(FieldValue::scalar(Scalar::new(1.0)));
                builder.push(FieldOp::Add, Vec::new(), vec![acc, one])
            },
        );
        builder.build(node)
    }

    #[test]
    fn displacement_is_the_seventh_channel_and_the_vertex_register_namespace_is_its_own() {
        assert_eq!(
            SurfaceChannel::ALL[DISPLACEMENT_CHANNEL],
            SurfaceChannel::Displacement
        );
        // A constant displacement still emits, and its register namespace cannot
        // collide with any fragment channel's.
        let surface = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(Vec3::new(0.0, 0.5, 0.0)),
            )
            .build()
            .expect("a vec3 constant is a legal displacement");
        let text = displace_function(&surface).expect("flattens");
        assert!(text.contains("let c6_n0 = vec4<f32>(0.0, 0.5, 0.0, 0.0);"));
        ["c0_n", "c1_n", "c2_n", "c3_n", "c4_n", "c5_n"]
            .iter()
            .for_each(|prefix| assert!(!text.contains(prefix), "leaked {prefix}"));
    }
}
