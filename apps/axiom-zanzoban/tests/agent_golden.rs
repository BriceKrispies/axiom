//! The agent's playthrough of Zanzoban level 1, captured as committed goldens.
//!
//! This is the **pre-change baseline** for the State Engine work: a run of the
//! real `axiom-agent` driver (`observe → decide → emit`, producing a real
//! `DecisionReport` per move) recorded here so that a later engine change can be
//! held to *exactly this behaviour*.
//!
//! Three artifacts are captured separately rather than fused into one blob, so a
//! future mismatch localizes to a stage instead of just saying "the bytes moved":
//!
//! * `agent_transcript.bin` — every command the driver applied to the game core,
//!   in order. **This is the recorded run.** The regression test replays *this*
//!   against a fresh core rather than asking the agent to improvise a new run,
//!   which is what makes the comparison a regression test and not a re-run.
//! * `agent_trajectory.bin` — the state after each command: the player, every
//!   ghost, the recording length, the tick, and the solved flag. This is the
//!   deterministic consequence of the transcript.
//! * `agent_outcome.bin` — the run's result: solved, move count, ghost count,
//!   final tick, and the milestone events.
//!
//! Every captured value is integer/enum data — there is not one `f32` in here —
//! so the bytes are platform-stable and exact equality is the right bar. The
//! puzzle core has no RNG: ghost replay is driven by a recorded path and time is
//! whole `Tick`s, so there is no seed to pin. Determinism comes from the level
//! plus the transcript, and nothing else.
//!
//! Run it (the driver is behind the native-only `agent` feature, so a plain
//! `cargo test --workspace` does not compile it):
//!
//! ```sh
//! cargo test -p axiom-zanzoban --features agent --test agent_golden
//! ```
//!
//! To re-capture after an *intended* change: `AXIOM_REGOLD=1 cargo test -p
//! axiom-zanzoban --features agent --test agent_golden`, then review the diff.

#![cfg(feature = "agent")]

use std::path::PathBuf;

use axiom_zanzoban::actor_state::{ActorKind, ActorState};
use axiom_zanzoban::agent;
use axiom_zanzoban::game_command::PuzzleCommand;
use axiom_zanzoban::{game_step, level_codec, Direction, PuzzleGameState, LEVEL_001_TOML};

// ---------------------------------------------------------------------------
// golden plumbing
// ---------------------------------------------------------------------------

fn golden_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("zanzoban/golden");
    path.push(format!("{name}.bin"));
    path
}

/// Compare against the committed golden, capturing it when absent.
///
/// The re-capture escape is compared against `"1"`, not merely tested for
/// presence: `AXIOM_REGOLD=0` silently reading as "yes, re-bless everything" is
/// exactly the footgun that destroys a baseline without anyone noticing.
fn assert_golden(name: &str, actual: &[u8]) {
    let path = golden_path(name);
    let force = std::env::var("AXIOM_REGOLD").as_deref() == Ok("1");
    match std::fs::read(&path).ok() {
        Some(expected) if !force => assert_eq!(
            actual,
            expected.as_slice(),
            "golden mismatch for `{name}` ({} actual vs {} expected bytes): the \
             agent's recorded run no longer reproduces. This is a behavioural \
             regression unless the change was intended — do NOT re-capture to \
             make it disappear. If it truly was intended, re-run with \
             AXIOM_REGOLD=1 and review the diff.",
            actual.len(),
            expected.len(),
        ),
        _ => {
            std::fs::create_dir_all(path.parent().expect("golden dir has a parent"))
                .expect("create golden dir");
            std::fs::write(&path, actual).expect("write golden");
        }
    }
}

// ---------------------------------------------------------------------------
// canonical encodings — little-endian, integer only
// ---------------------------------------------------------------------------

fn direction_code(direction: Direction) -> u8 {
    match direction {
        Direction::Up => 0,
        Direction::Down => 1,
        Direction::Left => 2,
        Direction::Right => 3,
    }
}

fn direction_of_code(code: u8) -> Direction {
    [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ][usize::from(code)]
}

/// One command as two bytes: `[kind, payload]`. `Move` carries its direction;
/// the other three commands carry a zero.
fn encode_command(out: &mut Vec<u8>, command: PuzzleCommand) {
    match command {
        PuzzleCommand::Move(direction) => {
            out.push(0);
            out.push(direction_code(direction));
        }
        PuzzleCommand::ResetLifeFromRecording => {
            out.push(1);
            out.push(0);
        }
        PuzzleCommand::RestartLevelFresh => {
            out.push(2);
            out.push(0);
        }
        PuzzleCommand::Tick => {
            out.push(3);
            out.push(0);
        }
    }
}

fn decode_commands(bytes: &[u8]) -> Vec<PuzzleCommand> {
    bytes
        .chunks_exact(2)
        .map(|pair| match pair[0] {
            0 => PuzzleCommand::Move(direction_of_code(pair[1])),
            1 => PuzzleCommand::ResetLifeFromRecording,
            2 => PuzzleCommand::RestartLevelFresh,
            _ => PuzzleCommand::Tick,
        })
        .collect()
}

fn encode_actor(out: &mut Vec<u8>, actor: &ActorState) {
    out.extend_from_slice(&actor.id.raw().to_le_bytes());
    out.push(match actor.kind {
        ActorKind::Player => 0,
        ActorKind::Ghost => 1,
    });
    out.extend_from_slice(&actor.position.x.to_le_bytes());
    out.extend_from_slice(&actor.position.y.to_le_bytes());
}

/// The state after one command: the player, every ghost, the recording length,
/// the tick, and the solved flag.
fn encode_step(out: &mut Vec<u8>, state: &PuzzleGameState) {
    encode_actor(out, &state.player());
    let ghosts = state.ghost_states();
    out.extend_from_slice(&(ghosts.len() as u32).to_le_bytes());
    ghosts.iter().for_each(|ghost| encode_actor(out, ghost));
    out.extend_from_slice(&(state.recording_len() as u32).to_le_bytes());
    out.extend_from_slice(&state.current_tick().to_le_bytes());
    out.push(u8::from(state.is_solved()));
}

// ---------------------------------------------------------------------------
// capture + replay
// ---------------------------------------------------------------------------

fn transcript_bytes(commands: &[PuzzleCommand]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(commands.len() as u32).to_le_bytes());
    commands
        .iter()
        .for_each(|command| encode_command(&mut out, *command));
    out
}

/// Replay a recorded transcript against a fresh core and capture the trajectory.
///
/// The agent is deliberately absent here: this is the *replay* half, and it must
/// depend on nothing but the level and the recorded commands.
fn replay(commands: &[PuzzleCommand]) -> Vec<u8> {
    let level = level_codec::from_toml(LEVEL_001_TOML).expect("embedded level parses");
    let mut state = PuzzleGameState::new(level);
    let mut out = Vec::new();
    commands.iter().for_each(|command| {
        game_step::step(&mut state, *command);
        encode_step(&mut out, &state);
    });
    out
}

fn outcome_bytes(run: &agent::Playthrough) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(u8::from(run.solved));
    out.extend_from_slice(&(run.moves.len() as u32).to_le_bytes());
    run.moves
        .iter()
        .for_each(|direction| out.push(direction_code(*direction)));
    out.extend_from_slice(&(run.ghosts as u32).to_le_bytes());
    out.extend_from_slice(&run.ticks.to_le_bytes());
    out.extend_from_slice(&(run.events.len() as u32).to_le_bytes());
    run.events.iter().for_each(|event| {
        out.extend_from_slice(&(event.len() as u32).to_le_bytes());
        out.extend_from_slice(event.as_bytes());
    });
    out
}

// ---------------------------------------------------------------------------
// the goldens
// ---------------------------------------------------------------------------

/// Write the transcript golden if it does not exist yet, so a test that reads it
/// never depends on a sibling test having run first.
fn ensure_transcript_golden() {
    golden_path("agent_transcript").exists().then_some(()).map_or_else(
        || {
            let run = agent::play_first_level();
            assert_golden("agent_transcript", &transcript_bytes(&run.commands));
        },
        |()| (),
    );
}

#[test]
fn golden_agent_transcript() {
    let run = agent::play_first_level();
    assert!(
        run.solved,
        "the baseline must be a WINNING run — a golden of a failed run pins \
         nothing interesting; events={:?}",
        run.events
    );
    assert_golden("agent_transcript", &transcript_bytes(&run.commands));
}

#[test]
fn golden_agent_trajectory() {
    let run = agent::play_first_level();
    assert_golden("agent_trajectory", &replay(&run.commands));
}

#[test]
fn golden_agent_outcome() {
    let run = agent::play_first_level();
    assert_golden("agent_outcome", &outcome_bytes(&run));
}

// ---------------------------------------------------------------------------
// the properties the goldens rest on
// ---------------------------------------------------------------------------

/// The capture is only trustworthy if the run repeats. Two independent agent
/// runs must produce byte-identical transcripts, trajectories and outcomes.
#[test]
fn the_agent_run_is_repeatable() {
    let first = agent::play_first_level();
    let second = agent::play_first_level();
    assert_eq!(
        transcript_bytes(&first.commands),
        transcript_bytes(&second.commands),
        "two agent runs of the same level produced different transcripts"
    );
    assert_eq!(
        replay(&first.commands),
        replay(&second.commands),
        "two agent runs produced different trajectories"
    );
    assert_eq!(
        outcome_bytes(&first),
        outcome_bytes(&second),
        "two agent runs produced different outcomes"
    );
}

/// **The regression test.** Replaying the *committed* transcript — not a fresh
/// agent run — must reproduce the committed trajectory. This is what a later
/// engine change is held to.
#[test]
fn replaying_the_committed_transcript_reproduces_the_trajectory() {
    // Capture-if-absent rather than depending on another test having run first:
    // tests share no order, so reading a file a sibling test writes is a race.
    // Once the golden is committed this is a no-op read.
    ensure_transcript_golden();
    let recorded =
        std::fs::read(golden_path("agent_transcript")).expect("transcript golden is present");
    let commands = decode_commands(&recorded[4..]);
    assert_eq!(
        (commands.len() as u32).to_le_bytes().to_vec(),
        recorded[..4].to_vec(),
        "transcript length header disagrees with its body"
    );
    assert_golden("agent_trajectory", &replay(&commands));
}

/// A golden that cannot fail is worse than no golden. Perturb the transcript in
/// a way the trajectory must notice, and prove the bytes move.
///
/// The perturbation drops the **last** command rather than editing an early one:
/// an early edit can be swallowed by the core (a rejected move changes nothing),
/// whereas the trajectory records one step per command, so removing a command
/// necessarily shortens it.
#[test]
fn a_perturbed_transcript_produces_different_bytes() {
    let run = agent::play_first_level();
    let baseline = replay(&run.commands);
    let mut shortened = run.commands.clone();
    shortened.pop().expect("the run issued at least one command");
    let perturbed = replay(&shortened);
    assert_ne!(
        baseline, perturbed,
        "dropping a command left the trajectory unchanged — this golden would \
         not catch a regression"
    );
}

/// The transcript must exercise the interesting path: a ghost is created and
/// replayed, not just a straight walk to the exit.
#[test]
fn the_transcript_exercises_ghost_replay() {
    let run = agent::play_first_level();
    assert!(
        run.ghosts >= 1,
        "the recorded run must create at least one ghost"
    );
    assert!(
        run.commands
            .iter()
            .any(|c| matches!(c, PuzzleCommand::ResetLifeFromRecording)),
        "the transcript must contain the freeze that creates the ghost"
    );
    assert!(
        run.commands
            .iter()
            .filter(|c| matches!(c, PuzzleCommand::Tick))
            .count()
            > 1,
        "the transcript must contain the ticks that drive ghost replay"
    );
}
