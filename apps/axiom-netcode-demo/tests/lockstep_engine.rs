//! Proof that the lockstep session keeps two *real* engine instances in sync.
//!
//! Unlike the module-level convergence proof (which uses a mock sim), these run
//! the actual `axiom` engine `App` on both peers and fingerprint its real
//! `FrameOutcome` each tick.

use axiom_netcode_demo::run_two_peer_lockstep;

#[test]
fn two_real_engine_instances_stay_byte_identical() {
    let report = run_two_peer_lockstep(90, 3);

    assert_eq!(report.peer_a_hashes.len(), 90, "every tick confirmed");
    assert_eq!(
        report.peer_a_hashes, report.peer_b_hashes,
        "the two real engines must agree byte-for-byte at every confirmed tick"
    );
    assert_eq!(report.desync_tick, None, "identical worlds never desync");
}

#[test]
fn the_engine_state_actually_changes_per_tick() {
    // Guards against a vacuous proof: equal hashes on a static world would prove
    // nothing.
    let report = run_two_peer_lockstep(30, 3);
    let distinct = report
        .peer_a_hashes
        .windows(2)
        .filter(|w| w[0] != w[1])
        .count();
    assert!(
        distinct > 20,
        "the rotating cubes should make most ticks distinct; only {distinct} changed"
    );
}

#[test]
fn the_run_is_deterministic_across_invocations() {
    let a = run_two_peer_lockstep(48, 3);
    let b = run_two_peer_lockstep(48, 3);
    assert_eq!(a.peer_a_hashes, b.peer_a_hashes);
    assert_eq!(a.peer_b_hashes, b.peer_b_hashes);
    assert_eq!(a.desync_tick, b.desync_tick);
}

#[test]
fn a_divergent_world_is_caught_by_reconcile() {
    let report = run_two_peer_lockstep(30, 2);
    assert_ne!(
        report.peer_a_hashes, report.peer_b_hashes,
        "different worlds must produce different state hashes"
    );
    assert_eq!(
        report.desync_tick,
        Some(0),
        "a divergent world must be caught at the first confirmed tick"
    );
}
