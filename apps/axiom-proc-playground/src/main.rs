//! Axiom procedural-generation playground (native, headless).
//!
//! Proves the `space → entropy → recipe → proc-core → proc-validate` stack end to
//! end: it builds a tiny deterministic [`RecipeGraph`], executes it at a content
//! [`Address`] through [`ProcCore`] into neutral output words, validates those
//! words against generic constraints, and reports the provenance digests. The same
//! `(seed, address)` always yields byte-identical words, on every run and
//! platform. The point is the *pipeline*, not the content — the recipe stays
//! trivial.
//!
//! This is a composition leaf (an app): exempt from the spine's coverage and
//! branchless gates, but it ships its own determinism + golden tests.
//!
//! Manifest P1 retired the v1 `axiom-proc` stack this app was written against.
//! The four word operators below (`const` / `draw` / `add` / `xor`) were that
//! layer's closed built-in op set; on the v2 stack the operator table belongs to
//! the *domain*, and this app's domain is "neutral words", so the table lives
//! here. `ProcTrace` is gone with the v1 stack: the per-node decision log is
//! recovered by collecting each node's output as the executor produces it.

use std::cell::RefCell;

use axiom_proc_core::{NodeEval, ProcCore};
use axiom_recipe::{Param, RecipeGraph, RecipeId};
use axiom_proc_validate::{Constraint, ProcValidateApi, ValidationReport};
use axiom_space::{Address, SpaceApi};

/// The playground recipe's stable identity and version. Bump the version to
/// deliberately re-key + regolden.
const PLAYGROUND_RECIPE: RecipeId = RecipeId::from_raw(1);
const PLAYGROUND_VERSION: u32 = 1;

/// Operator codes for the playground's neutral-word domain.
const OP_CONST: u16 = 0;
const OP_DRAW: u16 = 1;
const OP_ADD: u16 = 2;
const OP_XOR: u16 = 3;

/// The playground recipe: a tiny generator over neutral words — a literal, two
/// entropy draws, and combines of them — enough to exercise every node op while
/// staying trivial.
fn playground_recipe() -> RecipeGraph {
    let mut recipe = RecipeGraph::new(PLAYGROUND_RECIPE, PLAYGROUND_VERSION);
    let base = recipe.add(OP_CONST, vec![Param::int(1000), Param::int(0)], vec![]);
    let a = recipe.add(OP_DRAW, vec![], vec![]);
    let b = recipe.add(OP_DRAW, vec![], vec![]);
    let mixed = recipe.add(OP_ADD, vec![], vec![a, b]);
    recipe.add(OP_XOR, vec![], vec![mixed, base]);
    recipe
}

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

/// A content address built from a site path.
fn site(segments: &[u64]) -> Address {
    segments.iter().fold(SpaceApi::root(), |address, &segment| {
        SpaceApi::child(&address, segment)
    })
}

/// The generic, domain-free constraints the playground holds its words to: at
/// least one word, and every word non-zero.
fn playground_constraints() -> [Constraint; 2] {
    [Constraint::min_count(1), Constraint::non_zero()]
}

/// Run the full pipeline at `(seed, address)`: execute the recipe, collecting one
/// output word per node, then validate the words. The recipe is a valid DAG over
/// known operators, so execution always succeeds.
fn run(seed: u64, address: &Address) -> (RecipeGraph, Vec<u64>, ValidationReport) {
    let recipe = playground_recipe();
    let log: RefCell<Vec<u64>> = RefCell::new(Vec::new());
    ProcCore::new()
        .execute(&recipe, seed, address, |ctx| {
            let op = ctx.op();
            let out = OPS.get(usize::from(op)).copied().and_then(|f| f(ctx));
            out.inspect(|&word| log.borrow_mut().push(word))
        })
        .expect("the playground recipe is a valid DAG over known operators");
    let words = log.into_inner();
    let report = ProcValidateApi::validate(&words, &playground_constraints());
    (recipe, words, report)
}

fn main() {
    let address = site(&[7, 42]);
    let (recipe, words, report) = run(2026, &address);
    println!("axiom proc playground");
    println!("  site            : {:?}", address.segments());
    println!("  recipe nodes    : {}", recipe.node_count());
    println!("  recipe digest   : {:#018x}", recipe.digest().raw());
    println!("  output words    : {}", words.len());
    println!("  report digest   : {:#018x}", report.digest().raw());
    println!("  all_satisfied   : {}", report.all_satisfied());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stack_runs_end_to_end_and_validates() {
        let (recipe, words, report) = run(2026, &site(&[7, 42]));
        assert_eq!(recipe.node_count(), 5);
        assert_eq!(words.len(), 5);
        assert!(report.all_satisfied());
    }

    #[test]
    fn the_same_seed_and_site_replay_byte_for_byte() {
        let address = site(&[7, 42]);
        let (r1, w1, rep1) = run(2026, &address);
        let (r2, w2, rep2) = run(2026, &address);
        assert_eq!(r1.serialize(), r2.serialize());
        assert_eq!(w1, w2);
        assert_eq!(rep1.to_bytes(), rep2.to_bytes());
    }

    #[test]
    fn a_different_seed_or_site_changes_the_words() {
        let base = run(2026, &site(&[7, 42])).1;
        assert_ne!(base, run(2027, &site(&[7, 42])).1); // seed
        assert_ne!(base, run(2026, &site(&[7, 43])).1); // site
    }

    #[test]
    fn golden_provenance_digests_are_stable() {
        // The recipe digest was re-goldened by manifest P1: the playground moved
        // off the retired v1 `axiom-proc` evaluator onto `axiom-recipe` +
        // `axiom-proc-core`, so the recipe's canonical byte form changed. The
        // report digest is *unchanged* — it is a function of the word count and
        // the constraint verdicts, and both survived the substrate swap intact.
        // Regolden deliberately (and bump PLAYGROUND_VERSION) if this changes.
        let (recipe, _words, report) = run(2026, &site(&[7, 42]));
        assert_eq!(recipe.digest().raw(), 0x1aa4_d188_9001_e9b7);
        assert_eq!(report.digest().raw(), 0x25b8_eb67_d3bf_eba2);
    }
}
