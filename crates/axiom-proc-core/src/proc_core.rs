//! [`ProcCore`]: the deterministic recipe-graph executor.

use axiom_entropy::EntropyApi;
use axiom_recipe::RecipeGraph;
use axiom_space::{Address, SpaceApi};

use crate::node_eval::NodeEval;
use crate::node_step::NodeStep;
use crate::proc_error::{ProcError, ProcResult};

/// The stateless graph executor. It is generic over the output type, so one
/// executor drives every domain (textures, meshes, …); the domain supplies an
/// evaluator that turns one node into one output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcCore;

impl ProcCore {
    /// Construct the executor.
    pub const fn new() -> Self {
        Self
    }

    /// Evaluate `recipe` and return its result — the output of its final node.
    ///
    /// The recipe is validated first (`InvalidRecipe` on failure), then its nodes
    /// are evaluated in id order. Each node's already-computed input outputs are
    /// gathered from the cache and handed to `eval` together with the node's
    /// parameters and a deterministic per-node entropy stream keyed by
    /// `(seed, child(base, node), version)`. An `eval` returning `None`
    /// (unknown operator, wrong input count) is `OpFailed`; an empty recipe is
    /// `EmptyRecipe`.
    ///
    /// `eval` is `FnMut`, not `Fn`, so an evaluator may own mutable state — a
    /// scene evaluator binds a recipe to a live app and must mutate it as it
    /// goes. Every `Fn` is an `FnMut`, so this is strictly wider than the
    /// alternative and no caller changed. The alternative was a `RefCell`
    /// around the evaluator's state inside a `Fn` closure, which buys a runtime
    /// borrow-panic path that nothing can provoke from a test — an
    /// untestable branch, which the Coverage Law reads as a design signal
    /// rather than as something to write a contrived test for.
    pub fn execute<Out, F>(
        &self,
        recipe: &RecipeGraph,
        seed: u64,
        base: &Address,
        mut eval: F,
    ) -> ProcResult<Out>
    where
        Out: Clone,
        F: FnMut(NodeEval<'_, Out>) -> Option<Out>,
    {
        let mut cache = Vec::<Out>::new();
        let mut index = 0u64;
        self.evaluate(recipe, &mut cache, |step| {
            let inputs: Vec<Out> = step.inputs().cloned().collect();
            let address = SpaceApi::child(base, index);
            let stream = EntropyApi::stream(seed, &address, recipe.version());
            index += 1;
            eval(NodeEval::new(step.op(), step.params(), &inputs, stream))
        })
        .and_then(|()| cache.into_iter().next_back().ok_or(ProcError::EmptyRecipe))
    }

    /// Evaluate every node of `recipe` in id order into `cache`, with the
    /// domain supplying its own randomness.
    ///
    /// The recipe is validated first (`InvalidRecipe` on failure), then each
    /// node is handed to `eval` as a [`NodeStep`] — its operator code, its
    /// parameters, and borrowed access to the outputs of its inputs. An `eval`
    /// returning `None` is `OpFailed`. On success `cache` holds one output per
    /// node, in id order, which is what a caller needs when the values it wants
    /// are scattered through the graph rather than gathered at its last node.
    ///
    /// `cache` is cleared first and is the caller's to reuse, so a domain
    /// running the same graph once per emitted item allocates once rather than
    /// once per item — and nothing at all per node.
    ///
    /// **No entropy stream is manufactured here.** A domain whose randomness is
    /// one sequential source shared across the frame owns that source and draws
    /// from it inside `eval`, in the order the nodes are walked; that order is
    /// then the contract, which is exactly what an address-keyed per-node stream
    /// cannot express. `eval` is `FnMut` precisely so it can hold that source.
    pub fn evaluate<Out, F>(
        &self,
        recipe: &RecipeGraph,
        cache: &mut Vec<Out>,
        mut eval: F,
    ) -> ProcResult<()>
    where
        F: FnMut(NodeStep<'_, Out>) -> Option<Out>,
    {
        recipe
            .validate()
            .map_err(|_| ProcError::InvalidRecipe)
            .and_then(|()| {
                cache.clear();
                recipe.nodes().iter().try_for_each(|node| {
                    eval(NodeStep::new(
                        node.op(),
                        node.params(),
                        node.inputs(),
                        cache.as_slice(),
                    ))
                    .map(|out| cache.push(out))
                    .ok_or(ProcError::OpFailed)
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_recipe::{NodeId, Param, RecipeId};

    /// A tiny `u64` evaluator: op 0 returns its first param; op 1 sums its
    /// inputs; op 2 draws one word from the node's entropy stream; anything else
    /// fails. Tests may branch freely.
    fn eval(mut ctx: NodeEval<'_, u64>) -> Option<u64> {
        match ctx.op() {
            0 => ctx.params().first().map(|p| u64::from(p.as_int())),
            1 => Some(ctx.inputs().iter().copied().sum()),
            2 => Some(ctx.stream().next_u64()),
            _ => None,
        }
    }

    fn adder() -> RecipeGraph {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let a = g.add(0, vec![Param::int(5)], vec![]);
        let b = g.add(0, vec![Param::int(3)], vec![]);
        g.add(1, vec![], vec![a, b]);
        g
    }

    #[test]
    fn executes_the_graph_and_returns_the_final_output() {
        let out = ProcCore::new()
            .execute(&adder(), 7, &SpaceApi::root(), eval)
            .unwrap();
        assert_eq!(out, 8);
    }

    #[test]
    fn execution_is_deterministic_for_seed() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(2, vec![], vec![]); // draws from the per-node stream
        let core = ProcCore::new();
        let a = core.execute(&g, 42, &SpaceApi::root(), eval).unwrap();
        let b = core.execute(&g, 42, &SpaceApi::root(), eval).unwrap();
        let c = core.execute(&g, 43, &SpaceApi::root(), eval).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn invalid_recipe_is_rejected() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(1, vec![], vec![NodeId::from_raw(3)]); // forward ref → cyclic
        assert_eq!(
            ProcCore::new().execute(&g, 0, &SpaceApi::root(), eval),
            Err(ProcError::InvalidRecipe)
        );
    }

    #[test]
    fn empty_recipe_has_no_result() {
        let g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        assert_eq!(
            ProcCore::new().execute(&g, 0, &SpaceApi::root(), eval),
            Err(ProcError::EmptyRecipe)
        );
    }

    #[test]
    fn unknown_operator_fails_the_node() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(99, vec![], vec![]);
        assert_eq!(
            ProcCore::new().execute(&g, 0, &SpaceApi::root(), eval),
            Err(ProcError::OpFailed)
        );
    }

    /// The whole point of `FnMut`: an evaluator that owns mutable state and
    /// mutates it as the graph is walked. A scene evaluator does exactly this —
    /// it binds each node into a live app — and under `Fn` it could not.
    ///
    /// This also pins the *order*: nodes are evaluated in id order, so the
    /// recorded ops come out in the order the recipe declares them.
    #[test]
    fn a_stateful_evaluator_may_mutate_as_the_graph_is_walked() {
        let mut visited: Vec<u16> = Vec::new();
        let out = ProcCore::new()
            .execute(&adder(), 7, &SpaceApi::root(), |ctx| {
                visited.push(ctx.op());
                eval(ctx)
            })
            .unwrap();
        assert_eq!(out, 8);
        assert_eq!(visited, vec![0, 0, 1]);
    }

    fn defaulted<T: Default>() -> T {
        T::default()
    }

    #[test]
    fn new_and_default_agree() {
        assert_eq!(ProcCore::new(), ProcCore);
        assert_eq!(defaulted::<ProcCore>(), ProcCore::new());
    }
}

#[cfg(test)]
mod evaluate_tests {
    use super::*;
    use axiom_recipe::{NodeId, Param, RecipeId};

    /// A graph of four nodes: two constants, their sum, and the sum doubled.
    /// Every node's output is worth keeping, which is the point of `evaluate`.
    fn chain() -> RecipeGraph {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let a = g.add(0, vec![Param::int(5)], vec![]);
        let b = g.add(0, vec![Param::int(3)], vec![]);
        let s = g.add(1, vec![], vec![a, b]);
        g.add(2, vec![], vec![s]);
        g
    }

    fn arithmetic(step: NodeStep<'_, u64>) -> Option<u64> {
        match step.op() {
            0 => step.params().first().map(|p| u64::from(p.as_int())),
            1 => Some(step.inputs().sum()),
            2 => step.input(0).map(|v| v * 2),
            _ => None,
        }
    }

    #[test]
    fn evaluate_keeps_every_node_not_only_the_last() {
        let mut cache = Vec::new();
        ProcCore::new()
            .evaluate(&chain(), &mut cache, arithmetic)
            .expect("evaluates");
        assert_eq!(cache, vec![5, 3, 8, 16]);
    }

    /// The cache is the caller's to reuse, so a domain running one graph per
    /// emitted item allocates once rather than once per item. Reuse only helps
    /// if the buffer is cleared, and a stale tail would silently offset every
    /// node id.
    #[test]
    fn evaluate_clears_the_cache_it_is_handed() {
        let mut cache = vec![99, 98, 97];
        ProcCore::new()
            .evaluate(&chain(), &mut cache, arithmetic)
            .expect("evaluates");
        assert_eq!(cache, vec![5, 3, 8, 16]);
    }

    /// The reason this entry point exists: a domain whose randomness is one
    /// sequential source draws from it *in node order*, and that order is the
    /// contract. An address-keyed per-node stream cannot express that, because
    /// each node's draws are independent of every other node's.
    #[test]
    fn a_shared_sequential_source_is_drawn_in_node_order() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(2), 1);
        (0..4).for_each(|_| {
            g.add(9, vec![], vec![]);
        });

        let mut next = 100u64;
        let mut cache = Vec::new();
        ProcCore::new()
            .evaluate(&g, &mut cache, |_step| {
                next += 1;
                Some(next)
            })
            .expect("evaluates");
        assert_eq!(cache, vec![101, 102, 103, 104]);
    }

    #[test]
    fn an_evaluator_that_fails_stops_the_walk() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(3), 1);
        g.add(0, vec![Param::int(1)], vec![]);
        g.add(77, vec![], vec![]); // no arm for op 77
        g.add(0, vec![Param::int(2)], vec![]);

        let mut cache = Vec::new();
        let err = ProcCore::new()
            .evaluate(&g, &mut cache, arithmetic)
            .expect_err("refuses");
        assert_eq!(err, ProcError::OpFailed);
        assert_eq!(cache, vec![1], "the walk should stop where it failed");
    }

    #[test]
    fn an_invalid_recipe_is_refused_before_any_node_runs() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(4), 1);
        // A node whose input is itself — the acyclic check rejects it.
        g.add(1, vec![], vec![NodeId::from_raw(0)]);

        let mut cache = vec![7];
        let err = ProcCore::new()
            .evaluate(&g, &mut cache, arithmetic)
            .expect_err("refuses");
        assert_eq!(err, ProcError::InvalidRecipe);
        assert_eq!(cache, vec![7], "nothing should have been touched");
    }

    /// A slot the node does not have reads as `None`. A slot it *does* have can
    /// never be `None`, because validation has already established that every
    /// input of node `i` is a node before `i`.
    #[test]
    fn a_slot_the_node_does_not_have_reads_as_none() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(5), 1);
        let a = g.add(0, vec![Param::int(4)], vec![]);
        g.add(3, vec![], vec![a]);

        let mut seen = Vec::new();
        let mut cache = Vec::new();
        ProcCore::new()
            .evaluate(&g, &mut cache, |step| {
                seen.push((step.input_count(), step.input(1).copied()));
                step.input(0)
                    .copied()
                    .or_else(|| step.params().first().map(|p| u64::from(p.as_int())))
            })
            .expect("evaluates");
        assert_eq!(seen, vec![(0, None), (1, None)]);
        assert_eq!(cache, vec![4, 4], "the second node read the first's output");
    }
}
