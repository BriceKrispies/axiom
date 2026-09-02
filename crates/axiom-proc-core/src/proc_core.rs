//! [`ProcCore`]: the deterministic recipe-graph executor.

use axiom_entropy::EntropyApi;
use axiom_recipe::RecipeGraph;
use axiom_space::{Address, SpaceApi};

use crate::node_eval::NodeEval;
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
        recipe
            .validate()
            .map_err(|_| ProcError::InvalidRecipe)
            .and_then(|()| {
                recipe
                    .nodes()
                    .iter()
                    .enumerate()
                    .try_fold(Vec::<Out>::new(), |mut cache, (index, node)| {
                        let inputs: Vec<Out> = node
                            .inputs()
                            .iter()
                            .map(|id| cache[id.raw() as usize].clone())
                            .collect();
                        let address = SpaceApi::child(base, index as u64);
                        let stream = EntropyApi::stream(seed, &address, recipe.version());
                        eval(NodeEval::new(node.op(), node.params(), &inputs, stream))
                            .map(|out| {
                                cache.push(out);
                                cache
                            })
                            .ok_or(ProcError::OpFailed)
                    })
                    .and_then(|cache| cache.into_iter().next_back().ok_or(ProcError::EmptyRecipe))
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
