//! The palette of static tile/object kinds an authored level is built from.
//!
//! [`TileKind`] is the *editor palette* vocabulary: the kinds a cell can be
//! painted as. It is deliberately group-free — the button/door/switch wiring group
//! is a separate field the editor tracks alongside the painted kind (see
//! [`crate::zanzoban::group_id`]). The runtime grid uses a richer cell type that
//! carries the group ([`crate::zanzoban::game_state::Cell`]); `TileKind` is the
//! flat, human-facing menu.
//!
//! The base six kinds are always available. The four add-on kinds (well, switch,
//! crate, hazard) are only offered when their [`Addon`] is enabled for the level.

/// A configurable mechanic add-on that a tile kind belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addon {
    /// Afterimage decay (resonance wells).
    Decay,
    /// Latching switches.
    Switches,
    /// Pushable echo-crates.
    Crates,
    /// Lethal hazards.
    Hazards,
}

/// A static tile/object kind that can occupy a cell.
///
/// Exactly one kind occupies a cell. `Floor` is the empty default; `Wall`,
/// `Entrance`, `Exit`, `Button`, `Door`, `Well`, `Switch`, `Crate` and `Hazard`
/// are the placeable objects. Two placeable objects in one cell is a level error
/// (see [`crate::zanzoban::level_validation`]).
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
    /// A barrier that is passable only while its wiring group is pressed or latched.
    Door,
    /// A resonance well that refreshes ghost life (decay add-on).
    Well,
    /// A latching switch; entering it toggles its wiring group (switches add-on).
    Switch,
    /// A pushable crate (crates add-on).
    Crate,
    /// A lethal hazard tile (hazards add-on).
    Hazard,
}

impl TileKind {
    /// Every kind, in display order. `Floor` doubles as the eraser. The four
    /// add-on kinds trail the base six.
    pub const ALL: [TileKind; 10] = [
        TileKind::Floor,
        TileKind::Wall,
        TileKind::Entrance,
        TileKind::Exit,
        TileKind::Button,
        TileKind::Door,
        TileKind::Well,
        TileKind::Switch,
        TileKind::Crate,
        TileKind::Hazard,
    ];

    /// Does this kind carry a wiring group (button, door, or switch)?
    pub const fn has_group(self) -> bool {
        matches!(self, TileKind::Button | TileKind::Door | TileKind::Switch)
    }

    /// The add-on this kind belongs to, or `None` for the always-available base
    /// six. A kind whose add-on is disabled is hidden from the palette.
    pub const fn required_addon(self) -> Option<Addon> {
        match self {
            TileKind::Well => Some(Addon::Decay),
            TileKind::Switch => Some(Addon::Switches),
            TileKind::Crate => Some(Addon::Crates),
            TileKind::Hazard => Some(Addon::Hazards),
            _ => None,
        }
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
            TileKind::Well => "Well",
            TileKind::Switch => "Switch",
            TileKind::Crate => "Crate",
            TileKind::Hazard => "Hazard",
        }
    }

    /// The stable lowercase slug — the single source for the browser palette's
    /// `data-kind` strings (see the web shell's `tile_from_str`).
    pub const fn slug(self) -> &'static str {
        match self {
            TileKind::Floor => "floor",
            TileKind::Wall => "wall",
            TileKind::Entrance => "entrance",
            TileKind::Exit => "exit",
            TileKind::Button => "button",
            TileKind::Door => "door",
            TileKind::Well => "well",
            TileKind::Switch => "switch",
            TileKind::Crate => "crate",
            TileKind::Hazard => "hazard",
        }
    }

    /// Resolve a slug back to its kind (inverse of [`TileKind::slug`]).
    pub fn from_slug(slug: &str) -> Option<TileKind> {
        TileKind::ALL.into_iter().find(|k| k.slug() == slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_doors_and_switches_have_groups() {
        assert!(TileKind::Button.has_group());
        assert!(TileKind::Door.has_group());
        assert!(TileKind::Switch.has_group());
        for k in [
            TileKind::Floor,
            TileKind::Wall,
            TileKind::Entrance,
            TileKind::Exit,
            TileKind::Well,
            TileKind::Crate,
            TileKind::Hazard,
        ] {
            assert!(!k.has_group());
        }
    }

    #[test]
    fn palette_is_complete_labelled_and_slugged() {
        assert_eq!(TileKind::ALL.len(), 10);
        assert!(TileKind::ALL.iter().all(|k| !k.label().is_empty()));
        // Slugs are unique and round-trip.
        for k in TileKind::ALL {
            assert_eq!(TileKind::from_slug(k.slug()), Some(k));
        }
        assert_eq!(TileKind::from_slug("nope"), None);
    }

    #[test]
    fn only_addon_kinds_require_an_addon() {
        assert_eq!(TileKind::Well.required_addon(), Some(Addon::Decay));
        assert_eq!(TileKind::Switch.required_addon(), Some(Addon::Switches));
        assert_eq!(TileKind::Crate.required_addon(), Some(Addon::Crates));
        assert_eq!(TileKind::Hazard.required_addon(), Some(Addon::Hazards));
        for k in [TileKind::Floor, TileKind::Wall, TileKind::Button, TileKind::Door] {
            assert_eq!(k.required_addon(), None);
        }
    }
}
