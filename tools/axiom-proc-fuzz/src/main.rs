//! `axiom-proc-fuzz` — the procgen determinism / fuzz / replay gate harness.
//!
//! It seed-sweeps every generator (recipe graphs, terrain, biome, placement, and
//! composed levelgen worlds) and asserts byte-identical regeneration; fuzzes
//! randomly-built recipes (driven by a *seeded* RNG, so the fuzz is itself
//! reproducible) and asserts they never panic and either reproduce or are cleanly
//! rejected as data; and long-run-replays worlds (generate → serialize →
//! regenerate → byte-equal). The property tests run under `cargo test --workspace`
//! — the CI gate that fails on any determinism drift.
//!
//! Manifest P1 retired the v1 `axiom-proc` stack; the recipe arm of the sweep now
//! runs on `axiom-recipe` + `axiom-proc-core`. The four word operators below were
//! v1's closed built-in op set — on the v2 stack the operator table belongs to the
//! domain, and this harness's domain is "neutral words".
//!
//! ```text
//! cargo run -p axiom-proc-fuzz            # sweep 2000 seeds, exit non-zero on drift
//! ```

use axiom_biome::BiomeApi;
use axiom_levelgen::LevelGenApi;
use axiom_placement::PlacementApi;
use axiom_proc_core::{NodeEval, ProcCore, ProcResult};
use axiom_recipe::{Param, RecipeGraph, RecipeId};
use axiom_space::{Address, SpaceApi};
use axiom_terrain::TerrainApi;

/// Operator codes for the neutral-word domain the recipe sweep exercises.
const OP_CONST: u16 = 0;
const OP_DRAW: u16 = 1;
const OP_ADD: u16 = 2;
const OP_XOR: u16 = 3;

/// One word operator: a node context in, one neutral word out.
type WordOpFn = for<'a> fn(NodeEval<'a, u64>) -> Option<u64>;

/// The operator table, indexed by operator code.
const OPS: [WordOpFn; 4] = [op_const, op_draw, op_add, op_xor];

/// A literal carried as two parameter words (`lo`, `hi`) — a `Param` is 32 bits.
fn op_const(ctx: NodeEval<'_, u64>) -> Option<u64> {
    let lo = ctx.params().first().map(|p| u64::from(p.as_int()));
    let hi = ctx.params().get(1).map(|p| u64::from(p.as_int()));
    lo.zip(hi).map(|(lo, hi)| (hi << 32) | lo)
}

fn op_draw(mut ctx: NodeEval<'_, u64>) -> Option<u64> {
    Some(ctx.stream().next_u64())
}

fn op_add(ctx: NodeEval<'_, u64>) -> Option<u64> {
    two(&ctx).map(|(a, b)| a.wrapping_add(b))
}

fn op_xor(ctx: NodeEval<'_, u64>) -> Option<u64> {
    two(&ctx).map(|(a, b)| a ^ b)
}

fn two(ctx: &NodeEval<'_, u64>) -> Option<(u64, u64)> {
    ctx.inputs()
        .first()
        .copied()
        .zip(ctx.inputs().get(1).copied())
}

/// Evaluate one node: select its operator by code and run it. An unknown code
/// fails the node, which the executor reports as data (never a panic).
fn word_eval(ctx: NodeEval<'_, u64>) -> Option<u64> {
    OPS.get(usize::from(ctx.op()))
        .copied()
        .and_then(move |op| op(ctx))
}

/// Run a recipe over the neutral-word domain at `(seed, address)`.
fn evaluate(recipe: &RecipeGraph, seed: u64, address: &Address) -> ProcResult<u64> {
    ProcCore::new().execute(recipe, seed, address, word_eval)
}

/// A content address from a segment path.
fn site(segments: &[u64]) -> Address {
    segments.iter().fold(SpaceApi::root(), |address, &segment| {
        SpaceApi::child(&address, segment)
    })
}

/// The fixed recipe the sweep evaluates (exercises every node op).
fn sample_recipe() -> RecipeGraph {
    let mut recipe = RecipeGraph::new(RecipeId::from_raw(1), 1);
    let c = recipe.add(OP_CONST, vec![Param::int(5), Param::int(0)], vec![]);
    let a = recipe.add(OP_DRAW, vec![], vec![]);
    let s = recipe.add(OP_ADD, vec![], vec![c, a]);
    recipe.add(OP_XOR, vec![], vec![s, c]);
    recipe
}

/// Whether *every* generator reproduces byte-identically at `seed` — the recipe
/// graph, terrain, biome, placement, and a composed levelgen world. Branchless
/// (`&` over the per-generator equalities; nothing short-circuits).
fn all_reproduce(seed: u64) -> bool {
    let address = site(&[seed % 7, seed % 13]);
    let recipe = sample_recipe();
    let proc_ok = (recipe.serialize(), evaluate(&recipe, seed, &address))
        == (recipe.serialize(), evaluate(&recipe, seed, &address));
    let terrain_ok = TerrainApi::heightfield(seed, 0, 0, 12, 8).to_bytes()
        == TerrainApi::heightfield(seed, 0, 0, 12, 8).to_bytes();
    let biome_ok = BiomeApi::map(seed, &address, 48).to_bytes()
        == BiomeApi::map(seed, &address, 48).to_bytes();
    let placement_ok = PlacementApi::scatter(seed, &address, 16, 12, 8).to_bytes()
        == PlacementApi::scatter(seed, &address, 16, 12, 8).to_bytes();
    let world_ok = LevelGenApi::generate(seed, &address, 16, 16).to_bytes()
        == LevelGenApi::generate(seed, &address, 16, 16).to_bytes();
    proc_ok & terrain_ok & biome_ok & placement_ok & world_ok
}

/// How many of `0..count` seeds reproduced across every generator.
fn sweep(count: u64) -> u64 {
    (0..count).filter(|&seed| all_reproduce(seed)).count() as u64
}

fn main() {
    let count = 2000u64;
    let reproduced = sweep(count);
    let verdict =
        ["DRIFT DETECTED", "OK (every generator byte-identical)"][(reproduced == count) as usize];
    println!("axiom proc-fuzz — procedural-generation determinism gate");
    println!("  seed sweep      : {reproduced}/{count} seeds reproduced across all generators");
    println!("  result          : {verdict}");
    std::process::exit((reproduced != count) as i32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::DeterministicRng;
    use axiom_recipe::NodeId;

    /// Build a varied recipe from a seeded RNG: 1..8 nodes, each a random op wired
    /// to random (possibly out-of-range) earlier links, so the fuzz exercises both
    /// valid DAGs and recipes the executor must reject as data.
    fn random_recipe(rng: &mut DeterministicRng) -> RecipeGraph {
        let mut recipe = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let node_count = rng.next_bounded(8) + 1;
        for _ in 0..node_count {
            let len = recipe.node_count();
            match rng.next_bounded(4) {
                0 => {
                    let word = rng.next_u64();
                    recipe.add(
                        OP_CONST,
                        vec![Param::int(word as u32), Param::int((word >> 32) as u32)],
                        vec![],
                    );
                }
                1 => {
                    recipe.add(OP_DRAW, vec![], vec![]);
                }
                2 => {
                    recipe.add(OP_ADD, vec![], vec![pick(rng, len), pick(rng, len)]);
                }
                _ => {
                    recipe.add(OP_XOR, vec![], vec![pick(rng, len), pick(rng, len)]);
                }
            }
        }
        recipe
    }

    /// An input link that is usually valid (`< len`) but sometimes out of range.
    fn pick(rng: &mut DeterministicRng, len: usize) -> NodeId {
        NodeId::from_raw(rng.next_bounded(len as u64 + 2) as u32)
    }

    #[test]
    fn every_generator_reproduces_across_a_seed_sweep() {
        assert!((0..256u64).all(all_reproduce));
    }

    #[test]
    fn random_recipes_never_panic_and_reproduce_or_reject() {
        let mut rng = DeterministicRng::seeded(0xF1FF_F00D);
        let address = site(&[3, 9]);
        for _ in 0..600 {
            let recipe = random_recipe(&mut rng);
            // Never panics: an invalid recipe is a stable error, a valid one
            // evaluates; and re-evaluating yields the identical outcome.
            assert_eq!(evaluate(&recipe, 7, &address), evaluate(&recipe, 7, &address));
            assert_eq!(
                RecipeGraph::deserialize(&recipe.serialize()).is_ok(),
                recipe.validate().is_ok()
            );
        }
    }

    #[test]
    fn worlds_replay_byte_equal_long_run() {
        for seed in 0..128u64 {
            let a = site(&[seed % 7, seed % 13]);
            assert_eq!(
                LevelGenApi::generate(seed, &a, 16, 16).to_bytes(),
                LevelGenApi::generate(seed, &a, 16, 16).to_bytes()
            );
        }
    }

    #[test]
    fn biome_classify_is_total_over_the_value_domain() {
        // Classification never panics for any (elevation, moisture) in a swept
        // domain spanning past the noise range, and the full vocabulary is reached.
        let mut codes = std::collections::BTreeSet::new();
        for e in (0..1200u32).step_by(50) {
            for m in (0..1200u32).step_by(50) {
                codes.insert(BiomeApi::classify(e, m));
            }
        }
        assert_eq!(codes.len(), 6);
    }
}
