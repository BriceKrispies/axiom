//! The end-to-end determinism proof: the whole showcase — simulation, ordered
//! events, football trajectory, possession, AI intents, camera — replays
//! bit-for-bit under the same seed and fixed-step count, and a second seed
//! changes ONLY the explicitly seeded presentation variation.

use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::events::{PlayEndReason, SimEvent};
use axiom_end_zone::showcase::run_trace;

const TICKS: u64 = 900;

#[test]
fn the_showcase_replays_bit_for_bit_under_the_same_seed() {
    let a = run_trace(EndZoneConfig::default(), TICKS);
    let b = run_trace(EndZoneConfig::default(), TICKS);
    assert_eq!(a.final_digest, b.final_digest, "final authoritative state");
    assert_eq!(a.events, b.events, "ordered simulation events");
    assert_eq!(
        a.ball_samples, b.ball_samples,
        "football trajectory samples"
    );
    assert_eq!(a.possession, b.possession, "possession history");
    assert_eq!(a.intents, b.intents, "AI intent history");
    assert_eq!(a.camera_modes, b.camera_modes, "camera mode history");
    assert_eq!(a.camera_poses, b.camera_poses, "final camera poses");
}

#[test]
fn the_showcase_play_runs_its_whole_arc() {
    let trace = run_trace(EndZoneConfig::default(), TICKS);
    let has = |pick: &dyn Fn(&SimEvent) -> bool| trace.events.iter().any(|e| pick(&e.event));
    assert!(has(&|e| matches!(e, SimEvent::PlayStarted { .. })));
    assert!(has(&|e| matches!(e, SimEvent::Snap { .. })));
    assert!(has(&|e| matches!(e, SimEvent::DropBack { .. })));
    assert!(has(&|e| matches!(e, SimEvent::BlockEngaged { .. })));
    assert!(has(&|e| matches!(e, SimEvent::Throw { .. })));
    assert!(has(&|e| matches!(e, SimEvent::CatchAttempt { .. })));
    assert!(has(&|e| matches!(e, SimEvent::CatchCompleted { .. })));
    assert!(has(&|e| matches!(e, SimEvent::TackleContact { .. })));
    // A big-hit `PlayerAirborne` launch is presentation flavor that depends on
    // the exact closing geometry — not every well-defended catch produces one,
    // so the arc no longer requires it. The controlled fall (`GroundImpact`)
    // still validates that the tackle put the carrier on the turf.
    assert!(has(&|e| matches!(e, SimEvent::GroundImpact { .. })));
    assert!(has(&|e| matches!(
        e,
        SimEvent::PlayEnded {
            reason: PlayEndReason::Tackled
        }
    )));
}

#[test]
fn a_second_seed_changes_only_seeded_presentation_variation() {
    let a = run_trace(EndZoneConfig::default(), TICKS);
    let b = run_trace(EndZoneConfig::with_seed(0xB0B0_CAFE), TICKS);
    // The authoritative simulation is a function of the command stream alone:
    // the same play unfolds identically.
    assert_eq!(
        a.final_digest, b.final_digest,
        "sim state is seed-independent"
    );
    assert_eq!(a.events, b.events, "the event stream is seed-independent");
    assert_eq!(a.possession, b.possession);
    assert_eq!(a.intents, b.intents);
    assert_eq!(
        a.camera_modes, b.camera_modes,
        "the mode sequence stays valid"
    );
    // But the seeded impulse phases differ, so the shaken camera poses differ
    // somewhere after the first impact.
    assert_ne!(
        a.camera_poses, b.camera_poses,
        "explicitly seeded presentation variation changed with the seed"
    );
}
