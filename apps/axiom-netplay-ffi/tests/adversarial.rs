//! Phase 9 — malicious client harness. A hostile client can only ever hand the
//! worker an opaque, bounded, host-addressed intent. These tests drive every
//! attack the worker can see and assert the invariants: nothing panics, the
//! authoritative state stays legal and serializable, bad input is rejected with
//! a reason, and a rejected intent never mutates state or poisons the per-player
//! sequence cursor.

use axiom_netplay_ffi::replay;
use axiom_netplay_ffi::ruleset::encode_move;
use axiom_netplay_ffi::session::Session;
use axiom_netplay_ffi::status::*;

fn session() -> Session {
    Session::new(5, 2, 16_666_667)
}

#[test]
fn replayed_sequence_is_rejected_and_state_unchanged() {
    let mut s = session();
    assert_eq!(
        s.submit_intent(0, 1, 0, &encode_move(0.2, 0.0)),
        REASON_NONE
    );
    s.advance();
    let before = s.state_hash();
    // Replay attack: resend an already-accepted sequence.
    assert_eq!(
        s.submit_intent(0, 1, 0, &encode_move(0.2, 0.0)),
        REASON_DUPLICATE_SEQUENCE
    );
    let (_, after) = s.advance();
    assert_eq!(before, after);
}

#[test]
fn malformed_payload_is_rejected_and_process_survives() {
    let mut s = session();
    assert_eq!(s.submit_intent(0, 1, 0, &[0xFF, 0x00]), REASON_MALFORMED);
    // The worker is still fully usable after a malformed intent.
    assert_eq!(
        s.submit_intent(0, 1, 0, &encode_move(0.1, 0.0)),
        REASON_NONE
    );
    s.advance();
    assert!(!s.snapshot().is_empty());
}

#[test]
fn impossible_movement_is_rejected_or_clamped_by_ruleset() {
    let mut s = session();
    // Teleport attempt — far beyond the per-intent bound.
    assert_eq!(
        s.submit_intent(0, 1, 0, &encode_move(1000.0, 1000.0)),
        REASON_IMPOSSIBLE_MOVEMENT
    );
    // The illegal intent moved nothing.
    let before = s.state_hash();
    let (_, after) = s.advance();
    assert_eq!(before, after);
}

#[test]
fn oversized_payload_is_rejected() {
    let mut s = session();
    let huge = vec![0u8; 1024 * 1024];
    assert_eq!(s.submit_intent(0, 1, 0, &huge), REASON_PAYLOAD_TOO_LARGE);
}

#[test]
fn wrong_player_is_rejected() {
    let mut s = session();
    assert_eq!(
        s.submit_intent(7, 1, 0, &encode_move(0.1, 0.0)),
        REASON_INVALID_PLAYER
    );
}

#[test]
fn adversarial_stream_preserves_state_invariants() {
    let mut s = session();
    let mut seq0 = 0u64; // one monotonic cursor for player 0's legitimate stream

    for round in 0..50u64 {
        // A legitimate move (monotonic sequence).
        seq0 += 1;
        s.submit_intent(0, seq0, round, &encode_move(0.05, 0.0));

        // Every attack the worker can see. Each is rejected; a rejection never
        // advances the cursor, so none poisons the legitimate stream.
        s.submit_intent(0, seq0, round, &encode_move(0.05, 0.0)); // duplicate
        s.submit_intent(0, 0, round, &encode_move(0.05, 0.0)); // stale sequence
        s.submit_intent(9, seq0, round, &encode_move(0.05, 0.0)); // wrong player
        s.submit_intent(1, 1000 + round, u64::MAX, &encode_move(9999.0, 0.0)); // teleport + absurd future tick
        s.submit_intent(0, seq0, round, &[0x01]); // malformed

        // Action spam: many monotonic intents in one tick — accepted up to the
        // rate limit, then rejected, never panicking.
        for _ in 0..15u64 {
            seq0 += 1;
            s.submit_intent(0, seq0, round, &encode_move(0.005, 0.0));
        }

        s.advance();
    }

    // Invariant 1: state is still serializable and non-empty.
    assert!(!s.snapshot().is_empty());
    // Invariant 2: despite the attacks, the run is deterministic — its own replay
    // reproduces it exactly (hashes and final state).
    let outcome = replay::verify(&s.replay_record());
    assert!(outcome.matched);
    assert_eq!(outcome.final_hash, s.state_hash());
}
