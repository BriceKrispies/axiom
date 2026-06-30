//! The required deterministic-core behaviours, one test per numbered requirement
//! in the task. These drive the public game-core API directly (no browser), so
//! they prove the rules the engine actually runs on.

use std::collections::BTreeSet;

use axiom_gallery::roomed_puzzle::actor_state::ActorKind;
use axiom_gallery::roomed_puzzle::coord::GridCoord;
use axiom_gallery::roomed_puzzle::direction::Direction;
use axiom_gallery::roomed_puzzle::game_command::PuzzleCommand;
use axiom_gallery::roomed_puzzle::game_state::{PuzzleGameState, TICKS_PER_SECOND};
use axiom_gallery::roomed_puzzle::game_step::{run, step};
use axiom_gallery::roomed_puzzle::ghost_replay::GHOST_STEP_TICKS;
use axiom_gallery::roomed_puzzle::group_id::GroupId;
use axiom_gallery::roomed_puzzle::level_definition::{Button, Door, LevelDefinition};
use axiom_gallery::roomed_puzzle::level_validation::validate_level;
use axiom_gallery::roomed_puzzle::{level_codec, LEVEL_001_TOML};

// --- Test fixtures --------------------------------------------------------

/// entrance(0) · button(1) · floor(2) · door(3) · exit(4) — width 5, height 1.
/// The button and door are not adjacent, so the door cannot be solo-crossed.
fn corridor() -> LevelDefinition {
    LevelDefinition {
        title: "corridor".into(),
        width: 5,
        height: 1,
        entrance: GridCoord::new(0, 0),
        exit: GridCoord::new(4, 0),
        walls: vec![],
        buttons: vec![Button {
            position: GridCoord::new(1, 0),
            group: GroupId::new("main"),
        }],
        doors: vec![Door {
            position: GridCoord::new(3, 0),
            group: GroupId::new("main"),
        }],
    }
}

/// entrance(0) · wall(1) · exit(2) — width 3, height 1.
fn walled() -> LevelDefinition {
    LevelDefinition {
        title: "walled".into(),
        width: 3,
        height: 1,
        entrance: GridCoord::new(0, 0),
        exit: GridCoord::new(2, 0),
        walls: vec![GridCoord::new(1, 0)],
        buttons: vec![],
        doors: vec![],
    }
}

fn level_001() -> LevelDefinition {
    level_codec::from_toml(LEVEL_001_TOML).expect("level 001 parses")
}

const R: PuzzleCommand = PuzzleCommand::Move(Direction::Right);

fn main_group() -> GroupId {
    GroupId::new("main")
}

fn ticks(n: usize) -> Vec<PuzzleCommand> {
    vec![PuzzleCommand::Tick; n]
}

// --- 1. Player can move one square into floor ----------------------------

#[test]
fn req01_player_moves_one_square_into_floor() {
    let mut s = PuzzleGameState::new(corridor());
    step(&mut s, R); // 0 -> 1 (button, walkable)
    let into_floor = step(&mut s, R); // 1 -> 2 (floor)
    assert!(into_floor.player_moved());
    assert_eq!(s.player().position, GridCoord::new(2, 0));
}

// --- 2. Player cannot move outside the grid ------------------------------

#[test]
fn req02_player_cannot_move_outside_the_grid() {
    let mut s = PuzzleGameState::new(corridor());
    let off_edge = step(&mut s, PuzzleCommand::Move(Direction::Left)); // 0 -> -1
    assert!(off_edge.player_move_rejected());
    assert_eq!(s.player().position, GridCoord::new(0, 0));
}

// --- 3. Player cannot move into a wall -----------------------------------

#[test]
fn req03_player_cannot_move_into_a_wall() {
    let mut s = PuzzleGameState::new(walled());
    let into_wall = step(&mut s, R); // 0 -> 1 (wall)
    assert!(into_wall.player_move_rejected());
    assert_eq!(s.player().position, GridCoord::new(0, 0));
}

// --- 4. Player cannot move into a closed door ----------------------------

#[test]
fn req04_player_cannot_move_into_a_closed_door() {
    let mut s = PuzzleGameState::new(corridor());
    run(&mut s, &[R, R]); // 0 -> 1 (button) -> 2 (floor); door now closed
    let into_door = step(&mut s, R); // 2 -> 3 (closed door)
    assert!(into_door.player_move_rejected());
    assert_eq!(s.player().position, GridCoord::new(2, 0));
}

// --- 5. Standing on a button opens the matching door ---------------------

#[test]
fn req05_standing_on_button_opens_the_matching_door() {
    let mut s = PuzzleGameState::new(corridor());
    assert!(!s.is_group_open(&main_group()));
    step(&mut s, R); // onto the button
    assert!(s.is_group_open(&main_group()));
}

// --- 6. Moving off a button closes the matching door ---------------------

#[test]
fn req06_moving_off_button_closes_the_matching_door() {
    let mut s = PuzzleGameState::new(corridor());
    step(&mut s, R); // onto button -> open
    assert!(s.is_group_open(&main_group()));
    step(&mut s, R); // off button (onto floor) -> closed
    assert!(!s.is_group_open(&main_group()));
}

// --- 7. A ghost standing on a button opens the matching door -------------

#[test]
fn req07_ghost_on_button_opens_the_matching_door() {
    let mut s = PuzzleGameState::new(corridor());
    step(&mut s, R); // player onto button, record [Right]
    step(&mut s, PuzzleCommand::ResetLifeFromRecording); // ghost [Right]; player -> entrance
    run(&mut s, &ticks(GHOST_STEP_TICKS as usize)); // ghost steps once: 0 -> 1 (button)

    let ghost = s.ghost_states()[0];
    assert_eq!(ghost.kind, ActorKind::Ghost);
    assert_eq!(ghost.position, GridCoord::new(1, 0));
    // The player is back at the entrance, NOT on the button: the door is open
    // purely because the ghost holds the button.
    assert_eq!(s.player().position, GridCoord::new(0, 0));
    assert!(s.is_group_open(&main_group()));
}

// --- 8. A ghost finishing its replay remains in its final cell -----------

#[test]
fn req08_finished_ghost_stays_in_its_final_cell() {
    let mut s = PuzzleGameState::new(corridor());
    run(&mut s, &[R, R]); // record [Right, Right] -> would end a ghost at cell 2
    step(&mut s, PuzzleCommand::ResetLifeFromRecording);
    // Two full step windows finish the ghost at cell 2.
    run(&mut s, &ticks(2 * GHOST_STEP_TICKS as usize));
    assert_eq!(s.ghost_states()[0].position, GridCoord::new(2, 0));
    // Hundreds more ticks: it never moves again.
    run(&mut s, &ticks(500));
    assert_eq!(s.ghost_states()[0].position, GridCoord::new(2, 0));
}

// --- 9. q creates a ghost and resets the live player to the entrance ------

#[test]
fn req09_q_creates_ghost_and_resets_player() {
    let mut s = PuzzleGameState::new(corridor());
    run(&mut s, &[R, R]); // player at cell 2
    assert_eq!(s.ghost_count(), 0);
    step(&mut s, PuzzleCommand::ResetLifeFromRecording);
    assert_eq!(s.ghost_count(), 1);
    assert_eq!(s.player().position, s.entrance());
    assert_eq!(s.ghost_states()[0].position, s.entrance());
}

// --- 10. q clears the current-life recording after creating the ghost -----

#[test]
fn req10_q_clears_the_recording() {
    let mut s = PuzzleGameState::new(corridor());
    run(&mut s, &[R, R]);
    assert_eq!(s.recording_len(), 2);
    step(&mut s, PuzzleCommand::ResetLifeFromRecording);
    assert_eq!(s.recording_len(), 0);
    // The ghost still carries the two recorded moves (proven by it reaching
    // cell 2 after two windows).
    run(&mut s, &ticks(2 * GHOST_STEP_TICKS as usize));
    assert_eq!(s.ghost_states()[0].position, GridCoord::new(2, 0));
}

// --- 11. r clears all ghosts and resets the level fresh -------------------

#[test]
fn req11_r_restarts_fresh() {
    let mut s = PuzzleGameState::new(corridor());
    run(&mut s, &[R, PuzzleCommand::ResetLifeFromRecording, R]); // a ghost + a move
    run(&mut s, &ticks(45)); // advance the clock
    assert_eq!(s.ghost_count(), 1);
    assert!(s.current_tick() > 0);

    step(&mut s, PuzzleCommand::RestartLevelFresh);
    assert_eq!(s.ghost_count(), 0);
    assert_eq!(s.player().position, s.entrance());
    assert_eq!(s.recording_len(), 0);
    assert_eq!(s.current_tick(), 0, "the clock resets on a fresh restart");
}

// --- 12. Failed moves are not recorded -----------------------------------

#[test]
fn req12_failed_moves_are_not_recorded() {
    let mut s = PuzzleGameState::new(corridor());
    // An off-grid move and (after walking to the closed door) a blocked-door move.
    step(&mut s, PuzzleCommand::Move(Direction::Up)); // off grid (height 1)
    assert_eq!(s.recording_len(), 0);
    run(&mut s, &[R, R]); // to cell 2 (two real moves recorded)
    step(&mut s, R); // into the closed door: fails
    assert_eq!(
        s.recording_len(),
        2,
        "the blocked door move was not recorded"
    );
}

// --- 13. Ghosts replay one move per 0.5 s worth of fixed ticks ------------

#[test]
fn req13_ghost_moves_once_per_half_second_window() {
    // 0.5 s at the 60-tick/s fixed step is exactly the step window.
    assert_eq!(GHOST_STEP_TICKS, TICKS_PER_SECOND / 2);

    let mut s = PuzzleGameState::new(corridor());
    step(&mut s, R); // record [Right]
    step(&mut s, PuzzleCommand::ResetLifeFromRecording);
    // One tick short of a window: the ghost has not moved.
    run(&mut s, &ticks(GHOST_STEP_TICKS as usize - 1));
    assert_eq!(s.ghost_states()[0].position, GridCoord::new(0, 0));
    // The window-completing tick moves it exactly one cell.
    step(&mut s, PuzzleCommand::Tick);
    assert_eq!(s.ghost_states()[0].position, GridCoord::new(1, 0));
}

// --- 14. Two identical command streams produce identical state traces -----

#[test]
fn req14_identical_streams_produce_identical_traces() {
    let stream = {
        let mut v = vec![R, R, PuzzleCommand::ResetLifeFromRecording, R];
        v.extend(ticks(40)); // ghost steps while the player moves
        v.push(R);
        v.push(PuzzleCommand::RestartLevelFresh);
        v.extend([R, PuzzleCommand::ResetLifeFromRecording]);
        v.extend(ticks(35));
        v
    };

    let mut a = PuzzleGameState::new(corridor());
    let mut b = PuzzleGameState::new(corridor());
    let trace_a = run(&mut a, &stream);
    let trace_b = run(&mut b, &stream);

    assert_eq!(trace_a, trace_b, "per-step result traces must match");
    assert_eq!(a, b, "final states must be byte-identical");
}

// --- 15. Level 001 validates (and matches its intended geometry) ----------

#[test]
fn req15_level_001_validates() {
    let level = level_001();
    assert!(
        validate_level(&level).is_valid(),
        "{:?}",
        validate_level(&level).messages()
    );

    // The authored geometry is exactly what the design calls for.
    assert_eq!(level.title, "Button Door");
    assert_eq!((level.width, level.height), (10, 10));
    assert_eq!(level.entrance, GridCoord::new(1, 5));
    assert_eq!(level.exit, GridCoord::new(8, 5));
    assert_eq!(level.buttons.len(), 1);
    assert_eq!(level.buttons[0].position, GridCoord::new(4, 5));
    assert_eq!(level.buttons[0].group, main_group());
    assert_eq!(level.doors.len(), 1);
    assert_eq!(level.doors[0].position, GridCoord::new(7, 5));
    assert_eq!(level.doors[0].group, main_group());

    // Walls = boundary + the x=7 partition (gapped only at the door, y=5).
    let mut expected: BTreeSet<GridCoord> = BTreeSet::new();
    for x in 0..10 {
        expected.insert(GridCoord::new(x, 0));
        expected.insert(GridCoord::new(x, 9));
    }
    for y in 1..9 {
        expected.insert(GridCoord::new(0, y));
        expected.insert(GridCoord::new(9, y));
    }
    for y in [1, 2, 3, 4, 6, 7, 8] {
        expected.insert(GridCoord::new(7, y));
    }
    let actual: BTreeSet<GridCoord> = level.walls.iter().copied().collect();
    assert_eq!(
        actual, expected,
        "level 001 walls match the partitioned room"
    );
}

// --- 16. Level 001 is solvable via the ghost-on-button sequence -----------

#[test]
fn req16_level_001_is_solvable_with_a_ghost() {
    let mut s = PuzzleGameState::new(level_001());

    // Life 1: walk the player onto the button (1,5) -> (4,5).
    run(&mut s, &[R, R, R]);
    assert_eq!(s.player().position, GridCoord::new(4, 5));

    // q: snapshot that path into a ghost; the live player resets to (1,5).
    step(&mut s, PuzzleCommand::ResetLifeFromRecording);
    assert_eq!(s.ghost_count(), 1);

    // Life 2: walk to just left of the door (1,5) -> (6,5).
    run(&mut s, &[R, R, R, R, R]);
    assert_eq!(s.player().position, GridCoord::new(6, 5));
    // The door is still shut — the player alone cannot pass.
    assert!(!s.is_group_open(&main_group()));

    // Let the ghost replay its three moves onto the button.
    run(&mut s, &ticks(3 * GHOST_STEP_TICKS as usize));
    assert_eq!(s.ghost_states()[0].position, GridCoord::new(4, 5));
    assert!(
        s.is_group_open(&main_group()),
        "the ghost now holds the button open"
    );

    // Walk the live player through the open door to the exit.
    let through = step(&mut s, R); // (6,5) -> (7,5) door
    assert!(through.player_moved());
    let onto_exit = step(&mut s, R); // (7,5) -> (8,5) exit
    assert!(onto_exit.solved);
    assert!(s.is_solved());
    assert_eq!(s.player().position, GridCoord::new(8, 5));
}

#[test]
fn level_001_door_genuinely_blocks_a_lone_player() {
    // Extra proof the partition works: with no ghost, walking straight at the
    // exit gets stuck at the closed door — the level needs the ghost.
    let mut s = PuzzleGameState::new(level_001());
    let results = run(&mut s, &[R, R, R, R, R, R]); // (1,5) -> ... -> blocked at the door
    assert!(
        results[5].player_move_rejected(),
        "the 6th move hits the closed door"
    );
    assert_eq!(s.player().position, GridCoord::new(6, 5));
    assert!(!s.is_solved());
}

// --- 17. TOML export/import round-trips for level definitions -------------

#[test]
fn req17_toml_export_import_round_trips() {
    // A hand-built level round-trips...
    let level = corridor();
    let text = level_codec::to_toml(&level).expect("serializes");
    assert_eq!(level_codec::from_toml(&text).expect("parses"), level);

    // ...and so does the real Level 001 (parse -> serialize -> parse).
    let l1 = level_001();
    let reserialized = level_codec::to_toml(&l1).expect("serializes");
    assert_eq!(level_codec::from_toml(&reserialized).expect("parses"), l1);
}
