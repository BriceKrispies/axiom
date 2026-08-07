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
    // The bar, and how it is cleared. The course is flat out end to end — the
    // agent never lifts and never brakes — so lap time is very nearly a linear
    // function of how much boost the lap earns, and boost is earned by threading
    // traffic: each near miss is 0.13 of the meter and the meter buys 22 m/s.
    //
    // The numbers moved when the course became a compiled plan. The road is
    // genuinely a different road — constant-radius corners rather than relaxed
    // heading noise, a traffic density band rather than a fixed 85 m pitch, and
    // two authored figures (a rolling wall and a slalom) that were not there
    // before — and the agent's technique was fitted by measurement against the
    // *old* one, and the held-boost change moved it again. Measured on the
    // compiled course: 93.90 s, 74 near misses, 13 contacts. The bar is set around that rather than around the old road's,
    // because tightening it further is a re-fit of the driver and not a
    // statement about the course.
    assert!(
        run.elapsed_seconds < 105.0,
        "the agent took {:.2}s — it must beat 105 s",
        run.elapsed_seconds
    );
    assert!(
        run.near_misses > 60,
        "only {} near misses — the agent is not hunting them",
        run.near_misses
    );
    // Chaotic across seeds (1..13 measured on five of them), so the bar is set
    // to the spread rather than to one seed's draw — see `ghost::tests`.
    assert!(run.impacts <= 15, "{} impacts", run.impacts);
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
