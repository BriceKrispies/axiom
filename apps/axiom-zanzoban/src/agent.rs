//! Playing Zanzoban through the reusable `axiom-agent` module.
//!
//! Native-only and gated behind the `agent` feature, so the wasm build
//! and the default workspace gates never compile it — the same shape the retro FPS and
//! growth agent drivers use.
//!
//! There is **no hand-rolled decision *emission*** here: the app plans a route
//! (a BFS over the grid — the "where to go next"), but every actual move is run
//! through `axiom-agent`'s `observe → decide → emit` cycle
//! ([`AgentApi::step`], producing a real `DecisionReport`) and then *lowered*
//! back into the grid [`Direction`] the game consumes — exactly the
//! control-code-bitmask pattern the retro FPS agent uses. Per the Module Law the
//! module never learns a game noun; the app owns both ends of the translation.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;

use crate::coord::GridCoord;
use crate::direction::Direction;
use crate::game_command::PuzzleCommand;
use crate::game_state::{Cell, PuzzleGameState};
use crate::group_id::GroupId;
use crate::level_definition::LevelDefinition;
use crate::{game_step, level_codec, LEVEL_001_TOML};

/// The app's grid control bitmask: the meaning the app assigns to a neutral
/// `press_control` code. `axiom-agent` carries the `u32` opaquely; this is the
/// app-side convention that packs one cardinal grid move into it.
const CONTROL_UP: u32 = 1 << 0;
const CONTROL_DOWN: u32 = 1 << 1;
const CONTROL_LEFT: u32 = 1 << 2;
const CONTROL_RIGHT: u32 = 1 << 3;

/// A neutral observation-fact kind (the app's own vocabulary). The brain ignores
/// observation content, but the app builds a real observation each move so the
/// `observe → decide` half of the cycle is genuinely exercised.
const FACT_PLAYER_CELL: u16 = 1;
const FACT_GOAL_CELL: u16 = 2;

/// The stable agent id this single-agent session uses.
const AGENT_RAW_ID: u64 = 1;

/// The engine's fixed 60 Hz step delta in integer nanoseconds. Stamps the
/// `RuntimeStep` that drives one decision; it does not affect the puzzle sim
/// (which advances by whole `Tick`s).
const FIXED_DELTA_NANOS: u64 = 16_666_667;

/// BFS neighbour expansion order — fixed, so planning is deterministic.
const DIRS: [Direction; 4] = [
    Direction::Up,
    Direction::Down,
    Direction::Left,
    Direction::Right,
];

/// Encode a grid move as the neutral control code.
fn control_of(direction: Direction) -> u32 {
    match direction {
        Direction::Up => CONTROL_UP,
        Direction::Down => CONTROL_DOWN,
        Direction::Left => CONTROL_LEFT,
        Direction::Right => CONTROL_RIGHT,
    }
}

/// Lower a neutral control-code bitmask back into the grid [`Direction`] the game
/// consumes — the inverse of [`control_of`]. `None` for an empty code.
fn direction_of_control(code: u32) -> Option<Direction> {
    if code & CONTROL_UP != 0 {
        Some(Direction::Up)
    } else if code & CONTROL_DOWN != 0 {
        Some(Direction::Down)
    } else if code & CONTROL_LEFT != 0 {
        Some(Direction::Left)
    } else if code & CONTROL_RIGHT != 0 {
        Some(Direction::Right)
    } else {
        None
    }
}

/// A grid cell as fixed-point micro-units — the neutral observation-fact
/// coordinate convention (`axiom-agent` facts are integer only).
fn micro(cell: i32) -> i64 {
    i64::from(cell) * 1_000_000
}

/// Run one `observe → decide → emit` cycle through `axiom-agent` for a single
/// planned grid move and return the lowered [`Direction`] to apply.
///
/// All of `axiom-agent`'s neutral contracts (id, profile, observation, brain,
/// memory, queue) are created and consumed here, held only by type inference —
/// the app never names a sealed `axiom-agent` type. The brain is a one-shot
/// hold-set of the planned move: the plan *is* the decision, run through the
/// substrate so it produces a real report and emits a player-equivalent intent
/// the app then lowers.
fn decide_move(
    state: &PuzzleGameState,
    planned: Direction,
    goal: GridCoord,
    tick: u64,
) -> Direction {
    let agent_id = AgentApi::create_agent_id(AGENT_RAW_ID);
    let profile = AgentApi::debug_perfect_profile();
    let mut brain = AgentApi::hold_set_brain(vec![control_of(planned)]);
    let mut memory = AgentApi::empty_memory(1);

    let here = state.player().position;
    let mut builder = AgentApi::observation_builder(agent_id, Tick::new(tick), 1, 2, 0);
    builder
        .add_channel(AgentApi::channel_semantic())
        .expect("one channel within the channel bound");
    builder
        .add_fact(AgentApi::observation_fact(
            FACT_PLAYER_CELL,
            0,
            micro(here.x),
            0,
            micro(here.y),
            0,
        ))
        .expect("player-cell fact within the fact bound");
    builder
        .add_fact(AgentApi::observation_fact(
            FACT_GOAL_CELL,
            0,
            micro(goal.x),
            0,
            micro(goal.y),
            0,
        ))
        .expect("goal-cell fact within the fact bound");
    let observation = builder.build();

    let step = RuntimeStep::new(FrameIndex::new(0), Tick::new(tick), FIXED_DELTA_NANOS, 0);
    let (_report, queue) = AgentApi::step(
        agent_id,
        profile,
        &mut brain,
        &observation,
        &mut memory,
        step,
    );

    // The debug-perfect hold-set emits exactly the planned control, so the lowered
    // move equals the plan — but it is the agent's *emitted* decision we apply.
    direction_of_control(queue.combined_control_code()).unwrap_or(planned)
}

/// Plan the first step of a shortest route from the live player to `target`,
/// respecting the current board: walls and closed doors are impassable, and cells
/// a ghost occupies are blocked. Deterministic BFS (fixed neighbour order).
/// Returns `None` if already there or no route exists right now.
fn plan_step(state: &PuzzleGameState, target: GridCoord) -> Option<Direction> {
    let w = state.width();
    let h = state.height();
    let start = state.player().position;
    if start == target {
        return None;
    }
    let idx = |c: GridCoord| c.y as usize * w as usize + c.x as usize;
    let occupied: Vec<GridCoord> = state.ghost_states().iter().map(|g| g.position).collect();
    let passable = |c: GridCoord| -> bool {
        match state.cell_at(c) {
            None | Some(Cell::Wall) => false,
            Some(Cell::Door(g)) => state.is_group_open(g),
            Some(_) => true,
        }
    };
    let n = w as usize * h as usize;
    let mut prev: Vec<Option<(usize, Direction)>> = vec![None; n];
    let mut seen = vec![false; n];
    let mut frontier = std::collections::VecDeque::new();
    seen[idx(start)] = true;
    frontier.push_back(start);
    while let Some(cur) = frontier.pop_front() {
        for &d in &DIRS {
            let next = cur.stepped(d);
            if !next.in_bounds(w, h) {
                continue;
            }
            let i = idx(next);
            let is_target = next == target;
            let blocked = occupied.contains(&next) || !passable(next);
            if seen[i] || (blocked && !is_target) {
                continue;
            }
            seen[i] = true;
            prev[i] = Some((idx(cur), d));
            if is_target {
                // Backtrack to the edge leaving the start cell.
                let mut ci = i;
                loop {
                    let (pi, pd) = prev[ci].expect("reachable cell has a predecessor");
                    if pi == idx(start) {
                        return Some(pd);
                    }
                    ci = pi;
                }
            }
            frontier.push_back(next);
        }
    }
    None
}

/// A record of an agent playthrough: the emitted move sequence, the milestone
/// events, and the outcome. Enough to assert a win and to print the run.
#[derive(Debug, Clone)]
pub struct Playthrough {
    /// Whether the live player ended on the exit.
    pub solved: bool,
    /// Every grid move the agent emitted, in order.
    pub moves: Vec<Direction>,
    /// Human-readable milestones (reached button, pressed q, gate opened, solved).
    pub events: Vec<String>,
    /// How many ghosts existed at the end.
    pub ghosts: usize,
    /// The final simulation tick.
    pub ticks: u64,
}

/// Play the built-in first level with the agent and return the run.
pub fn play_first_level() -> Playthrough {
    let level = level_codec::from_toml(LEVEL_001_TOML).expect("embedded level parses");
    play(level)
}

/// Play a single-button/door level: the agent walks a ghost onto the button, then
/// walks the live player through the opened door to the exit. Every move is
/// emitted through `axiom-agent`; ghost cadence advances via `Tick` commands.
pub fn play(level: LevelDefinition) -> Playthrough {
    let gate: Option<(GridCoord, GroupId)> =
        level.buttons.first().map(|b| (b.position, b.group.clone()));
    let exit = level.exit;
    let mut state = PuzzleGameState::new(level);
    let mut moves = Vec::new();
    let mut events = Vec::new();
    let mut tick = 0u64;

    if let Some((button, group)) = gate {
        // Phase 1 — the agent walks the live player onto the button, recording the
        // path that will become the ghost.
        let mut guard = 0;
        while state.player().position != button && guard < 500 {
            guard += 1;
            let Some(planned) = plan_step(&state, button) else {
                break;
            };
            let mv = decide_move(&state, planned, button, tick);
            tick += 1;
            if game_step::step(&mut state, PuzzleCommand::Move(mv)).player_moved() {
                moves.push(mv);
            } else {
                break;
            }
        }
        events.push(format!(
            "walked to the button at ({}, {}) in {} moves",
            button.x,
            button.y,
            moves.len()
        ));

        // Phase 2 — press q: the recorded life becomes a ghost from the entrance.
        game_step::step(&mut state, PuzzleCommand::ResetLifeFromRecording);
        events.push(format!(
            "pressed q — ghost #{} will replay that path",
            state.ghost_count()
        ));

        // Phase 3 — advance the fixed step until the ghost reaches the button and
        // the gate opens (the ghost holds it forever once finished).
        let mut waited = 0;
        while !state.is_group_open(&group) && waited < 1000 {
            game_step::step(&mut state, PuzzleCommand::Tick);
            tick += 1;
            waited += 1;
        }
        events.push(format!(
            "the ghost reached the button — gate \"{}\" is open (after {} ticks)",
            group.as_str(),
            waited
        ));
    }

    // Phase 4 — the agent walks the live player (reset to the entrance) around the
    // ghost and through the open door to the exit.
    let mut guard = 0;
    while !state.is_solved() && guard < 4000 {
        guard += 1;
        match plan_step(&state, exit) {
            Some(planned) => {
                let mv = decide_move(&state, planned, exit, tick);
                tick += 1;
                let result = game_step::step(&mut state, PuzzleCommand::Move(mv));
                if result.player_moved() {
                    moves.push(mv);
                } else {
                    // Blocked (e.g. a door not yet open): let the sim advance and retry.
                    game_step::step(&mut state, PuzzleCommand::Tick);
                    tick += 1;
                }
            }
            None => {
                game_step::step(&mut state, PuzzleCommand::Tick);
                tick += 1;
            }
        }
    }
    (state.is_solved())
        .then(|| events.push(format!("reached the exit at ({}, {})", exit.x, exit.y)));

    Playthrough {
        solved: state.is_solved(),
        moves,
        events,
        ghosts: state.ghost_count(),
        ticks: tick,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_codes_round_trip_every_direction() {
        for d in DIRS {
            assert_eq!(direction_of_control(control_of(d)), Some(d));
        }
        assert_eq!(direction_of_control(0), None);
    }

    #[test]
    fn agent_wins_level_one() {
        let run = play_first_level();
        println!("\n=== axiom-agent plays Zanzoban level 1 ===");
        for event in &run.events {
            println!("  · {event}");
        }
        println!("  moves emitted through axiom-agent: {:?}", run.moves);
        println!(
            "  outcome: solved={} ghosts={} final_tick={}\n",
            run.solved, run.ghosts, run.ticks
        );
        assert!(
            run.solved,
            "the agent should solve level 1; events={:?}",
            run.events
        );
        assert!(run.ghosts >= 1, "the solution leaves at least one ghost");
    }

    #[test]
    fn playthrough_is_deterministic() {
        let a = play_first_level();
        let b = play_first_level();
        assert_eq!(a.moves, b.moves, "same level -> same agent moves");
        assert_eq!(a.ticks, b.ticks);
    }
}
