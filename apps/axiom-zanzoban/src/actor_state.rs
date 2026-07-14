//! Solid actors: the live player and the ghosts.
//!
//! Both the live player and every ghost are *solid actors* — each occupies
//! exactly one cell, blocks movement, can stand on a button, and collides with
//! walls and closed doors. The only differences are how they decide their moves
//! (the player from input, a ghost from a recorded path) and how they are drawn
//! (ghosts are translucent). [`ActorState`] is the shared positional state.

use crate::coord::GridCoord;

/// What kind of solid actor this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActorKind {
    /// The single live, input-driven player.
    Player,
    /// A recorded ghost replaying a past life.
    Ghost,
}

/// A stable identity for a solid actor.
///
/// The live player is always [`ActorId::PLAYER`]. Each ghost gets a 1-based id
/// in creation order ([`ActorId::ghost`]), which is also the deterministic
/// tie-break order used when resolving who moves first on a tick (ghosts in
/// creation order, then the player — though the player never moves on a tick).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActorId(u32);

impl ActorId {
    /// The live player's id.
    pub const PLAYER: ActorId = ActorId(0);

    /// The id of the ghost created `creation_index`-th (0-based).
    pub const fn ghost(creation_index: u32) -> ActorId {
        ActorId(creation_index + 1)
    }

    /// The raw id value (`0` for the player, `1..` for ghosts).
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// The positional state of one solid actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorState {
    /// Stable identity (player or n-th ghost).
    pub id: ActorId,
    /// Player or ghost.
    pub kind: ActorKind,
    /// The cell this actor currently occupies.
    pub position: GridCoord,
}

impl ActorState {
    /// A live player at `position`.
    pub const fn player(position: GridCoord) -> Self {
        ActorState {
            id: ActorId::PLAYER,
            kind: ActorKind::Player,
            position,
        }
    }

    /// The `creation_index`-th ghost, placed at `position` (the entrance).
    pub const fn ghost(creation_index: u32, position: GridCoord) -> Self {
        ActorState {
            id: ActorId::ghost(creation_index),
            kind: ActorKind::Ghost,
            position,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_and_ghost_ids_are_distinct_and_ordered() {
        assert_eq!(ActorId::PLAYER.raw(), 0);
        assert_eq!(ActorId::ghost(0).raw(), 1);
        assert_eq!(ActorId::ghost(1).raw(), 2);
        // Ghosts created earlier sort before later ones (creation order).
        assert!(ActorId::ghost(0) < ActorId::ghost(1));
    }

    #[test]
    fn constructors_set_kind_and_position() {
        let p = ActorState::player(GridCoord::new(1, 5));
        assert_eq!(p.kind, ActorKind::Player);
        assert_eq!(p.position, GridCoord::new(1, 5));
        let g = ActorState::ghost(0, GridCoord::new(1, 5));
        assert_eq!(g.kind, ActorKind::Ghost);
        assert_eq!(g.id, ActorId::ghost(0));
    }
}
