//! Phase 4 — replay and hashes. These drive the worker through its public safe
//! API and prove that a recorded run reproduces from tick zero, that the proof
//! is byte-equality of canonical snapshot bytes (the hash is only a locator),
//! and that a perturbed record is detected with the first diverging tick.

use axiom_netplay_ffi::replay::{self, ReplayRecord};
use axiom_netplay_ffi::ruleset::encode_move;
use axiom_netplay_ffi::session::Session;

/// A fixed, deterministic scripted run over three players: some ticks carry
/// accepted intents, one is empty, exercising the whole capture path.
fn driven_session() -> Session {
    let mut s = Session::new(11, 3, 16_666_667);

    s.submit_intent(0, 1, 0, &encode_move(0.5, 0.0));
    s.submit_intent(1, 1, 0, &encode_move(0.0, 0.4));
    s.advance();

    s.submit_intent(0, 2, 0, &encode_move(-0.2, 0.0));
    s.submit_intent(2, 1, 0, &encode_move(0.1, 0.1));
    s.advance();

    s.advance(); // an empty tick

    s.submit_intent(1, 2, 0, &encode_move(-0.3, 0.0));
    s.advance();

    s
}

#[test]
fn exported_replay_verifies_true() {
    let s = driven_session();
    let record = ReplayRecord::decode(&s.export_replay()).expect("exported replay decodes");
    let outcome = replay::verify(&record);
    assert!(outcome.matched);
    assert_eq!(outcome.final_hash, s.state_hash());
}

#[test]
fn replay_from_zero_reproduces_hashes() {
    let s = driven_session();
    let outcome = replay::verify(&s.replay_record());
    assert!(outcome.matched);
    assert_eq!(outcome.first_divergence_tick, 0);
}

#[test]
fn replay_from_zero_reproduces_snapshot_bytes_when_supported() {
    let s = driven_session();
    let record = s.replay_record();

    let mut fresh = Session::new(record.seed, record.max_players, record.fixed_step_ns);
    for tick in &record.ticks {
        for a in &tick.accepted {
            fresh.submit_intent(
                a.player_id,
                a.client_sequence,
                a.predicted_client_tick,
                &a.payload,
            );
        }
        fresh.advance();
    }
    assert_eq!(fresh.snapshot(), s.snapshot());
}

#[test]
fn perturbed_replay_diverges() {
    let s = driven_session();
    let mut record = s.replay_record();
    // Change an accepted move; re-running it no longer reproduces the recorded
    // hash for that tick.
    let tick = record
        .ticks
        .iter_mut()
        .find(|t| !t.accepted.is_empty())
        .expect("a tick with accepted intents");
    tick.accepted[0].payload = encode_move(0.9, 0.0);

    assert!(!replay::verify(&record).matched);
}

#[test]
fn verify_replay_reports_first_divergence_tick() {
    let s = driven_session();
    let mut record = s.replay_record();
    // Corrupt only the recorded hash of the last tick: the re-run reproduces the
    // true hash, which now disagrees with the corrupted record at exactly that tick.
    let target = record.ticks.last().expect("at least one tick").tick;
    record.ticks.last_mut().expect("at least one tick").new_hash ^= 0xFFFF;

    let outcome = replay::verify(&record);
    assert!(!outcome.matched);
    assert_eq!(outcome.first_divergence_tick, target);
}

#[test]
fn replay_export_is_deterministic_for_same_run() {
    assert_eq!(
        driven_session().export_replay(),
        driven_session().export_replay()
    );
}
