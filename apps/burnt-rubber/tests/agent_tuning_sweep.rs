//! A search over driving technique. Ignored by default — this is a measuring
//! tool, not a gate.
//!
//! `cargo test -p axiom-burnt-rubber --features agent --release --test agent_tuning_sweep -- --ignored --nocapture`


use axiom_burnt_rubber::agent::{self, DriverTuning};
use axiom_burnt_rubber::RaceSim;

const STEP_LIMIT: u32 = 60 * 60 * 20;

fn run(driver: &DriverTuning) -> agent::AgentRace {
    agent::race(RaceSim::shipping(), driver, STEP_LIMIT)
}

type Setter = fn(&mut DriverTuning, f32);

fn axes() -> Vec<(&'static str, Vec<f32>, Setter)> {
    vec![
        (
            "contact_penalty",
            vec![8.0, 15.0, 30.0, 60.0, 120.0, 300.0],
            |d, v| d.contact_penalty = v,
        ),
        (
            "touch_margin",
            vec![0.8, 1.2, 1.6, 2.2, 3.0, 4.0],
            |d, v| d.touch_margin = v,
        ),
        (
            "edge_margin",
            vec![0.0, 0.2, 0.5, 0.8, 1.2, 1.8],
            |d, v| d.edge_margin = v,
        ),
        (
            "urgency_falloff",
            vec![0.0, 0.05, 0.12, 0.2, 0.4, 0.9],
            |d, v| d.urgency_falloff = v,
        ),
        (
            "lane_change_cost",
            vec![0.0, 0.02, 0.05, 0.1, 0.2],
            |d, v| d.lane_change_cost = v,
        ),
        (
            "centre_pull",
            vec![0.04, 0.08, 0.12, 0.2, 0.35, 0.6],
            |d, v| d.centre_pull = v,
        ),
        (
            "traffic_horizon",
            vec![20.0, 30.0, 40.0, 50.0, 65.0, 85.0],
            |d, v| d.traffic_horizon = v,
        ),
        (
            "lookahead_base",
            vec![1.0, 2.5, 4.0, 6.0, 9.0, 13.0],
            |d, v| d.lookahead_base = v,
        ),
        (
            "lookahead_per_speed",
            vec![0.24, 0.3, 0.34, 0.4, 0.48, 0.6],
            |d, v| d.lookahead_per_speed = v,
        ),
        (
            "steer_gain_milli",
            vec![4000.0, 5500.0, 7500.0, 10000.0, 14000.0, 20000.0],
            |d, v| d.steer_gain_milli = v as i64,
        ),
        (
            "steer_damping_milli",
            vec![0.0, 60.0, 130.0, 200.0, 300.0, 450.0],
            |d, v| d.steer_damping_milli = v as i64,
        ),
        (
            "boost_min_headroom",
            vec![0.02, 0.3, 1.5, 5.0],
            |d, v| d.boost_min_headroom = v,
        ),
        (
            "grip_usage",
            vec![0.9, 1.0, 1.3],
            |d, v| d.grip_usage = v,
        ),
    ]
}

#[test]
#[ignore]
fn sweep() {
    let mut best = DriverTuning::FAST;
    let base = run(&best);
    let mut best_t = base.elapsed_seconds;
    println!(
        "base {:.2}s impacts={} nm={} offroad={} boost={}",
        base.elapsed_seconds, base.impacts, base.near_misses, base.offroad_steps, base.boost_steps
    );

    let axes = axes();
    (0..4).for_each(|pass| {
        axes.iter().for_each(|(name, values, set)| {
            values.iter().for_each(|&v| {
                let mut candidate = best;
                set(&mut candidate, v);
                let r = run(&candidate);
                let better = r.finished && r.elapsed_seconds < best_t - 0.005;
                (better | (pass == 0)).then(|| {
                    println!(
                        "  p{pass} {name}={v}: {:.2}s fin={} imp={} nm={} off={} boost={}{}",
                        r.elapsed_seconds,
                        r.finished,
                        r.impacts,
                        r.near_misses,
                        r.offroad_steps,
                        r.boost_steps,
                        if better { "  <-- BEST" } else { "" }
                    );
                });
                better.then(|| {
                    best_t = r.elapsed_seconds;
                    best = candidate;
                });
            });
        });
        println!("== pass {pass} best {best_t:.2}s");
    });

    let final_run = run(&best);
    println!(
        "\nBEST {:.2}s  impacts={} nm={} offroad={} boost={} mean={:.1}\n{:#?}",
        final_run.elapsed_seconds,
        final_run.impacts,
        final_run.near_misses,
        final_run.offroad_steps,
        final_run.boost_steps,
        final_run.mean_speed,
        best
    );
}
