//! A neutral, presentation-agnostic description of what to draw.
//!
//! The game is logically 2D but drawn top-down with depth cues. The *depth* of
//! each thing — floor flat, walls and closed doors raised, open doors recessed,
//! buttons slightly raised (released) or slightly depressed (pressed), the player
//! a solid block, ghosts translucent blocks — is decided here, once, as data.
//! The browser canvas reads this model and paints it; it makes no gameplay or
//! depth decisions of its own. Keeping the mapping here makes the visual rules
//! testable on native, away from the DOM.

use crate::roomed_puzzle::actor_state::ActorKind;
use crate::roomed_puzzle::coord::GridCoord;
use crate::roomed_puzzle::game_state::{Cell, PuzzleGameState};

/// The live player is fully opaque.
pub const PLAYER_ALPHA: f32 = 1.0;
/// Ghosts are translucent — clearly solid blocks, just see-through enough to
/// read as past selves (not wireframes, not outlines).
pub const GHOST_ALPHA: f32 = 0.45;

/// How far a cell reads above or below the floor plane — the depth cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    /// Sunk well below the floor (an open doorway you can walk into).
    Recessed,
    /// Sunk slightly (a pressed button).
    SlightlyRecessed,
    /// Level with the floor.
    Flat,
    /// Standing slightly proud (a released button).
    SlightlyRaised,
    /// A full raised block (wall or closed door).
    Raised,
}

/// What a single cell is, for drawing — the live open/pressed state folded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTile {
    /// Walkable ground.
    Floor,
    /// A solid wall block.
    Wall,
    /// The start pad.
    Entrance,
    /// The goal pad.
    Exit,
    /// A pressure button; `pressed` while an actor stands on it.
    Button {
        /// Whether an actor currently holds this button down.
        pressed: bool,
    },
    /// A door; `open` while its wiring group is pressed.
    Door {
        /// Whether the door is currently open (passable, recessed).
        open: bool,
    },
}

impl RenderTile {
    /// The depth cue for this tile.
    pub const fn elevation(self) -> Elevation {
        match self {
            RenderTile::Floor | RenderTile::Entrance | RenderTile::Exit => Elevation::Flat,
            RenderTile::Wall => Elevation::Raised,
            RenderTile::Door { open: true } => Elevation::Recessed,
            RenderTile::Door { open: false } => Elevation::Raised,
            RenderTile::Button { pressed: true } => Elevation::SlightlyRecessed,
            RenderTile::Button { pressed: false } => Elevation::SlightlyRaised,
        }
    }

    /// Can a solid actor stand on this tile as drawn? (Walls and closed doors
    /// cannot be stood on; everything else can.) Used only for presentation.
    pub const fn is_solid_block(self) -> bool {
        matches!(self, RenderTile::Wall | RenderTile::Door { open: false })
    }
}

/// One cell to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCell {
    /// Grid position.
    pub coord: GridCoord,
    /// What and how raised/recessed.
    pub tile: RenderTile,
}

/// One actor to draw, on top of the cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderActor {
    /// Player or ghost.
    pub kind: ActorKind,
    /// Grid position.
    pub coord: GridCoord,
    /// Opacity: opaque for the player, translucent for ghosts.
    pub alpha: f32,
}

/// A complete frame to draw: the grid then the actors (ghosts first, player
/// last, so the live player draws on top of any ghost sharing a cell).
#[derive(Debug, Clone, PartialEq)]
pub struct RenderModel {
    /// Grid width.
    pub width: u32,
    /// Grid height.
    pub height: u32,
    /// Row-major cells.
    pub cells: Vec<RenderCell>,
    /// Actors in draw order (ghosts, then the player).
    pub actors: Vec<RenderActor>,
}

impl RenderModel {
    /// Build the render model for a live playtest state. Door/button depth
    /// reflects current occupancy; ghosts are drawn translucent under the player.
    pub fn from_state(state: &PuzzleGameState) -> Self {
        let pressed = state.pressed_groups();
        let cells = (0..state.height() as i32)
            .flat_map(|y| (0..state.width() as i32).map(move |x| GridCoord::new(x, y)))
            .map(|coord| {
                let tile = match state.cell_at(coord) {
                    Some(Cell::Wall) => RenderTile::Wall,
                    Some(Cell::Entrance) => RenderTile::Entrance,
                    Some(Cell::Exit) => RenderTile::Exit,
                    Some(Cell::Button(g)) => RenderTile::Button {
                        pressed: pressed.contains(g),
                    },
                    Some(Cell::Door(g)) => RenderTile::Door {
                        open: pressed.contains(g),
                    },
                    _ => RenderTile::Floor,
                };
                RenderCell { coord, tile }
            })
            .collect();

        let mut actors: Vec<RenderActor> = state
            .ghost_states()
            .into_iter()
            .map(|g| RenderActor {
                kind: g.kind,
                coord: g.position,
                alpha: GHOST_ALPHA,
            })
            .collect();
        let player = state.player();
        actors.push(RenderActor {
            kind: player.kind,
            coord: player.position,
            alpha: PLAYER_ALPHA,
        });

        RenderModel {
            width: state.width(),
            height: state.height(),
            cells,
            actors,
        }
    }

    /// The cell at `coord`, if present.
    pub fn cell_at(&self, coord: GridCoord) -> Option<&RenderCell> {
        coord
            .in_bounds(self.width, self.height)
            .then(|| &self.cells[coord.y as usize * self.width as usize + coord.x as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roomed_puzzle::coord::GridCoord;
    use crate::roomed_puzzle::direction::Direction;
    use crate::roomed_puzzle::group_id::GroupId;
    use crate::roomed_puzzle::level_definition::{Button, Door, LevelDefinition};

    fn corridor() -> LevelDefinition {
        LevelDefinition {
            title: "c".into(),
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

    #[test]
    fn elevation_encodes_the_depth_cues() {
        assert_eq!(RenderTile::Floor.elevation(), Elevation::Flat);
        assert_eq!(RenderTile::Wall.elevation(), Elevation::Raised);
        assert_eq!(
            RenderTile::Door { open: false }.elevation(),
            Elevation::Raised
        );
        assert_eq!(
            RenderTile::Door { open: true }.elevation(),
            Elevation::Recessed
        );
        assert_eq!(
            RenderTile::Button { pressed: false }.elevation(),
            Elevation::SlightlyRaised
        );
        assert_eq!(
            RenderTile::Button { pressed: true }.elevation(),
            Elevation::SlightlyRecessed
        );
    }

    #[test]
    fn ghosts_are_more_transparent_than_the_player() {
        const { assert!(GHOST_ALPHA < PLAYER_ALPHA) };
        const {
            assert!(
                GHOST_ALPHA > 0.0,
                "ghosts are still visibly solid, not invisible"
            )
        };
    }

    #[test]
    fn live_state_folds_in_open_door_and_pressed_button() {
        let mut s = PuzzleGameState::new(corridor());
        let m0 = RenderModel::from_state(&s);
        assert_eq!(
            m0.cell_at(GridCoord::new(3, 0)).unwrap().tile,
            RenderTile::Door { open: false }
        );
        s.apply_player_move(Direction::Right);
        let m1 = RenderModel::from_state(&s);
        assert_eq!(
            m1.cell_at(GridCoord::new(3, 0)).unwrap().tile,
            RenderTile::Door { open: true }
        );
        assert_eq!(
            m1.cell_at(GridCoord::new(1, 0)).unwrap().tile,
            RenderTile::Button { pressed: true }
        );
        assert_eq!(m1.actors.last().unwrap().kind, ActorKind::Player);
        assert_eq!(m1.actors.last().unwrap().alpha, PLAYER_ALPHA);
    }
}
