//! The agent plays Burnt Rubber, headlessly, from the grid to the finish arch.
//!
//! Native-only and behind the `agent` feature:
//! `cargo test -p axiom-burnt-rubber --features agent --test agent_race -- --nocapture`


use axiom_burnt_rubber::agent;

/// Twenty minutes of simulated racing — far more than the course needs, so a
/// failure here is "the agent cannot finish", never "the budget was tight".
const STEP_LIMIT: u32 = 60 * 60 * 20;

/// The same seed and the same agent produce the same race, to the step.
#[test]
fn the_agent_race_is_deterministic() {
    let a = agent::race_to_the_finish(STEP_LIMIT);
    let b = agent::race_to_the_finish(STEP_LIMIT);
    assert_eq!(a, b);
}
