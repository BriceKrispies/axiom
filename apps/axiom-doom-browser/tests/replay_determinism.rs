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

use axiom_doom_browser::{DoomGame, Hud, Intent};
use axiom_recording::RecordingApi;

/// A fixed scenario of held-input intents, one per tick. Fixing these fixes the
/// whole run, exactly as a recorded input track would.
fn scenario() -> Vec<Intent> {
    let mut forward = Intent::default();
    forward.forward = true;
    let mut turn = Intent::default();
    turn.turn_left = true;
    let mut fire = Intent::default();
    fire.fire = true;
    let mut strafe_fire = Intent::default();
    strafe_fire.strafe_right = true;
    strafe_fire.fire = true;
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
    let mut game = DoomGame::new();
    scenario().into_iter().enumerate().for_each(|(i, intent)| {
        let commands = game.step(intent);
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
    let mut game = DoomGame::new();
    scenario().into_iter().enumerate().for_each(|(i, intent)| {
        let mut intent = intent;
        // Force-fire on the very first frame only.
        intent.fire = intent.fire || i == 0;
        let commands = game.step(intent);
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

/// Run the fixed scenario and return the game's serialized state at every frame
/// boundary (state *before* applying intent `i` is `states[i]`), plus the final
/// state after the whole scenario.
fn states_per_frame() -> (Vec<Vec<u8>>, Vec<u8>) {
    let mut game = DoomGame::new();
    let states = scenario()
        .into_iter()
        .map(|intent| {
            let before = game.write_state();
            game.step(intent);
            before
        })
        .collect();
    (states, game.write_state())
}

#[test]
fn forking_from_a_recorded_frame_and_replaying_reproduces_the_timeline() {
    // Capture the state at the start of frame K, then fork a fresh game to it and
    // replay the SAME remaining intents — the resulting end state must be
    // byte-identical to the un-forked run. This is the heart of fork-and-resume.
    let (states, original_end) = states_per_frame();
    let fork_at = 3;

    let mut forked = DoomGame::new();
    assert!(forked.read_state(&states[fork_at]));
    // Replay the tail of the scenario from the fork point.
    scenario()
        .into_iter()
        .skip(fork_at)
        .for_each(|intent| {
            forked.step(intent);
        });
    assert_eq!(forked.write_state(), original_end);
}

#[test]
fn forking_then_diverging_branches_away_from_the_original() {
    // From the same fork point, a DIFFERENT input must produce a different end
    // state — proving the fork is a live branch, not a frozen replay.
    let (states, original_end) = states_per_frame();
    let fork_at = 3;

    let mut branch = DoomGame::new();
    assert!(branch.read_state(&states[fork_at]));
    scenario().into_iter().skip(fork_at).for_each(|intent| {
        // Inject an extra forward press the original tail never had.
        let mut intent = intent;
        intent.forward = true;
        branch.step(intent);
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
