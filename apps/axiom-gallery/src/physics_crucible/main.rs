//! Headless entry point for the Axiom Physics Crucible: run every station's
//! deterministic scripted scenario and print the structured report (counts,
//! replay-match, query hits, and the honestly-marked physics gaps). The rendered
//! frame is produced separately by `axiom-shot` via `build_physics_crucible`.

fn main() {
    print!("{}", axiom_gallery::physics_crucible::run_report());
}
