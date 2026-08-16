//! [`PlacementApi`] — deterministic object scatter on the recipe substrate.
//!
//! `scatter` bakes a two-node scatter recipe — draw `count` words, then reduce
//! each word to a grid cell — through the shared [`ProcCore`] executor at a
//! content [`Address`]. The same `(seed, address, count, bounds)` always yields
//! the same [`Placement`]. Branchless and integer-only: no naked floats cross the
//! boundary, and the operators are selected from a `const` table by operator code,
//! never a `match`.

use axiom_proc_core::{NodeEval, ProcCore};
use axiom_recipe::{Param, RecipeGraph, RecipeId};
use axiom_space::Address;

use crate::placement::Placement;

/// The scatter recipe's stable identity. One recipe shape, so one id.
const SCATTER_RECIPE: RecipeId = RecipeId::from_raw(1);

/// The scatter recipe version. Bump to deliberately re-key generation (+ regolden);
/// versioning is a first-class input. `2` is the recipe-substrate scatter: manifest
/// P1 moved this module from the retired v1 `axiom-proc` evaluator (one stream per
/// recipe) onto `axiom-recipe` + `axiom-proc-core` (one stream per node), which
/// re-keys every drawn word.
const SCATTER_VERSION: u32 = 2;

/// Operator code: draw `params[0]` words from the node's entropy stream.
const OP_DRAW: u16 = 0;
/// Operator code: reduce each input word to a grid cell packed as `x << 32 | y`,
/// bounded by `params[0] × params[1]`.
const OP_CELLS: u16 = 1;

/// A scatter operator: node context in, the node's word output out (`None` when
/// the node is malformed for this operator).
type ScatterOpFn = for<'a> fn(NodeEval<'a, Vec<u64>>) -> Option<Vec<u64>>;

/// The dispatch table, indexed by operator code — a table index, never a `match`.
const OPS: [ScatterOpFn; 2] = [draw_words, pack_cells];

/// The deterministic object-placement facade.
#[derive(Debug)]
pub struct PlacementApi;

impl PlacementApi {
    /// Scatter `count` objects across a `width × height` integer grid at `address`
    /// under `seed`. Deterministic: identical inputs always yield the same
    /// placement. Degenerate bounds (a `0` width or height) collapse to the origin
    /// rather than panic, and a `0` count yields an empty placement.
    pub fn scatter(seed: u64, address: &Address, count: u32, width: u32, height: u32) -> Placement {
        let cells = ProcCore::new()
            .execute(
                &scatter_recipe(count, width, height),
                seed,
                address,
                scatter_eval,
            )
            .unwrap_or_default();
        Placement::new(cells.into_iter().map(unpack_cell).collect())
    }
}

/// The scatter recipe: draw `count` words, then reduce them to `width × height`
/// grid cells. Two nodes whatever the count, so the recipe stays far inside the
/// recipe node budget and the object count is data, not graph size.
fn scatter_recipe(count: u32, width: u32, height: u32) -> RecipeGraph {
    let mut recipe = RecipeGraph::new(SCATTER_RECIPE, SCATTER_VERSION);
    let drawn = recipe.add(OP_DRAW, vec![Param::int(count)], Vec::new());
    recipe.add(
        OP_CELLS,
        vec![Param::int(width), Param::int(height)],
        vec![drawn],
    );
    recipe
}

/// Evaluate one node: select its operator by code and run it. An operator code
/// outside the table fails the node (`None`).
fn scatter_eval(ctx: NodeEval<'_, Vec<u64>>) -> Option<Vec<u64>> {
    OPS.get(usize::from(ctx.op()))
        .copied()
        .and_then(move |op| op(ctx))
}

/// Draw `params[0]` words from the node's deterministic entropy stream.
fn draw_words(mut ctx: NodeEval<'_, Vec<u64>>) -> Option<Vec<u64>> {
    ctx.params()
        .first()
        .copied()
        .map(|count| (0..count.as_int()).map(|_| ctx.stream().next_u64()).collect())
}

/// Reduce each input word to a packed grid cell within `params[0] × params[1]`.
fn pack_cells(ctx: NodeEval<'_, Vec<u64>>) -> Option<Vec<u64>> {
    ctx.params()
        .first()
        .copied()
        .zip(ctx.params().get(1).copied())
        .zip(ctx.inputs().first())
        .map(|((width, height), words)| {
            words
                .iter()
                .map(|&word| pack_cell(cell(word, width.as_int(), height.as_int())))
                .collect()
        })
}

/// Reduce one drawn word into a grid position. A `0` width/height clamps to `1`
/// (so the position collapses to the origin axis) — branchless and panic-free.
fn cell(word: u64, width: u32, height: u32) -> (u32, u32) {
    let w = u64::from(width).max(1);
    let h = u64::from(height).max(1);
    ((word % w) as u32, ((word / w) % h) as u32)
}

/// Pack a cell into one neutral word, so the recipe's output type stays a flat
/// word list (the executor is generic over exactly one output type per graph).
fn pack_cell(position: (u32, u32)) -> u64 {
    (u64::from(position.0) << 32) | u64::from(position.1)
}

/// Unpack a word written by [`pack_cell`].
fn unpack_cell(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_space::SpaceApi;

    fn site(segments: &[u64]) -> Address {
        segments
            .iter()
            .fold(SpaceApi::root(), |a, &s| SpaceApi::child(&a, s))
    }

    #[test]
    fn scatter_is_deterministic_and_within_bounds() {
        let a = site(&[3, 9]);
        let p1 = PlacementApi::scatter(7, &a, 12, 16, 16);
        let p2 = PlacementApi::scatter(7, &a, 12, 16, 16);
        assert_eq!(p1, p2);
        assert_eq!(p1.to_bytes(), p2.to_bytes());
        assert_eq!(p1.len(), 12);
        assert!(!p1.is_empty());
        assert!(p1.positions().iter().all(|&(x, y)| x < 16 && y < 16));
    }

    #[test]
    fn distinct_seeds_or_sites_scatter_differently() {
        let base = PlacementApi::scatter(7, &site(&[3, 9]), 12, 16, 16);
        assert_ne!(base, PlacementApi::scatter(8, &site(&[3, 9]), 12, 16, 16));
        assert_ne!(base, PlacementApi::scatter(7, &site(&[3, 10]), 12, 16, 16));
    }

    #[test]
    fn scatter_reproduces_across_a_sweep() {
        let a = site(&[1]);
        for count in 0..20u32 {
            assert_eq!(
                PlacementApi::scatter(1, &a, count, 8, 8),
                PlacementApi::scatter(1, &a, count, 8, 8)
            );
        }
    }

    #[test]
    fn zero_count_is_empty_and_zero_bounds_are_safe() {
        let a = site(&[0]);
        let empty = PlacementApi::scatter(1, &a, 0, 8, 8);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        // Degenerate bounds never panic; everything collapses to the origin.
        let degenerate = PlacementApi::scatter(1, &a, 4, 0, 0);
        assert_eq!(degenerate.len(), 4);
        assert!(degenerate.positions().iter().all(|&p| p == (0, 0)));
    }

    #[test]
    fn a_large_count_stays_a_two_node_recipe_and_scatters() {
        // The object count is a parameter word, not a node count, so a scatter far
        // past the recipe node budget is still a legal graph.
        let recipe = scatter_recipe(10_000, 16, 16);
        assert_eq!(recipe.node_count(), 2);
        assert_eq!(recipe.validate(), Ok(()));
        assert_eq!(PlacementApi::scatter(1, &site(&[2]), 1000, 16, 16).len(), 1000);
    }

    #[test]
    fn a_cell_packs_and_unpacks_through_one_word() {
        assert_eq!(unpack_cell(pack_cell((7, 9))), (7, 9));
        assert_eq!(
            unpack_cell(pack_cell((u32::MAX, u32::MAX))),
            (u32::MAX, u32::MAX)
        );
    }

    #[test]
    fn an_unknown_operator_code_fails_the_node() {
        // The dispatch table is total over its own codes and rejects anything
        // else as data — the executor turns that into a stable error, never a
        // panic, so a malformed recipe can only ever yield an empty placement.
        let mut graph = RecipeGraph::new(SCATTER_RECIPE, SCATTER_VERSION);
        graph.add(250, Vec::new(), Vec::new());
        assert!(ProcCore::new()
            .execute(&graph, 0, &SpaceApi::root(), scatter_eval)
            .is_err());
        // A node missing its parameter words fails the same way.
        let mut bare = RecipeGraph::new(SCATTER_RECIPE, SCATTER_VERSION);
        bare.add(OP_DRAW, Vec::new(), Vec::new());
        assert!(ProcCore::new()
            .execute(&bare, 0, &SpaceApi::root(), scatter_eval)
            .is_err());
        let mut unbounded = RecipeGraph::new(SCATTER_RECIPE, SCATTER_VERSION);
        let drawn = unbounded.add(OP_DRAW, vec![Param::int(2)], Vec::new());
        unbounded.add(OP_CELLS, vec![Param::int(4)], vec![drawn]);
        assert!(ProcCore::new()
            .execute(&unbounded, 0, &SpaceApi::root(), scatter_eval)
            .is_err());
    }

    #[test]
    fn golden_scatter_digest_is_stable() {
        // Re-goldened by manifest P1: the scatter moved from the v1 `axiom-proc`
        // evaluator (one entropy stream per recipe) to `axiom-proc-core` (one
        // stream per node), which re-keys every drawn word. SCATTER_VERSION was
        // bumped 1 -> 2 to make the change deliberate rather than silent.
        let p = PlacementApi::scatter(7, &site(&[3, 9]), 12, 16, 16);
        assert_eq!(p.digest().raw(), 99_925_531_047_522_645);
    }

    #[test]
    fn types_are_debug() {
        let p = PlacementApi::scatter(7, &site(&[3, 9]), 2, 8, 8);
        assert!(!format!("{p:?}").is_empty());
        assert!(!format!("{:?}", PlacementApi).is_empty());
    }
}
