//! Deterministic replay proof for `axiom-recording`.
//!
//! This is the module-level instance of the engine's replay-determinism test:
//! a fixed initial state plus a fixed scenario of synthetic input packets is run
//! through a tiny deterministic "simulation", recording each frame's opaque
//! artifact bytes into timeline A. The same scenario is then replayed from the
//! same initial state into timeline B, and the two recordings are compared
//! *through `RecordingApi`*. A correct deterministic pipeline yields a
//! byte-identical pair; the determinism report is the diagnostic when it does
//! not. (It is exercised here at the recorder's own boundary — the lowest
//! deterministic boundary that owns canonical artifacts — independent of any
//! app, renderer, or GPU.)

use axiom_kernel::FrameIndex;
use axiom_recording::RecordingApi;

/// A fixed scenario of synthetic input packets (one per frame). These are the
/// only nondeterminism source a real sim would have; fixing them fixes the run.
const SCENARIO: [&[u8]; 6] = [
    b"start", b"move:+1", b"move:+3", b"wait", b"move:-2", b"stop",
];

/// A tiny, fully deterministic "simulation": an accumulator advanced by hashing
/// the previous state with the frame's input packet. It has no clock, no
/// randomness, and no hidden state — identical inputs from an identical initial
/// state always produce an identical sequence of states.
fn step_state(prev: u64, input: &[u8]) -> u64 {
    input
        .iter()
        .fold(prev.wrapping_mul(0x100_0000_01b3), |acc, &b| {
            (acc ^ u64::from(b)).wrapping_add(0x9e37_79b9_7f4a_7c15)
        })
}

/// Run the fixed scenario from `initial_state` into a fresh browser-safe
/// recorder, capturing canonical opaque bytes for every artifact each frame.
fn run_scenario(initial_state: u64) -> RecordingApi {
    let mut recorder = RecordingApi::browser_safe().expect("browser-safe budget is valid");
    let mut state = initial_state;
    SCENARIO.iter().enumerate().for_each(|(i, &input)| {
        let frame = i as u64;
        let tick = i as u64;
        state = step_state(state, input);
        // Canonical little-endian serialization of the deterministic state.
        let state_bytes = state.to_le_bytes().to_vec();
        // Runtime artifact: the canonical (frame, tick) advance record.
        let mut runtime_bytes = Vec::new();
        runtime_bytes.extend_from_slice(&frame.to_le_bytes());
        runtime_bytes.extend_from_slice(&tick.to_le_bytes());
        // Render artifact: a deterministic function of the state (what a real
        // render boundary would derive); still just opaque bytes to the recorder.
        let render_bytes = state.rotate_left(7).to_le_bytes().to_vec();
        recorder
            .record_frame(
                frame,
                tick,
                input.to_vec(),
                runtime_bytes,
                state_bytes,
                render_bytes,
            )
            .expect("scenario fits the browser-safe budget");
    });
    recorder
}

/// The fixed initial simulation state for the replay proof.
const INITIAL_STATE: u64 = 0x1234_5678_9abc_def0;

#[test]
fn identical_runs_produce_byte_identical_recordings() {
    let original = run_scenario(INITIAL_STATE);
    let replay = run_scenario(INITIAL_STATE);

    let report = original
        .compare_with(&replay)
        .expect("identical scenarios have matching timeline shapes");

    // The deterministic pipeline must be byte-identical. If it ever is not, the
    // report localizes the first divergence (frame / artifact / byte index)
    // instead of an opaque failure — no ad-hoc debug printing required.
    assert!(
        report.matched(),
        "replay diverged at frame {:?}, artifact {:?}, byte {:?} (hashes {:#x} vs {:#x})",
        report.first_mismatching_frame(),
        report.first_mismatching_artifact(),
        report.first_mismatching_byte_index(),
        report.original_hash(),
        report.replay_hash(),
    );
    assert_eq!(report.first_mismatching_frame(), None);
    assert_eq!(original.frame_count(), SCENARIO.len());
}

#[test]
fn a_perturbed_replay_is_caught_and_localized() {
    let original = run_scenario(1);
    // A replay from a *different* initial state diverges immediately and
    // deterministically; the report must localize it rather than panic.
    let perturbed = run_scenario(2);

    let report = original
        .compare_with(&perturbed)
        .expect("same scenario length and frame indices");
    assert!(!report.matched());
    // The very first frame's state already differs, so frame 0 is reported.
    assert_eq!(report.first_mismatching_frame(), Some(FrameIndex::new(0)));
    assert!(report.first_mismatching_byte_index().is_some());
    assert_ne!(report.original_hash(), report.replay_hash());
}
