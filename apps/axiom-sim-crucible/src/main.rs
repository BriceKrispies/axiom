//! Headless entry point for the Axiom Simulation Crucible: run the deterministic
//! scenario and print the structured causal-chain report + replay verification.

fn main() {
    print!("{}", axiom_sim_crucible::run_report());
}
