//! The agent plays Burnt Rubber, headlessly, from the grid to the finish arch.
//!
//! Native-only and behind the `agent` feature:
//! `cargo test -p axiom-burnt-rubber --features agent --test agent_race -- --nocapture`


use axiom_burnt_rubber::agent;

/// Twenty minutes of simulated racing — far more than the course needs, so a
/// failure here is "the agent cannot finish", never "the budget was tight".
const STEP_LIMIT: u32 = 60 * 60 * 20;

#[test]
fn the_agent_drives_the_shipping_course_to_the_finish() {
    let run = agent::race_to_the_finish(STEP_LIMIT);

    println!("\n=== Burnt Rubber — agent run ===");
    run.milestones.iter().for_each(|line| println!("{line}"));
    println!(
        "\nfinished      : {}\n\
         race time     : {:.2} s\n\
         progress      : {:.1}%\n\
         top speed     : {:.1} m/s ({:.0} km/h)\n\
         mean speed    : {:.1} m/s ({:.0} km/h)\n\
         near misses   : {}\n\
         impacts       : {} ({} traffic)\n\
         off road      : {} steps ({:.1} s)\n\
         lifted/braking: {} / {} steps\n\
         boost steps   : {} ({:.1} s)\n\
         decisions     : {}\n\
         axis intents  : {}\n",
        run.finished,
        run.elapsed_seconds,
        run.progress * 100.0,
        run.top_speed,
        run.top_speed * 3.6,
        run.mean_speed,
        run.mean_speed * 3.6,
        run.near_misses,
        run.impacts,
        run.traffic_impacts,
        run.offroad_steps,
        run.offroad_steps as f32 / 60.0,
        run.lifted_steps,
        run.braking_steps,
        run.boost_steps,
        run.boost_steps as f32 / 60.0,
        run.decisions,
        run.axis_intents,
    );

    assert!(run.finished, "the agent did not reach the finish line");
    assert!(run.progress > 0.99);
    // Every step of the race was an agent decision, and every decision emitted
    // at least the two steering intents — cut the agent out and the car does not
    // move.
    assert_eq!(run.decisions, u64::from(run.steps));
    assert!(run.axis_intents >= run.decisions * 2);
}

/// The same seed and the same agent produce the same race, to the step.
#[test]
fn the_agent_race_is_deterministic() {
    let a = agent::race_to_the_finish(STEP_LIMIT);
    let b = agent::race_to_the_finish(STEP_LIMIT);
    assert_eq!(a, b);
}
