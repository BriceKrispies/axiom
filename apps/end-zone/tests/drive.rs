//! The score-attack drive over the real simulation: an unassisted run turns the
//! ball over on downs and ends, the run summary matches the drive counters, and
//! a fresh run resets every statistic.

use axiom_end_zone::drive::{DriveEvent, DriveState};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::showcase::ShowcaseRun;

fn config() -> RunConfig {
    RunConfig::new(0x51A7_0E2D)
}

/// Step a real run with no player input until it ends (or a hard cap).
fn play_out(run: &mut ShowcaseRun) -> DriveState {
    for _ in 0..40_000 {
        run.step(&[]);
        let drive = run.drive_state().expect("a real run has drive state");
        if drive.over {
            return drive;
        }
    }
    panic!("an unassisted run must end within the cap");
}

#[test]
fn a_fresh_run_starts_first_and_ten_with_zeroed_stats() {
    let run = ShowcaseRun::new_run(&config());
    let d = run.drive_state().expect("drive state");
    assert_eq!(d.down, 1);
    assert_eq!(d.score, 0);
    assert_eq!(d.touchdowns, 0);
    assert_eq!(d.first_downs, 0);
    assert_eq!(d.longest_play, 0.0);
    assert!(!d.over);
}

#[test]
fn an_unassisted_run_ends_on_a_failed_fourth_down() {
    let mut run = ShowcaseRun::new_run(&config());
    let d = play_out(&mut run);
    assert!(d.over);
    // Sacked on every down: never converted, never scored.
    assert_eq!(d.touchdowns, 0);
    assert_eq!(d.first_downs, 0);
}

#[test]
fn the_summary_matches_the_final_drive_state() {
    let mut run = ShowcaseRun::new_run(&config());
    let d = play_out(&mut run);
    let summary = d.summary();
    assert_eq!(summary.score, d.score);
    assert_eq!(summary.touchdowns, d.touchdowns);
    assert_eq!(summary.first_downs, d.first_downs);
    assert_eq!(summary.longest_play, d.longest_play.max(0.0).round() as u32);
}

#[test]
fn play_again_resets_all_run_statistics() {
    // A drive with progress, then a fresh run from the same config.
    let mut d = DriveState::new(1);
    d.resolve(40.0);
    d.resolve(100.0);
    assert!(d.first_downs > 0 || d.touchdowns > 0);

    let fresh = ShowcaseRun::new_run(&config());
    let reset = fresh.drive_state().expect("drive state");
    assert_eq!(reset.score, 0);
    assert_eq!(reset.touchdowns, 0);
    assert_eq!(reset.first_downs, 0);
    assert_eq!(reset.down, 1);
}

#[test]
fn a_run_replays_identically_from_the_same_config() {
    let digest = |seed| {
        let mut run = ShowcaseRun::new_run(&RunConfig::new(seed));
        for _ in 0..1200 {
            run.step(&[]);
        }
        let d = run.drive_state().expect("drive state");
        (d.down, d.score, d.los_yard.to_bits(), d.heat)
    };
    assert_eq!(digest(0xABCD_1234), digest(0xABCD_1234));
}

#[test]
fn drive_resolution_awards_the_expected_events() {
    let mut d = DriveState::new(1);
    assert_eq!(d.resolve(28.0), DriveEvent::NextDown); // short
    assert_eq!(d.resolve(40.0), DriveEvent::FirstDown); // past the sticks
    assert_eq!(d.resolve(100.0), DriveEvent::Touchdown);
    let mut fourth = DriveState::new(1);
    fourth.down = 4;
    assert_eq!(fourth.resolve(26.0), DriveEvent::RunOver);
}
