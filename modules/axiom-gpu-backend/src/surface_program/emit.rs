//! The emitter: an authored surface in, one WGSL `axiom_surface` function out.
//!
//! ## A flat forward pass, because the graph is already SSA
//!
//! A field graph is an id-ordered DAG in which every input names a *strictly
//! earlier* node. That property **is** single static assignment, and it was
//! designed in so that this stage could be a single `fold` in node id order
//! emitting one `let n = …;` per node. There is no recursion, no
//! expression-tree flattening, no temporary allocator and no naming scheme
//! beyond `c{channel}_n{node}`.
//!
//! ## One function, six channels, one register namespace per channel
//!
//! A surface's six fragment channels are six *independent* graphs, each with its
//! own node ids, so the SSA names are prefixed by the channel index. They are
//! emitted into one function body in channel order, and the function ends by
//! assembling the fixed `SurfaceOut`. `Displacement` is a vertex-stage channel
//! and is not emitted here — but its parameter slots are still counted, because
//! the shared uniform region packs every channel's parameters end to end and the
//! offsets must agree with `crate::surface_program::params::pack`.
//!
//! ## Emission is total; only flattening can fail
//!
//! Every lookup below is total against the same documented defaults the CPU
//! evaluator uses: an operator code the table does not have emits the zero
//! value, a parameter slot the table does not have emits the zero value, an
//! absent input slot emits the zero value. That is deliberate — a per-node error
//! path would be a second, drift-prone statement of rules
//! `axiom_surface::Surface::validate` already enforces. The one genuine failure
//! is a layer tree whose composed graphs will not fit the field node budget, and
//! it is reported as a [`SurfaceProgramError`] naming the surface's digest.

use axiom_field::{FieldGraph, FieldType};
use axiom_recipe::NodeId;
use axiom_surface::{Surface, SurfaceChannel};

use crate::surface_program::emit_ops::{emit_node, EmitOperand, EmitStep};
use crate::surface_program::program_error::{SurfaceProgramError, SurfaceProgramFault};

/// The channels a fragment stage evaluates: every channel but `Displacement`,
/// which is last in `SurfaceChannel::ALL` precisely so this is a prefix.
pub(crate) const FRAGMENT_CHANNEL_COUNT: usize = 6;

/// The most inputs any operator consumes — `Compose` at width four. The mirror
/// of `axiom-field`'s `MAX_INPUTS`, so a node carrying more inputs than any
/// operator can read is truncated here exactly as it is there.
const MAX_INPUTS: usize = 4;

/// Where each fragment channel's value lands in `SurfaceOut`: the struct field,
/// and the swizzle that narrows the emitter's four-lane register to it.
const CHANNEL_TARGET: [(&str, &str); FRAGMENT_CHANNEL_COUNT] = [
    ("base_color", ""),
    ("roughness", ".x"),
    ("metallic", ".x"),
    ("normal", ".xyz"),
    ("emission", ".xyz"),
    ("opacity", ".x"),
];

/// The bits of the six fragment channels — what a failing fragment program
/// covered.
const FRAGMENT_CHANNEL_BITS: u16 = SurfaceChannel::BaseColor.bit()
    | SurfaceChannel::Roughness.bit()
    | SurfaceChannel::Metallic.bit()
    | SurfaceChannel::Normal.bit()
    | SurfaceChannel::Emission.bit()
    | SurfaceChannel::Opacity.bit();

/// The WGSL `axiom_surface` function `surface` lowers to.
///
/// Deterministic: the same surface always yields byte-identical text, because
/// every step is an ordered fold and nothing here iterates a map. That is the
/// property a program cache keys on.
///
/// Emitted from the surface's **flattened** form, because flattening is what
/// composes a layered surface's per-channel graphs — and therefore its parameter
/// table — into the one program a backend runs.
pub(crate) fn surface_function(surface: &Surface) -> Result<String, SurfaceProgramError> {
    let program_id = surface.digest().raw();
    surface
        .flatten()
        .map_err(|error| {
            SurfaceProgramError::new(
                program_id,
                FRAGMENT_CHANNEL_BITS,
                SurfaceProgramFault::Flatten,
                String::from(error.message()),
            )
        })
        .map(|flat| function_text(&flat))
}

/// The function text for an already-flattened surface.
fn function_text(flat: &Surface) -> String {
    let (body, assignments, _base) = (0..FRAGMENT_CHANNEL_COUNT).fold(
        (String::new(), String::new(), 0_u32),
        |(body, assignments, base), index| {
            let graph = flat.binding(SurfaceChannel::ALL[index]).as_graph();
            let (lines, output) = channel_text(&graph, index, base);
            let (field, lane) = CHANNEL_TARGET[index];
            (
                body + &lines,
                assignments + &format!("    out.{field} = {output}{lane};\n"),
                base + graph.params().len() as u32,
            )
        },
    );
    format!(
        // `out.transmission = 0.0` is emitted unconditionally. A field-algebra
        // surface has no way to author transmission yet — there is no
        // `SurfaceChannel::Transmission` — and leaving the field unwritten
        // would hand the lighting stage whatever `var out: SurfaceOut` was
        // initialised to. An explicit zero is an exact identity there.
        "fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut {{\n\
         {body}    var out: SurfaceOut;\n{assignments}    out.transmission = 0.0;\n\
         \x20   return out;\n}}\n"
    )
}

/// Where the **vertex** stage's parameters begin in the shared uniform region:
/// the running total of every fragment channel's slot count.
///
/// `params::pack` writes the seven channels' slots end to end in
/// `SurfaceChannel::ALL` order, and displacement is last — so the vertex stage's
/// base is exactly the sum of the six that precede it. Derived from the same
/// `params()` lengths the fragment fold accumulates, so the two cannot disagree.
pub(crate) fn vertex_param_base(flat: &Surface) -> u32 {
    (0..FRAGMENT_CHANNEL_COUNT).fold(0_u32, |base, index| {
        base + flat
            .binding(SurfaceChannel::ALL[index])
            .as_graph()
            .params()
            .len() as u32
    })
}

/// One channel's `let` lines and the SSA name holding its result.
///
/// `param_base` is where this channel's parameters begin in the shared uniform
/// region — the running total of every earlier channel's slot count, which is
/// exactly the order `params::pack` writes them in.
///
/// Shared with [`crate::surface_program::emit_vertex`]: the vertex stage's
/// displacement channel is the same SSA fold over the same operator table, with
/// a different channel index and a different result type. Writing it twice would
/// be two definitions of one language.
pub(crate) fn channel_text(graph: &FieldGraph, channel: usize, param_base: u32) -> (String, String) {
    let types = node_types(graph);
    let param_count = graph.params().len() as u32;
    let body = graph
        .recipe()
        .nodes()
        .iter()
        .enumerate()
        .fold(String::new(), |body, (index, node)| {
            // `RecipeGraph::validate` — run by both `FieldGraph::deserialize` and
            // `FieldBuilder` — proves every input names a strictly earlier node,
            // so an input id always indexes the derived-type table.
            let operands: Vec<EmitOperand> = node
                .inputs()
                .iter()
                .take(MAX_INPUTS)
                .map(|input| {
                    let id = input.raw() as usize;
                    EmitOperand::new(node_name(channel, id), types[id])
                })
                .collect();
            let width = operands
                .iter()
                .fold(FieldType::Scalar, |widest, operand| widest.max(operand.ty()));
            let words: Vec<u32> = node.params().iter().map(|param| param.bits()).collect();
            let step = EmitStep::new(&operands, &words, width, param_base, param_count);
            body + &format!(
                "    let {} = {};\n",
                node_name(channel, index),
                emit_node(node.op(), &step)
            )
        });
    (body, node_name(channel, graph.output().raw() as usize))
}

/// The derived type of every node, in id order.
///
/// Preparation-time only. `FieldGraph::type_at` type-checks the whole graph to
/// answer for one node, so this is `O(nodes^2)` — which is 65 536 steps at the
/// 256-node budget, paid once per surface at bind, and the honest cost of the
/// layer not publishing its whole type vector.
///
/// A graph that does not type has no derived types to report; every node then
/// reads as `Scalar`, which is the same totality rule the evaluator follows
/// (`FieldValue::ZERO` is a `Scalar`). Such a graph cannot reach here through a
/// validated `Surface` — but the emitter still yields text rather than a fault,
/// exactly as the evaluator yields a value rather than a panic.
fn node_types(graph: &FieldGraph) -> Vec<FieldType> {
    (0..graph.node_count() as u32)
        .map(|index| {
            graph
                .type_at(NodeId::from_raw(index))
                .unwrap_or_else(|_error| FieldType::Scalar)
        })
        .collect()
}

/// The SSA name node `id` of channel `channel` is bound to.
fn node_name(channel: usize, id: usize) -> String {
    format!("c{channel}_n{id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldValue};
    use axiom_math::{Vec3, Vec4};
    use axiom_recipe::{Param, Scalar};
    use axiom_surface::{LayerBlend, SurfaceBuilder, SurfaceLayer};

    /// A vec4 base colour driven by `Uv.x`.
    fn uv_color() -> FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("gpu/emit/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let (builder, splat) = builder.push(
            FieldOp::Compose,
            vec![Param::int(4)],
            vec![lane, lane, lane, lane],
        );
        builder.build(splat)
    }

    /// A scalar chain of `steps` `Add`s over fresh constants: `2 * steps + 1`
    /// nodes.
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
    fn a_constant_surface_emits_the_channel_defaults_and_the_fixed_signature() {
        let surface = SurfaceBuilder::new().build().expect("legal");
        let text = surface_function(&surface).expect("a flat surface flattens");
        assert!(text.starts_with(
            "fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut {\n"
        ));
        assert!(text.ends_with("    return out;\n}\n"));
        // Every fragment channel is assigned, in channel order, through the
        // swizzle that narrows the four-lane register to its type.
        [
            "    out.base_color = c0_n0;\n",
            "    out.roughness = c1_n0.x;\n",
            "    out.metallic = c2_n0.x;\n",
            "    out.normal = c3_n0.xyz;\n",
            "    out.emission = c4_n0.xyz;\n",
            "    out.opacity = c5_n0.x;\n",
        ]
        .iter()
        .for_each(|line| assert!(text.contains(line), "missing {line}"));
        // The unbound channels are the `SurfaceChannel` defaults.
        assert!(text.contains("let c0_n0 = vec4<f32>(1.0, 1.0, 1.0, 1.0);"));
        assert!(text.contains("let c1_n0 = vec4<f32>(0.5, 0.0, 0.0, 0.0);"));
        assert!(text.contains("let c3_n0 = vec4<f32>(0.0, 0.0, 1.0, 0.0);"));
        // Displacement is a vertex-stage channel and is not emitted.
        assert!(!text.contains("c6_n"));
    }

    #[test]
    fn a_uv_driven_colour_emits_one_ssa_line_per_node_in_id_order() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("a vec4 uv field is a legal base colour");
        let text = surface_function(&surface).expect("flattens");
        let uv_at = text.find("let c0_n0 = vec4<f32>(in.uv, 0.0, 0.0);").expect("uv");
        let lane_at = text
            .find("let c0_n1 = vec4<f32>(c0_n0.x, 0.0, 0.0, 0.0);")
            .expect("component");
        let splat_at = text
            .find("let c0_n2 = vec4<f32>(c0_n1.x, c0_n1.x, c0_n1.x, c0_n1.x);")
            .expect("compose");
        assert!(uv_at < lane_at, "emission is in node id order");
        assert!(lane_at < splat_at);
        assert!(text.contains("    out.base_color = c0_n2;\n"));
    }

    #[test]
    fn the_same_surface_always_emits_byte_identical_text() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .constant(
                SurfaceChannel::Emission,
                FieldValue::vec4(Vec4::new(0.1, 0.2, 0.3, 0.0)),
            )
            .build()
            .expect("legal");
        let once = surface_function(&surface).expect("flattens");
        let twice = surface_function(&surface).expect("flattens");
        assert_eq!(once, twice, "generation must be a pure function");
        assert!(once.contains("let c4_n0 = vec4<f32>(0.1, 0.2, 0.3, 0.0);"));
    }

    #[test]
    fn each_channels_parameters_are_offset_past_every_earlier_channels() {
        // Two parameterised channels: base colour (four slots' worth of nothing —
        // one vec4 slot) and roughness (one slot). Roughness must read the slot
        // AFTER base colour's, because that is the order `params::pack` writes.
        let (builder, tint) = FieldBuilder::new(FieldId::of_name("gpu/emit/tint"), 1)
            .declare("tint", FieldValue::vec4(Vec4::new(1.0, 0.0, 0.0, 1.0)));
        let (builder, node) = builder.push_param(tint, FieldType::Vec4);
        let color = builder.build(node);
        let (builder, rough) = FieldBuilder::new(FieldId::of_name("gpu/emit/rough"), 1)
            .declare("rough", FieldValue::scalar(Scalar::new(0.25)));
        let (builder, node) = builder.push_param(rough, FieldType::Scalar);
        let roughness = builder.build(node);
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, color)
            .field(SurfaceChannel::Roughness, roughness)
            .build()
            .expect("legal");
        let text = surface_function(&surface).expect("flattens");
        assert!(text.contains("let c0_n0 = params.slots[0u];"));
        assert!(
            text.contains("let c1_n0 = params.slots[1u];"),
            "roughness reads the slot after base colour's: {text}"
        );
    }

    #[test]
    fn a_layer_tree_that_will_not_fit_the_node_budget_is_a_named_flatten_failure() {
        // Two ~130-node chains composed by a masked layer cannot fit the 256-node
        // field budget, so the surface will not flatten into one program.
        let layer = SurfaceLayer::new(
            SurfaceBuilder::new()
                .field(SurfaceChannel::Opacity, chain("gpu/emit/over", 65))
                .build()
                .expect("legal"),
            SurfaceLayer::opaque_mask(),
            LayerBlend::Over,
        );
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, chain("gpu/emit/under", 65))
            .layer(layer)
            .build()
            .expect("one layer is within budget");
        let error = surface_function(&surface).expect_err("the composition is over budget");
        assert_eq!(error.program_id(), surface.digest().raw());
        assert_eq!(error.fault(), SurfaceProgramFault::Flatten);
        assert_eq!(error.channels(), FRAGMENT_CHANNEL_BITS);
        assert!(error.to_string().contains("base_color"));
    }

    /// A graph whose first node carries an operator code the algebra does not
    /// have, built by patching the canonical bytes: the container's structural
    /// rules still pass, so it decodes, but it does not type.
    fn untypeable() -> FieldGraph {
        let (builder, node) = FieldBuilder::new(FieldId::of_name("gpu/emit/bad"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let mut bytes = builder.build(node).serialize();
        // The first node's `u16` operator code sits after the field schema stamp
        // (4), the recipe schema stamp (4), the recipe id (8), its version (4)
        // and its node count (4).
        bytes[24] = 99;
        FieldGraph::deserialize(&bytes).expect("the container's rules still hold")
    }

    #[test]
    fn a_graph_that_does_not_type_still_emits_the_zero_default_rather_than_faulting() {
        let graph = untypeable();
        assert!(graph.type_at(graph.output()).is_err());
        assert_eq!(node_types(&graph), vec![FieldType::Scalar]);
        let (body, output) = channel_text(&graph, 0, 0);
        assert_eq!(
            body,
            "    let c0_n0 = vec4<f32>(0.0, 0.0, 0.0, 0.0);\n",
            "an operator code the table does not have emits the zero value"
        );
        assert_eq!(output, "c0_n0");
    }

    #[test]
    fn a_node_carrying_more_inputs_than_any_operator_reads_is_truncated() {
        let (builder, one) = FieldBuilder::new(FieldId::of_name("gpu/emit/flood"), 1)
            .push_const(FieldValue::scalar(Scalar::new(2.0)));
        let (builder, node) = builder.push(
            FieldOp::Compose,
            vec![Param::int(4)],
            vec![one, one, one, one],
        );
        let graph = builder.build(node);
        let (body, _output) = channel_text(&graph, 3, 0);
        assert!(body.contains("let c3_n1 = vec4<f32>(c3_n0.x, c3_n0.x, c3_n0.x, c3_n0.x);"));
        assert_eq!(MAX_INPUTS, 4);
    }

    #[test]
    fn displacement_is_the_last_channel_so_the_fragment_set_is_a_prefix() {
        // The whole parameter-offset scheme rests on this: the six fragment
        // channels are `SurfaceChannel::ALL`'s first six, so skipping the
        // vertex-stage channel cannot shift any earlier channel's slots.
        assert_eq!(SurfaceChannel::ALL[FRAGMENT_CHANNEL_COUNT], SurfaceChannel::Displacement);
        assert_eq!(SurfaceChannel::ALL.len(), FRAGMENT_CHANNEL_COUNT + 1);
        assert_eq!(CHANNEL_TARGET.len(), FRAGMENT_CHANNEL_COUNT);
        // A displacing surface still emits its six fragment channels.
        let surface = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(Vec3::new(0.0, 1.0, 0.0)),
            )
            .build()
            .expect("legal");
        let text = surface_function(&surface).expect("flattens");
        assert!(text.contains("out.opacity = c5_n0.x;"));
        assert!(!text.contains("c6_n0"));
    }

    #[test]
    fn a_node_name_is_its_channel_and_its_id() {
        assert_eq!(node_name(0, 0), "c0_n0");
        assert_eq!(node_name(5, 17), "c5_n17");
    }
}
