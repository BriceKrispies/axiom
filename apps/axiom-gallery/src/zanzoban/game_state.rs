//! The deterministic, replayable puzzle state and its transitions.
//!
//! [`PuzzleGameState`] is pure simulation: it reads no wall clock and uses no
//! randomness. Time is the kernel's deterministic [`SimulationClock`], advanced
//! by exactly one [`FixedStep`] per `Tick` command. Given the same level and the
//! same ordered command stream, two states reach byte-identical results — that
//! is what makes ghost replay reproducible.
//!
//! ## Time
//!
//! The app runs a 60-tick/second fixed step ([`TICKS_PER_SECOND`]). A ghost
//! takes one recorded move every [`crate::zanzoban::ghost_replay::GHOST_STEP_TICKS`] ticks
//! (== 0.5 s). The clock is the kernel's, so the tick the simulation runs on is
//! the kernel's `Tick`, not an ad-hoc counter.
//!
//! ## Occupancy, buttons, and doors
//!
//! Both the live player and every ghost are *solid actors* occupying one cell.
//! A button's wiring group is *pressed* while any actor stands on any button of
//! that group; a door is *open* exactly while its group is pressed — re-evaluated
//! on demand, so a door closes the instant the last actor steps off its button.
//! A move into a wall, a closed door, an out-of-grid cell, or a cell another
//! actor occupies fails.
//!
//! ## Actor order
//!
//! On a tick, ghosts are resolved in creation order; each ghost sees the
//! up-to-date positions of the ghosts resolved before it. The live player never
//! moves on a tick (it moves only on its own `Move` command), so it is naturally
//! last in the stable order the task specifies.

use std::collections::BTreeSet;

use axiom_kernel::{FixedStep, ReplayTimeline, SimulationClock};

use crate::zanzoban::actor_state::{ActorId, ActorState};
use crate::zanzoban::coord::GridCoord;
use crate::zanzoban::direction::Direction;
use crate::zanzoban::game_command::{PuzzleStepResult, StepKind};
use crate::zanzoban::ghost_replay::GhostReplay;
use crate::zanzoban::group_id::GroupId;
use crate::zanzoban::level_definition::LevelDefinition;

/// The app's fixed simulation rate: 60 ticks per second.
pub const TICKS_PER_SECOND: u32 = 60;

/// The app's fixed step in integer nanoseconds (`1 / 60` s, rounded up). The
/// exact value only sets the kernel clock's elapsed-nanos bookkeeping; ghost
/// cadence is pure tick counting, so it is unaffected by the rounding.
pub const FIXED_STEP_NANOS: u64 = 16_666_667;

/// A static grid cell. Doors and buttons carry their wiring group; whether a
/// door is *open* is not stored here — it is derived from live occupancy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// Empty, walkable ground.
    Floor,
    /// Solid, impassable.
    Wall,
    /// The player/ghost start cell (walkable).
    Entrance,
    /// The goal cell (walkable).
    Exit,
    /// A pressure button of the given wiring group (walkable).
    Button(GroupId),
    /// A door of the given wiring group (walkable only while the group is open).
    Door(GroupId),
    /// A resonance well: walkable; refreshes a ghost's life to full (decay add-on).
    Well,
    /// A latching switch of the given wiring group: walkable; an actor entering it
    /// toggles the group's latch (switches add-on).
    Switch(GroupId),
    /// A lethal hazard: walkable, but the live player entering it dies (hazards add-on).
    Hazard,
}

impl Cell {
    /// Is this cell ever a button of `group`?
    fn button_group(&self) -> Option<&GroupId> {
        match self {
            Cell::Button(g) => Some(g),
            _ => None,
        }
    }
}

/// A ghost: its positional state, its replay cursor, and its remaining life.
///
/// `life` is `None` when the decay add-on is off (the ghost is immortal, exactly
/// as before decay existed); `Some(n)` means `n` more of its own steps before it
/// fades. Landing on a [`Cell::Well`] refreshes it to the rule's `lifetime_steps`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GhostActor {
    actor: ActorState,
    replay: GhostReplay,
    life: Option<u32>,
}

/// The full deterministic puzzle state.
///
/// `PartialEq`/`Eq` compare the entire state (grid, actors, recording, and the
/// kernel clock), so two states driven by the same command stream can be
/// asserted byte-for-byte identical — the determinism guarantee, made testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleGameState {
    level: LevelDefinition,
    width: u32,
    height: u32,
    entrance: GridCoord,
    exit: GridCoord,
    /// Row-major `width * height` static cells.
    cells: Vec<Cell>,
    player: ActorState,
    /// Ghosts in creation order.
    ghosts: Vec<GhostActor>,
    recording: ReplayTimeline<Direction>,
    clock: SimulationClock,
    /// Total ghosts created since the last fresh restart (assigns ghost ids).
    ghosts_created: u32,
    /// Wiring groups currently held open by a switch latch (switches add-on).
    /// Persists across `q`; cleared only by `r`.
    latched: BTreeSet<GroupId>,
    /// Live pushable-crate positions (crates add-on). Persists across `q`;
    /// re-cloned from the level on `r`.
    crates: Vec<GridCoord>,
}

impl PuzzleGameState {
    /// Build the initial state for a level. The level should already be valid
    /// (the editor gates playtest on validation); an invalid level still
    /// produces a state, with later object placements overwriting earlier cells.
    pub fn new(level: LevelDefinition) -> Self {
        let width = level.width;
        let height = level.height;
        let entrance = level.entrance;
        let exit = level.exit;
        let cells = build_cells(&level);
        let crates = level.crates.clone();
        let clock = SimulationClock::new(
            FixedStep::new(FIXED_STEP_NANOS).expect("fixed step nanos is non-zero"),
        );
        PuzzleGameState {
            level,
            width,
            height,
            entrance,
            exit,
            cells,
            player: ActorState::player(entrance),
            ghosts: Vec::new(),
            recording: ReplayTimeline::new(),
            clock,
            ghosts_created: 0,
            latched: BTreeSet::new(),
            crates,
        }
    }

    /// The level being played.
    pub fn level(&self) -> &LevelDefinition {
        &self.level
    }

    /// Grid width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Grid height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The entrance cell.
    pub fn entrance(&self) -> GridCoord {
        self.entrance
    }

    /// The exit cell.
    pub fn exit(&self) -> GridCoord {
        self.exit
    }

    /// The live player's state.
    pub fn player(&self) -> ActorState {
        self.player
    }

    /// Every ghost's positional state, in creation order.
    pub fn ghost_states(&self) -> Vec<ActorState> {
        self.ghosts.iter().map(|g| g.actor).collect()
    }

    /// Every ghost's positional state paired with its remaining life (`None` when
    /// decay is off), in creation order — the input to the ghost-fade render cue.
    pub fn ghost_lives(&self) -> Vec<(ActorState, Option<u32>)> {
        self.ghosts.iter().map(|g| (g.actor, g.life)).collect()
    }

    /// How many ghosts currently exist.
    pub fn ghost_count(&self) -> usize {
        self.ghosts.len()
    }

    /// The number of moves recorded in the current life.
    pub fn recording_len(&self) -> usize {
        self.recording.len()
    }

    /// The current simulation tick (kernel time).
    pub fn current_tick(&self) -> u64 {
        self.clock.tick().raw()
    }

    /// Does the live player stand on the exit?
    pub fn is_solved(&self) -> bool {
        self.player.position == self.exit
    }

    /// The static cell at `coord`, if in bounds.
    pub fn cell_at(&self, coord: GridCoord) -> Option<&Cell> {
        self.index(coord).map(|i| &self.cells[i])
    }

    /// The set of wiring groups currently pressed by some actor. A door is open
    /// exactly when its group is in this set.
    pub fn pressed_groups(&self) -> BTreeSet<GroupId> {
        self.actors()
            .filter_map(|actor| {
                self.cell_at(actor.position)
                    .and_then(Cell::button_group)
                    .cloned()
            })
            .collect()
    }

    /// Is the door of `group` currently open? A group opens while any actor holds
    /// one of its buttons **or** the group is held by a switch latch.
    pub fn is_group_open(&self, group: &GroupId) -> bool {
        self.pressed_groups().contains(group) || self.latched.contains(group)
    }

    /// Is `group` currently held open by a switch latch (switches add-on)?
    pub fn is_latched(&self, group: &GroupId) -> bool {
        self.latched.contains(group)
    }

    /// The live pushable-crate positions (crates add-on).
    pub fn crates(&self) -> &[GridCoord] {
        &self.crates
    }

    /// Apply a live-player move. On success the player moves (pushing a crate if
    /// one is in the way and the cell beyond is free) and the move is recorded; on
    /// failure nothing changes and nothing is recorded. Stepping onto a hazard is a
    /// successful move that then kills the life; stepping onto a switch toggles it.
    pub fn apply_player_move(&mut self, direction: Direction) -> PuzzleStepResult {
        let target = self.player.position.stepped(direction);
        if !self.resolve_move(ActorId::PLAYER, target, direction) {
            return PuzzleStepResult::new(StepKind::PlayerMoveRejected(direction), self.is_solved());
        }
        self.player.position = target;
        self.recording.record(direction);
        // Hazard death: a hazard cell is never also a switch (cells are exclusive),
        // so short-circuit here — reset the life with no ghost.
        if self.level.rules.hazards && matches!(self.cell_at(target), Some(Cell::Hazard)) {
            self.player.position = self.entrance;
            self.recording.clear();
            return PuzzleStepResult::new(StepKind::PlayerDied, self.is_solved());
        }
        self.enter_cell_effects(target);
        PuzzleStepResult::new(StepKind::PlayerMoved(direction), self.is_solved())
    }

    /// End the current life (`q`): create a ghost from the recording, reset the
    /// player to the entrance, clear the recording. Existing ghosts, latches,
    /// crates, and the clock are untouched. Refused with no effect when the ghost
    /// budget is full (budget add-on).
    pub fn reset_life_from_recording(&mut self) -> PuzzleStepResult {
        if let Some(budget) = self.level.rules.budget {
            if self.ghosts.len() as u32 >= budget.max_ghosts {
                return PuzzleStepResult::new(
                    StepKind::LifeRejectedBudgetFull,
                    self.is_solved(),
                );
            }
        }
        let replay = GhostReplay::new(self.recording.recorded().to_vec());
        let ghost = GhostActor {
            actor: ActorState::ghost(self.ghosts_created, self.entrance),
            replay,
            life: self.level.rules.decay.map(|d| d.lifetime_steps),
        };
        self.ghosts.push(ghost);
        self.ghosts_created += 1;
        self.player.position = self.entrance;
        self.recording.clear();
        PuzzleStepResult::new(StepKind::LifeReset, self.is_solved())
    }

    /// Restart fresh (`r`): reset the player, clear all ghosts, clear the
    /// recording, drop every switch latch, restore crates to their authored
    /// positions, and reset the clock to zero.
    pub fn restart_fresh(&mut self) -> PuzzleStepResult {
        self.player = ActorState::player(self.entrance);
        self.ghosts.clear();
        self.recording.clear();
        self.ghosts_created = 0;
        self.latched.clear();
        self.crates = self.level.crates.clone();
        self.clock = SimulationClock::new(
            FixedStep::new(FIXED_STEP_NANOS).expect("fixed step nanos is non-zero"),
        );
        PuzzleStepResult::new(StepKind::LevelRestarted, self.is_solved())
    }

    /// Advance one fixed tick: step the clock, then advance each ghost's replay in
    /// creation order, moving the ones whose move is due and unobstructed (pushing
    /// crates and toggling switches on the way), ageing them under the decay rule,
    /// and reaping any that have fully faded.
    pub fn tick(&mut self) -> PuzzleStepResult {
        // The clock cannot overflow in any realistic session (u64 nanoseconds is
        // ~580 years at 60 Hz); advancing is total either way.
        let _ = self.clock.advance();
        let decay = self.level.rules.decay;

        let mut ghosts_stepped = 0u32;
        for i in 0..self.ghosts.len() {
            // Take this tick's due move first; the borrow ends immediately.
            let due = self.ghosts[i].replay.advance_tick();
            if let Some(direction) = due {
                let id = self.ghosts[i].actor.id;
                let target = self.ghosts[i].actor.position.stepped(direction);
                // `resolve_move` reads current occupancy (incl. earlier ghosts that
                // already moved this tick) and may push a crate; the position write
                // and switch toggle are separate statements.
                let moved = self.resolve_move(id, target, direction);
                if moved {
                    self.ghosts[i].actor.position = target;
                    ghosts_stepped += 1;
                    self.enter_cell_effects(target);
                }
                // A due (consumed) step ages the afterimage; landing on a well
                // refreshes it (decrement first, then refresh, so a ghost at life 1
                // that reaches a well survives).
                if let Some(rule) = decay {
                    // Resolve the well check before taking the mutable ghost borrow.
                    let on_well = moved && matches!(self.cell_at(target), Some(Cell::Well));
                    if let Some(life) = self.ghosts[i].life.as_mut() {
                        *life = life.saturating_sub(1);
                        on_well.then(|| *life = rule.lifetime_steps);
                    }
                }
            }
        }

        // Reap faded ghosts AFTER the loop (removing mid-loop would invalidate
        // indices). A removed ghost releases anything it held, because door-open is
        // derived from live actor occupancy.
        let before = self.ghosts.len();
        (decay.is_some()).then(|| self.ghosts.retain(|g| g.life != Some(0)));
        let ghosts_faded = (before - self.ghosts.len()) as u32;

        PuzzleStepResult::new(
            StepKind::Ticked {
                ghosts_stepped,
                ghosts_faded,
            },
            self.is_solved(),
        )
    }

    /// All solid actors (player + ghosts), in stable order (ghosts, then player).
    fn actors(&self) -> impl Iterator<Item = ActorState> + '_ {
        self.ghosts
            .iter()
            .map(|g| g.actor)
            .chain(std::iter::once(self.player))
    }

    /// Row-major index of `coord`, if in bounds.
    fn index(&self, coord: GridCoord) -> Option<usize> {
        coord
            .in_bounds(self.width, self.height)
            .then(|| coord.y as usize * self.width as usize + coord.x as usize)
    }

    /// Is any actor other than `mover` standing on `coord`?
    fn is_occupied(&self, coord: GridCoord, mover: ActorId) -> bool {
        self.actors().any(|a| a.id != mover && a.position == coord)
    }

    /// Is `coord` passable terrain (ignoring actors and crates)? False out of grid,
    /// into a wall, or into a closed door; wells/switches/hazards are all walkable.
    fn is_passable_terrain(&self, coord: GridCoord) -> bool {
        match self.cell_at(coord) {
            None | Some(Cell::Wall) => false,
            Some(Cell::Door(group)) => self.is_group_open(group),
            Some(_) => true,
        }
    }

    /// The index of a crate at `coord`, if any (crates add-on).
    fn crate_index_at(&self, coord: GridCoord) -> Option<usize> {
        self.crates.iter().position(|&c| c == coord)
    }

    /// Try to move `mover` into `target` coming from `dir`. Pushes a crate one cell
    /// if one is in the way and the cell beyond is passable and free. Returns
    /// whether the actor may occupy `target`. With no crates present this reduces
    /// exactly to the old occupancy+terrain test (the base game is unchanged).
    fn resolve_move(&mut self, mover: ActorId, target: GridCoord, dir: Direction) -> bool {
        if !self.is_passable_terrain(target) || self.is_occupied(target, mover) {
            return false;
        }
        if let Some(ci) = self.crate_index_at(target) {
            let beyond = target.stepped(dir);
            let pushable = self.is_passable_terrain(beyond)
                && !self.is_occupied(beyond, mover)
                && self.crate_index_at(beyond).is_none();
            if !pushable {
                return false;
            }
            self.crates[ci] = beyond;
        }
        true
    }

    /// Edge-triggered effects of an actor entering `coord`: toggle a switch's group
    /// latch (switches add-on). Called only after a successful entry.
    fn enter_cell_effects(&mut self, coord: GridCoord) {
        if self.level.rules.switches {
            if let Some(Cell::Switch(group)) = self.cell_at(coord) {
                let group = group.clone();
                (!self.latched.remove(&group)).then(|| self.latched.insert(group));
            }
        }
    }
}

/// Build the static cell grid from a level. Floor everywhere, then stamp walls,
/// buttons, doors, the entrance, and the exit. A valid level has no overlaps; if
/// a hand-edited level does, the later stamp wins (entrance/exit last so the
/// start/goal stay visible). Out-of-grid placements are skipped (validation
/// reports them separately).
fn build_cells(level: &LevelDefinition) -> Vec<Cell> {
    let w = level.width as usize;
    let h = level.height as usize;
    let mut cells = vec![Cell::Floor; w * h];
    let mut stamp = |coord: GridCoord, cell: Cell| {
        if coord.in_bounds(level.width, level.height) {
            cells[coord.y as usize * w + coord.x as usize] = cell;
        }
    };
    level.walls.iter().for_each(|&c| stamp(c, Cell::Wall));
    level
        .buttons
        .iter()
        .for_each(|b| stamp(b.position, Cell::Button(b.group.clone())));
    level
        .doors
        .iter()
        .for_each(|d| stamp(d.position, Cell::Door(d.group.clone())));
    level.wells.iter().for_each(|&c| stamp(c, Cell::Well));
    level
        .switches
        .iter()
        .for_each(|s| stamp(s.position, Cell::Switch(s.group.clone())));
    level.hazards.iter().for_each(|&c| stamp(c, Cell::Hazard));
    stamp(level.entrance, Cell::Entrance);
    stamp(level.exit, Cell::Exit);
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zanzoban::ghost_replay::GHOST_STEP_TICKS;
    use crate::zanzoban::level_definition::{Button, Door};

    /// A tiny 5×1 corridor: entrance(0) · button(1) · floor(2) · door(3) · exit(4).
    /// (Height 1 keeps the geometry trivial for unit tests.)
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
            wells: Vec::new(),
            switches: Vec::new(),
            crates: Vec::new(),
            hazards: Vec::new(),
            rules: Default::default(),
        }
    }

    #[test]
    fn fixed_step_matches_half_second_ghost_cadence() {
        // 0.5 s at 60 ticks/s is exactly the ghost step window.
        assert_eq!(GHOST_STEP_TICKS, TICKS_PER_SECOND / 2);
    }

    #[test]
    fn player_moves_onto_floor() {
        let mut s = PuzzleGameState::new(corridor());
        let r = s.apply_player_move(Direction::Right);
        assert!(r.player_moved());
        assert_eq!(s.player().position, GridCoord::new(1, 0));
    }

    #[test]
    fn standing_on_button_opens_the_matching_door() {
        let mut s = PuzzleGameState::new(corridor());
        assert!(!s.is_group_open(&GroupId::new("main")));
        s.apply_player_move(Direction::Right);
        assert!(s.is_group_open(&GroupId::new("main")));
    }

    #[test]
    fn closed_door_blocks_but_open_door_passes() {
        let mut s = PuzzleGameState::new(corridor());
        // Walk to cell 2 (floor just before the door). The player steps on the
        // button at cell 1 en route, but leaves it, so at cell 2 the door is shut.
        s.apply_player_move(Direction::Right);
        s.apply_player_move(Direction::Right);
        let blocked = s.apply_player_move(Direction::Right);
        assert!(blocked.player_move_rejected());
        assert_eq!(s.player().position, GridCoord::new(2, 0));
    }
}
