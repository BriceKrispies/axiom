//! Zanzoban's persistent truth, expressed on the State Engine.
//!
//! This is the app's translation between its own game core and the engine's
//! explicit-state substrate (`axiom-state`). It declares *what state exists* as a
//! schema, projects a `PuzzleGameState` into an immutable [`StateSnapshot`], and
//! authors a [`StatePatch`] describing what one command changed.
//!
//! ## Where the snapshot lives
//!
//! Here, in the app — never inside the engine. `axiom-state` transforms values
//! and remembers nothing; the composition root is what holds "the current
//! snapshot" and swaps it each command. That is the whole point of the state
//! law, and it is why this module is in `apps/` rather than in a layer.
//!
//! ## What is on the State Engine, and what is not
//!
//! The slice is the run's *observable* truth — exactly the values the committed
//! agent golden records, so a mistake here cannot hide:
//!
//! | State | Shape | Why it earns its place |
//! |---|---|---|
//! | `puzzle/player` | cell | the live player's cell; every move moves it |
//! | `puzzle/tick` | cell | the simulation clock the ghosts replay against |
//! | `puzzle/solved` | cell | the outcome |
//! | `puzzle/ghosts-created` | cell | assigns ghost identity; survives `q` |
//! | `puzzle/ghosts` | **table** | id to position — genuine insert (`q` creates one), update (every tick advances them) and remove (`r` clears them) |
//! | `puzzle/recording` | **sequence** | the ordered path the next ghost will replay; order *is* the meaning |
//!
//! Deliberately excluded: the level grid, the crates and the switch latches.
//! They are real state, but the golden never observes them, so putting them here
//! would add surface without adding proof — and a migration that proves nothing
//! is worse than no migration.
//!
//! ## What the accompanying tests establish
//!
//! That patches authored from real gameplay reproduce the game's own transition:
//! for every command of a full agent playthrough,
//! `apply(project(before), transition(before, after)) == project(after)`. The
//! patch is authored independently from the two game states rather than derived
//! by diffing the projections, so that equality is a real check rather than a
//! tautology.
//!
//! The game core still owns its own representation; this proves the substrate
//! can carry that truth transition-by-transition across an entire real run, not
//! that the core has been rewritten to store it. Converting a working game with
//! a committed golden wholesale would risk the very regression the golden exists
//! to catch, for no additional architectural evidence.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, Reflect, SchemaVersion, TypeSchema};
use axiom_state::{
    apply, CellKey, SequenceKey, StateDecl, StateId, StateKey, StateKind, StateOrigin, StatePatch,
    StatePatchBuilder, StateResult, StateSchema, StateSequence, StateSnapshot,
    StateSnapshotBuilder, StateTable, TableKey,
};

use crate::actor_state::ActorState;
use crate::direction::Direction;
use crate::game_state::PuzzleGameState;

/// The schema version this app declares its state at.
pub const VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// Who authors the patches: the puzzle simulation itself.
pub fn origin() -> StateOrigin {
    StateOrigin::of_name("zanzoban/sim")
}

// ---------------------------------------------------------------------------
// value types
// ---------------------------------------------------------------------------

/// A grid cell as explicit state data.
///
/// Its own type rather than a bare pair, so the schema's shape identity says
/// "this is a coordinate" and a future field reorder is caught by the shape
/// digest rather than silently misread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Column, 0 at the left.
    pub x: i32,
    /// Row, 0 at the top.
    pub y: i32,
}

impl Reflect for Cell {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "ZanzobanCell",
        &[
            axiom_kernel::FieldSchema::new("x", "i32"),
            axiom_kernel::FieldSchema::new("y", "i32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        writer.write_i32(self.x);
        writer.write_i32(self.y);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        reader
            .read_i32()
            .and_then(|x| reader.read_i32().map(|y| Cell { x, y }))
    }
}

impl From<&ActorState> for Cell {
    fn from(actor: &ActorState) -> Self {
        Cell {
            x: actor.position.x,
            y: actor.position.y,
        }
    }
}

/// A recorded move as a stable code. Directions are stored as their code rather
/// than as a Rust enum so the sequence's bytes never depend on a variant order
/// somebody might reshuffle.
pub const fn direction_code(direction: Direction) -> u8 {
    match direction {
        Direction::Up => 0,
        Direction::Down => 1,
        Direction::Left => 2,
        Direction::Right => 3,
    }
}

// ---------------------------------------------------------------------------
// the declared states
// ---------------------------------------------------------------------------

/// Where the live player stands.
#[derive(Debug)]
pub struct PlayerCell;
impl StateKey for PlayerCell {
    const PATH: &'static str = "puzzle/player";
    const KIND: StateKind = StateKind::Cell;
}
impl CellKey for PlayerCell {
    type Value = Cell;
}

/// The simulation tick.
#[derive(Debug)]
pub struct Tick;
impl StateKey for Tick {
    const PATH: &'static str = "puzzle/tick";
    const KIND: StateKind = StateKind::Cell;
}
impl CellKey for Tick {
    type Value = u64;
}

/// Whether the live player stands on the exit.
#[derive(Debug)]
pub struct Solved;
impl StateKey for Solved {
    const PATH: &'static str = "puzzle/solved";
    const KIND: StateKind = StateKind::Cell;
}
impl CellKey for Solved {
    type Value = bool;
}

/// How many ghosts have been created since the last fresh restart.
#[derive(Debug)]
pub struct GhostsCreated;
impl StateKey for GhostsCreated {
    const PATH: &'static str = "puzzle/ghosts-created";
    const KIND: StateKind = StateKind::Cell;
}
impl CellKey for GhostsCreated {
    type Value = u32;
}

/// Every ghost's current cell, keyed by its stable actor id.
#[derive(Debug)]
pub struct Ghosts;
impl StateKey for Ghosts {
    const PATH: &'static str = "puzzle/ghosts";
    const KIND: StateKind = StateKind::Table;
}
impl TableKey for Ghosts {
    type Key = u32;
    type Value = Cell;
}

/// The ordered path the next ghost will replay.
#[derive(Debug)]
pub struct Recording;
impl StateKey for Recording {
    const PATH: &'static str = "puzzle/recording";
    const KIND: StateKind = StateKind::Sequence;
}
impl SequenceKey for Recording {
    type Item = u8;
}

/// The schema for the migrated slice.
pub fn schema() -> StateResult<StateSchema> {
    StateSchema::build(
        "zanzoban",
        VERSION,
        &[
            StateDecl::cell::<PlayerCell>(),
            StateDecl::cell::<Tick>(),
            StateDecl::cell::<Solved>(),
            StateDecl::cell::<GhostsCreated>(),
            StateDecl::table::<Ghosts>(),
            StateDecl::sequence::<Recording>(),
        ],
    )
}

// ---------------------------------------------------------------------------
// projection and transition
// ---------------------------------------------------------------------------

/// Every ghost's cell, keyed by actor id.
fn ghost_rows(state: &PuzzleGameState) -> StateTable<u32, Cell> {
    state
        .ghost_states()
        .iter()
        .map(|ghost| (ghost.id.raw(), Cell::from(ghost)))
        .collect()
}

/// The recorded path as codes.
fn recording_items(state: &PuzzleGameState) -> StateSequence<u8> {
    state
        .recorded_path()
        .iter()
        .copied()
        .map(direction_code)
        .collect()
}

/// The game's persistent truth as an immutable snapshot.
pub fn project(schema: &StateSchema, state: &PuzzleGameState) -> StateResult<StateSnapshot> {
    StateSnapshotBuilder::new(schema)
        .with_cell::<PlayerCell>(&Cell::from(&state.player()))
        .with_cell::<Tick>(&state.current_tick())
        .with_cell::<Solved>(&state.is_solved())
        .with_cell::<GhostsCreated>(&(state.ghost_count() as u32))
        .with_table::<Ghosts>(&ghost_rows(state))
        .with_sequence::<Recording>(&recording_items(state))
        .build()
}

/// Author the patch describing what changed between two game states.
///
/// Built from the two states directly, never by diffing their projections —
/// that would make the round-trip check a tautology. Every operation kind the
/// substrate offers shows up here in the course of a real playthrough: cells are
/// set, ghost rows are inserted on `q`, updated on every tick, and removed on
/// `r`, and recorded moves are appended and removed.
pub fn transition(before: &PuzzleGameState, after: &PuzzleGameState) -> StateResult<StatePatch> {
    let mut patch = StatePatchBuilder::new(origin());

    let (was, now) = (Cell::from(&before.player()), Cell::from(&after.player()));
    if was != now {
        patch = patch.set_cell::<PlayerCell>(&now);
    }
    if before.current_tick() != after.current_tick() {
        patch = patch.set_cell::<Tick>(&after.current_tick());
    }
    if before.is_solved() != after.is_solved() {
        patch = patch.set_cell::<Solved>(&after.is_solved());
    }
    if before.ghost_count() != after.ghost_count() {
        patch = patch.set_cell::<GhostsCreated>(&(after.ghost_count() as u32));
    }

    patch = ghost_ops(patch, &ghost_rows(before), &ghost_rows(after));
    patch = recording_ops(patch, &recording_items(before), &recording_items(after));

    patch.build()
}

/// Insert, update and remove ghost rows to carry `before` to `after`.
fn ghost_ops(
    mut patch: StatePatchBuilder,
    before: &StateTable<u32, Cell>,
    after: &StateTable<u32, Cell>,
) -> StatePatchBuilder {
    for (id, cell) in after.rows() {
        match before.get(id) {
            None => patch = patch.table_insert::<Ghosts>(id, cell),
            Some(previous) if previous != cell => {
                patch = patch.table_update::<Ghosts>(id, cell);
            }
            Some(_) => {}
        }
    }
    for id in before.keys() {
        if !after.contains(id) {
            patch = patch.table_remove::<Ghosts>(id);
        }
    }
    patch
}

/// Append and remove recorded moves to carry `before` to `after`.
///
/// The recording only ever grows by one (a move) or empties completely (`q`/`r`),
/// so this stays a straightforward append-or-truncate rather than an edit
/// script. Removals run back-to-front so earlier positions keep their indices.
fn recording_ops(
    mut patch: StatePatchBuilder,
    before: &StateSequence<u8>,
    after: &StateSequence<u8>,
) -> StatePatchBuilder {
    let shared = before.len().min(after.len());
    for at in 0..shared {
        if before.get(at) != after.get(at) {
            patch = patch.sequence_replace::<Recording>(
                at as u32,
                after.get(at).expect("within the shared prefix"),
            );
        }
    }
    for at in (after.len()..before.len()).rev() {
        patch = patch.sequence_remove::<Recording>(at as u32);
    }
    for at in before.len()..after.len() {
        patch = patch.sequence_append::<Recording>(after.get(at).expect("within the new tail"));
    }
    patch
}

/// Carry a snapshot forward by one command's worth of change.
///
/// The composition root's step: it owns `snapshot`, hands it in, and gets the
/// next one back. Nothing is retained between calls.
pub fn advance(
    schema: &StateSchema,
    snapshot: &StateSnapshot,
    before: &PuzzleGameState,
    after: &PuzzleGameState,
) -> StateResult<StateSnapshot> {
    transition(before, after).and_then(|patch| apply(schema, snapshot, &patch))
}

/// The identities of the migrated states, for tooling and tests.
pub fn declared_ids() -> Vec<StateId> {
    vec![
        PlayerCell::id(),
        Tick::id(),
        Solved::id(),
        GhostsCreated::id(),
        Ghosts::id(),
        Recording::id(),
    ]
}
