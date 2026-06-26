//! `axiom-proc-fuzz` — the procgen determinism / fuzz / replay gate harness.
//!
//! It seed-sweeps every generator (proc recipes, terrain, biome, placement, and
//! composed levelgen worlds) and asserts byte-identical regeneration; fuzzes
//! randomly-built recipes (driven by a *seeded* RNG, so the fuzz is itself
//! reproducible) and asserts they never panic and either reproduce or are cleanly
//! rejected as data; and long-run-replays worlds (generate → serialize →
//! regenerate → byte-equal). The property tests run under `cargo test --workspace`
//! — the CI gate that fails on any determinism drift.
//!
//! Repo **tooling**, outside the engine dependency graph (exempt from the coverage
//! and branchless gates). The non-test sweep is still written branchlessly, so it
//! adds nothing to the engine branch count; the inherently branchy recipe fuzz
//! lives in the (exempt) test module.
//!
//! ```text
//! cargo run -p axiom-proc-fuzz            # sweep 2000 seeds, exit non-zero on drift
//! ```

use axiom_biome::BiomeApi;
use axiom_levelgen::LevelGenApi;
use axiom_placement::PlacementApi;
use axiom_proc::{ProcApi, Recipe};
use axiom_space::{Address, SpaceApi};
use axiom_terrain::TerrainApi;

/// A content address from a segment path.
fn site(segments: &[u64]) -> Address {
    segments.iter().fold(SpaceApi::root(), |address, &segment| {
        SpaceApi::child(&address, segment)
    })
}

/// The fixed recipe the proc sweep evaluates (exercises every node op).
fn sample_recipe() -> Recipe {
    let mut recipe = Recipe::new(1);
    let c = recipe.const_node(5);
    let a = recipe.draw();
    let s = recipe.add(c, a);
    recipe.xor(s, c);
    recipe
}

/// Whether *every* generator reproduces byte-identically at `seed` — proc, terrain,
/// biome, placement, and a composed levelgen world. Branchless (`&` over the
/// per-generator equalities; nothing short-circuits).
fn all_reproduce(seed: u64) -> bool {
    let address = site(&[seed % 7, seed % 13]);
    let recipe = sample_recipe();
    let proc_ok = ProcApi::evaluate(&recipe, seed, &address)
        .map(|(a, t)| (a.to_bytes(), t.to_bytes()))
        == ProcApi::evaluate(&recipe, seed, &address).map(|(a, t)| (a.to_bytes(), t.to_bytes()));
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

    /// Build a varied recipe from a seeded RNG: 1..8 nodes, each a random op wired
    /// to random (possibly out-of-range) earlier indices, so the fuzz exercises
    /// both valid DAGs and recipes proc must reject as data.
    fn random_recipe(rng: &mut DeterministicRng) -> Recipe {
        let mut recipe = Recipe::new(1);
        let node_count = rng.next_bounded(8) + 1;
        for _ in 0..node_count {
            let len = recipe.len();
            match rng.next_bounded(4) {
                0 => {
                    recipe.const_node(rng.next_u64());
                }
                1 => {
                    recipe.draw();
                }
                2 => {
                    recipe.add(pick(rng, len), pick(rng, len));
                }
                _ => {
                    recipe.xor(pick(rng, len), pick(rng, len));
                }
            }
        }
        recipe
    }

    /// An input index that is usually valid (`< len`) but sometimes out of range.
    fn pick(rng: &mut DeterministicRng, len: usize) -> usize {
        rng.next_bounded(len as u64 + 2) as usize
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
            // Never panics: an invalid recipe is None, a valid one evaluates; and
            // re-evaluating yields the identical outcome.
            let first =
                ProcApi::evaluate(&recipe, 7, &address).map(|(a, t)| (a.to_bytes(), t.to_bytes()));
            let again =
                ProcApi::evaluate(&recipe, 7, &address).map(|(a, t)| (a.to_bytes(), t.to_bytes()));
            assert_eq!(first, again);
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
