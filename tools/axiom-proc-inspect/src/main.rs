//! `axiom-proc-inspect` — dump the provenance of a generation.
//!
//! Given a seed and a content-address path, it evaluates a sample proc recipe,
//! validates the artifact, and prints the whole `seed → address → artifact →
//! trace → validation` chain: the artifact words, the per-node trace decisions,
//! the constraint verdicts, and the stable digest that indexes each boundary. An
//! agent (or a human) can read off exactly how a piece of content was made and
//! reproduce it from `(seed, address)`.
//!
//! This is **repo tooling**, outside the engine dependency graph — exempt from the
//! coverage and branchless gates — so it uses ordinary control flow and `println!`.
//!
//! ```text
//! cargo run -p axiom-proc-inspect -- [seed] [addr-seg ...]
//! cargo run -p axiom-proc-inspect -- 2026 7 42
//! ```

use std::env;

use axiom_proc::{ProcApi, ProcTrace, Recipe};
use axiom_proc_validate::{Constraint, ProcValidateApi, ValidationReport};
use axiom_space::{Address, SpaceApi};

/// The sample recipe inspected: a literal, two entropy draws, and combines of
/// them — enough to show every node op and a non-trivial trace.
fn sample_recipe() -> Recipe {
    let mut recipe = Recipe::new(1);
    let base = recipe.const_node(1000);
    let a = recipe.draw();
    let b = recipe.draw();
    let mixed = recipe.add(a, b);
    recipe.xor(mixed, base);
    recipe
}

/// The generic constraints the artifact is validated against.
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

/// The full provenance report for `(seed, address)`, as printable text. Pure and
/// deterministic — the same inputs always produce the same report.
fn provenance_report(seed: u64, address: &Address) -> String {
    let recipe = sample_recipe();
    let (artifact, trace) =
        ProcApi::evaluate(&recipe, seed, address).expect("the sample recipe is a valid DAG");
    let report = ProcValidateApi::validate(&artifact, &constraints());

    let mut out = String::new();
    out.push_str("axiom proc-inspect — generation provenance\n");
    out.push_str(&format!("  seed            : {seed}\n"));
    out.push_str(&format!("  address         : {:?}\n", address.segments()));
    out.push_str(&format!("  recipe nodes    : {}\n\n", recipe.len()));

    out.push_str(&format!("  artifact words  : {:?}\n", artifact.words()));
    out.push_str(&format!(
        "  artifact digest : {:#018x}\n\n",
        artifact.digest().raw()
    ));

    out.push_str(&format!("  trace ({} steps):\n", trace.steps().len()));
    push_trace(&mut out, &trace);
    out.push_str(&format!(
        "  trace digest    : {:#018x}\n\n",
        trace.digest().raw()
    ));

    push_validation(&mut out, &report);
    out.push_str(&format!(
        "  report digest   : {:#018x}\n",
        report.digest().raw()
    ));
    out
}

fn push_trace(out: &mut String, trace: &ProcTrace) {
    for (i, &(op, value)) in trace.steps().iter().enumerate() {
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
            "artifact digest",
            "trace",
            "trace digest",
            "validation",
            "report digest",
        ] {
            assert!(a.contains(needle), "report should mention `{needle}`");
        }
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
