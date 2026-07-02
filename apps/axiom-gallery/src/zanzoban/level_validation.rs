//! Authored-level validation.
//!
//! Validation runs on a [`LevelCensus`] — a multiplicity-capable inventory of
//! everything placed on the grid. The census is what makes *all* of the task's
//! rules reachable from one validator:
//!
//! * A [`LevelDefinition`] always yields a census with exactly one entrance and
//!   one exit (single fields), so validating a parsed level exercises the
//!   bounds / group / overlap / blocked-start rules.
//! * The editor yields a census straight from its paint grid, where the player
//!   can paint zero or many entrances/exits — so the "not exactly one
//!   entrance/exit" rules are reachable there.
//!
//! One [`LevelError`] enum and one [`LevelValidationReport`] cover both paths.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::zanzoban::coord::{GridCoord, MAX_DIMENSION};
use crate::zanzoban::group_id::GroupId;
use crate::zanzoban::level_definition::LevelDefinition;

/// A multiplicity-capable inventory of a level's placements — the input to
/// validation. Built either from a [`LevelDefinition`] or from the editor grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelCensus {
    /// Declared grid width.
    pub width: u32,
    /// Declared grid height.
    pub height: u32,
    /// Every cell painted as the entrance (a valid level has exactly one).
    pub entrances: Vec<GridCoord>,
    /// Every cell painted as the exit (a valid level has exactly one).
    pub exits: Vec<GridCoord>,
    /// Wall cells.
    pub walls: Vec<GridCoord>,
    /// Button cells with their wiring group.
    pub buttons: Vec<(GridCoord, GroupId)>,
    /// Door cells with their wiring group.
    pub doors: Vec<(GridCoord, GroupId)>,
    /// Resonance-well cells (decay add-on).
    pub wells: Vec<GridCoord>,
    /// Switch cells with their wiring group (switches add-on).
    pub switches: Vec<(GridCoord, GroupId)>,
    /// Pushable-crate cells (crates add-on).
    pub crates: Vec<GridCoord>,
    /// Hazard cells (hazards add-on).
    pub hazards: Vec<GridCoord>,
}

impl LevelCensus {
    /// The census of a canonical [`LevelDefinition`] (always one entrance/exit).
    pub fn of_level(level: &LevelDefinition) -> Self {
        LevelCensus {
            width: level.width,
            height: level.height,
            entrances: vec![level.entrance],
            exits: vec![level.exit],
            walls: level.walls.clone(),
            buttons: level
                .buttons
                .iter()
                .map(|b| (b.position, b.group.clone()))
                .collect(),
            doors: level
                .doors
                .iter()
                .map(|d| (d.position, d.group.clone()))
                .collect(),
            wells: level.wells.clone(),
            switches: level
                .switches
                .iter()
                .map(|s| (s.position, s.group.clone()))
                .collect(),
            crates: level.crates.clone(),
            hazards: level.hazards.clone(),
        }
    }

    /// Every placed cell, for the overlap and out-of-grid scans.
    fn placements(&self) -> Vec<GridCoord> {
        self.entrances
            .iter()
            .chain(self.exits.iter())
            .chain(self.walls.iter())
            .chain(self.buttons.iter().map(|(c, _)| c))
            .chain(self.doors.iter().map(|(c, _)| c))
            .chain(self.wells.iter())
            .chain(self.switches.iter().map(|(c, _)| c))
            .chain(self.crates.iter())
            .chain(self.hazards.iter())
            .copied()
            .collect()
    }
}

/// A configurable mechanic ("add-on"), used to report a placement whose add-on is
/// not enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mechanic {
    /// Afterimage decay (resonance wells).
    Decay,
    /// Latching switches.
    Switch,
    /// Pushable echo-crates.
    Crate,
    /// Lethal hazards.
    Hazard,
}

impl Mechanic {
    /// A short label for messages.
    pub const fn label(self) -> &'static str {
        match self {
            Mechanic::Decay => "resonance well",
            Mechanic::Switch => "switch",
            Mechanic::Crate => "crate",
            Mechanic::Hazard => "hazard",
        }
    }

    /// The add-on that must be enabled for this placement to be legal.
    pub const fn addon(self) -> &'static str {
        match self {
            Mechanic::Decay => "decay",
            Mechanic::Switch => "switches",
            Mechanic::Crate => "crates",
            Mechanic::Hazard => "hazards",
        }
    }
}

/// A single thing wrong with a level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelError {
    /// Width is zero.
    ZeroWidth,
    /// Height is zero.
    ZeroHeight,
    /// Width exceeds [`MAX_DIMENSION`].
    WidthTooLarge(u32),
    /// Height exceeds [`MAX_DIMENSION`].
    HeightTooLarge(u32),
    /// No entrance was placed.
    NoEntrance,
    /// More than one entrance was placed.
    MultipleEntrances(usize),
    /// No exit was placed.
    NoExit,
    /// More than one exit was placed.
    MultipleExits(usize),
    /// A button names an empty wiring group.
    EmptyButtonGroup(GridCoord),
    /// A door names an empty wiring group.
    EmptyDoorGroup(GridCoord),
    /// A switch names an empty wiring group.
    EmptySwitchGroup(GridCoord),
    /// A door's group has no button or switch to open it.
    DoorWithoutButton(String),
    /// A placement (well/switch/crate/hazard) exists but its add-on is disabled.
    PlacementWithoutRule {
        /// Where the offending object sits.
        coord: GridCoord,
        /// Which mechanic's add-on it needs.
        mechanic: Mechanic,
    },
    /// A placement sits outside the grid.
    OutsideGrid(GridCoord),
    /// Two exclusive static objects occupy the same cell.
    OverlappingObjects(GridCoord),
    /// The player start cell is blocked by a wall.
    PlayerStartBlocked(GridCoord),
}

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LevelError::ZeroWidth => write!(f, "width must be greater than zero"),
            LevelError::ZeroHeight => write!(f, "height must be greater than zero"),
            LevelError::WidthTooLarge(w) => {
                write!(f, "width {w} exceeds the maximum of {MAX_DIMENSION}")
            }
            LevelError::HeightTooLarge(h) => {
                write!(f, "height {h} exceeds the maximum of {MAX_DIMENSION}")
            }
            LevelError::NoEntrance => write!(f, "the level has no entrance (player start)"),
            LevelError::MultipleEntrances(n) => {
                write!(f, "the level has {n} entrances; exactly one is required")
            }
            LevelError::NoExit => write!(f, "the level has no exit"),
            LevelError::MultipleExits(n) => {
                write!(f, "the level has {n} exits; exactly one is required")
            }
            LevelError::EmptyButtonGroup(c) => {
                write!(f, "the button at ({}, {}) has an empty group", c.x, c.y)
            }
            LevelError::EmptyDoorGroup(c) => {
                write!(f, "the door at ({}, {}) has an empty group", c.x, c.y)
            }
            LevelError::EmptySwitchGroup(c) => {
                write!(f, "the switch at ({}, {}) has an empty group", c.x, c.y)
            }
            LevelError::DoorWithoutButton(g) => {
                write!(f, "door group \"{g}\" has no matching button or switch")
            }
            LevelError::PlacementWithoutRule { coord, mechanic } => write!(
                f,
                "a {} at ({}, {}) needs the \"{}\" add-on enabled",
                mechanic.label(),
                coord.x,
                coord.y,
                mechanic.addon()
            ),
            LevelError::OutsideGrid(c) => {
                write!(f, "an object at ({}, {}) is outside the grid", c.x, c.y)
            }
            LevelError::OverlappingObjects(c) => write!(
                f,
                "two static objects occupy the same cell ({}, {})",
                c.x, c.y
            ),
            LevelError::PlayerStartBlocked(c) => write!(
                f,
                "the player start ({}, {}) is blocked by a wall",
                c.x, c.y
            ),
        }
    }
}

/// The result of validating a level: the (possibly empty) list of errors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LevelValidationReport {
    errors: Vec<LevelError>,
}

impl LevelValidationReport {
    /// Is the level valid (no errors)?
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// The errors, in a deterministic order.
    pub fn errors(&self) -> &[LevelError] {
        &self.errors
    }

    /// Human-readable, one message per error.
    pub fn messages(&self) -> Vec<String> {
        self.errors.iter().map(|e| e.to_string()).collect()
    }

    /// Does the report contain a specific error?
    pub fn contains(&self, error: &LevelError) -> bool {
        self.errors.contains(error)
    }
}

/// Validate a canonical [`LevelDefinition`]: the structural census rules plus the
/// rule-consistency rules (a placement whose add-on is disabled).
pub fn validate_level(level: &LevelDefinition) -> LevelValidationReport {
    let mut report = validate_census(&LevelCensus::of_level(level));
    report.errors.extend(rule_errors(level));
    report
}

/// Add-on-consistency errors: a well/switch/crate/hazard placed while its rule is
/// off. Needs `level.rules`, which the census does not carry, so it lives here.
fn rule_errors(level: &LevelDefinition) -> Vec<LevelError> {
    let mut errors = Vec::new();
    level
        .rules
        .decay
        .is_none()
        .then(|| {
            level.wells.iter().for_each(|&coord| {
                errors.push(LevelError::PlacementWithoutRule {
                    coord,
                    mechanic: Mechanic::Decay,
                })
            })
        });
    (!level.rules.switches).then(|| {
        level.switches.iter().for_each(|s| {
            errors.push(LevelError::PlacementWithoutRule {
                coord: s.position,
                mechanic: Mechanic::Switch,
            })
        })
    });
    (!level.rules.crates).then(|| {
        level.crates.iter().for_each(|&coord| {
            errors.push(LevelError::PlacementWithoutRule {
                coord,
                mechanic: Mechanic::Crate,
            })
        })
    });
    (!level.rules.hazards).then(|| {
        level.hazards.iter().for_each(|&coord| {
            errors.push(LevelError::PlacementWithoutRule {
                coord,
                mechanic: Mechanic::Hazard,
            })
        })
    });
    errors
}

/// Validate any [`LevelCensus`] (the shared core). Errors are emitted in a fixed
/// order so the report is deterministic.
pub fn validate_census(census: &LevelCensus) -> LevelValidationReport {
    let mut errors = Vec::new();

    (census.width == 0).then(|| errors.push(LevelError::ZeroWidth));
    (census.height == 0).then(|| errors.push(LevelError::ZeroHeight));
    (census.width > MAX_DIMENSION).then(|| errors.push(LevelError::WidthTooLarge(census.width)));
    (census.height > MAX_DIMENSION).then(|| errors.push(LevelError::HeightTooLarge(census.height)));

    match census.entrances.len() {
        1 => {}
        0 => errors.push(LevelError::NoEntrance),
        n => errors.push(LevelError::MultipleEntrances(n)),
    }
    match census.exits.len() {
        1 => {}
        0 => errors.push(LevelError::NoExit),
        n => errors.push(LevelError::MultipleExits(n)),
    }

    census
        .buttons
        .iter()
        .filter(|(_, g)| g.is_empty())
        .for_each(|(c, _)| errors.push(LevelError::EmptyButtonGroup(*c)));
    census
        .doors
        .iter()
        .filter(|(_, g)| g.is_empty())
        .for_each(|(c, _)| errors.push(LevelError::EmptyDoorGroup(*c)));
    census
        .switches
        .iter()
        .filter(|(_, g)| g.is_empty())
        .for_each(|(c, _)| errors.push(LevelError::EmptySwitchGroup(*c)));

    // A door opens from a button OR a switch of the same group.
    let opener_groups: BTreeSet<&str> = census
        .buttons
        .iter()
        .chain(census.switches.iter())
        .map(|(_, g)| g.as_str())
        .filter(|g| !g.is_empty())
        .collect();
    let mut unmatched: BTreeSet<&str> = BTreeSet::new();
    census
        .doors
        .iter()
        .map(|(_, g)| g.as_str())
        .filter(|g| !g.is_empty() && !opener_groups.contains(g))
        .for_each(|g| {
            unmatched.insert(g);
        });
    unmatched
        .into_iter()
        .for_each(|g| errors.push(LevelError::DoorWithoutButton(g.to_string())));

    if census.width > 0 && census.height > 0 {
        // Stable, de-duplicated order so the report doesn't repeat a coord.
        let mut outside: BTreeSet<GridCoord> = BTreeSet::new();
        census
            .placements()
            .into_iter()
            .filter(|c| !c.in_bounds(census.width, census.height))
            .for_each(|c| {
                outside.insert(c);
            });
        outside
            .into_iter()
            .for_each(|c| errors.push(LevelError::OutsideGrid(c)));
    }

    let mut counts: BTreeMap<GridCoord, usize> = BTreeMap::new();
    census.placements().into_iter().for_each(|c| {
        *counts.entry(c).or_insert(0) += 1;
    });
    counts
        .iter()
        .filter(|(_, n)| **n > 1)
        .for_each(|(c, _)| errors.push(LevelError::OverlappingObjects(*c)));

    let walls: BTreeSet<GridCoord> = census.walls.iter().copied().collect();
    census
        .entrances
        .iter()
        .filter(|e| walls.contains(e))
        .for_each(|e| errors.push(LevelError::PlayerStartBlocked(*e)));

    LevelValidationReport { errors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zanzoban::level_definition::{Button, Door};

    /// A minimal valid level: 10×10, one entrance, one exit, a wired button+door.
    fn valid_level() -> LevelDefinition {
        LevelDefinition {
            title: "t".into(),
            width: 10,
            height: 10,
            entrance: GridCoord::new(1, 5),
            exit: GridCoord::new(8, 5),
            walls: vec![GridCoord::new(0, 0)],
            buttons: vec![Button {
                position: GridCoord::new(4, 5),
                group: GroupId::new("main"),
            }],
            doors: vec![Door {
                position: GridCoord::new(7, 5),
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
    fn a_well_formed_level_is_valid() {
        assert!(validate_level(&valid_level()).is_valid());
    }

    #[test]
    fn zero_dimensions_are_rejected() {
        let mut l = valid_level();
        l.width = 0;
        let r = validate_level(&l);
        assert!(r.contains(&LevelError::ZeroWidth));
        // No out-of-grid flood on a degenerate grid.
        assert!(!r
            .errors()
            .iter()
            .any(|e| matches!(e, LevelError::OutsideGrid(_))));
    }

    #[test]
    fn oversized_dimensions_are_rejected() {
        let mut l = valid_level();
        l.width = MAX_DIMENSION + 1;
        assert!(validate_level(&l).contains(&LevelError::WidthTooLarge(MAX_DIMENSION + 1)));
        let mut l = valid_level();
        l.height = MAX_DIMENSION + 7;
        assert!(validate_level(&l).contains(&LevelError::HeightTooLarge(MAX_DIMENSION + 7)));
    }

    #[test]
    fn empty_groups_are_rejected() {
        let mut l = valid_level();
        l.buttons[0].group = GroupId::new("");
        // The door's "main" group now has no (non-empty) button → also flagged.
        let r = validate_level(&l);
        assert!(r.contains(&LevelError::EmptyButtonGroup(GridCoord::new(4, 5))));

        let mut l = valid_level();
        l.doors[0].group = GroupId::new("");
        assert!(validate_level(&l).contains(&LevelError::EmptyDoorGroup(GridCoord::new(7, 5))));
    }

    #[test]
    fn door_without_matching_button_is_rejected() {
        let mut l = valid_level();
        l.doors[0].group = GroupId::new("other");
        assert!(validate_level(&l).contains(&LevelError::DoorWithoutButton("other".into())));
    }

    #[test]
    fn out_of_grid_placement_is_rejected() {
        let mut l = valid_level();
        l.walls.push(GridCoord::new(99, 99));
        assert!(validate_level(&l).contains(&LevelError::OutsideGrid(GridCoord::new(99, 99))));
    }

    #[test]
    fn overlapping_objects_are_rejected() {
        // Put a wall on the button's cell.
        let mut l = valid_level();
        l.walls.push(GridCoord::new(4, 5));
        assert!(validate_level(&l).contains(&LevelError::OverlappingObjects(GridCoord::new(4, 5))));
    }

    #[test]
    fn blocked_player_start_is_rejected() {
        let mut l = valid_level();
        l.walls.push(l.entrance);
        let r = validate_level(&l);
        assert!(r.contains(&LevelError::PlayerStartBlocked(GridCoord::new(1, 5))));
    }

    #[test]
    fn census_detects_zero_and_many_entrances() {
        let base = LevelCensus::of_level(&valid_level());
        let none = LevelCensus {
            entrances: vec![],
            ..base.clone()
        };
        assert!(validate_census(&none).contains(&LevelError::NoEntrance));
        let many = LevelCensus {
            exits: vec![GridCoord::new(8, 5), GridCoord::new(8, 6)],
            ..base
        };
        assert!(validate_census(&many).contains(&LevelError::MultipleExits(2)));
    }
}
