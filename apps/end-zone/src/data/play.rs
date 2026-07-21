//! Play definitions: the smallest reusable model of an arcade-football play.
//! A play names its formations, per-slot assignments, routes, possession, and
//! drive direction. Routes are deterministic offense-relative segment shapes
//! compiled to waypoint chains — explicit football-domain data interpreted by
//! small state machines, not a scripting language.

use crate::config::PLAYERS_PER_TEAM;
use crate::field::{DriveDirection, OffensePoint};
use crate::identity::{PlayId, TeamId};

use super::formation::{base_defense, spread_offense, FormationDefinition};

/// A named route shape. Depths/widths are yards in the offense frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RouteShape {
    /// Straight downfield.
    Straight { depth: f32 },
    /// Break inside at 45° after the stem.
    Slant { stem: f32, cut: f32 },
    /// Break to the sideline after the stem.
    Out { stem: f32, cut: f32 },
    /// Break to the middle after the stem.
    In { stem: f32, cut: f32 },
    /// Stem, then break deep toward the sideline corner.
    Corner { stem: f32, cut: f32 },
    /// Stem, then break deep toward the middle.
    Post { stem: f32, cut: f32 },
    /// Stem, then hook back toward the quarterback.
    Curl { stem: f32, back: f32 },
}

/// A route: a shape (or explicit waypoints) run from the receiver's alignment.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteDefinition {
    Shape(RouteShape),
    /// An explicit offense-relative waypoint chain.
    Waypoints(Vec<OffensePoint>),
}

impl RouteDefinition {
    /// Compile to the offense-relative waypoint chain, starting from the
    /// receiver's alignment `start`. `side` is the sign of the receiver's
    /// lateral alignment (breaks toward "out" use it so one shape serves both
    /// sides of the field).
    pub fn waypoints(&self, start: OffensePoint) -> Vec<OffensePoint> {
        let side = if start.lateral >= 0.0 { 1.0 } else { -1.0 };
        let p = |lateral: f32, downfield: f32| OffensePoint::new(lateral, downfield);
        match self {
            RouteDefinition::Waypoints(points) => points
                .iter()
                .map(|w| p(start.lateral + w.lateral, start.downfield + w.downfield))
                .collect(),
            RouteDefinition::Shape(shape) => match *shape {
                RouteShape::Straight { depth } => {
                    vec![p(start.lateral, start.downfield + depth)]
                }
                RouteShape::Slant { stem, cut } => vec![
                    p(start.lateral, start.downfield + stem),
                    p(start.lateral - side * cut, start.downfield + stem + cut),
                ],
                RouteShape::Out { stem, cut } => vec![
                    p(start.lateral, start.downfield + stem),
                    p(start.lateral + side * cut, start.downfield + stem),
                ],
                RouteShape::In { stem, cut } => vec![
                    p(start.lateral, start.downfield + stem),
                    p(start.lateral - side * cut, start.downfield + stem),
                ],
                RouteShape::Corner { stem, cut } => vec![
                    p(start.lateral, start.downfield + stem),
                    p(
                        start.lateral + side * cut,
                        start.downfield + stem + cut * 1.4,
                    ),
                ],
                RouteShape::Post { stem, cut } => vec![
                    p(start.lateral, start.downfield + stem),
                    p(
                        start.lateral - side * cut,
                        start.downfield + stem + cut * 1.4,
                    ),
                ],
                RouteShape::Curl { stem, back } => vec![
                    p(start.lateral, start.downfield + stem),
                    p(start.lateral, start.downfield + stem - back),
                ],
            },
        }
    }
}

/// One offensive slot's job for the play.
#[derive(Debug, Clone, PartialEq)]
pub enum OffenseAssignment {
    /// Take the snap and drop back `drop_depth` yards. The pass TARGET is not
    /// authored here: it is resolved at throw time from the quarterback's
    /// throwing cone (`crate::football::targeting`), so who the ball goes to
    /// depends on where he is facing, not on the play sheet.
    Quarterback { drop_depth: f32 },
    /// Snap the ball, then pass-block.
    Snapper,
    /// Run a pass route (the primary or a real option).
    Route(RouteDefinition),
    /// Run a route purely to pull coverage.
    DecoyRoute(RouteDefinition),
    /// Protect the passer.
    PassBlock,
    /// Lead upfield and block the nearest threat.
    LeadBlock,
    /// Carry the ball (used after a catch or handoff).
    BallCarry,
}

/// One defensive slot's job for the play.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DefenseAssignment {
    /// Man coverage on an offensive roster slot.
    ManCover { target_slot: usize },
    /// Guard an offense-relative zone.
    ZoneCover { center: OffensePoint, radius: f32 },
    /// Rush the quarterback.
    QuarterbackRush,
    /// Contain the edge at a lateral offset, then pursue.
    EdgeContain { lateral: f32 },
    /// Pursue the ball carrier.
    Pursuit,
    /// Close and tackle the carrier.
    TackleTarget,
}

/// A complete play definition.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayDefinition {
    pub id: PlayId,
    pub name: &'static str,
    pub offense_formation: FormationDefinition,
    pub defense_formation: FormationDefinition,
    pub offense_assignments: [OffenseAssignment; PLAYERS_PER_TEAM],
    pub defense_assignments: [DefenseAssignment; PLAYERS_PER_TEAM],
    /// Which team has the ball.
    pub possession: TeamId,
    pub drive_direction: DriveDirection,
    /// Line of scrimmage, yards from the offense's own goal line.
    pub line_of_scrimmage: f32,
}

/// The showcase play: spread formation, three live route runners, edge
/// rush, man coverage outside, free safety pursuing the catch.
pub fn showcase_play() -> PlayDefinition {
    PlayDefinition {
        id: PlayId(1),
        name: "SLOT POST",
        offense_formation: spread_offense(),
        defense_formation: base_defense(),
        offense_assignments: [
            // 0: QB drops and reads the cone — the target is whoever he faces.
            OffenseAssignment::Quarterback { drop_depth: 3.0 },
            OffenseAssignment::Snapper,
            OffenseAssignment::PassBlock,
            OffenseAssignment::PassBlock,
            OffenseAssignment::DecoyRoute(RouteDefinition::Shape(RouteShape::Straight {
                depth: 16.0,
            })),
            OffenseAssignment::Route(RouteDefinition::Shape(RouteShape::Out {
                stem: 8.0,
                cut: 5.0,
            })),
            // 6: the primary — slot post.
            OffenseAssignment::Route(RouteDefinition::Shape(RouteShape::Post {
                stem: 7.0,
                cut: 6.0,
            })),
        ],
        defense_assignments: [
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::EdgeContain { lateral: -4.5 },
            DefenseAssignment::EdgeContain { lateral: 4.5 },
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::ManCover { target_slot: 4 },
            DefenseAssignment::ManCover { target_slot: 5 },
            DefenseAssignment::Pursuit,
        ],
        possession: TeamId(0),
        drive_direction: DriveDirection::PlusZ,
        line_of_scrimmage: 35.0,
    }
}
