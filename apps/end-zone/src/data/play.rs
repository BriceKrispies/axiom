//! Play definitions: the smallest reusable model of an arcade-football play.
//! A play names its formations, per-slot assignments, routes, possession, and
//! drive direction. Routes are deterministic offense-relative segment shapes
//! compiled to waypoint chains — explicit football-domain data interpreted by
//! small state machines, not a scripting language.

use crate::config::PLAYERS_PER_TEAM;
use crate::field::{DriveDirection, OffensePoint};
use crate::identity::{PlayId, TeamId};

use super::formation::FormationDefinition;

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

/// What an offensive play is trying to do. The defensive selector reads this
/// tag to pick a sensible answer — it is the play's intent, not its mechanics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffenseTag {
    /// Timing/rhythm throws that beat pressure — slants, hitches, quick outs.
    QuickPass,
    /// Intermediate-to-deep route concepts that need protection to develop.
    DeepPass,
    /// A concept that floods one side of the field to outnumber the coverage.
    Flood,
}

/// A named offensive play: a formation plus each slot's job. This is the unit
/// the player selects in the huddle each down.
#[derive(Debug, Clone, PartialEq)]
pub struct OffensivePlay {
    pub id: PlayId,
    pub name: &'static str,
    pub tag: OffenseTag,
    pub formation: FormationDefinition,
    pub assignments: [OffenseAssignment; PLAYERS_PER_TEAM],
}

/// The defensive front a call presents at the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenseFront {
    /// A four-man front: a standard rush with balanced coverage behind it.
    Base,
    /// A lighter front trading a rusher for an extra defensive back.
    Nickel,
    /// A minimal rush dropping everyone into coverage — a prevent shell.
    Dime,
}

/// How a call defends the pass behind its front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Man coverage: defenders travel with assigned receivers.
    Man,
    /// Zone coverage: defenders guard areas and rally to the ball.
    Zone,
    /// An extra rusher, single coverage behind — high risk, high reward.
    Blitz,
}

/// A named defensive call: a formation plus each slot's job, tagged with the
/// front and coverage it presents. The game — never the player — selects one
/// deterministically in response to the offense (see [`crate::ai::playcall`]).
#[derive(Debug, Clone, PartialEq)]
pub struct DefensiveCall {
    pub name: &'static str,
    pub front: DefenseFront,
    pub coverage: Coverage,
    pub formation: FormationDefinition,
    pub assignments: [DefenseAssignment; PLAYERS_PER_TEAM],
}

/// A complete play definition: the composed offense-and-defense the simulation
/// lines up and runs for one down. Built by [`PlayDefinition::compose`] from a
/// selected [`OffensivePlay`] and a chosen [`DefensiveCall`].
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

impl PlayDefinition {
    /// Fuse a selected offensive play and a chosen defensive call into the
    /// runtime play the sim lines up. The play carries the offense's identity
    /// (id + name); the defensive call rides only in its formation/assignments.
    pub fn compose(
        offense: &OffensivePlay,
        defense: &DefensiveCall,
        possession: TeamId,
        drive_direction: DriveDirection,
        line_of_scrimmage: f32,
    ) -> Self {
        PlayDefinition {
            id: offense.id,
            name: offense.name,
            offense_formation: offense.formation.clone(),
            defense_formation: defense.formation.clone(),
            offense_assignments: offense.assignments.clone(),
            defense_assignments: defense.assignments,
            possession,
            drive_direction,
            line_of_scrimmage,
        }
    }
}
