//! Phase 1 — stored golden capture for the deterministic retro FPS game state.
//!
//! `tests/replay_determinism.rs` already proves the game replays
//! byte-identically *within a run* (record the scenario twice, compare). This
//! file pins the actual bytes *across commits*: it drives a fixed intent
//! scenario through `RetroFpsGame` and stores the canonical `write_state()` bytes
//! at every frame boundary plus the final state as committed golden files.
//! `write_state()` is already the engine's canonical serialization for the
//! game (it round-trips through `read_state` and underpins fork-and-resume),
//! so no extra encoder is needed — the bytes are golden as-is.
//!
//! A *missing* golden is captured on the next run (written, test passes); an
//! *existing* golden must match. To re-capture after an intended change,
//! delete the golden(s) or run with `AXIOM_REGOLD=1`, then review the diff.

use std::path::PathBuf;

use axiom_game_retro_fps::level::LevelDoc;
use axiom_game_retro_fps::{apply_lifecycle, build_retro_fps_app, RetroFpsGame, Hud, Intent};

/// The same fixed scenario the replay-determinism test uses: one held-input
/// intent per tick. Fixing these fixes the whole run.
fn scenario() -> Vec<Intent> {
    let forward = Intent {
        forward: true,
        ..Default::default()
    };
    let turn = Intent {
        turn_left: true,
        ..Default::default()
    };
    let fire = Intent {
        fire: true,
        ..Default::default()
    };
    let strafe_fire = Intent {
        strafe_right: true,
        fire: true,
        ..Default::default()
    };
    vec![
        Intent::default(),
        forward,
        forward,
        turn,
        fire,
        forward,
        strafe_fire,
        turn,
    ]
}

/// Canonical HUD bytes (deterministic integer projection: health/score/ammo/
/// enemies as little-endian), mirroring the replay test's `encode_hud`.
fn encode_hud(h: &Hud) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(&h.health.to_le_bytes());
    bytes.extend_from_slice(&h.score.to_le_bytes());
    bytes.extend_from_slice(&h.ammo.to_le_bytes());
    bytes.extend_from_slice(&h.enemies_alive.to_le_bytes());
    bytes
}

fn golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("retro_fps/golden");
    p.push(format!("{name}.bin"));
    p
}

fn assert_golden(name: &str, actual: &[u8]) {
    let path = golden_path(name);
    let force = std::env::var_os("AXIOM_REGOLD").is_some();
    match std::fs::read(&path).ok() {
        Some(expected) if !force => assert_eq!(
            actual,
            expected.as_slice(),
            "golden mismatch for `{name}` ({} vs {} bytes): retro FPS output drifted. \
             If intended, re-capture (delete the golden or set AXIOM_REGOLD=1).",
            actual.len(),
            expected.len(),
        ),
        _ => {
            std::fs::create_dir_all(path.parent().unwrap()).expect("create golden dir");
            std::fs::write(&path, actual).expect("write golden");
        }
    }
}

/// Drive the fixed scenario, concatenating the canonical state at the start of
/// every frame (length-prefixed so frame boundaries are recoverable) and the
/// HUD projection after each step.
fn run_canonical() -> (Vec<u8>, Vec<u8>) {
    // Drive the real game-against-engine loop: the game asks the engine for its
    // spatial answers, then the engine applies the commands so its world tracks.
    let doc = LevelDoc::default();
    let mut game = RetroFpsGame::from_level(&doc);
    let (mut app, assets) = build_retro_fps_app(&doc);
    game.bind_entities(&app);
    let mut states = Vec::new();
    let mut huds = Vec::new();
    for (tick, intent) in scenario().into_iter().enumerate() {
        let before = game.write_state();
        states.extend_from_slice(&(before.len() as u32).to_le_bytes());
        states.extend_from_slice(&before);
        let commands = game.step(intent, &app);
        apply_lifecycle(&mut game, &mut app, &assets, &commands);
        app.tick_with_controls(tick as u64, &commands.enemies, &[commands.control]);
        huds.extend_from_slice(&encode_hud(&commands.hud));
    }
    // The final state after the whole scenario.
    let end = game.write_state();
    states.extend_from_slice(&(end.len() as u32).to_le_bytes());
    states.extend_from_slice(&end);
    (states, huds)
}

#[test]
fn golden_retro_fps_state_sequence() {
    let (states, _) = run_canonical();
    assert_golden("retro_fps_state_sequence", &states);
}

#[test]
fn golden_retro_fps_hud_sequence() {
    let (_, huds) = run_canonical();
    assert_golden("retro_fps_hud_sequence", &huds);
}

#[test]
fn canonical_run_is_stable() {
    // The capture is a pure function of the fixed scenario.
    assert_eq!(run_canonical(), run_canonical());
}

#[test]
fn a_perturbed_scenario_yields_different_bytes() {
    // Force-fire on frame 0 only — the state/HUD bytes must differ, proving the
    // golden is sensitive to a genuine input change (not a constant).
    let (baseline_states, _) = run_canonical();
    let doc = LevelDoc::default();
    let mut game = RetroFpsGame::from_level(&doc);
    let (mut app, assets) = build_retro_fps_app(&doc);
    game.bind_entities(&app);
    let mut states = Vec::new();
    for (i, intent) in scenario().into_iter().enumerate() {
        let mut intent = intent;
        intent.fire = intent.fire || i == 0;
        let before = game.write_state();
        states.extend_from_slice(&(before.len() as u32).to_le_bytes());
        states.extend_from_slice(&before);
        let commands = game.step(intent, &app);
        apply_lifecycle(&mut game, &mut app, &assets, &commands);
        app.tick_with_controls(i as u64, &commands.enemies, &[commands.control]);
    }
    let end = game.write_state();
    states.extend_from_slice(&(end.len() as u32).to_le_bytes());
    states.extend_from_slice(&end);
    assert_ne!(baseline_states, states);
}
