//! Phase 1 — stored golden capture for the deterministic Zanzoban game.
//!
//! The puzzle core is pure and deterministic: time is the kernel's
//! `SimulationClock` and ghost replay is driven by a recorded path, so a fixed
//! command script produces a fully reproducible trajectory. This test drives
//! such a script through `PuzzleGameState` (built from the embedded level 001)
//! and pins the resulting trajectory — player position, every ghost's
//! position, the current tick, and the solved flag after each command — as
//! committed golden bytes. Every captured value is integer/enum data (no
//! `f32`), so the bytes are platform-stable.
//!
//! A *missing* golden is captured on the next run; an *existing* golden must
//! match. To re-capture after an intended change, delete the golden or run
//! with `AXIOM_REGOLD=1`, then review the diff.

use std::path::PathBuf;

use axiom_gallery::zanzoban::actor_state::ActorState;
use axiom_gallery::zanzoban::game_command::PuzzleCommand;
use axiom_gallery::zanzoban::{level_codec, Direction, PuzzleGameState, LEVEL_001_TOML};

fn golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("zanzoban/golden");
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
            "golden mismatch for `{name}` ({} vs {} bytes): Zanzoban output \
             drifted. If intended, re-capture (delete the golden or set \
             AXIOM_REGOLD=1).",
            actual.len(),
            expected.len(),
        ),
        _ => {
            std::fs::create_dir_all(path.parent().unwrap()).expect("create golden dir");
            std::fs::write(&path, actual).expect("write golden");
        }
    }
}

/// A fixed command script exercising movement, a freeze-into-ghost (`q`), more
/// movement, and ticks that drive the ghost's replay.
fn script() -> Vec<PuzzleCommand> {
    use Direction::{Down, Left, Right, Up};
    use PuzzleCommand::{Move, ResetLifeFromRecording, Tick};
    vec![
        Move(Up),
        Move(Up),
        Move(Right),
        Move(Right),
        ResetLifeFromRecording,
        Tick,
        Tick,
        Move(Down),
        Move(Left),
        Tick,
        Tick,
        Tick,
        Move(Up),
        Move(Right),
    ]
}

fn encode_actor(out: &mut Vec<u8>, a: &ActorState) {
    out.extend_from_slice(&a.id.raw().to_le_bytes());
    // kind: 0 = player, 1 = ghost.
    out.push(match a.kind {
        axiom_gallery::zanzoban::actor_state::ActorKind::Player => 0,
        axiom_gallery::zanzoban::actor_state::ActorKind::Ghost => 1,
    });
    out.extend_from_slice(&a.position.x.to_le_bytes());
    out.extend_from_slice(&a.position.y.to_le_bytes());
}

fn apply(state: &mut PuzzleGameState, command: PuzzleCommand) {
    match command {
        PuzzleCommand::Move(d) => {
            state.apply_player_move(d);
        }
        PuzzleCommand::ResetLifeFromRecording => {
            state.reset_life_from_recording();
        }
        PuzzleCommand::RestartLevelFresh => {
            state.restart_fresh();
        }
        PuzzleCommand::Tick => {
            state.tick();
        }
    }
}

/// Drive the fixed script and capture the trajectory as canonical bytes: after
/// each command, the player, the ghost count + each ghost, the current tick,
/// and the solved flag.
fn run_script() -> Vec<u8> {
    let level = level_codec::from_toml(LEVEL_001_TOML).expect("embedded level parses");
    let mut state = PuzzleGameState::new(level);
    let mut out = Vec::new();
    for command in script() {
        apply(&mut state, command);
        encode_actor(&mut out, &state.player());
        let ghosts = state.ghost_states();
        out.extend_from_slice(&(ghosts.len() as u32).to_le_bytes());
        ghosts.iter().for_each(|g| encode_actor(&mut out, g));
        out.extend_from_slice(&state.current_tick().to_le_bytes());
        out.push(u8::from(state.is_solved()));
    }
    out
}

#[test]
fn golden_zanzoban_trajectory() {
    assert_golden("zanzoban_trajectory", &run_script());
}

#[test]
fn script_is_deterministic() {
    assert_eq!(run_script(), run_script());
}

#[test]
fn the_script_creates_a_ghost() {
    // Guard against a degenerate golden: the `q` command must actually produce
    // a ghost, so the trajectory exercises ghost replay (the interesting path).
    let level = level_codec::from_toml(LEVEL_001_TOML).expect("embedded level parses");
    let mut state = PuzzleGameState::new(level);
    for command in script() {
        apply(&mut state, command);
    }
    assert!(
        state.ghost_count() >= 1,
        "the script's freeze must create at least one ghost"
    );
}
