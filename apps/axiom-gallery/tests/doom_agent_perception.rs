//! Behavioural proof that the DOOM agent genuinely **perceives** its world,
//! driven only through the public `perception::DoomPerceiver` surface.
//! It locks the three capabilities the live perception build exists to prove —
//! all game-agnostic (the sensor model is `axiom-perception`; this app only casts
//! its rays against the DOOM engine world):
//! 1. **Facing a wall, with a real distance.** From the start pose (yaw 0) the
//!    centre probe finds the north wall ~7.5 m ahead, classified geometry (no
//!    enemy `Tag`).
//! 2. **Seeing and classifying an entity.** A chasing enemy entering the cone is
//!    reported visible and classified `ENEMY` off its engine-native `Tag`.
//! 3. **Tracking a moving object.** A subject seen on consecutive ticks reports a
//!    non-zero per-tick velocity.
//! Plus determinism: the same run perceives the same thing every time.
#![cfg(feature = "doom-agent")]

use axiom_gallery::doom::perception::{DoomPerceiver, KIND_ENEMY};

#[test]
fn the_agent_faces_the_north_wall_at_a_real_distance() {
    // Start 'S' is (col 1, row 8) facing -Z; the nearest wall straight ahead is
    // the top row's cell at col 1 (near face z = 0.5), so 8 - 0.5 = 7.5 m.
    let perceiver = DoomPerceiver::new();
    let ahead = perceiver.sight().ahead.expect("a wall is dead ahead");
    assert!(
        (ahead.distance_m - 7.5).abs() < 0.05,
        "north wall ~7.5 m ahead, got {}",
        ahead.distance_m
    );
    assert_eq!(ahead.kind, None, "untagged geometry — a wall, not an enemy");
}

#[test]
fn the_agent_sees_classifies_and_tracks_a_moving_enemy() {
    let mut perceiver = DoomPerceiver::new();
    let mut saw_enemy = false;
    let mut tracked_moving = false;
    for _ in 0..150 {
        let sight = perceiver.advance();
        assert!(sight.visible.iter().all(|v| v.kind == KIND_ENEMY));
        saw_enemy |= !sight.visible.is_empty();
        tracked_moving |= sight
            .tracked
            .iter()
            .any(|t| t.vx.abs() > 1.0e-5 || t.vz.abs() > 1.0e-5);
    }
    assert!(saw_enemy, "the agent saw and classified an enemy");
    assert!(tracked_moving, "the agent tracked a moving enemy's velocity");
}

#[test]
fn perception_replays_identically() {
    let run = || {
        let mut p = DoomPerceiver::new();
        let mut report = String::new();
        for _ in 0..60 {
            for line in p.advance().report_lines() {
                report.push_str(&line);
                report.push('\n');
            }
        }
        report
    };
    assert_eq!(run(), run(), "same perception every run");
}
