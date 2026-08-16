//! `axiom-proc-inspect` — dump the provenance of a generation.
//!
//! Given a seed and a content-address path, it executes a sample recipe graph,
//! validates the output words, and prints the whole `seed → address → recipe →
//! per-node outputs → validation` chain: the recipe's content digest, each node's
//! operator code and produced word, the constraint verdicts, and the stable digest
//! that indexes each boundary. An agent (or a human) can read off exactly how a
//! piece of content was made and reproduce it from `(seed, address)`.
//!
//! Manifest P1 retired the v1 `axiom-proc` stack, and with it `ProcTrace`. The
//! per-node decision log this tool exists to print is now collected directly from
//! the [`ProcCore`] executor as it produces each node's output — the same
//! information, without a second recipe generation to carry it. The four word
//! operators below were v1's closed built-in op set; on the v2 stack the operator
//! table belongs to the domain, and this tool's domain is "neutral words".
//!
//! ```text
//! cargo run -p axiom-proc-inspect -- [seed] [addr-seg ...]
//! cargo run -p axiom-proc-inspect -- 2026 7 42
//! ```

use std::cell::RefCell;
use std::env;

use axiom_proc_core::{NodeEval, ProcCore};
use axiom_proc_validate::{Constraint, ProcValidateApi, ValidationReport};
use axiom_recipe::{Param, RecipeGraph, RecipeId};
use axiom_space::{Address, SpaceApi};

/// The inspected recipe's stable identity and version.
const SAMPLE_RECIPE: RecipeId = RecipeId::from_raw(1);
const SAMPLE_VERSION: u32 = 1;

/// Operator codes for the neutral-word domain this tool inspects.
const OP_CONST: u16 = 0;
const OP_DRAW: u16 = 1;
const OP_ADD: u16 = 2;
const OP_XOR: u16 = 3;

/// The sample recipe inspected: a literal, two entropy draws, and combines of
/// them — enough to show every node op and a non-trivial per-node log.
fn sample_recipe() -> RecipeGraph {
    let mut recipe = RecipeGraph::new(SAMPLE_RECIPE, SAMPLE_VERSION);
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

/// The generic constraints the output words are validated against.
fn constraints() -> [Constraint; 2] {
    [Constraint::min_count(1), Constraint::non_zero()]
}

/// Build a content address from a segment path.
fn site(segments: &[u64]) -> Address {
    segments.iter().fold(SpaceApi::root(), |address, &segment| {
        SpaceApi::child(&address, segment)
    })
}

/// Parse `[seed] [addr-seg ...]` from the argument list, defaulting the seed to
/// `2026` and the address to the root.
fn parse(args: &[String]) -> (u64, Vec<u64>) {
    let seed = args.first().and_then(|s| s.parse().ok()).unwrap_or(2026);
    let segments = args.iter().skip(1).filter_map(|s| s.parse().ok()).collect();
    (seed, segments)
}

/// Execute the sample recipe, returning one `(op_code, word)` entry per node in
/// evaluation order — the per-node output dump that replaces v1's `ProcTrace`.
fn node_outputs(recipe: &RecipeGraph, seed: u64, address: &Address) -> Vec<(u16, u64)> {
    let log: RefCell<Vec<(u16, u64)>> = RefCell::new(Vec::new());
    ProcCore::new()
        .execute(recipe, seed, address, |ctx| {
            let op = ctx.op();
            let out = OPS.get(usize::from(op)).copied().and_then(|f| f(ctx));
            out.inspect(|&word| log.borrow_mut().push((op, word)))
        })
        .expect("the sample recipe is a valid DAG over known operators");
    log.into_inner()
}

/// The full provenance report for `(seed, address)`, as printable text. Pure and
/// deterministic — the same inputs always produce the same report.
fn provenance_report(seed: u64, address: &Address) -> String {
    let recipe = sample_recipe();
    let outputs = node_outputs(&recipe, seed, address);
    let words: Vec<u64> = outputs.iter().map(|&(_, word)| word).collect();
    let report = ProcValidateApi::validate(&words, &constraints());

    let mut out = String::new();
    out.push_str("axiom proc-inspect — generation provenance\n");
    out.push_str(&format!("  seed            : {seed}\n"));
    out.push_str(&format!("  address         : {:?}\n", address.segments()));
    out.push_str(&format!("  recipe nodes    : {}\n", recipe.node_count()));
    out.push_str(&format!(
        "  recipe digest   : {:#018x}\n\n",
        recipe.digest().raw()
    ));

    out.push_str(&format!("  output words    : {words:?}\n"));
    out.push_str(&format!("  trace ({} nodes):\n", outputs.len()));
    push_node_outputs(&mut out, &outputs);
    out.push('\n');

    push_validation(&mut out, &report);
    out.push_str(&format!(
        "  report digest   : {:#018x}\n",
        report.digest().raw()
    ));
    out
}

fn push_node_outputs(out: &mut String, outputs: &[(u16, u64)]) {
    for (i, &(op, value)) in outputs.iter().enumerate() {
        out.push_str(&format!("    [{i}] op={op} -> {value}\n"));
    }
}

fn push_validation(out: &mut String, report: &ValidationReport) {
    out.push_str(&format!(
        "  validation: all_satisfied={} total_score={}\n",
        report.all_satisfied(),
        report.total_score()
    ));
    for &(kind, satisfied, score) in report.verdicts() {
        out.push_str(&format!(
            "    constraint kind={kind} satisfied={satisfied} score={score}\n"
        ));
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (seed, segments) = parse(&args);
    print!("{}", provenance_report(seed, &site(&segments)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_is_deterministic_and_complete() {
        let address = site(&[7, 42]);
        let a = provenance_report(2026, &address);
        let b = provenance_report(2026, &address);
        assert_eq!(a, b);
        // The report names every boundary of the chain.
        for needle in [
            "seed",
            "address",
            "recipe digest",
            "output words",
            "trace",
            "validation",
            "report digest",
        ] {
            assert!(a.contains(needle), "report should mention `{needle}`");
        }
    }

    #[test]
    fn every_node_op_appears_in_the_dump() {
        let outputs = node_outputs(&sample_recipe(), 2026, &site(&[7, 42]));
        let ops: Vec<u16> = outputs.iter().map(|&(op, _)| op).collect();
        assert_eq!(ops, vec![OP_CONST, OP_DRAW, OP_DRAW, OP_ADD, OP_XOR]);
        assert_eq!(outputs[0].1, 1000, "the literal node reports its immediate");
    }

    #[test]
    fn distinct_inputs_change_the_report() {
        let base = provenance_report(2026, &site(&[7, 42]));
        assert_ne!(base, provenance_report(2027, &site(&[7, 42]))); // seed
        assert_ne!(base, provenance_report(2026, &site(&[7, 43]))); // address
    }

    #[test]
    fn parse_defaults_and_reads_seed_and_segments() {
        assert_eq!(parse(&[]), (2026, vec![]));
        let args = ["99".to_string(), "1".to_string(), "2".to_string()];
        assert_eq!(parse(&args), (99, vec![1, 2]));
        // A non-numeric seed falls back to the default; junk segments are skipped.
        let junk = ["xyz".to_string(), "5".to_string(), "no".to_string()];
        assert_eq!(parse(&junk), (2026, vec![5]));
    }
}
