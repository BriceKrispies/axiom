//! The canonical form: four passes that make two graphs computing the same
//! thing produce the same bytes, and therefore the same digest.
//!
//! 1. **Constant folding** — a node all of whose inputs are known constants
//!    becomes a `Const`. Exact only; see [`crate::const_fold`].
//! 2. **Common-subexpression elimination** — nodes are keyed by
//!    `(op, params, canonical input ids)` and the first node with a key is
//!    reused. `Add`, `Mul`, `Min` and `Max` sort their input ids first, so `a+b`
//!    and `b+a` are one node. Associativity and distributivity are **not**
//!    attempted: they move floating-point results.
//! 3. **Dead-node elimination** — nodes the output cannot reach are dropped.
//! 4. **Deterministic relabelling** — the survivors are emitted in ascending
//!    original id order into a fresh dense `0..n`. That order is already a valid
//!    topological order, so no sort is involved and no tie-break rule can drift.
//!
//! Passes 1 and 2 are one forward walk: ids are dense and processed in order, so
//! by the time a node is reached its inputs have already been folded, shared and
//! renumbered.
//!
//! ## The map, and where this may run
//!
//! CSE's key→id map is a [`BTreeMap`], not a hash map. Ordering is by the key's
//! own bytes, so nothing depends on a hasher; and the renderer has already been
//! burned once by per-frame hashing. **Canonicalisation is a preparation-time
//! operation.** Do not call it from a frame path.
//!
//! ## The parameter table is not touched
//!
//! Dead-node elimination may drop the last `Param` node reading a slot; the slot
//! stays. Shrinking the table would change [`crate::FieldGraph::digest`] for a
//! reason that is not structural, which is the one property the table exists to
//! protect.

use std::collections::BTreeMap;

use axiom_recipe::{Node, NodeId, Param, RecipeGraph};

use crate::const_fold::fold_value;
use crate::field_graph::FieldGraph;
use crate::field_op::{FieldOp, FIELD_OP_COUNT};
use crate::field_params::FieldParams;
use crate::field_value::FieldValue;

/// Which operators may have their input ids sorted before keying and emitting.
/// Indexed by the operator code, in discriminant order.
///
/// Only true commutativity qualifies. `Sub` is absent for the obvious reason;
/// `Dot` is absent because sorting is only sound where it changes nothing, and
/// leaving a two-input operator alone costs one missed merge, never a wrong
/// answer.
#[rustfmt::skip]
const COMMUTATIVE: [bool; FIELD_OP_COUNT] = [
    false,                              // Const
    false, false, false, false,         // Point / Uv / Normal / Time
    false,                              // Param
    true,  false, true,  true,  true,   // Add / Sub / Mul / Min / Max
    false,                              // Abs
    false, false, false,                // Clamp / Mix / Smoothstep
    false, false, false,                // Dot / Length / Normalize
    false, false,                       // Compose / Component
    false, false,                       // Noise / Fbm
    false,                              // Transform
];

/// The canonical form of `source`, which the caller has already validated.
pub(crate) fn canonicalize(source: &FieldGraph) -> FieldGraph {
    let shared = fold_and_share(source.recipe());
    let output = shared.remap[source.output().raw() as usize];
    let live = live_nodes(&shared.recipe, output);
    relabel(&shared.recipe, &live, output, source.params())
}

/// The state of the forward fold-and-share walk.
struct Shared {
    recipe: RecipeGraph,
    /// The first node emitted for each structural key.
    keys: BTreeMap<NodeKey, NodeId>,
    /// The constant value of each **emitted** node, when it has one.
    values: Vec<Option<FieldValue>>,
    /// Original id -> emitted id.
    remap: Vec<NodeId>,
}

/// A node's structural identity: its operator, its parameter words, and its
/// already-canonical input ids. Two nodes with the same key compute the same
/// value, so the second is redundant.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct NodeKey {
    op: u16,
    params: Vec<u32>,
    inputs: Vec<u32>,
}

impl Shared {
    fn new(source: &RecipeGraph) -> Self {
        Shared {
            recipe: RecipeGraph::new(source.id(), source.version()),
            keys: BTreeMap::new(),
            values: Vec::new(),
            remap: Vec::with_capacity(source.node_count()),
        }
    }

    /// Emit one node, reusing an identical earlier one if there is one.
    ///
    /// `&mut self` is deliberate and private: this is the accumulator of a fold,
    /// not an API. The Axiom State Law governs public boundaries.
    fn emit(
        &mut self,
        op: u16,
        params: Vec<Param>,
        inputs: Vec<NodeId>,
        value: Option<FieldValue>,
    ) -> NodeId {
        let key = NodeKey {
            op,
            params: params.iter().map(|param| param.bits()).collect(),
            inputs: inputs.iter().map(|input| input.raw()).collect(),
        };
        let existing = self.keys.get(&key).copied();
        let fresh = NodeId::from_raw(self.recipe.node_count() as u32);
        existing.is_none().then(|| {
            self.recipe.add(op, params, inputs);
            self.values.push(value);
            self.keys.insert(key, fresh);
        });
        existing.unwrap_or_else(|| fresh)
    }

    /// Fold, share and record one original node.
    fn absorb(mut self, node: &Node) -> Self {
        let op = FieldOp::from_code(node.op())
            .expect("canonicalisation runs only on a graph that has already type-checked");
        let inputs = order(op, node.inputs(), &self.remap);
        let known: Vec<Option<FieldValue>> = inputs
            .iter()
            .map(|input| self.values[input.raw() as usize])
            .collect();
        let folded = fold_value(op, &known, node.params());
        let (code, params, links) = folded
            .map(|value| (FieldOp::Const.code(), value.const_params(), Vec::new()))
            .unwrap_or_else(|| (node.op(), node.params().to_vec(), inputs));
        let id = self.emit(code, params, links, folded);
        self.remap.push(id);
        self
    }
}

/// Passes 1 and 2, as one forward walk in id order.
fn fold_and_share(source: &RecipeGraph) -> Shared {
    source
        .nodes()
        .iter()
        .fold(Shared::new(source), Shared::absorb)
}

/// A node's inputs, remapped into the emitted graph and put in canonical order.
///
/// The sort key is the input's *slot* for an ordinary operator — the identity
/// permutation — and the input's *id* for a commutative one. One expression, no
/// branch, and a non-commutative node's operand order is provably untouched.
fn order(op: FieldOp, inputs: &[NodeId], remap: &[NodeId]) -> Vec<NodeId> {
    let commutative = usize::from(COMMUTATIVE[op as usize]);
    let mut keyed: Vec<(u32, NodeId)> = inputs
        .iter()
        .enumerate()
        .map(|(slot, input)| {
            let id = remap[input.raw() as usize];
            ([slot as u32, id.raw()][commutative], id)
        })
        .collect();
    keyed.sort_unstable_by_key(|(key, _)| *key);
    keyed.into_iter().map(|(_, id)| id).collect()
}

/// Pass 3. Which nodes the output can reach.
///
/// A **reverse fold over the id-ordered node list**, written that way
/// deliberately: every input id is strictly smaller than its node's id, so one
/// descending pass propagates every mark to completion. The obvious
/// implementation — a recursive walk from the output — is banned
/// (`engine_no_recursion` is at 0) and would be strictly worse here anyway.
fn live_nodes(recipe: &RecipeGraph, output: NodeId) -> Vec<bool> {
    let seed: Vec<bool> = (0..recipe.node_count())
        .map(|index| index == output.raw() as usize)
        .collect();
    (0..recipe.node_count())
        .rev()
        .fold(seed, |mut live, index| {
            let alive = live[index];
            recipe.nodes()[index].inputs().iter().for_each(|input| {
                live[input.raw() as usize] |= alive;
            });
            live
        })
}

/// Pass 4. Emit the survivors in ascending original id order into a fresh dense
/// `0..n`.
fn relabel(
    recipe: &RecipeGraph,
    live: &[bool],
    output: NodeId,
    params: &FieldParams,
) -> FieldGraph {
    let mut kept = RecipeGraph::new(recipe.id(), recipe.version());
    let mut moved: Vec<NodeId> = vec![NodeId::NULL; recipe.node_count()];
    recipe.nodes().iter().enumerate().for_each(|(index, node)| {
        live[index].then(|| {
            let inputs: Vec<NodeId> = node
                .inputs()
                .iter()
                .map(|input| moved[input.raw() as usize])
                .collect();
            moved[index] = kept.add(node.op(), node.params().to_vec(), inputs);
        });
    });
    FieldGraph::new(kept, moved[output.raw() as usize], params.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::StableHash;
    use axiom_math::Vec3;
    use axiom_recipe::Scalar;

    use crate::field_builder::FieldBuilder;
    use crate::field_error::FieldErrorCode;
    use crate::field_type::FieldType;
    use crate::ids::FieldId;

    fn builder() -> FieldBuilder {
        FieldBuilder::new(FieldId::of_name("field/canonical"), 1)
    }

    fn constant(value: f32) -> FieldValue {
        FieldValue::scalar(Scalar::new(value))
    }

    /// A deliberately messy authoring of `length(point + 5)`: a dead `Uv`
    /// branch, a foldable constant chain, and the same sum written twice with
    /// its operands swapped.
    fn messy() -> FieldGraph {
        let (build, point) = builder().push(FieldOp::Point, Vec::new(), Vec::new());
        let (build, _dead) = build.push(FieldOp::Uv, Vec::new(), Vec::new());
        let (build, two) = build.push_const(constant(2.0));
        let (build, three) = build.push_const(constant(3.0));
        let (build, five) = build.push(FieldOp::Add, Vec::new(), vec![two, three]);
        let (build, _sum) = build.push(FieldOp::Add, Vec::new(), vec![point, five]);
        let (build, flipped) = build.push(FieldOp::Add, Vec::new(), vec![five, point]);
        let (build, length) = build.push(FieldOp::Length, Vec::new(), vec![flipped]);
        build.build(length)
    }

    /// The same field, authored the way canonicalisation would write it.
    fn tidy() -> FieldGraph {
        let (build, point) = builder().push(FieldOp::Point, Vec::new(), Vec::new());
        let (build, five) = build.push_const(constant(5.0));
        let (build, sum) = build.push(FieldOp::Add, Vec::new(), vec![five, point]);
        let (build, length) = build.push(FieldOp::Length, Vec::new(), vec![sum]);
        build.build(length)
    }

    fn canonical(field: &FieldGraph) -> FieldGraph {
        field.canonicalize().expect("the graph type-checks")
    }

    fn ops_of(field: &FieldGraph) -> Vec<u16> {
        field
            .recipe()
            .nodes()
            .iter()
            .map(|node| node.op())
            .collect()
    }

    fn const_lane(field: &FieldGraph, id: u32) -> f32 {
        f32::from_bits(
            field
                .recipe()
                .node(NodeId::from_raw(id))
                .expect("the node exists")
                .params()[1]
                .bits(),
        )
    }

    #[test]
    fn a_chain_of_constant_arithmetic_collapses_to_one_node() {
        let (build, two) = builder().push_const(constant(2.0));
        let (build, three) = build.push_const(constant(3.0));
        let (build, five) = build.push(FieldOp::Add, Vec::new(), vec![two, three]);
        let (build, four) = build.push_const(constant(4.0));
        let (build, twenty) = build.push(FieldOp::Mul, Vec::new(), vec![five, four]);
        let field = build.build(twenty);

        assert_eq!(field.node_count(), 5);
        let folded = canonical(&field);
        assert_eq!(folded.node_count(), 1);
        assert_eq!(ops_of(&folded), vec![FieldOp::Const.code()]);
        assert_eq!(const_lane(&folded, 0), 20.0);
        assert_eq!(folded.output(), NodeId::from_raw(0));
    }

    #[test]
    fn a_repeated_subexpression_becomes_one_node() {
        let (build, point) = builder().push(FieldOp::Point, Vec::new(), Vec::new());
        let (build, first) = build.push(FieldOp::Length, Vec::new(), vec![point]);
        let (build, second) = build.push(FieldOp::Length, Vec::new(), vec![point]);
        let (build, total) = build.push(FieldOp::Add, Vec::new(), vec![first, second]);
        let field = build.build(total);

        assert_eq!(field.node_count(), 4);
        let shared = canonical(&field);
        assert_eq!(shared.node_count(), 3);
        assert_eq!(
            shared
                .recipe()
                .node(NodeId::from_raw(2))
                .expect("the sum survives")
                .inputs(),
            [NodeId::from_raw(1), NodeId::from_raw(1)]
        );
    }

    #[test]
    fn a_plus_b_and_b_plus_a_are_the_same_node() {
        let (build, point) = builder().push(FieldOp::Point, Vec::new(), Vec::new());
        let (build, time) = build.push(FieldOp::Time, Vec::new(), Vec::new());
        let (build, forward) = build.push(FieldOp::Add, Vec::new(), vec![point, time]);
        let (build, backward) = build.push(FieldOp::Add, Vec::new(), vec![time, point]);
        let (build, total) = build.push(FieldOp::Add, Vec::new(), vec![forward, backward]);
        let field = build.build(total);

        assert_eq!(field.node_count(), 5);
        let shared = canonical(&field);
        assert_eq!(shared.node_count(), 4);
        assert_eq!(
            shared
                .recipe()
                .node(NodeId::from_raw(3))
                .expect("the outer sum survives")
                .inputs(),
            [NodeId::from_raw(2), NodeId::from_raw(2)]
        );
    }

    #[test]
    fn a_non_commutative_operator_keeps_its_operand_order() {
        let (build, point) = builder().push(FieldOp::Point, Vec::new(), Vec::new());
        let (build, time) = build.push(FieldOp::Time, Vec::new(), Vec::new());
        let (build, difference) = build.push(FieldOp::Sub, Vec::new(), vec![time, point]);
        let field = build.build(difference);

        let kept = canonical(&field);
        assert_eq!(
            kept.recipe()
                .node(NodeId::from_raw(2))
                .expect("the difference survives")
                .inputs(),
            [NodeId::from_raw(1), NodeId::from_raw(0)],
            "sorting a Sub's operands would change what it computes"
        );
    }

    #[test]
    fn a_branch_the_output_cannot_reach_is_dropped() {
        let (build, point) = builder().push(FieldOp::Point, Vec::new(), Vec::new());
        let (build, _dead_source) = build.push(FieldOp::Uv, Vec::new(), Vec::new());
        let (build, dead) = build.push(FieldOp::Length, Vec::new(), vec![_dead_source]);
        let (build, live) = build.push(FieldOp::Length, Vec::new(), vec![point]);
        let field = build.build(live);

        assert_eq!(field.node_count(), 4);
        assert_ne!(dead, live);
        let pruned = canonical(&field);
        assert_eq!(pruned.node_count(), 2);
        assert_eq!(
            ops_of(&pruned),
            vec![FieldOp::Point.code(), FieldOp::Length.code()]
        );
        assert_eq!(pruned.output(), NodeId::from_raw(1));
    }

    #[test]
    fn two_authoring_orders_produce_one_set_of_bytes_and_one_digest() {
        let from_messy = canonical(&messy());
        let from_tidy = canonical(&tidy());
        assert_eq!(messy().node_count(), 8);
        assert_eq!(tidy().node_count(), 4);
        assert_eq!(from_messy.serialize(), from_tidy.serialize());
        assert_eq!(from_messy.digest(), from_tidy.digest());
        assert_eq!(from_messy.node_count(), 4);
        assert_eq!(
            ops_of(&from_messy),
            vec![
                FieldOp::Point.code(),
                FieldOp::Const.code(),
                FieldOp::Add.code(),
                FieldOp::Length.code(),
            ]
        );
        assert_eq!(const_lane(&from_messy, 1), 5.0);
    }

    #[test]
    fn canonicalisation_is_idempotent() {
        let once = canonical(&messy());
        let twice = canonical(&once);
        assert_eq!(once.serialize(), twice.serialize());
        assert!(once.is_canonical());
        assert!(canonical(&tidy()).is_canonical());
        // Neither authoring is canonical as written: `messy` carries dead and
        // foldable nodes, and `tidy` writes its sum's operands the other way up.
        assert!(!messy().is_canonical());
        assert!(!tidy().is_canonical());
    }

    #[test]
    fn a_canonical_graph_still_validates_and_still_type_checks() {
        let kept = canonical(&messy());
        assert_eq!(kept.validate(), Ok(()));
        assert_eq!(kept.type_of(kept.output()), Ok(FieldType::Scalar));
        assert_eq!(kept.type_of(NodeId::from_raw(0)), Ok(FieldType::Vec3));
    }

    #[test]
    fn a_graph_that_does_not_type_has_no_canonical_form() {
        let (build, point) = builder().push(FieldOp::Point, Vec::new(), Vec::new());
        let (build, uv) = build.push(FieldOp::Uv, Vec::new(), Vec::new());
        let (build, bad) = build.push(FieldOp::Add, Vec::new(), vec![point, uv]);
        let field = build.build(bad);
        let error = field
            .canonicalize()
            .expect_err("Vec3 and Vec2 do not meet");
        assert_eq!(error.kind(), FieldErrorCode::TypeMismatch);
        assert!(!field.is_canonical());

        let orphaned = builder()
            .push(FieldOp::Point, Vec::new(), Vec::new())
            .0
            .build(NodeId::from_raw(9));
        assert_eq!(
            orphaned
                .canonicalize()
                .expect_err("node 9 does not exist")
                .kind(),
            FieldErrorCode::OutputNodeMissing
        );
    }

    #[test]
    fn a_spatial_sampler_over_a_constant_point_folds_to_its_value() {
        let (build, origin) = builder().push_const(FieldValue::vec3(Vec3::new(0.5, 0.25, 0.0)));
        let (build, grain) = build.push_noise(99, origin);
        let field = build.build(grain);

        assert_eq!(field.validate(), Ok(()));
        let folded = canonical(&field);
        assert_eq!(folded.node_count(), 1);
        assert_eq!(ops_of(&folded), vec![FieldOp::Const.code()]);
        assert_eq!(
            const_lane(&folded, 0),
            axiom_noise::value_noise(99, Vec3::new(0.5, 0.25, 0.0)).get()
        );
    }

    #[test]
    fn the_operators_that_read_the_context_or_the_table_survive_untouched() {
        let (build, slot) = builder().declare("knob", constant(0.5));
        let (build, knob) = build.push_param(slot, FieldType::Scalar);
        let (build, point) = build.push(FieldOp::Point, Vec::new(), Vec::new());
        let (build, noise) = build.push(
            FieldOp::Noise,
            (0..2).map(Param::int).collect(),
            vec![point],
        );
        let (build, fbm) = build.push(FieldOp::Fbm, (0..6).map(Param::int).collect(), vec![point]);
        let (build, moved) = build.push(
            FieldOp::Transform,
            (0..4).map(Param::int).collect(),
            vec![point],
        );
        let (build, grain) = build.push(FieldOp::Add, Vec::new(), vec![noise, fbm]);
        let (build, tuned) = build.push(FieldOp::Add, Vec::new(), vec![knob, grain]);
        let (build, distance) = build.push(FieldOp::Length, Vec::new(), vec![moved]);
        let (build, total) = build.push(FieldOp::Add, Vec::new(), vec![tuned, distance]);
        let field = build.build(total);

        assert_eq!(field.validate(), Ok(()));
        let kept = canonical(&field);
        assert_eq!(kept.node_count(), field.node_count());
        assert_eq!(ops_of(&kept), ops_of(&field));
        assert!(field.is_canonical());
        assert_eq!(kept.params().len(), 1);
    }

    /// The committed golden: the canonical form of [`messy`].
    ///
    /// Four nodes — `Point`, the folded `Const 5`, their sum, and its length —
    /// with the dead `Uv`, the two folded constants and the duplicated sum all
    /// gone. A change to any pass moves these bytes, which is the point.
    #[rustfmt::skip]
    const GOLDEN_CANONICAL: [u8; 104] = [
        1, 0, 0, 0,                                     // field schema 1.0
        1, 0, 0, 0,                                     // recipe schema 1.0
        0, 0, 0, 0, 0, 0, 0, 0,                         // recipe id (patched below)
        1, 0, 0, 0,                                     // version 1
        4, 0, 0, 0,                                     // node count 4
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0,                   // 0: Point
        0, 0, 5, 0, 0, 0,                               // 1: Const, 5 param words...
        0, 0, 0, 0,                                     //    type = Scalar
        0, 0, 160, 64,                                  //    x = 5.0
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,             //    y, z, w = 0
        0, 0, 0, 0,                                     //    no inputs
        6, 0, 0, 0, 0, 0, 2, 0, 0, 0,                   // 2: Add, 0 params, 2 inputs...
        0, 0, 0, 0, 1, 0, 0, 0,                         //    nodes 0 and 1
        16, 0, 0, 0, 0, 0, 1, 0, 0, 0,                  // 3: Length, 1 input...
        2, 0, 0, 0,                                     //    node 2
        3, 0, 0, 0,                                     // output = node 3
        0, 0, 0, 0,                                     // no parameter slots
    ];

    #[test]
    fn the_canonical_golden_bytes_and_digest_are_unchanged() {
        let kept = canonical(&messy());
        let mut expected = GOLDEN_CANONICAL;
        expected[8..16].copy_from_slice(&FieldId::of_name("field/canonical").raw().to_le_bytes());
        assert_eq!(kept.serialize(), expected);
        assert_eq!(kept.digest(), StableHash::from_raw(GOLDEN_CANONICAL_DIGEST));
        assert_eq!(
            FieldGraph::deserialize(&kept.serialize()),
            Ok(kept),
            "the canonical bytes must decode to the graph that produced them"
        );
    }

    /// The digest of the canonical form above.
    const GOLDEN_CANONICAL_DIGEST: u64 = 17_273_213_141_671_979_415;
}
