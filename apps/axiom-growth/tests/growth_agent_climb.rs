//! Locks the reusable agent climb: holding "forward" through
//! `axiom-agent-harness` walks the growth player from the flat spawn shelf to the
//! Everest-scale summit, **deterministically**, while the agent reports the
//! player's height the whole way. Native + `agent` feature only.
#![cfg(feature = "agent")]

use axiom_agent_harness::AgentHarnessApi;
use axiom_growth::agent::{Action, AgentSession, Observation};

/// Hold FORWARD until the summit (or the cap), returning the final observation.
fn climb_to_summit(session: &mut AgentSession, cap: u64) -> Observation {
    let forward = Action::forward();
    let mut obs = session.observe();
    while !obs.reached_summit && obs.tick < cap {
        obs = session.step(&forward);
    }
    obs
}

#[test]
fn hold_forward_reaches_the_summit_and_reports_height() {
    let mut session = AgentSession::earthlike();
    let start = session.observe();

    // The player begins on the flat spawn shelf, with the mountain far off.
    assert_eq!(start.tick, 0);
    assert!(
        start.height_above_spawn_m.abs() < 1.0,
        "spawn sits on the flat shelf, got {} m",
        start.height_above_spawn_m,
    );
    assert!(
        start.distance_to_peak_m > 100.0,
        "the mountain is a long way off at spawn, got {} m",
        start.distance_to_peak_m,
    );
    assert!(
        start.prominence_m > 4000.0,
        "the vista is Everest-scale, got {} m prominence",
        start.prominence_m,
    );

    let end = climb_to_summit(&mut session, 20_000);

    assert!(
        end.reached_summit,
        "holding forward must reach the summit; ended at {end:?}",
    );
    assert!(
        end.height_above_spawn_m >= end.prominence_m * 0.9,
        "should climb most of the {:.0} m prominence, got {:.0} m above spawn",
        end.prominence_m,
        end.height_above_spawn_m,
    );
    assert!(
        end.height_above_spawn_m > 5000.0,
        "the summit is thousands of metres above spawn, got {:.0} m",
        end.height_above_spawn_m,
    );
    assert!(
        (end.eye_height_m - (end.ground_height_m + 1.7)).abs() < 0.01,
        "eye sits ~1.7 m above the climbed ground",
    );
}

#[test]
fn the_agent_holds_forward_through_the_harness_every_tick() {
    let mut session = AgentSession::earthlike();
    let forward = Action::forward();
    for _ in 0..400 {
        let obs = session.step(&forward);
        // The harness echoed the held FORWARD control back each tick — the agent,
        // not hand-rolled code, is driving the engine.
        assert_eq!(obs.control_code, AgentHarnessApi::FORWARD);
    }
}

#[test]
fn the_climb_is_deterministic() {
    let trace = || {
        let mut session = AgentSession::earthlike();
        let forward = Action::forward();
        (0..1500)
            .map(|_| {
                let obs = session.step(&forward);
                // Compare exact bits so any divergence is caught.
                (
                    obs.x.to_bits(),
                    obs.z.to_bits(),
                    obs.ground_height_m.to_bits(),
                    obs.height_above_spawn_m.to_bits(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        trace(),
        trace(),
        "same seed + same held inputs must replay byte-for-byte",
    );
}

#[test]
fn seeking_the_summit_also_climbs() {
    // The harness's real (non-pass-through) seek policy reaches the summit too:
    // it turns toward the goal and walks. Same destination, different brain.
    let mut session = AgentSession::earthlike();
    let seek = Action::seek();
    let mut obs = session.observe();
    while !obs.reached_summit && obs.tick < 20_000 {
        obs = session.step(&seek);
    }
    assert!(
        obs.reached_summit,
        "seek policy should reach the summit; ended at {obs:?}"
    );
}
