//! App-level deterministic replay proof for the DOOM game.
//!
//! Drives the **real** deterministic `DoomGame` (not a stand-in) through a fixed
//! initial level and a fixed scenario of synthetic input intents, recording each
//! frame's canonical opaque artifacts into an `axiom-recording` timeline. It then
//! replays the identical scenario from a fresh game into a second timeline and
//! compares the two **through `RecordingApi`**, asserting they are byte-identical.
//! If they ever diverged, the `DeterminismReport` would localize the first
//! mismatch (frame / artifact / byte) — no ad-hoc debug printing.
//!
//! Artifact coverage at this boundary: the **input** artifact is the per-tick
//! intent and the **state** artifact is the deterministic HUD projection of the
//! game (health / score / ammo / enemies). The **render** and **runtime**
//! artifacts are intentionally empty here: the instance-float render output comes
//! from the live windowing pipeline (driven only in the wasm32 browser arm via
//! the frame scrubber), which a native test does not stand up. Empty artifacts
//! record and compare exactly like populated ones, so the game's determinism is
//! still proven end-to-end through the recorder.

use axiom_gallery::doom::level::LevelDoc;
use axiom_gallery::doom::{apply_lifecycle, build_doom_app, DoomGame, Hud, Intent};
use axiom_recording::RecordingApi;

/// A fixed scenario of held-input intents, one per tick. Fixing these fixes the
/// whole run, exactly as a recorded input track would.
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

/// Canonical *input* artifact: a 7-bit button mask + the two look deltas (LE f32).
fn encode_intent(i: &Intent) -> Vec<u8> {
    let mask = (i.forward as u8)
        | (i.backward as u8) << 1
        | (i.turn_left as u8) << 2
        | (i.turn_right as u8) << 3
        | (i.strafe_left as u8) << 4
        | (i.strafe_right as u8) << 5
        | (i.fire as u8) << 6;
    let mut bytes = vec![mask];
    bytes.extend_from_slice(&i.look_yaw.to_le_bytes());
    bytes.extend_from_slice(&i.look_pitch.to_le_bytes());
    bytes
}

/// Canonical *state* artifact: the deterministic HUD projection of the game.
fn encode_hud(h: &Hud) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(&h.health.to_le_bytes());
    bytes.extend_from_slice(&h.score.to_le_bytes());
    bytes.extend_from_slice(&h.ammo.to_le_bytes());
    bytes.extend_from_slice(&h.enemies_alive.to_le_bytes());
    bytes
}

/// Run the fixed scenario through a fresh DOOM game, recording each frame's
/// input + state artifacts (render/runtime left empty — see the module note).
fn record_run() -> RecordingApi {
    let mut recorder = RecordingApi::native().expect("native recorder budget is valid");
    let doc = LevelDoc::default();
    let mut game = DoomGame::from_level(&doc);
    let (mut app, assets) = build_doom_app(&doc);
    game.bind_entities(&app);
    scenario().into_iter().enumerate().for_each(|(i, intent)| {
        let commands = game.step(intent, &app);
        apply_lifecycle(&mut game, &mut app, &assets, &commands);
        app.tick_with_controls(i as u64, &commands.enemies, &[commands.control]);
        let frame = i as u64;
        recorder
            .record_frame(
                frame,
                frame,
                encode_intent(&intent),
                Vec::new(),
                encode_hud(&commands.hud),
                Vec::new(),
            )
            .expect("fits the native budget");
    });
    recorder
}

#[test]
fn doom_game_replays_byte_identically_through_the_recorder() {
    let original = record_run();
    let replay = record_run();

    let report = original
        .compare_with(&replay)
        .expect("identical scenarios share timeline shape");

    assert!(
        report.matched(),
        "DOOM replay diverged at frame {:?}, artifact {:?}, byte {:?} ({:#x} vs {:#x})",
        report.first_mismatching_frame(),
        report.first_mismatching_artifact(),
        report.first_mismatching_byte_index(),
        report.original_hash(),
        report.replay_hash(),
    );
    assert_eq!(original.frame_count(), scenario().len());
}

#[test]
fn a_diverging_input_track_is_detected() {
    // A different scenario (one extra fire press on frame 0) must NOT match —
    // proving the comparison detects divergence, not just confirms identity.
    let original = record_run();

    let mut perturbed = RecordingApi::native().unwrap();
    let doc = LevelDoc::default();
    let mut game = DoomGame::from_level(&doc);
    let (mut app, assets) = build_doom_app(&doc);
    game.bind_entities(&app);
    scenario().into_iter().enumerate().for_each(|(i, intent)| {
        let mut intent = intent;
        // Force-fire on the very first frame only.
        intent.fire = intent.fire || i == 0;
        let commands = game.step(intent, &app);
        apply_lifecycle(&mut game, &mut app, &assets, &commands);
        app.tick_with_controls(i as u64, &commands.enemies, &[commands.control]);
        let frame = i as u64;
        perturbed
            .record_frame(
                frame,
                frame,
                encode_intent(&intent),
                Vec::new(),
                encode_hud(&commands.hud),
                Vec::new(),
            )
            .unwrap();
    });

    let report = original.compare_with(&perturbed).unwrap();
    assert!(!report.matched());
    assert!(report.first_mismatching_frame().is_some());
}

/// Per-frame paired (game-state, engine-scene) snapshots, plus the final game state.
type StatesPerFrame = (Vec<(Vec<u8>, Vec<u8>)>, Vec<u8>);

/// Run the fixed scenario and return, at every frame boundary, the paired
/// (game-state, engine-scene) snapshot *before* applying intent `i`, plus the
/// final game state. Fork-and-resume now restores BOTH halves: the game's
/// spatial decisions depend on the engine's enemy positions, so the engine scene
/// must be forked alongside the game (exactly what the browser scrubber does).
fn states_per_frame() -> StatesPerFrame {
    let doc = LevelDoc::default();
    let mut game = DoomGame::from_level(&doc);
    let (mut app, assets) = build_doom_app(&doc);
    game.bind_entities(&app);
    let states = scenario()
        .into_iter()
        .enumerate()
        .map(|(i, intent)| {
            let snap = (game.write_state(), app.snapshot_sim());
            let commands = game.step(intent, &app);
            apply_lifecycle(&mut game, &mut app, &assets, &commands);
            app.tick_with_controls(i as u64, &commands.enemies, &[commands.control]);
            snap
        })
        .collect();
    (states, game.write_state())
}

#[test]
fn forking_from_a_recorded_frame_and_replaying_reproduces_the_timeline() {
    // Capture the paired state at the start of frame K, fork a fresh game+engine
    // to it, and replay the SAME remaining intents — the resulting end game state
    // must be byte-identical to the un-forked run. This is fork-and-resume.
    let (states, original_end) = states_per_frame();
    let fork_at = 3;

    let doc = LevelDoc::default();
    let mut forked = DoomGame::from_level(&doc);
    let (mut app, assets) = build_doom_app(&doc);
    assert!(forked.read_state(&states[fork_at].0));
    app.restore_sim(&states[fork_at].1)
        .expect("engine fork snapshot round-trips");
    // Re-bind the forked game's enemies to the restored scene's nodes (handles
    // aren't serialized), so its spatial hits classify when the fork resumes.
    forked.bind_entities(&app);
    // Replay the tail on the forked app's own fresh tick sequence (movement is
    // tick-independent — no spin/procanim — so the scene evolution reproduces).
    scenario()
        .into_iter()
        .skip(fork_at)
        .enumerate()
        .for_each(|(j, intent)| {
            let commands = forked.step(intent, &app);
            apply_lifecycle(&mut forked, &mut app, &assets, &commands);
            app.tick_with_controls(j as u64, &commands.enemies, &[commands.control]);
        });
    assert_eq!(forked.write_state(), original_end);
}

#[test]
fn forking_then_diverging_branches_away_from_the_original() {
    // From the same fork point, a DIFFERENT input must produce a different end
    // state — proving the fork is a live branch, not a frozen replay.
    let (states, original_end) = states_per_frame();
    let fork_at = 3;

    let doc = LevelDoc::default();
    let mut branch = DoomGame::from_level(&doc);
    let (mut app, assets) = build_doom_app(&doc);
    assert!(branch.read_state(&states[fork_at].0));
    app.restore_sim(&states[fork_at].1)
        .expect("engine fork snapshot round-trips");
    branch.bind_entities(&app);
    scenario()
        .into_iter()
        .skip(fork_at)
        .enumerate()
        .for_each(|(j, intent)| {
            // Inject an extra forward press the original tail never had.
            let mut intent = intent;
            intent.forward = true;
            let commands = branch.step(intent, &app);
            apply_lifecycle(&mut branch, &mut app, &assets, &commands);
            app.tick_with_controls(j as u64, &commands.enemies, &[commands.control]);
        });
    assert_ne!(branch.write_state(), original_end);
}

#[test]
fn read_state_rejects_truncated_bytes_and_leaves_game_unchanged() {
    let mut game = DoomGame::new();
    let before = game.write_state();
    assert!(!game.read_state(&[1, 2, 3]));
    assert_eq!(game.write_state(), before);
}
