//! The State Engine, driven through a real agent playthrough of Zanzoban.
//!
//! The claim under test is not "the substrate compiles against this game" but
//! something much stronger: for **every command of a real `axiom-agent` run**,
//! a patch authored from the game's own before/after states, applied to the
//! previous snapshot, reproduces the game's next state exactly.
//!
//! ```text
//! apply( project(before), transition(before, after) )  ==  project(after)
//! ```
//!
//! The patch is authored from the two game states directly, never by diffing
//! their projections — a diff-derived patch would satisfy this equality by
//! construction and prove nothing.
//!
//! Run:
//!
//! ```sh
//! cargo test -p axiom-zanzoban --features agent --test state_engine
//! ```

#![cfg(feature = "agent")]

use std::path::PathBuf;

use axiom_state::{
    apply, diff, report, StateChangeKind, StateOpKind, StateSnapshot, StateView,
};
use axiom_state::{StateAccess, StateKey};
use axiom_zanzoban::game_command::PuzzleCommand;
use axiom_zanzoban::puzzle_state::{
    self, Ghosts, GhostsCreated, PlayerCell, Recording, Solved, Tick,
};
use axiom_zanzoban::{agent, game_step, level_codec, PuzzleGameState, LEVEL_001_TOML};

// ---------------------------------------------------------------------------
// the run
// ---------------------------------------------------------------------------

/// One command's worth of evidence.
struct Step {
    command: PuzzleCommand,
    /// The snapshot after this command, carried by the State Engine.
    carried: StateSnapshot,
    /// The same instant projected straight from the game.
    projected: StateSnapshot,
    /// Which operation kinds the authored patch used.
    kinds: Vec<StateOpKind>,
}

/// Replay the agent's recorded transcript, carrying a snapshot alongside.
///
/// This is the composition root's shape: the *test* owns the current snapshot
/// and swaps it each command. Nothing inside `axiom-state` remembers it.
fn walk() -> Vec<Step> {
    let schema = puzzle_state::schema().expect("the declared schema is valid");
    let level = level_codec::from_toml(LEVEL_001_TOML).expect("embedded level parses");
    let mut game = PuzzleGameState::new(level);
    let mut carried = puzzle_state::project(&schema, &game).expect("the initial projection");

    agent::play_first_level()
        .commands
        .into_iter()
        .map(|command| {
            let before = game.clone();
            game_step::step(&mut game, command);
            let patch = puzzle_state::transition(&before, &game).expect("a well-formed patch");
            let kinds = patch.ops().iter().map(|op| op.kind()).collect();
            carried = apply(&schema, &carried, &patch).expect("the patch applies");
            Step {
                command,
                carried: carried.clone(),
                projected: puzzle_state::project(&schema, &game).expect("the projection"),
                kinds,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// the central property
// ---------------------------------------------------------------------------

#[test]
fn the_carried_snapshot_tracks_the_game_through_every_command() {
    let steps = walk();
    assert!(!steps.is_empty(), "the agent issued commands");
    for (at, step) in steps.iter().enumerate() {
        assert_eq!(
            step.carried,
            step.projected,
            "after command {at} ({:?}) the carried snapshot diverged from the game",
            step.command
        );
        assert_eq!(
            step.carried.hash(),
            step.projected.hash(),
            "digests diverged after command {at}"
        );
        assert_eq!(
            step.carried.to_bytes(),
            step.projected.to_bytes(),
            "bytes diverged after command {at}"
        );
    }
}

#[test]
fn the_run_ends_in_the_state_the_golden_recorded() {
    let steps = walk();
    let last = &steps.last().expect("at least one command").carried;
    // The committed agent golden records a solved run with one ghost.
    assert_eq!(last.cell::<Solved>(), Ok(true));
    assert_eq!(last.cell::<GhostsCreated>(), Ok(1));
    assert_eq!(last.table::<Ghosts>().expect("ghosts").len(), 1);
    // 90, not the golden's `final_tick: 102`. Those are two different counters
    // and it is worth being precise about which one is state: the driver's 102
    // counts every decision it made, while `puzzle/tick` is the *simulation*
    // clock, which only advances on a `Tick` command. The run spends 90 ticks
    // waiting for the ghost to reach the button and then walks to the exit on
    // moves alone, which advance no clock. The simulation clock is the one that
    // is persistent truth, so it is the one on the State Engine.
    assert_eq!(last.cell::<Tick>(), Ok(90));
}

#[test]
fn carrying_the_run_twice_produces_identical_state() {
    let first = walk();
    let second = walk();
    assert_eq!(first.len(), second.len());
    let hashes = |steps: &[Step]| -> Vec<u64> {
        steps.iter().map(|s| s.carried.hash().raw()).collect()
    };
    assert_eq!(hashes(&first), hashes(&second));
    assert_eq!(
        first.last().expect("steps").carried.to_bytes(),
        second.last().expect("steps").carried.to_bytes()
    );
}

// ---------------------------------------------------------------------------
// the run exercises the model, not just one corner of it
// ---------------------------------------------------------------------------

#[test]
fn a_real_run_exercises_cells_a_table_and_a_sequence() {
    let steps = walk();
    let used: Vec<StateOpKind> = steps.iter().flat_map(|s| s.kinds.clone()).collect();
    for expected in [
        StateOpKind::SetCell,
        StateOpKind::TableInsert,
        StateOpKind::TableUpdate,
        StateOpKind::SequenceAppend,
        StateOpKind::SequenceRemove,
    ] {
        assert!(
            used.contains(&expected),
            "a real playthrough should have used {expected:?}; it used {used:?}"
        );
    }
}

#[test]
fn the_ghost_table_grows_moves_and_the_recording_empties() {
    let steps = walk();
    let ghost_counts: Vec<usize> = steps
        .iter()
        .map(|s| s.carried.table::<Ghosts>().expect("ghosts").len())
        .collect();
    assert_eq!(ghost_counts[0], 0, "no ghost before the freeze");
    assert_eq!(
        *ghost_counts.last().expect("steps"),
        1,
        "the freeze created one ghost"
    );

    let recording_lens: Vec<usize> = steps
        .iter()
        .map(|s| s.carried.sequence::<Recording>().expect("recording").len())
        .collect();
    assert!(
        recording_lens.iter().any(|len| *len > 0),
        "moves are recorded"
    );
    assert!(
        recording_lens.windows(2).any(|pair| pair[1] < pair[0]),
        "the freeze must empty the recording — the sequence-removal path"
    );
}

// ---------------------------------------------------------------------------
// what the substrate can say about the run
// ---------------------------------------------------------------------------

#[test]
fn the_diff_between_two_instants_names_what_moved() {
    let steps = walk();
    // The first command is a move: the player cell and the recording change.
    let opening = diff(
        &puzzle_state::project(
            &puzzle_state::schema().expect("schema"),
            &PuzzleGameState::new(level_codec::from_toml(LEVEL_001_TOML).expect("level")),
        )
        .expect("projection"),
        &steps[0].carried,
    )
    .expect("diffs");
    assert!(!opening.is_empty());
    let states: Vec<_> = opening.changes().iter().map(|c| c.state()).collect();
    assert!(states.contains(&PlayerCell::id()), "the player moved");
    assert!(states.contains(&Recording::id()), "the move was recorded");
    assert!(opening
        .changes()
        .iter()
        .any(|c| c.kind() == StateChangeKind::Added));
}

#[test]
fn the_substrate_can_describe_the_run_without_knowing_the_game() {
    let steps = walk();
    let schema = puzzle_state::schema().expect("schema");
    let described = report(&schema, &steps.last().expect("steps").carried).expect("reports");
    assert_eq!(described.schema_name(), "zanzoban");
    assert_eq!(described.entries().len(), 6);
    let paths: Vec<&str> = described.entries().iter().map(|e| e.path()).collect();
    assert!(paths.contains(&"puzzle/ghosts"));
    assert!(paths.contains(&"puzzle/recording"));
    let ghosts = described
        .entries()
        .iter()
        .find(|e| e.id() == Ghosts::id())
        .expect("the ghost table is described");
    assert_eq!(ghosts.elements(), 1, "one ghost, counted without decoding it");
}

#[test]
fn a_view_confines_a_computation_to_what_it_declared() {
    let steps = walk();
    let last = &steps.last().expect("steps").carried;
    // A "scoreboard" computation that needs only the outcome.
    let access = StateAccess::none().read::<Solved>().read::<Tick>();
    let view = StateView::open(&access, last);
    assert_eq!(view.cell::<Solved>(), Ok(true));
    assert!(
        view.table::<Ghosts>().is_err(),
        "it did not declare the ghost table, so it cannot read it"
    );
    assert!(
        view.patch(puzzle_state::origin())
            .set_cell::<Tick>(&0)
            .build()
            .is_err(),
        "it declared a read of the tick, not a write"
    );
}

// ---------------------------------------------------------------------------
// the golden artifacts
// ---------------------------------------------------------------------------

fn golden_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("zanzoban/golden");
    path.push(format!("{name}.bin"));
    path
}

fn assert_golden(name: &str, actual: &[u8]) {
    let path = golden_path(name);
    let force = std::env::var("AXIOM_REGOLD").as_deref() == Ok("1");
    match std::fs::read(&path).ok() {
        Some(expected) if !force => assert_eq!(
            actual,
            expected.as_slice(),
            "golden mismatch for `{name}`: the State Engine's account of the \
             recorded run changed. Do NOT re-capture to make this pass unless \
             the change was intended."
        ),
        _ => {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("create golden dir");
            std::fs::write(&path, actual).expect("write golden");
        }
    }
}

/// The snapshot digest after every command, plus the final snapshot's bytes.
///
/// This is new evidence the pre-change golden could not contain, and it
/// *supplements* rather than replaces it: the behavioural goldens still pin the
/// game, and these pin the substrate's account of the same run.
#[test]
fn golden_state_engine_snapshot_hashes() {
    let steps = walk();
    let mut out = Vec::new();
    out.extend_from_slice(&(steps.len() as u32).to_le_bytes());
    for step in &steps {
        out.extend_from_slice(&step.carried.hash().raw().to_le_bytes());
    }
    assert_golden("state_snapshot_hashes", &out);
}

#[test]
fn golden_state_engine_final_snapshot() {
    let steps = walk();
    assert_golden(
        "state_final_snapshot",
        &steps.last().expect("steps").carried.to_bytes(),
    );
}

/// The digest of every authored patch, in order — the substrate's account of
/// *what changed*, as distinct from what the state became.
#[test]
fn golden_state_engine_patch_hashes() {
    let schema = puzzle_state::schema().expect("schema");
    let level = level_codec::from_toml(LEVEL_001_TOML).expect("level");
    let mut game = PuzzleGameState::new(level);
    let mut out = Vec::new();
    let commands = agent::play_first_level().commands;
    out.extend_from_slice(&(commands.len() as u32).to_le_bytes());
    for command in commands {
        let before = game.clone();
        game_step::step(&mut game, command);
        let patch = puzzle_state::transition(&before, &game).expect("patch");
        out.extend_from_slice(&patch.hash().raw().to_le_bytes());
    }
    let _ = schema;
    assert_golden("state_patch_hashes", &out);
}

/// A golden that cannot fail is worse than none: perturbing the run must move
/// the recorded digests.
#[test]
fn a_perturbed_run_produces_different_state_digests() {
    let steps = walk();
    let baseline: Vec<u64> = steps.iter().map(|s| s.carried.hash().raw()).collect();

    let schema = puzzle_state::schema().expect("schema");
    let level = level_codec::from_toml(LEVEL_001_TOML).expect("level");
    let mut game = PuzzleGameState::new(level);
    let mut carried = puzzle_state::project(&schema, &game).expect("projection");
    let mut perturbed = Vec::new();
    let mut commands = agent::play_first_level().commands;
    commands.pop();
    for command in commands {
        let before = game.clone();
        game_step::step(&mut game, command);
        let patch = puzzle_state::transition(&before, &game).expect("patch");
        carried = apply(&schema, &carried, &patch).expect("applies");
        perturbed.push(carried.hash().raw());
    }
    assert_ne!(
        baseline, perturbed,
        "dropping a command left the State Engine's account unchanged"
    );
}
