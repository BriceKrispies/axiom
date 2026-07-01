//! Axiom procedural-generation playground (native, headless).
//!
//! Proves the `space → entropy → proc → proc-validate` stack end to end: it builds
//! a tiny deterministic recipe, evaluates it at a content [`Address`] into a
//! neutral [`Artifact`] + [`ProcTrace`], validates the artifact against generic
//! constraints, and reports the provenance digests. The same `(seed, address)`
//! always yields a byte-identical artifact, on every run and platform. The point
//! is the *pipeline*, not the content — the recipe stays trivial.
//!
//! This is a composition leaf (an app): exempt from the spine's coverage and
//! branchless gates, but it ships its own determinism + golden tests.

use axiom_proc::{Artifact, ProcApi, ProcTrace, Recipe};
use axiom_proc_validate::{Constraint, ProcValidateApi, ValidationReport};
use axiom_space::{Address, SpaceApi};

/// The playground recipe: a tiny generator over neutral words — a literal, two
/// entropy draws, and combines of them — enough to exercise every node op while
/// staying trivial. Version 1; bump it to deliberately re-key + regolden.
fn playground_recipe() -> Recipe {
    let mut recipe = Recipe::new(1);
    let base = recipe.const_node(1000);
    let a = recipe.draw();
    let b = recipe.draw();
    let mixed = recipe.add(a, b);
    let _shifted = recipe.xor(mixed, base);
    recipe
}

/// The generic, domain-free constraints the playground holds its artifact to: at
/// least one word, and every word non-zero.
fn playground_constraints() -> [Constraint; 2] {
    [Constraint::min_count(1), Constraint::non_zero()]
}

/// A content address built from a site path.
fn site(segments: &[u64]) -> Address {
    segments.iter().fold(SpaceApi::root(), |address, &segment| {
        SpaceApi::child(&address, segment)
    })
}

/// Run the full pipeline at `(seed, address)`: evaluate the recipe, then validate
/// the artifact. The recipe is a valid DAG, so evaluation always succeeds.
fn run(seed: u64, address: &Address) -> (Artifact, ProcTrace, ValidationReport) {
    let (artifact, trace) = ProcApi::evaluate(&playground_recipe(), seed, address)
        .expect("the playground recipe is a valid DAG");
    let report = ProcValidateApi::validate(&artifact, &playground_constraints());
    (artifact, trace, report)
}

fn main() {
    let address = site(&[7, 42]);
    let (artifact, trace, report) = run(2026, &address);
    println!("axiom proc playground");
    println!("  site            : {:?}", address.segments());
    println!("  artifact words  : {}", artifact.words().len());
    println!("  artifact digest : {:#018x}", artifact.digest().raw());
    println!("  trace digest    : {:#018x}", trace.digest().raw());
    println!("  report digest   : {:#018x}", report.digest().raw());
    println!("  all_satisfied   : {}", report.all_satisfied());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stack_runs_end_to_end_and_validates() {
        let (artifact, trace, report) = run(2026, &site(&[7, 42]));
        assert_eq!(artifact.words().len(), 5);
        assert_eq!(trace.len(), 5);
        assert!(report.all_satisfied());
    }

    #[test]
    fn the_same_seed_and_site_replay_byte_for_byte() {
        let address = site(&[7, 42]);
        let (a1, t1, r1) = run(2026, &address);
        let (a2, t2, r2) = run(2026, &address);
        assert_eq!(a1.to_bytes(), a2.to_bytes());
        assert_eq!(t1.to_bytes(), t2.to_bytes());
        assert_eq!(r1.to_bytes(), r2.to_bytes());
    }

    #[test]
    fn a_different_seed_or_site_changes_the_artifact() {
        let base = run(2026, &site(&[7, 42])).0;
        assert_ne!(base.to_bytes(), run(2027, &site(&[7, 42])).0.to_bytes()); // seed
        assert_ne!(base.to_bytes(), run(2026, &site(&[7, 43])).0.to_bytes()); // site
    }

    #[test]
    fn golden_provenance_digests_are_stable() {
        // Regolden deliberately (and bump the recipe version) if this changes.
        let (artifact, trace, report) = run(2026, &site(&[7, 42]));
        assert_eq!(artifact.digest().raw(), 0xa9ed_8e1e_48df_3777);
        assert_eq!(trace.digest().raw(), 0x6a97_e4bb_1cb2_25ae);
        assert_eq!(report.digest().raw(), 0x25b8_eb67_d3bf_eba2);
    }
}
