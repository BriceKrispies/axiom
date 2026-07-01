//! The palette of static tile/object kinds an authored level is built from.
//!
//! [`TileKind`] is the *editor palette* vocabulary: the six kinds a cell can be
//! painted as. It is deliberately group-free — the button/door wiring group is a
//! separate field the editor tracks alongside the painted kind (see
//! [`crate::zanzoban::group_id`]). The runtime grid uses a richer cell type that carries
//! the group ([`crate::zanzoban::game_state::Cell`]); `TileKind` is the flat,
//! human-facing menu.

/// A static tile/object kind that can occupy a cell.
///
/// Exactly one kind occupies a cell. `Floor` is the empty default; `Wall`,
/// `Entrance`, `Exit`, `Button` and `Door` are the placeable objects. Two
/// placeable objects in one cell is a level error (see
/// [`crate::zanzoban::level_validation`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileKind {
    /// Empty, walkable ground.
    Floor,
    /// A solid, impassable block.
    Wall,
    /// The single cell the live player (and every ghost) starts on.
    Entrance,
    /// The single cell the live player must reach to solve the level.
    Exit,
    /// A pressure plate; standing on it presses its wiring group.
    Button,
    /// A barrier that is passable only while its wiring group is pressed.
    Door,
}

impl TileKind {
    /// The palette, in display order. `Floor` doubles as the eraser.
    pub const PALETTE: [TileKind; 6] = [
        TileKind::Floor,
        TileKind::Wall,
        TileKind::Entrance,
        TileKind::Exit,
        TileKind::Button,
        TileKind::Door,
    ];

    /// Does this kind carry a wiring group (i.e. is it a button or a door)?
    pub const fn has_group(self) -> bool {
        matches!(self, TileKind::Button | TileKind::Door)
    }

    /// A short, stable label for UI and the palette.
    pub const fn label(self) -> &'static str {
        match self {
            TileKind::Floor => "Floor",
            TileKind::Wall => "Wall",
            TileKind::Entrance => "Entrance",
            TileKind::Exit => "Exit",
            TileKind::Button => "Button",
            TileKind::Door => "Door",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_buttons_and_doors_have_groups() {
        assert!(TileKind::Button.has_group());
        assert!(TileKind::Door.has_group());
        for k in [
            TileKind::Floor,
            TileKind::Wall,
            TileKind::Entrance,
            TileKind::Exit,
        ] {
            assert!(!k.has_group());
        }
    }

    #[test]
    fn palette_is_complete_and_labelled() {
        assert_eq!(TileKind::PALETTE.len(), 6);
        assert_eq!(TileKind::Door.label(), "Door");
        assert!(TileKind::PALETTE.iter().all(|k| !k.label().is_empty()));
    }
}
