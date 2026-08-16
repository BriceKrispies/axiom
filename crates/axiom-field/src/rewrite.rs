//! Rewriting a graph: replace a node, insert an operation, inline a library
//! graph.
//!
//! ## Every rewrite returns a new graph
//!
//! There is no `&mut self` here, and that is not a stylistic preference. The
//! Axiom State Law forbids `&mut self` on a public boundary, and immutability is
//! what makes a rewrite **diffable and revertible** — the property an agent needs
//! most. An agent holds the graph it started from, the graph it produced, and
//! [`FieldGraph::diff`] between them; nothing was mutated, so nothing has to be
//! undone.
//!
//! ## A rewrite does not validate, and the caller must
//!
//! A rewrite is a structural splice. It can happily produce a graph whose types
//! do not compose (inlining a `Vec2` expression where a `Vec3` was read), and it
//! says nothing about that. **Re-validate every rewrite result** with
//! [`FieldGraph::validate`] before evaluating, lowering or storing it. The one
//! thing every rewrite *does* enforce is the node budget, because a graph past
//! it cannot even be evaluated.
//!
//! ## Reusable material functions need no machinery at all
//!
//! A library effect — marble, wood grain, scratches — is a `FieldGraph` whose
//! **leaf** `Point`, `Uv` and `Param` nodes stand for its free variables.
//! [`FieldGraph::inline`] binds those leaves to nodes of the host graph and
//! appends the rest. There is no function type, no call node, no linker and no
//! symbol table: a library of a hundred effects costs this crate nothing.
//!
//! Inlining multiplies node count against [`MAX_NODES`], so a composition that
//! does not fit is rejected with [`FieldErrorCode::InlineBudgetExceeded`]. That
//! is a design signal to compose fewer layers. **It is not a reason to raise the
//! cap.**
//!
//! ## Relationship to `axiom-surface`'s composer
//!
//! `axiom_surface` has its own graph composer, and it is deliberately a
//! different shape: it accumulates *several* graphs plus freshly-pushed nodes
//! into one builder, which is what a blend of three bindings needs. The
//! operations here are two-graph **value** transforms — a graph in, a graph out.
//! Neither is expressible as the other without contorting one of them, so both
//! exist and both rebase parameter slots by the same rule.

use axiom_recipe::{Node, NodeId, Param, RecipeGraph, MAX_NODES};

use crate::field_error::{FieldError, FieldErrorCode, FieldResult};
use crate::field_graph::FieldGraph;
use crate::field_op::{FieldOp, FIELD_OP_COUNT};
use crate::field_params::FieldParams;
use crate::field_value::FieldValue;
use crate::ids::FieldParamSlot;

/// A rewrite would produce a graph past the node budget.
const INLINE_BUDGET_EXCEEDED: FieldError = FieldError::at(
    FieldErrorCode::InlineBudgetExceeded,
    NodeId::NULL,
    "the rewrite would produce a graph with more nodes than the node budget allows",
);

/// An inline supplied a different number of bindings than the inlined graph has
/// bindable leaves.
const BINDING_COUNT_MISMATCH: FieldError = FieldError::at(
    FieldErrorCode::BindingCountMismatch,
    NodeId::NULL,
    "the inlined graph has a different number of bindable leaves than bindings supplied",
);

/// Which operators are a graph's **bindable leaves** — the free variables an
/// inline substitutes. Indexed by operator code, in discriminant order.
///
/// `Point` and `Uv` are the domain a library effect is written over, and `Param`
/// is its knob. `Normal` and `Time` are deliberately absent: they are ambient
/// facts about *where and when* a sample is taken, identical in host and
/// library, and rebinding them is not a composition an author has asked for.
/// `Const` is absent because a literal is not a free variable.
#[rustfmt::skip]
const BINDABLE: [bool; FIELD_OP_COUNT] = [
    false,                              // Const
    true,  true,  false, false,         // Point / Uv / Normal / Time
    true,                               // Param
    false, false, false, false, false,  // Add / Sub / Mul / Min / Max
    false,                              // Abs
    false, false, false,                // Clamp / Mix / Smoothstep
    false, false, false,                // Dot / Length / Normalize
    false, false,                       // Compose / Component
    false, false,                       // Noise / Fbm
    false,                              // Transform
];

impl FieldGraph {
    /// The graph with the expression at `at` **replaced** by `with`.
    ///
    /// Every node that read `at` — and the declared output, if it was `at` —
    /// reads `with`'s output instead. `with` is spliced in verbatim: its `Point`
    /// and `Uv` nodes still read the evaluation context and its `Param` nodes
    /// keep their own values, rebased onto the merged parameter table.
    ///
    /// The replaced node stays in the graph as an **unreachable** node, along
    /// with anything only it reached. That is deliberate: dropping them is
    /// [`FieldGraph::canonicalize`]'s job, stated in one place, and a rewrite
    /// that quietly deleted a subgraph would be a second definition of
    /// reachability.
    ///
    /// Authoring-time only, and the result must be re-validated.
    pub fn replace_subgraph(&self, at: NodeId, with: &FieldGraph) -> FieldResult<FieldGraph> {
        self.check_node(at)
            .map(|()| splice_at(self, at, with, false))
            .and_then(within_budget)
    }

    /// The graph with `node` inserted into the dataflow **immediately after
    /// `at`, before every consumer of it**.
    ///
    /// `at` survives and keeps computing what it computed; everything that read
    /// `at` now reads `node`'s output instead. Every one of `node`'s bindable
    /// leaves (see [`FieldGraph::inline`]) is bound to `at`, so a one-argument
    /// transform written over `Point` — `Clamp(Point, lo, hi)`, `Mul(Point, k)` —
    /// inserts as itself with no ceremony. An insertion that must keep its own
    /// knobs is [`FieldGraph::inline`] with an explicit binding list, followed by
    /// [`FieldGraph::replace_subgraph`].
    ///
    /// Authoring-time only, and the result must be re-validated.
    pub fn insert_before(&self, at: NodeId, node: &FieldGraph) -> FieldResult<FieldGraph> {
        self.check_node(at)
            .map(|()| splice_at(self, at, node, true))
            .and_then(within_budget)
    }

    /// The graph with `other` appended, its bindable leaves bound to nodes of
    /// this graph, and its output declared as the result.
    ///
    /// **This is how a reusable material function works, and it is the whole
    /// mechanism.** `other`'s bindable leaves — its `Point`, `Uv` and `Param`
    /// nodes, in node id order — are the parameters of the function; `bind` is
    /// the argument list, one host [`NodeId`] per leaf, in that same order.
    /// Because every leaf is bound, `other`'s parameter table plays no part: its
    /// knobs become the host's nodes.
    ///
    /// Every node of this graph is kept and keeps its id, so an id an agent held
    /// before the inline still names the same expression afterwards. Nodes the
    /// new output cannot reach are dropped by
    /// [`FieldGraph::canonicalize`], not here.
    ///
    /// Fails with [`FieldErrorCode::BindingCountMismatch`] when `bind` is not the
    /// leaf count, [`FieldErrorCode::OutputNodeMissing`] when a binding names no
    /// node of this graph, and [`FieldErrorCode::InlineBudgetExceeded`] when the
    /// two graphs together do not fit [`MAX_NODES`].
    ///
    /// Authoring-time only, and the result must be re-validated.
    pub fn inline(&self, other: &FieldGraph, bind: &[NodeId]) -> FieldResult<FieldGraph> {
        let leaves = bindable_leaves(other);
        (leaves.len() == bind.len())
            .then_some(())
            .ok_or(BINDING_COUNT_MISMATCH)
            .and_then(|()| {
                bind.iter()
                    .try_fold((), |(), node| self.check_node(*node))
            })
            .map(|()| {
                let mut splice = Splice::empty(self);
                let base = splice.merge_params(self.params().values());
                let host = splice.graft(self, &[], base);
                let bound = positional_binding(other, &leaves, bind, &host);
                let inner = splice.graft(other, &bound, base);
                splice.finish(lookup(&inner, other.output()))
            })
            .and_then(within_budget)
    }
}

/// A graph being assembled out of parts of others.
///
/// `&mut self` on its methods is deliberate and **private**: this is the
/// accumulator of a fold, not an API, exactly as `canonical::Shared` is. The
/// Axiom State Law governs public boundaries.
struct Splice {
    recipe: RecipeGraph,
    params: FieldParams,
    slots: u16,
}

impl Splice {
    /// A splice carrying `host`'s recipe identity, with no nodes and no
    /// parameter slots yet.
    fn empty(host: &FieldGraph) -> Splice {
        Splice {
            recipe: RecipeGraph::new(host.recipe().id(), host.recipe().version()),
            params: FieldParams::new(),
            slots: 0,
        }
    }

    /// Append `values` to the merged parameter table, returning the slot base
    /// every `Param` node of that graph must be rebased onto.
    fn merge_params(&mut self, values: &[FieldValue]) -> u16 {
        let base = self.slots;
        let table = core::mem::take(&mut self.params);
        self.params = values.iter().enumerate().fold(table, |table, (index, value)| {
            table.with(
                FieldParamSlot::from_raw(base.saturating_add(index as u16)),
                *value,
            )
        });
        self.slots = base.saturating_add(values.len() as u16);
        base
    }

    /// Append one node, remapping its inputs through `map` and rebasing a
    /// `Param` node's slot word by `base`.
    fn copy(&mut self, node: &Node, map: &[NodeId], base: u16) -> NodeId {
        let inputs: Vec<NodeId> = node
            .inputs()
            .iter()
            .map(|input| lookup(map, *input))
            .collect();
        self.recipe.add(node.op(), rebase(node, base), inputs)
    }

    /// Append every node of `source`, substituting `subst[i]` for source node
    /// `i` wherever it is `Some`. Returns the source id → spliced id map.
    ///
    /// A substituted node is **not** emitted: the substitution *is* its value.
    /// The id a copy would have taken is computed before the copy so the two
    /// cases share one expression rather than a branch.
    fn graft(&mut self, source: &FieldGraph, subst: &[Option<NodeId>], base: u16) -> Vec<NodeId> {
        source
            .recipe()
            .nodes()
            .iter()
            .enumerate()
            .fold(Vec::new(), |mut map, (index, node)| {
                let bound = subst.get(index).copied().flatten();
                let next = NodeId::from_raw(self.recipe.node_count() as u32);
                bound.is_none().then(|| self.copy(node, &map, base));
                map.push(bound.unwrap_or_else(|| next));
                map
            })
    }

    /// Finish, declaring `output` as the spliced graph's result.
    fn finish(self, output: NodeId) -> FieldGraph {
        FieldGraph::new(self.recipe, output, self.params)
    }
}

/// The shared machinery of [`FieldGraph::replace_subgraph`] and
/// [`FieldGraph::insert_before`]: one forward pass over `host`, splicing `other`
/// in at `at`.
///
/// `bind_leaves` is the whole difference between the two. An **insertion** binds
/// `other`'s leaves to the value `at` just produced, so `other` reads it; a
/// **replacement** binds nothing, so `other` stands alone and brings its own
/// parameter slots with it.
fn splice_at(host: &FieldGraph, at: NodeId, other: &FieldGraph, bind_leaves: bool) -> FieldGraph {
    let mut splice = Splice::empty(host);
    let host_base = splice.merge_params(host.params().values());
    let other_base = splice.merge_params(
        [&[] as &[FieldValue], other.params().values()][usize::from(!bind_leaves)],
    );
    let index_of = at.raw() as usize;
    let map = host
        .recipe()
        .nodes()
        .iter()
        .enumerate()
        .fold(Vec::new(), |mut map, (index, node)| {
            let copied = splice.copy(node, &map, host_base);
            let spliced = (index == index_of).then(|| {
                let subst = leaf_binding(other, copied, bind_leaves);
                let inner = splice.graft(other, &subst, other_base);
                lookup(&inner, other.output())
            });
            map.push(spliced.unwrap_or_else(|| copied));
            map
        });
    splice.finish(lookup(&map, host.output()))
}

/// The bindable leaves of `graph`, in node id order — the parameters of the
/// function a library graph is.
fn bindable_leaves(graph: &FieldGraph) -> Vec<NodeId> {
    graph
        .recipe()
        .nodes()
        .iter()
        .enumerate()
        .filter(|(_index, node)| is_bindable(node))
        .map(|(index, _node)| NodeId::from_raw(index as u32))
        .collect()
}

/// Whether a node is a bindable leaf. An operator code that names no operator is
/// not one — there is nothing to bind.
fn is_bindable(node: &Node) -> bool {
    BINDABLE
        .get(node.op() as usize)
        .copied()
        .unwrap_or_default()
}

/// Every bindable leaf of `other` bound to `target`, indexed by `other`'s node
/// ids. With `enabled` false, nothing is bound.
fn leaf_binding(other: &FieldGraph, target: NodeId, enabled: bool) -> Vec<Option<NodeId>> {
    other
        .recipe()
        .nodes()
        .iter()
        .map(|node| (enabled & is_bindable(node)).then_some(target))
        .collect()
}

/// `other`'s leaves bound to `bind`, positionally, through the host's id map.
fn positional_binding(
    other: &FieldGraph,
    leaves: &[NodeId],
    bind: &[NodeId],
    host: &[NodeId],
) -> Vec<Option<NodeId>> {
    let bound: Vec<(usize, NodeId)> = leaves
        .iter()
        .zip(bind.iter())
        .map(|(leaf, target)| (leaf.raw() as usize, lookup(host, *target)))
        .collect();
    (0..other.node_count())
        .map(|index| {
            bound
                .iter()
                .find(|(leaf, _target)| *leaf == index)
                .map(|(_leaf, target)| *target)
        })
        .collect()
}

/// One node's parameter words, with a `Param` node's slot word rebased by
/// `base`. One expression, no branch: the rebased word is selected by table
/// index, and a node that is not a `Param` provably keeps every word.
fn rebase(node: &Node, base: u16) -> Vec<Param> {
    let is_param = usize::from(node.op() == FieldOp::Param.code());
    node.params()
        .iter()
        .enumerate()
        .map(|(word, value)| {
            [
                *value,
                Param::int(value.as_int().wrapping_add(u32::from(base))),
            ][is_param & usize::from(word == 0)]
        })
        .collect()
}

/// The spliced id of an original id, or [`NodeId::NULL`] when the original named
/// no node — which makes the spliced graph fail validation with this layer's own
/// diagnostic rather than panicking on a hostile graph.
fn lookup(map: &[NodeId], original: NodeId) -> NodeId {
    map.get(original.raw() as usize)
        .copied()
        .unwrap_or_else(|| NodeId::NULL)
}

/// A rewrite result that fits the node budget, or the design signal that it does
/// not.
fn within_budget(graph: FieldGraph) -> FieldResult<FieldGraph> {
    let fits = graph.node_count() <= MAX_NODES;
    fits.then_some(graph).ok_or(INLINE_BUDGET_EXCEEDED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3};
    use axiom_recipe::Scalar;

    use crate::eval_context::EvalContext;
    use crate::field_builder::FieldBuilder;
    use crate::field_type::FieldType;
    use crate::ids::FieldId;

    fn scalar(value: f32) -> FieldValue {
        FieldValue::scalar(Scalar::new(value))
    }

    /// `length(point) + 1` — three nodes, the sum shared by nobody.
    fn host() -> FieldGraph {
        let (build, point) = FieldBuilder::new(FieldId::of_name("field/rewrite/host"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, length) = build.push(FieldOp::Length, Vec::new(), vec![point]);
        let (build, one) = build.push_const(scalar(1.0));
        let (build, sum) = build.push(FieldOp::Add, Vec::new(), vec![length, one]);
        build.build(sum)
    }

    /// `abs(point)` — a one-argument library function over the domain leaf
    /// `Point`.
    fn absolute() -> FieldGraph {
        let (build, point) = FieldBuilder::new(FieldId::of_name("field/rewrite/abs"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, magnitude) = build.push(FieldOp::Abs, Vec::new(), vec![point]);
        build.build(magnitude)
    }

    fn at(point: Vec3) -> EvalContext {
        EvalContext::at(point, Vec2::ZERO, Vec3::UNIT_Y)
    }

    #[test]
    fn a_replaced_node_is_read_by_everything_that_read_it() {
        let base = host();
        // Replace `length(point)` (node 1) with the literal 10.
        let (build, ten) = FieldBuilder::new(FieldId::from_raw(1), 1).push_const(scalar(10.0));
        let rewritten = base
            .replace_subgraph(NodeId::from_raw(1), &build.build(ten))
            .expect("node 1 exists");
        assert_eq!(rewritten.validate(), Ok(()));
        assert_eq!(
            rewritten.evaluate(&at(Vec3::new(3.0, 4.0, 0.0))),
            Ok(scalar(11.0))
        );
        // The original is untouched: a rewrite is a value, not a mutation.
        assert_eq!(base.evaluate(&at(Vec3::new(3.0, 4.0, 0.0))), Ok(scalar(6.0)));
    }

    #[test]
    fn replacing_the_output_replaces_the_whole_field() {
        let (build, uv) = FieldBuilder::new(FieldId::from_raw(2), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (build, lane) = build.push(FieldOp::Component, vec![Param::int(1)], vec![uv]);
        let rewritten = host()
            .replace_subgraph(host().output(), &build.build(lane))
            .expect("the output exists");
        assert_eq!(rewritten.validate(), Ok(()));
        assert_eq!(
            rewritten.evaluate(&EvalContext::at(
                Vec3::ZERO,
                Vec2::new(0.25, 0.75),
                Vec3::UNIT_Y
            )),
            Ok(scalar(0.75))
        );
        // The replaced cone survives as unreachable nodes until canonicalisation.
        assert_eq!(rewritten.node_count(), host().node_count() + 2);
        assert_eq!(
            rewritten
                .canonicalize()
                .expect("it types")
                .node_count(),
            2
        );
    }

    #[test]
    fn a_replacement_brings_its_own_parameter_slots_rebased_onto_the_host_table() {
        let (build, slot) = FieldBuilder::new(FieldId::of_name("field/rewrite/host-knob"), 1)
            .declare("host", scalar(2.0));
        let (build, knob) = build.push_param(slot, FieldType::Scalar);
        let (build, doubled) = build.push(FieldOp::Add, Vec::new(), vec![knob, knob]);
        let tuned = build.build(doubled);

        let (build, other_slot) = FieldBuilder::new(FieldId::from_raw(3), 1)
            .declare("library", scalar(5.0));
        let (build, other_knob) = build.push_param(other_slot, FieldType::Scalar);
        let library = build.build(other_knob);

        let rewritten = tuned
            .replace_subgraph(knob, &library)
            .expect("the knob node exists");
        assert_eq!(rewritten.params().len(), 2);
        assert_eq!(rewritten.validate(), Ok(()));
        // Both knobs survive: the host's in slot 0, the library's rebased to 1,
        // and the sum now reads the library's value twice.
        assert_eq!(rewritten.evaluate(&EvalContext::ORIGIN), Ok(scalar(10.0)));
    }

    #[test]
    fn an_insertion_wraps_the_value_at_a_node_for_every_later_reader() {
        // Insert `abs` after `point`, so the length is taken of |point|.
        let rewritten = host()
            .insert_before(NodeId::from_raw(0), &absolute())
            .expect("node 0 exists");
        assert_eq!(rewritten.validate(), Ok(()));
        assert_eq!(
            rewritten.evaluate(&at(Vec3::new(-3.0, -4.0, 0.0))),
            Ok(scalar(6.0))
        );
        // `Point` itself still computes what it always did; the insertion is
        // downstream of it.
        assert_eq!(
            rewritten.evaluate_at(&at(Vec3::new(-3.0, -4.0, 0.0)), NodeId::from_raw(0)),
            Ok(FieldValue::vec3(Vec3::new(-3.0, -4.0, 0.0)))
        );
        // Only the `Abs` is emitted: the library graph's `Point` leaf was bound
        // to the host node and never became a node of its own.
        assert_eq!(host().node_count() + 1, rewritten.node_count());
    }

    #[test]
    fn inserting_at_the_output_wraps_the_whole_field() {
        let rewritten = host()
            .insert_before(host().output(), &absolute())
            .expect("the output exists");
        assert_eq!(rewritten.validate(), Ok(()));
        // length(point) + 1 = -4 is impossible, so drive it through a point that
        // makes the sum negative is not available; assert the wrap is applied by
        // its type and value instead.
        assert_eq!(
            rewritten.evaluate(&at(Vec3::new(3.0, 4.0, 0.0))),
            Ok(scalar(6.0))
        );
        assert_eq!(rewritten.output().raw(), (rewritten.node_count() - 1) as u32);
    }

    #[test]
    fn a_rewrite_names_a_node_the_graph_does_not_have() {
        let missing = NodeId::from_raw(9);
        [
            host().replace_subgraph(missing, &absolute()).err(),
            host().insert_before(missing, &absolute()).err(),
            host().inline(&absolute(), &[missing]).err(),
        ]
        .iter()
        .for_each(|error| {
            let error = error.expect("node 9 does not exist");
            assert_eq!(error.kind(), FieldErrorCode::OutputNodeMissing);
            assert_eq!(error.node(), missing);
        });
    }

    #[test]
    fn inline_binds_every_leaf_positionally_and_declares_the_library_output() {
        // A library function of two arguments: `mix(a, b, uv.x)` written over
        // `Point`, `Uv` and a `Param` knob.
        let (build, a) = FieldBuilder::new(FieldId::of_name("field/rewrite/lib"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, uv) = build.push(FieldOp::Uv, Vec::new(), Vec::new());
        let (build, slot) = build.declare("t", scalar(0.0));
        let (build, knob) = build.push_param(slot, FieldType::Scalar);
        let (build, lane) = build.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let (build, mixed) = build.push(FieldOp::Mix, Vec::new(), vec![a, lane, knob]);
        let library = build.build(mixed);
        assert_eq!(bindable_leaves(&library), vec![a, uv, knob]);

        // Bind: the argument `Point` -> host node 1 (a scalar length), the `Uv`
        // -> host node 2 (the literal 1), the knob -> host node 2 as well.
        let one = NodeId::from_raw(2);
        let inlined = host()
            .inline(&library, &[NodeId::from_raw(1), one, one])
            .expect("three leaves, three bindings");
        assert_eq!(inlined.validate(), Ok(()));
        // mix(length(p), 1, 1) = 1, whatever the point is.
        assert_eq!(inlined.evaluate(&at(Vec3::new(3.0, 4.0, 0.0))), Ok(scalar(1.0)));
        // Every host node kept its id.
        assert_eq!(inlined.op_at(NodeId::from_raw(0)), Ok(FieldOp::Point));
        assert_eq!(inlined.op_at(NodeId::from_raw(3)), Ok(FieldOp::Add));
        // The library's own parameter table played no part: every leaf was bound.
        assert_eq!(inlined.params().len(), 0);
    }

    #[test]
    fn a_binding_list_of_the_wrong_length_is_rejected() {
        let error = host()
            .inline(&absolute(), &[])
            .expect_err("abs has one bindable leaf and no binding was given");
        assert_eq!(error.kind(), FieldErrorCode::BindingCountMismatch);
        assert_eq!(error.code(), 15);
        assert_eq!(
            host()
                .inline(
                    &absolute(),
                    &[NodeId::from_raw(0), NodeId::from_raw(1)]
                )
                .expect_err("one leaf, two bindings")
                .kind(),
            FieldErrorCode::BindingCountMismatch
        );
    }

    #[test]
    fn a_library_graph_with_no_leaves_inlines_with_no_bindings() {
        let (build, literal) = FieldBuilder::new(FieldId::from_raw(4), 1).push_const(scalar(7.0));
        let inlined = host()
            .inline(&build.build(literal), &[])
            .expect("a literal has no bindable leaves");
        assert_eq!(inlined.validate(), Ok(()));
        assert_eq!(inlined.evaluate(&EvalContext::ORIGIN), Ok(scalar(7.0)));
    }

    /// A graph of `count` parameterless nodes — the cheapest way to spend the
    /// node budget.
    fn wide(name: &str, count: usize) -> FieldGraph {
        let (build, last) = (0..count).fold(
            (FieldBuilder::new(FieldId::of_name(name), 1), NodeId::NULL),
            |(build, _last), _| build.push(FieldOp::Time, Vec::new(), Vec::new()),
        );
        build.build(last)
    }

    #[test]
    fn an_over_budget_rewrite_is_rejected_rather_than_produced() {
        let big = wide("field/rewrite/big", 200);
        let error = big
            .inline(&wide("field/rewrite/other", 100), &[])
            .expect_err("300 nodes do not fit the budget of 256");
        assert_eq!(error.kind(), FieldErrorCode::InlineBudgetExceeded);
        assert_eq!(error.code(), 14);
        assert_eq!(error.node(), NodeId::NULL);
        // Exactly the budget is admitted, so the cap is a cap and not a fence.
        let exact = big
            .inline(&wide("field/rewrite/fit", MAX_NODES - 200), &[])
            .expect("256 nodes are exactly the budget");
        assert_eq!(exact.node_count(), MAX_NODES);
        // The positional rewrites are held to the same budget.
        assert_eq!(
            big.replace_subgraph(NodeId::from_raw(0), &wide("field/rewrite/rep", 100))
                .expect_err("300 nodes do not fit")
                .kind(),
            FieldErrorCode::InlineBudgetExceeded
        );
        assert_eq!(
            big.insert_before(NodeId::from_raw(0), &wide("field/rewrite/ins", 100))
                .expect_err("300 nodes do not fit")
                .kind(),
            FieldErrorCode::InlineBudgetExceeded
        );
    }

    #[test]
    fn a_rewrite_that_does_not_type_is_produced_and_the_caller_must_validate_it() {
        // `Uv` is a Vec2; adding it to the Vec3 `Point` does not compose — two
        // non-scalar widths never meet. The rewrite happily builds it anyway:
        // validation is the caller's step.
        let (build, point) = FieldBuilder::new(FieldId::from_raw(5), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, offset) = build.push_const(FieldValue::vec3(Vec3::ZERO));
        let (build, sum) = build.push(FieldOp::Add, Vec::new(), vec![point, offset]);
        let vectors = build.build(sum);
        assert_eq!(vectors.validate(), Ok(()));

        let (build, uv) = FieldBuilder::new(FieldId::from_raw(6), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let rewritten = vectors
            .replace_subgraph(offset, &build.build(uv))
            .expect("the offset node exists");
        assert_eq!(
            rewritten
                .validate()
                .expect_err("a Vec3 sum cannot take a Vec2")
                .kind(),
            FieldErrorCode::TypeMismatch
        );
    }

    #[test]
    fn a_hostile_graph_splices_into_one_that_is_rejected_rather_than_panicking() {
        // A node whose input names no node, and an output that names no node.
        let (build, _node) = FieldBuilder::new(FieldId::from_raw(7), 1).push(
            FieldOp::Abs,
            Vec::new(),
            vec![NodeId::from_raw(5)],
        );
        let broken = build.build(NodeId::from_raw(9));
        let rewritten = host()
            .replace_subgraph(NodeId::from_raw(1), &broken)
            .expect("the host node exists");
        assert!(rewritten.validate().is_err());
        assert_eq!(
            rewritten.op_at(NodeId::from_raw(1)),
            Ok(FieldOp::Length),
            "the host's own nodes are copied faithfully"
        );

        // An unknown operator code is not a bindable leaf: there is nothing to
        // bind, so it is copied through.
        let mut recipe = RecipeGraph::new(axiom_recipe::RecipeId::from_raw(7), 1);
        recipe.add(999, Vec::new(), Vec::new());
        let unknown = FieldGraph::new(recipe, NodeId::from_raw(0), FieldParams::new());
        assert_eq!(bindable_leaves(&unknown), Vec::new());
        assert!(host()
            .inline(&unknown, &[])
            .expect("no leaves, no bindings")
            .validate()
            .is_err());
    }
}
