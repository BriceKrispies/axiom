//! The canonical authored-level type.
//!
//! A [`LevelDefinition`] is the in-memory shape of a level file: a titled
//! rectangular room with a single entrance (the player/ghost start), a single
//! exit, a list of walls, and lists of buttons and doors wired by group. It is
//! the type that:
//!
//! * round-trips through TOML ([`crate::zanzoban::level_codec`]),
//! * is checked by [`crate::zanzoban::level_validation::validate_level`], and
//! * is played by [`crate::zanzoban::game_state::PuzzleGameState`].
//!
//! Multiplicity errors ("not exactly one entrance") live one level up, in the
//! editor and the census, because a `LevelDefinition` structurally has exactly
//! one entrance and one exit (single fields). The remaining structural rules
//! (bounds, empty groups, door-without-button, out-of-grid, overlap, blocked
//! start) are all reachable on a hand-built or hand-edited `LevelDefinition`.

use crate::zanzoban::coord::GridCoord;
use crate::zanzoban::group_id::GroupId;

/// A pressure button: standing on it presses its wiring [`GroupId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    /// The cell the button occupies.
    pub position: GridCoord,
    /// The wiring group this button presses.
    pub group: GroupId,
}

/// A door: passable only while its wiring [`GroupId`] is pressed by some actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Door {
    /// The cell the door occupies.
    pub position: GridCoord,
    /// The wiring group that opens this door.
    pub group: GroupId,
}

/// A latching switch: stepping *onto* it flips its wiring [`GroupId`] between
/// latched and unlatched (edge-triggered), and a door of that group is open while
/// the group is latched — the persistent, order-dependent counterpart to a
/// hold-only [`Button`]. Part of the **switches** add-on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    /// The cell the switch occupies.
    pub position: GridCoord,
    /// The wiring group this switch toggles.
    pub group: GroupId,
}

/// The **afterimage-decay** add-on: a ghost fades after this many of its own
/// steps and then vanishes (releasing anything it held). Standing on a resonance
/// well refreshes a ghost back to full life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecayRule {
    /// How many ghost-steps an afterimage lasts before it fades.
    pub lifetime_steps: u32,
}

/// The **echo-budget** add-on: cap how many ghosts a life may leave, and record a
/// par for scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetRule {
    /// The most ghosts allowed to exist at once (`q` is refused at the cap).
    pub max_ghosts: u32,
    /// An optional target ghost count for a clean solve (scoring only).
    pub par: Option<u32>,
}

/// The per-level configurable mechanics ("add-ons"). Every field defaults to
/// off/absent, so a level that names no rules plays exactly like the base game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleSet {
    /// Afterimage decay (with resonance wells to refresh); `None` = ghosts never fade.
    pub decay: Option<DecayRule>,
    /// Echo budget + par; `None` = unlimited ghosts.
    pub budget: Option<BudgetRule>,
    /// Whether latching switches are active.
    pub switches: bool,
    /// Whether pushable echo-crates are active.
    pub crates: bool,
    /// Whether lethal hazard tiles are active.
    pub hazards: bool,
}

/// A complete authored level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelDefinition {
    /// Human-facing title.
    pub title: String,
    /// Grid width in cells.
    pub width: u32,
    /// Grid height in cells.
    pub height: u32,
    /// The cell the live player (and every ghost) starts on.
    pub entrance: GridCoord,
    /// The cell the live player must reach to solve the level.
    pub exit: GridCoord,
    /// Solid, impassable wall cells.
    pub walls: Vec<GridCoord>,
    /// Pressure buttons.
    pub buttons: Vec<Button>,
    /// Doors.
    pub doors: Vec<Door>,
    /// Resonance wells that refresh ghost life (decay add-on).
    pub wells: Vec<GridCoord>,
    /// Latching switches (switches add-on).
    pub switches: Vec<Switch>,
    /// Initial pushable-crate cells (crates add-on).
    pub crates: Vec<GridCoord>,
    /// Lethal hazard cells (hazards add-on).
    pub hazards: Vec<GridCoord>,
    /// The configurable mechanics enabled for this level.
    pub rules: RuleSet,
}

impl LevelDefinition {
    /// The set of distinct wiring groups any button uses.
    pub fn button_groups(&self) -> Vec<GroupId> {
        let mut groups: Vec<GroupId> = self.buttons.iter().map(|b| b.group.clone()).collect();
        groups.sort();
        groups.dedup();
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_groups_are_deduped_and_sorted() {
        let level = LevelDefinition {
            title: "t".into(),
            width: 10,
            height: 10,
            entrance: GridCoord::new(1, 5),
            exit: GridCoord::new(8, 5),
            walls: vec![],
            buttons: vec![
                Button {
                    position: GridCoord::new(4, 5),
                    group: GroupId::new("main"),
                },
                Button {
                    position: GridCoord::new(4, 6),
                    group: GroupId::new("main"),
                },
                Button {
                    position: GridCoord::new(2, 2),
                    group: GroupId::new("alt"),
                },
            ],
            doors: vec![],
            wells: Vec::new(),
            switches: Vec::new(),
            crates: Vec::new(),
            hazards: Vec::new(),
            rules: Default::default(),
        };
        assert_eq!(
            level.button_groups(),
            vec![GroupId::new("alt"), GroupId::new("main")]
        );
    }
}
