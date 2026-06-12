//! Player control: a `Player` marks a node as controllable, and a `PlayerInput`
//! is one tick's move for a player.

use axiom_math::Vec3;

/// Marks a spawned node as the controllable cube for a player, addressed by
/// `index`. Per-tick [`PlayerInput`]s addressed to that index translate the
/// node — the engine's answer to "this node is driven by a player's input".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Player {
    /// The player index this node belongs to.
    pub index: u32,
}

impl Player {
    /// The controllable node for player `index`.
    pub const fn new(index: u32) -> Self {
        Player { index }
    }
}

/// One tick's move for a player: translate the node of player `player` by
/// `delta`. The app builds these from input each tick and hands them to
/// [`crate::prelude::RunningApp::tick_with`]; the engine applies them
/// deterministically before stepping the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerInput {
    /// The player this input is for.
    pub player: u32,
    /// The translation delta to apply this tick.
    pub delta: Vec3,
}

impl PlayerInput {
    /// A move for `player` by `delta`.
    pub const fn new(player: u32, delta: Vec3) -> Self {
        PlayerInput { player, delta }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_carries_its_index() {
        assert_eq!(Player::new(2).index, 2);
    }

    #[test]
    fn player_input_carries_its_fields() {
        let input = PlayerInput::new(1, Vec3::new(0.5, -0.5, 0.0));
        assert_eq!(input.player, 1);
        assert_eq!(input.delta, Vec3::new(0.5, -0.5, 0.0));
    }
}
