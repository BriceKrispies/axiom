//! TOML (de)serialization for [`LevelDefinition`].
//!
//! Levels are authored as plain, hand-editable TOML (documented in
//! `LEVEL_FORMAT.md`) — not a bespoke binary format. The on-disk shape is kept
//! decoupled from the domain type by a small set of `serde` document structs, so
//! the wire format and the runtime type can evolve independently. The schema:
//!
//! ```toml
//! title  = "Button Door"
//! width  = 10
//! height = 10
//!
//! [player]
//! start = [1, 5]
//!
//! [exit]
//! position = [8, 5]
//!
//! [[walls]]
//! position = [0, 0]
//!
//! [[buttons]]
//! position = [4, 5]
//! group = "main"
//!
//! [[doors]]
//! position = [7, 5]
//! group = "main"
//! ```

use serde::{Deserialize, Serialize};

use crate::coord::GridCoord;
use crate::group_id::GroupId;
use crate::level_definition::{
    BudgetRule, Button, DecayRule, Door, LevelDefinition, RuleSet, Switch,
};

/// A failure encoding or decoding a level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelCodecError {
    /// The TOML text could not be parsed into the level schema (syntax error, or
    /// a missing required field such as `[player]` or `[exit]`).
    Parse(String),
    /// The level could not be serialized to TOML.
    Serialize(String),
}

impl std::fmt::Display for LevelCodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LevelCodecError::Parse(m) => write!(f, "could not parse level TOML: {m}"),
            LevelCodecError::Serialize(m) => write!(f, "could not serialize level: {m}"),
        }
    }
}

impl std::error::Error for LevelCodecError {}

#[derive(Debug, Serialize, Deserialize)]
struct LevelDoc {
    title: String,
    width: u32,
    height: u32,
    player: PlayerDoc,
    exit: ExitDoc,
    #[serde(default)]
    walls: Vec<WallDoc>,
    #[serde(default)]
    buttons: Vec<WiredDoc>,
    #[serde(default)]
    doors: Vec<WiredDoc>,
    // Add-on placements + config. All default to empty/absent and are skipped on
    // serialize, so a level with no add-ons round-trips byte-identically to the
    // pre-add-on schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rules: Option<RulesDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    wells: Vec<WallDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    switches: Vec<WiredDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    crates: Vec<WallDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hazards: Vec<WallDoc>,
}

/// The `[rules]` table: which add-ons are enabled and their params.
#[derive(Debug, Default, Serialize, Deserialize)]
struct RulesDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decay: Option<DecayDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    budget: Option<BudgetDoc>,
    #[serde(default, skip_serializing_if = "is_false")]
    switches: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    crates: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    hazards: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct DecayDoc {
    lifetime_steps: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct BudgetDoc {
    max_ghosts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    par: Option<u32>,
}

/// `skip_serializing_if` predicate for a `false` flag (keeps default rules out of
/// the emitted TOML).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Serialize, Deserialize)]
struct PlayerDoc {
    start: [i32; 2],
}

#[derive(Debug, Serialize, Deserialize)]
struct ExitDoc {
    position: [i32; 2],
}

#[derive(Debug, Serialize, Deserialize)]
struct WallDoc {
    position: [i32; 2],
}

#[derive(Debug, Serialize, Deserialize)]
struct WiredDoc {
    position: [i32; 2],
    group: String,
}

fn coord(arr: [i32; 2]) -> GridCoord {
    GridCoord::new(arr[0], arr[1])
}

fn arr(coord: GridCoord) -> [i32; 2] {
    [coord.x, coord.y]
}

impl From<&LevelDefinition> for LevelDoc {
    fn from(level: &LevelDefinition) -> Self {
        LevelDoc {
            title: level.title.clone(),
            width: level.width,
            height: level.height,
            player: PlayerDoc {
                start: arr(level.entrance),
            },
            exit: ExitDoc {
                position: arr(level.exit),
            },
            walls: level
                .walls
                .iter()
                .map(|&c| WallDoc { position: arr(c) })
                .collect(),
            buttons: level
                .buttons
                .iter()
                .map(|b| WiredDoc {
                    position: arr(b.position),
                    group: b.group.as_str().to_string(),
                })
                .collect(),
            doors: level
                .doors
                .iter()
                .map(|d| WiredDoc {
                    position: arr(d.position),
                    group: d.group.as_str().to_string(),
                })
                .collect(),
            rules: rules_doc(&level.rules),
            wells: level
                .wells
                .iter()
                .map(|&c| WallDoc { position: arr(c) })
                .collect(),
            switches: level
                .switches
                .iter()
                .map(|s| WiredDoc {
                    position: arr(s.position),
                    group: s.group.as_str().to_string(),
                })
                .collect(),
            crates: level
                .crates
                .iter()
                .map(|&c| WallDoc { position: arr(c) })
                .collect(),
            hazards: level
                .hazards
                .iter()
                .map(|&c| WallDoc { position: arr(c) })
                .collect(),
        }
    }
}

/// `RuleSet` → `[rules]` doc, `None` when the rules are all-default (so a plain
/// level emits no `[rules]` table).
fn rules_doc(rules: &RuleSet) -> Option<RulesDoc> {
    (*rules != RuleSet::default()).then(|| RulesDoc {
        decay: rules.decay.map(|d| DecayDoc {
            lifetime_steps: d.lifetime_steps,
        }),
        budget: rules.budget.map(|b| BudgetDoc {
            max_ghosts: b.max_ghosts,
            par: b.par,
        }),
        switches: rules.switches,
        crates: rules.crates,
        hazards: rules.hazards,
    })
}

/// `[rules]` doc → `RuleSet` (all-default when the table is absent).
fn rule_set(doc: Option<RulesDoc>) -> RuleSet {
    doc.map(|d| RuleSet {
        decay: d.decay.map(|x| DecayRule {
            lifetime_steps: x.lifetime_steps,
        }),
        budget: d.budget.map(|x| BudgetRule {
            max_ghosts: x.max_ghosts,
            par: x.par,
        }),
        switches: d.switches,
        crates: d.crates,
        hazards: d.hazards,
    })
    .unwrap_or_default()
}

impl From<LevelDoc> for LevelDefinition {
    fn from(doc: LevelDoc) -> Self {
        LevelDefinition {
            title: doc.title,
            width: doc.width,
            height: doc.height,
            entrance: coord(doc.player.start),
            exit: coord(doc.exit.position),
            walls: doc.walls.into_iter().map(|w| coord(w.position)).collect(),
            buttons: doc
                .buttons
                .into_iter()
                .map(|b| Button {
                    position: coord(b.position),
                    group: GroupId::new(b.group),
                })
                .collect(),
            doors: doc
                .doors
                .into_iter()
                .map(|d| Door {
                    position: coord(d.position),
                    group: GroupId::new(d.group),
                })
                .collect(),
            wells: doc.wells.into_iter().map(|w| coord(w.position)).collect(),
            switches: doc
                .switches
                .into_iter()
                .map(|s| Switch {
                    position: coord(s.position),
                    group: GroupId::new(s.group),
                })
                .collect(),
            crates: doc.crates.into_iter().map(|c| coord(c.position)).collect(),
            hazards: doc.hazards.into_iter().map(|h| coord(h.position)).collect(),
            rules: rule_set(doc.rules),
        }
    }
}

/// Serialize a level to hand-editable TOML. Uses the compact serializer so
/// coordinate pairs stay on one line (`start = [1, 5]`) — the documented,
/// hand-editable shape — rather than the pretty serializer's one-int-per-line
/// arrays.
pub fn to_toml(level: &LevelDefinition) -> Result<String, LevelCodecError> {
    let doc = LevelDoc::from(level);
    toml::to_string(&doc).map_err(|e| LevelCodecError::Serialize(e.to_string()))
}

/// Parse a level from TOML text.
pub fn from_toml(text: &str) -> Result<LevelDefinition, LevelCodecError> {
    let doc: LevelDoc = toml::from_str(text).map_err(|e| LevelCodecError::Parse(e.to_string()))?;
    Ok(LevelDefinition::from(doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LevelDefinition {
        LevelDefinition {
            title: "Button Door".into(),
            width: 10,
            height: 10,
            entrance: GridCoord::new(1, 5),
            exit: GridCoord::new(8, 5),
            walls: vec![GridCoord::new(0, 0), GridCoord::new(0, 1)],
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
    fn round_trips_through_toml() {
        let level = sample();
        let text = to_toml(&level).expect("serializes");
        let back = from_toml(&text).expect("parses");
        assert_eq!(level, back);
    }

    #[test]
    fn emits_the_documented_schema() {
        let text = to_toml(&sample()).expect("serializes");
        assert!(text.contains("title = \"Button Door\""));
        assert!(text.contains("[player]"));
        assert!(text.contains("start = [1, 5]"));
        assert!(text.contains("[exit]"));
        assert!(text.contains("position = [8, 5]"));
        assert!(text.contains("[[walls]]"));
        assert!(text.contains("[[buttons]]"));
        assert!(text.contains("group = \"main\""));
    }

    #[test]
    fn missing_required_section_is_a_parse_error() {
        let text = "title=\"x\"\nwidth=10\nheight=10\n[player]\nstart=[1,5]\n";
        let err = from_toml(text).unwrap_err();
        assert!(matches!(err, LevelCodecError::Parse(_)));
    }

    #[test]
    fn parses_a_level_with_no_walls_or_objects() {
        let text =
            "title=\"bare\"\nwidth=3\nheight=3\n[player]\nstart=[0,0]\n[exit]\nposition=[2,2]\n";
        let level = from_toml(text).expect("parses");
        assert!(level.walls.is_empty());
        assert!(level.buttons.is_empty());
        assert_eq!(level.exit, GridCoord::new(2, 2));
    }
}
