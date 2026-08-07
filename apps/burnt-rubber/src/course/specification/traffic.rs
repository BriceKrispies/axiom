//! The authored traffic model: **ambient flow**, **choreographed encounters**
//! and **near-miss opportunity windows**.
//!
//! The split between the first two is the design. Ambient flow is a *statistical*
//! description of a stretch of road — this many cars per kilometre, this far
//! apart, distributed across these lanes — and the compiler turns it into
//! concrete vehicles by drawing from a seeded stream. An encounter is an
//! *authored figure*: a zipper, a rolling wall, a slalom. It states exactly what
//! it wants and the compiler places precisely those cars.
//!
//! Both compile to the **same** concrete
//! [`TrafficPlan`](crate::course::traffic::TrafficPlan). The runtime cannot tell
//! which a car came from, and does not need to: by the time the game runs there
//! is only a sorted list of vehicles with spawn distances.
//!
//! Near-miss windows are *opportunities*, never awards. Compiling one says "a
//! skilled player can earn a near miss here"; whether they did is
//! `sim::collision::is_near_miss`'s business and nothing here can reach it.

use crate::course::error::{
    finite, positive, CourseError, CourseErrorCode, CourseResult,
};

use super::units::{CountRange, ScalarRange};

/// A shape of traffic vehicle. Purely cosmetic — the collision box is one size
/// (`RaceTuning::traffic_half_length`/`traffic_half_width`) for every car,
/// because a game about judging gaps must not have gaps that are secretly
/// different sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VehicleArchetype {
    /// A saloon.
    Saloon,
    /// A hatchback.
    Hatch,
    /// A van.
    Van,
    /// A pickup.
    Pickup,
}

impl VehicleArchetype {
    /// Every archetype, in a stable order.
    pub const ALL: [VehicleArchetype; 4] = [
        VehicleArchetype::Saloon,
        VehicleArchetype::Hatch,
        VehicleArchetype::Van,
        VehicleArchetype::Pickup,
    ];

    /// The visual variant index the car renderer draws this as.
    pub const fn variant(self) -> u8 {
        match self {
            VehicleArchetype::Saloon => 0,
            VehicleArchetype::Hatch => 1,
            VehicleArchetype::Van => 2,
            VehicleArchetype::Pickup => 3,
        }
    }

    /// The DSL token.
    pub const fn token(self) -> &'static str {
        match self {
            VehicleArchetype::Saloon => "saloon",
            VehicleArchetype::Hatch => "hatch",
            VehicleArchetype::Van => "van",
            VehicleArchetype::Pickup => "pickup",
        }
    }

    /// Resolve a DSL token.
    pub fn parse(token: &str) -> Option<VehicleArchetype> {
        VehicleArchetype::ALL.into_iter().find(|a| a.token() == token)
    }
}

/// A weight attached to a lane index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneWeight {
    /// The lane, numbered out from the centreline.
    pub lane: i32,
    /// How likely this lane is, relative to the others. Non-negative.
    pub weight: f32,
}

/// Everything a stretch of road says about its traffic.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrafficZoneSpec {
    /// The ambient flow, if this zone has any.
    pub flow: Option<TrafficFlowSpec>,
    /// Authored encounters inside the zone, in course order.
    pub encounters: Vec<EncounterSpec>,
    /// Explicit near-miss opportunity windows, beyond the ones encounters
    /// compile for themselves.
    pub near_miss_windows: Vec<NearMissWindowSpec>,
}

impl TrafficZoneSpec {
    /// Whether the zone asks for anything at all.
    pub fn is_empty(&self) -> bool {
        self.flow.is_none() & self.encounters.is_empty() & self.near_miss_windows.is_empty()
    }

    /// Reject an unbuildable zone.
    pub fn validate(&self, lane_reach: i32) -> CourseResult<()> {
        self.flow.as_ref().map(|f| f.validate(lane_reach)).transpose()?;
        self.encounters
            .iter()
            .try_for_each(|e| e.validate(lane_reach))?;
        self.near_miss_windows.iter().try_for_each(|w| w.validate())
    }
}

/// Ambient traffic as a **distance-based density description**.
///
/// Every field is per-metre or per-kilometre of *course*, never per second of
/// play. That is deliberate and it is the defect this model replaced: traffic
/// generated on a spawn timer puts more cars in front of a slow player and fewer
/// in front of a fast one, so the road a player meets depends on how well they
/// are driving, and a course cannot be authored at all.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficFlowSpec {
    /// Target vehicles per kilometre of course.
    pub vehicles_per_km: f32,
    /// The closest two consecutive vehicles may be along the course (m).
    pub min_headway_m: f32,
    /// The headway the generator aims for (m). Defaults to `1000/vehicles_per_km`.
    pub preferred_headway_m: f32,
    /// The furthest apart two consecutive vehicles may be (m).
    pub max_headway_m: f32,
    /// The speed band traffic cruises in (m/s).
    pub speed_mps: ScalarRange,
    /// Blend toward the section's expected player speed, `0..1`. At `0` the
    /// speed band is absolute; at `1` it is scaled so that the *same* relative
    /// closing speed is presented on a fast section as on a slow one.
    pub speed_relative_to_expected: f32,
    /// How likely each lane is. Empty means "every lane the road has, evenly".
    pub lane_weights: Vec<LaneWeight>,
    /// Probability that a vehicle starts a platoon rather than standing alone.
    pub platoon_probability: f32,
    /// How many vehicles a platoon holds (including the leader).
    pub platoon_size: CountRange,
    /// The gap inside a platoon (m).
    pub platoon_gap_m: f32,
    /// How much road a dense burst covers before the flow relaxes (m).
    pub burst_length_m: f32,
    /// How much road the flow relaxes over after a burst (m).
    pub recovery_length_m: f32,
    /// How often a deliberately empty corridor is left (m of course between
    /// corridors).
    pub open_corridor_every_m: ScalarRange,
    /// How long an open corridor is (m).
    pub open_corridor_length_m: f32,
    /// Relative likelihood of each vehicle shape.
    pub archetype_weights: Vec<(VehicleArchetype, f32)>,
}

impl TrafficFlowSpec {
    /// A flow at `vehicles_per_km`, with every other field taking a sensible
    /// value derived from it. Authoring one number is the common case.
    pub fn at_density(vehicles_per_km: f32) -> TrafficFlowSpec {
        let preferred = 1_000.0 / vehicles_per_km.max(0.1);
        TrafficFlowSpec {
            vehicles_per_km,
            min_headway_m: preferred * 0.45,
            preferred_headway_m: preferred,
            max_headway_m: preferred * 1.55,
            speed_mps: ScalarRange::new(22.0, 38.0),
            speed_relative_to_expected: 0.0,
            lane_weights: Vec::new(),
            platoon_probability: 0.0,
            platoon_size: CountRange::new(2, 3),
            platoon_gap_m: 26.0,
            burst_length_m: 0.0,
            recovery_length_m: 0.0,
            open_corridor_every_m: ScalarRange::new(f32::MAX, f32::MAX),
            open_corridor_length_m: 0.0,
            archetype_weights: Vec::new(),
        }
    }

    /// The lane weights this flow actually uses at a road with `lane_reach`,
    /// with an empty authored list meaning "every lane, evenly".
    pub fn resolved_lane_weights(&self, lane_reach: i32) -> Vec<LaneWeight> {
        self.lane_weights
            .is_empty()
            .then(|| {
                (-lane_reach..=lane_reach)
                    .map(|lane| LaneWeight { lane, weight: 1.0 })
                    .collect()
            })
            .unwrap_or_else(|| {
                self.lane_weights
                    .iter()
                    .copied()
                    .filter(|w| w.lane.abs() <= lane_reach)
                    .collect()
            })
    }

    /// The archetype weights this flow actually uses, with an empty authored
    /// list meaning "every shape, evenly".
    pub fn resolved_archetype_weights(&self) -> Vec<(VehicleArchetype, f32)> {
        self.archetype_weights
            .is_empty()
            .then(|| VehicleArchetype::ALL.map(|a| (a, 1.0)).to_vec())
            .unwrap_or_else(|| self.archetype_weights.clone())
    }

    /// Reject an unbuildable flow.
    pub fn validate(&self, lane_reach: i32) -> CourseResult<()> {
        positive(
            self.vehicles_per_km,
            "vehicles_per_km",
            CourseErrorCode::InvalidHeadwayRange,
        )?;
        positive(
            self.min_headway_m,
            "min_headway_m",
            CourseErrorCode::InvalidHeadwayRange,
        )?;
        positive(
            self.preferred_headway_m,
            "preferred_headway_m",
            CourseErrorCode::InvalidHeadwayRange,
        )?;
        positive(
            self.max_headway_m,
            "max_headway_m",
            CourseErrorCode::InvalidHeadwayRange,
        )?;
        ((self.min_headway_m <= self.preferred_headway_m)
            & (self.preferred_headway_m <= self.max_headway_m))
            .then_some(())
            .ok_or_else(|| {
                CourseError::new(
                    CourseErrorCode::InvalidHeadwayRange,
                    format!(
                        "headway must be ordered min <= preferred <= max, got {} <= {} <= {}",
                        self.min_headway_m, self.preferred_headway_m, self.max_headway_m
                    ),
                )
                .in_field("headway")
            })?;
        self.speed_mps.validate("speed_mps", true)?;
        finite(self.speed_relative_to_expected, "speed_relative_to_expected")?;
        finite(self.platoon_probability, "platoon_probability")?;
        finite(self.platoon_gap_m, "platoon_gap_m")?;
        finite(self.burst_length_m, "burst_length_m")?;
        finite(self.recovery_length_m, "recovery_length_m")?;
        finite(self.open_corridor_length_m, "open_corridor_length_m")?;
        (self.open_corridor_every_m.lo > 0.0)
            .then_some(())
            .ok_or_else(|| {
                CourseError::new(
                    CourseErrorCode::InvalidHeadwayRange,
                    format!(
                        "`open_corridor_every` must be positive, got {}",
                        self.open_corridor_every_m.lo
                    ),
                )
                .in_field("open_corridor_every")
            })?;

        let weights = self.resolved_lane_weights(lane_reach);
        validate_lane_weights(&weights, lane_reach)?;
        let archetypes = self.resolved_archetype_weights();
        let total: f32 = archetypes.iter().map(|(_, w)| w.max(0.0)).sum();
        (total > 0.0).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidLaneWeights,
                "archetype weights sum to zero — no vehicle shape can be drawn".to_string(),
            )
            .in_field("archetype_weights")
        })?;
        Ok(())
    }
}

/// Reject lane weights that name a lane the road does not have, are negative,
/// or cannot be drawn from.
pub fn validate_lane_weights(weights: &[LaneWeight], lane_reach: i32) -> CourseResult<()> {
    (!weights.is_empty()).then_some(()).ok_or_else(|| {
        CourseError::new(
            CourseErrorCode::InvalidLaneWeights,
            "no lane is available to place traffic in".to_string(),
        )
        .in_field("lane_weights")
    })?;
    weights.iter().try_for_each(|w| {
        finite(w.weight, "lane_weight")?;
        (w.weight >= 0.0).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidLaneWeights,
                format!("lane {} has a negative weight of {}", w.lane, w.weight),
            )
            .in_field("lane_weights")
        })?;
        (w.lane.abs() <= lane_reach).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidEncounterLane,
                format!(
                    "lane {} does not exist on a road reaching {lane_reach} lanes either side",
                    w.lane
                ),
            )
            .in_field("lane_weights")
        })
    })?;
    let total: f32 = weights.iter().map(|w| w.weight).sum();
    (total > 0.0).then_some(()).ok_or_else(|| {
        CourseError::new(
            CourseErrorCode::InvalidLaneWeights,
            "lane weights sum to zero — no lane can be drawn".to_string(),
        )
        .in_field("lane_weights")
    })
}

/// One authored traffic figure.
#[derive(Debug, Clone, PartialEq)]
pub enum EncounterSpec {
    /// Alternating blockers leaving a weaving route.
    Zipper(ZipperSpec),
    /// A moving wall of traffic with one opening.
    RollingWall(RollingWallSpec),
    /// A line of alternating single blockers.
    Slalom(SlalomSpec),
}

impl EncounterSpec {
    /// The DSL token / dump keyword.
    pub const fn token(&self) -> &'static str {
        match self {
            EncounterSpec::Zipper(_) => "zipper",
            EncounterSpec::RollingWall(_) => "rolling_wall",
            EncounterSpec::Slalom(_) => "slalom",
        }
    }

    /// Where in the zone the encounter starts (m from the zone's start).
    pub const fn start_offset_m(&self) -> f32 {
        match self {
            EncounterSpec::Zipper(z) => z.start_offset_m,
            EncounterSpec::RollingWall(w) => w.start_offset_m,
            EncounterSpec::Slalom(s) => s.start_offset_m,
        }
    }

    /// How much road the encounter occupies (m).
    pub fn length_m(&self) -> f32 {
        match self {
            EncounterSpec::Zipper(z) => z.length_m,
            EncounterSpec::RollingWall(w) => w.phases as f32 * w.phase_length_m,
            EncounterSpec::Slalom(s) => {
                s.blockers.max(1) as f32 * s.spacing_m + s.recovery_gap_m
            }
        }
    }

    /// The speed the encounter's vehicles travel at (m/s).
    pub const fn speed_mps(&self) -> f32 {
        match self {
            EncounterSpec::Zipper(z) => z.speed_mps,
            EncounterSpec::RollingWall(w) => w.speed_mps,
            EncounterSpec::Slalom(s) => s.speed_mps,
        }
    }

    /// Reject an unbuildable encounter.
    pub fn validate(&self, lane_reach: i32) -> CourseResult<()> {
        match self {
            EncounterSpec::Zipper(z) => z.validate(lane_reach),
            EncounterSpec::RollingWall(w) => w.validate(lane_reach),
            EncounterSpec::Slalom(s) => s.validate(lane_reach),
        }
    }
}

/// **Zipper** — every lane but one is blocked, and the open lane alternates, so
/// the only way through is to weave.
#[derive(Debug, Clone, PartialEq)]
pub struct ZipperSpec {
    /// Where the encounter starts, from the start of its zone (m).
    pub start_offset_m: f32,
    /// How much road it covers (m).
    pub length_m: f32,
    /// Distance between one row of blockers and the next (m).
    pub spacing_m: f32,
    /// Speed of every blocker (m/s).
    pub speed_mps: f32,
    /// The lane that is open in the first row.
    pub first_open_lane: i32,
    /// Which way the opening walks between rows.
    pub alternation: super::road::TurnDirection,
    /// The lateral gap a player must be left, beyond the two half-widths (m).
    pub lateral_clearance_m: f32,
    /// How many near-miss opportunities the figure is meant to offer.
    pub target_near_misses: u32,
    /// The least time a player may be given to react to a row (s).
    pub minimum_reaction_time_s: f32,
    /// Whether validation must prove a continuous route through the figure.
    pub require_continuous_route: bool,
}

impl ZipperSpec {
    /// A zipper with sensible defaults for everything but its length.
    pub fn of_length(length_m: f32) -> ZipperSpec {
        ZipperSpec {
            start_offset_m: 0.0,
            length_m,
            spacing_m: 55.0,
            speed_mps: 30.0,
            first_open_lane: 0,
            alternation: super::road::TurnDirection::Right,
            lateral_clearance_m: 0.55,
            target_near_misses: 6,
            minimum_reaction_time_s: 0.75,
            require_continuous_route: true,
        }
    }

    fn validate(&self, lane_reach: i32) -> CourseResult<()> {
        positive(self.length_m, "length_m", CourseErrorCode::InvalidSectionLength)?;
        positive(self.spacing_m, "spacing_m", CourseErrorCode::InvalidHeadwayRange)?;
        positive(self.speed_mps, "speed_mps", CourseErrorCode::InvalidSpeedRange)?;
        positive(
            self.lateral_clearance_m,
            "minimum_clearance",
            CourseErrorCode::ImpossibleLateralClearance,
        )?;
        positive(
            self.minimum_reaction_time_s,
            "minimum_reaction_time",
            CourseErrorCode::ImpossibleReactionTime,
        )?;
        finite(self.start_offset_m, "start_offset_m")?;
        lane_exists(self.first_open_lane, lane_reach, "first_open_lane")
    }
}

/// **Rolling wall** — a group of vehicles occupies most of the road and the
/// opening moves between phases.
#[derive(Debug, Clone, PartialEq)]
pub struct RollingWallSpec {
    /// Where the encounter starts, from the start of its zone (m).
    pub start_offset_m: f32,
    /// How many lanes the wall occupies.
    pub wall_width_lanes: u32,
    /// The lane that is open in the first phase.
    pub open_lane: i32,
    /// How far the opening moves each phase, in lanes (signed).
    pub opening_step_lanes: i32,
    /// How much road one phase covers (m) — the cadence at which the opening
    /// changes.
    pub phase_length_m: f32,
    /// How many phases the wall runs for.
    pub phases: u32,
    /// Speed of every wall vehicle (m/s).
    pub speed_mps: f32,
    /// Along-course gap between rows inside one phase (m).
    pub group_spacing_m: f32,
    /// How much road the player must be able to see the wall over before
    /// reaching it (m).
    pub reaction_distance_m: f32,
}

impl RollingWallSpec {
    /// A wall of `phases` phases with sensible defaults.
    pub fn of_phases(phases: u32) -> RollingWallSpec {
        RollingWallSpec {
            start_offset_m: 0.0,
            wall_width_lanes: 2,
            open_lane: 1,
            opening_step_lanes: -1,
            phase_length_m: 140.0,
            phases,
            speed_mps: 30.0,
            group_spacing_m: 70.0,
            reaction_distance_m: 120.0,
        }
    }

    fn validate(&self, lane_reach: i32) -> CourseResult<()> {
        positive(
            self.phase_length_m,
            "phase_length_m",
            CourseErrorCode::InvalidSectionLength,
        )?;
        positive(
            self.group_spacing_m,
            "group_spacing_m",
            CourseErrorCode::InvalidHeadwayRange,
        )?;
        positive(self.speed_mps, "speed_mps", CourseErrorCode::InvalidSpeedRange)?;
        positive(
            self.reaction_distance_m,
            "reaction_distance_m",
            CourseErrorCode::ImpossibleReactionTime,
        )?;
        finite(self.start_offset_m, "start_offset_m")?;
        (self.phases > 0).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidSectionLength,
                "a rolling wall with no phases is not an encounter".to_string(),
            )
            .in_field("phases")
        })?;
        let lanes = (lane_reach * 2 + 1) as u32;
        ((self.wall_width_lanes >= 1) & (self.wall_width_lanes < lanes))
            .then_some(())
            .ok_or_else(|| {
                CourseError::new(
                    CourseErrorCode::ImpossibleLateralClearance,
                    format!(
                        "a wall {} lanes wide leaves no opening on a {lanes}-lane road",
                        self.wall_width_lanes
                    ),
                )
                .in_field("wall_width")
            })?;
        lane_exists(self.open_lane, lane_reach, "open_lane")?;
        // Every phase's opening has to land on a lane that exists, or the wall
        // closes completely partway through.
        (0..self.phases).try_for_each(|phase| {
            let lane = self.open_lane + self.opening_step_lanes * phase as i32;
            lane_exists(lane, lane_reach, "open_lane")
        })
    }

    /// The lane that is open during `phase`.
    pub fn open_lane_for(&self, phase: u32) -> i32 {
        self.open_lane + self.opening_step_lanes * phase as i32
    }
}

/// **Slalom** — single blockers on alternating sides, spaced so a clean line
/// through them is a rhythm.
#[derive(Debug, Clone, PartialEq)]
pub struct SlalomSpec {
    /// Where the encounter starts, from the start of its zone (m).
    pub start_offset_m: f32,
    /// How many blockers.
    pub blockers: u32,
    /// Along-course spacing between blockers (m).
    pub spacing_m: f32,
    /// The lanes the blockers cycle through, in order.
    pub lane_sequence: Vec<i32>,
    /// Speed of every blocker (m/s).
    pub speed_mps: f32,
    /// Lateral clearance the route is meant to leave (m).
    pub clearance_m: f32,
    /// Clear road left after the last blocker (m).
    pub recovery_gap_m: f32,
}

impl SlalomSpec {
    /// A slalom of `blockers` alternating between the outer lanes.
    pub fn of_blockers(blockers: u32) -> SlalomSpec {
        SlalomSpec {
            start_offset_m: 0.0,
            blockers,
            spacing_m: 65.0,
            lane_sequence: vec![-1, 1],
            speed_mps: 30.0,
            clearance_m: 0.8,
            recovery_gap_m: 120.0,
        }
    }

    fn validate(&self, lane_reach: i32) -> CourseResult<()> {
        positive(self.spacing_m, "spacing_m", CourseErrorCode::InvalidHeadwayRange)?;
        positive(self.speed_mps, "speed_mps", CourseErrorCode::InvalidSpeedRange)?;
        positive(
            self.clearance_m,
            "clearance",
            CourseErrorCode::ImpossibleLateralClearance,
        )?;
        finite(self.recovery_gap_m, "recovery_gap_m")?;
        finite(self.start_offset_m, "start_offset_m")?;
        (self.blockers > 0).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidSectionLength,
                "a slalom with no blockers is not an encounter".to_string(),
            )
            .in_field("blockers")
        })?;
        (!self.lane_sequence.is_empty()).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidEncounterLane,
                "a slalom needs at least one lane in its sequence".to_string(),
            )
            .in_field("lane_sequence")
        })?;
        self.lane_sequence
            .iter()
            .try_for_each(|lane| lane_exists(*lane, lane_reach, "lane_sequence"))
    }
}

/// A compiled-ahead **opportunity** for a near miss.
///
/// This never awards anything. It says: between these distances, passing one of
/// these vehicles on this side at this clearance and this relative speed is the
/// pass the course was designed around. The scoring system decides whether the
/// player actually made it.
#[derive(Debug, Clone, PartialEq)]
pub struct NearMissWindowSpec {
    /// Where the window opens, from the start of its zone (m).
    pub start_offset_m: f32,
    /// How much road it covers (m).
    pub length_m: f32,
    /// The clearance band the pass is meant to happen in (m).
    pub clearance_m: ScalarRange,
    /// Which side the pass is meant to be on.
    pub side: PassingSide,
    /// The least relative speed that counts (m/s).
    pub minimum_relative_speed_mps: f32,
    /// How many opportunities the window is meant to offer.
    pub intended_opportunities: u32,
    /// How much of this window a skilled route is expected to convert, `0..1`.
    ///
    /// Higher is easier: a chance on open road converts reliably, a chance
    /// inside a zipper does not. It is what the boost budget weights the
    /// window's chances by.
    pub difficulty_weight: f32,
}

impl NearMissWindowSpec {
    /// Reject an unbuildable window.
    pub fn validate(&self) -> CourseResult<()> {
        positive(self.length_m, "length_m", CourseErrorCode::InvalidSectionLength)?;
        finite(self.start_offset_m, "start_offset_m")?;
        self.clearance_m.validate("clearance", true)?;
        finite(
            self.minimum_relative_speed_mps,
            "minimum_relative_speed_mps",
        )?;
        finite(self.difficulty_weight, "difficulty_weight")?;
        Ok(())
    }
}

/// Which side of a vehicle a pass is meant to happen on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassingSide {
    /// The player's left.
    Left,
    /// The player's right.
    Right,
    /// Either.
    Either,
}

impl PassingSide {
    /// The DSL token.
    pub const fn token(self) -> &'static str {
        match self {
            PassingSide::Left => "left",
            PassingSide::Right => "right",
            PassingSide::Either => "either",
        }
    }

    /// Resolve a DSL token.
    pub fn parse(token: &str) -> Option<PassingSide> {
        match token {
            "left" => Some(PassingSide::Left),
            "right" => Some(PassingSide::Right),
            "either" => Some(PassingSide::Either),
            _ => None,
        }
    }

    /// Whether a pass with the vehicle at `lane_delta` lanes from the player
    /// satisfies this side. Positive delta = the vehicle is to the player's
    /// right.
    pub fn accepts(self, lane_delta: i32) -> bool {
        match self {
            PassingSide::Left => lane_delta > 0,
            PassingSide::Right => lane_delta < 0,
            PassingSide::Either => lane_delta != 0,
        }
    }
}

/// Reject a lane the road does not have.
fn lane_exists(lane: i32, lane_reach: i32, field: &str) -> CourseResult<()> {
    (lane.abs() <= lane_reach).then_some(()).ok_or_else(|| {
        CourseError::new(
            CourseErrorCode::InvalidEncounterLane,
            format!(
                "lane {lane} does not exist on a road reaching {lane_reach} lanes either side \
                 of the centreline"
            ),
        )
        .in_field(field)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::road::TurnDirection;

    #[test]
    fn a_density_derives_a_consistent_headway_band() {
        let flow = TrafficFlowSpec::at_density(24.0);
        assert!((flow.preferred_headway_m - 1_000.0 / 24.0).abs() < 1.0e-3);
        assert!(flow.min_headway_m < flow.preferred_headway_m);
        assert!(flow.max_headway_m > flow.preferred_headway_m);
        assert!(flow.validate(2).is_ok());
    }

    #[test]
    fn an_empty_lane_or_archetype_list_means_everything_evenly() {
        let flow = TrafficFlowSpec::at_density(12.0);
        let lanes = flow.resolved_lane_weights(2);
        assert_eq!(lanes.len(), 5);
        assert_eq!(lanes.iter().map(|w| w.lane).collect::<Vec<_>>(), vec![-2, -1, 0, 1, 2]);
        assert!(lanes.iter().all(|w| w.weight == 1.0));
        let archetypes = flow.resolved_archetype_weights();
        assert_eq!(archetypes.len(), VehicleArchetype::ALL.len());

        // An authored list is filtered to the lanes that exist, not clamped
        // onto them — clamping would silently double a lane's weight.
        let narrow = TrafficFlowSpec {
            lane_weights: vec![
                LaneWeight { lane: -2, weight: 1.0 },
                LaneWeight { lane: 0, weight: 3.0 },
            ],
            ..TrafficFlowSpec::at_density(12.0)
        };
        let resolved = narrow.resolved_lane_weights(1);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].lane, 0);
    }

    #[test]
    fn a_flow_with_bad_numbers_is_rejected_with_the_right_code() {
        let base = TrafficFlowSpec::at_density(12.0);
        let reversed = TrafficFlowSpec {
            min_headway_m: 90.0,
            preferred_headway_m: 40.0,
            ..base.clone()
        };
        assert_eq!(
            reversed.validate(2).unwrap_err().code,
            CourseErrorCode::InvalidHeadwayRange
        );
        let no_density = TrafficFlowSpec {
            vehicles_per_km: 0.0,
            ..base.clone()
        };
        assert_eq!(
            no_density.validate(2).unwrap_err().code,
            CourseErrorCode::InvalidHeadwayRange
        );
        let bad_speed = TrafficFlowSpec {
            speed_mps: ScalarRange::new(40.0, 20.0),
            ..base.clone()
        };
        assert_eq!(
            bad_speed.validate(2).unwrap_err().code,
            CourseErrorCode::InvalidSpeedRange
        );
        let bad_lane = TrafficFlowSpec {
            lane_weights: vec![LaneWeight { lane: 9, weight: 1.0 }],
            ..base.clone()
        };
        // Filtering removes the impossible lane, leaving nothing to draw from.
        assert_eq!(
            bad_lane.validate(2).unwrap_err().code,
            CourseErrorCode::InvalidLaneWeights
        );
        let negative_weight = TrafficFlowSpec {
            lane_weights: vec![LaneWeight { lane: 0, weight: -1.0 }],
            ..base.clone()
        };
        assert_eq!(
            negative_weight.validate(2).unwrap_err().code,
            CourseErrorCode::InvalidLaneWeights
        );
        let zero_weights = TrafficFlowSpec {
            lane_weights: vec![LaneWeight { lane: 0, weight: 0.0 }],
            ..base.clone()
        };
        assert_eq!(
            zero_weights.validate(2).unwrap_err().code,
            CourseErrorCode::InvalidLaneWeights
        );
        let no_archetype = TrafficFlowSpec {
            archetype_weights: vec![(VehicleArchetype::Van, 0.0)],
            ..base.clone()
        };
        assert_eq!(
            no_archetype.validate(2).unwrap_err().code,
            CourseErrorCode::InvalidLaneWeights
        );
        let no_corridor = TrafficFlowSpec {
            open_corridor_every_m: ScalarRange::new(0.0, 100.0),
            ..base
        };
        assert_eq!(
            no_corridor.validate(2).unwrap_err().code,
            CourseErrorCode::InvalidHeadwayRange
        );
    }

    #[test]
    fn lane_weights_are_checked_against_the_road_that_exists() {
        assert!(validate_lane_weights(&[LaneWeight { lane: 1, weight: 1.0 }], 2).is_ok());
        assert_eq!(
            validate_lane_weights(&[], 2).unwrap_err().code,
            CourseErrorCode::InvalidLaneWeights
        );
        assert_eq!(
            validate_lane_weights(&[LaneWeight { lane: 5, weight: 1.0 }], 2)
                .unwrap_err()
                .code,
            CourseErrorCode::InvalidEncounterLane
        );
        assert_eq!(
            validate_lane_weights(&[LaneWeight { lane: 0, weight: f32::NAN }], 2)
                .unwrap_err()
                .code,
            CourseErrorCode::InvalidFiniteScalar
        );
    }

    #[test]
    fn encounters_report_their_extent_and_reject_impossible_lanes() {
        let zipper = ZipperSpec::of_length(280.0);
        let spec = EncounterSpec::Zipper(zipper.clone());
        assert_eq!(spec.token(), "zipper");
        assert_eq!(spec.length_m(), 280.0);
        assert_eq!(spec.start_offset_m(), 0.0);
        assert_eq!(spec.speed_mps(), 30.0);
        assert!(spec.validate(1).is_ok());

        let off_road = EncounterSpec::Zipper(ZipperSpec {
            first_open_lane: 4,
            ..zipper.clone()
        });
        assert_eq!(
            off_road.validate(1).unwrap_err().code,
            CourseErrorCode::InvalidEncounterLane
        );
        let no_length = EncounterSpec::Zipper(ZipperSpec {
            length_m: 0.0,
            ..zipper.clone()
        });
        assert!(no_length.validate(1).is_err());
        let no_reaction = EncounterSpec::Zipper(ZipperSpec {
            minimum_reaction_time_s: 0.0,
            ..zipper.clone()
        });
        assert_eq!(
            no_reaction.validate(1).unwrap_err().code,
            CourseErrorCode::ImpossibleReactionTime
        );
        let no_clearance = EncounterSpec::Zipper(ZipperSpec {
            lateral_clearance_m: -1.0,
            ..zipper
        });
        assert_eq!(
            no_clearance.validate(1).unwrap_err().code,
            CourseErrorCode::ImpossibleLateralClearance
        );
    }

    #[test]
    fn a_rolling_wall_walks_its_opening_and_rejects_a_wall_with_no_way_through() {
        let wall = RollingWallSpec::of_phases(4);
        assert_eq!(wall.open_lane_for(0), 1);
        assert_eq!(wall.open_lane_for(1), 0);
        assert_eq!(wall.open_lane_for(2), -1);
        let spec = EncounterSpec::RollingWall(wall.clone());
        assert_eq!(spec.token(), "rolling_wall");
        assert_eq!(spec.length_m(), 4.0 * 140.0);
        // Phase 3 would open lane -2, which a reach-1 road does not have.
        assert_eq!(
            spec.validate(1).unwrap_err().code,
            CourseErrorCode::InvalidEncounterLane
        );
        assert!(EncounterSpec::RollingWall(RollingWallSpec {
            phases: 3,
            ..wall.clone()
        })
        .validate(1)
        .is_ok());
        let solid = EncounterSpec::RollingWall(RollingWallSpec {
            wall_width_lanes: 3,
            phases: 1,
            ..wall.clone()
        });
        assert_eq!(
            solid.validate(1).unwrap_err().code,
            CourseErrorCode::ImpossibleLateralClearance
        );
        let no_phases = EncounterSpec::RollingWall(RollingWallSpec {
            phases: 0,
            ..wall
        });
        assert!(no_phases.validate(1).is_err());
    }

    #[test]
    fn a_slalom_reports_its_extent_and_needs_a_lane_sequence() {
        let slalom = SlalomSpec::of_blockers(5);
        let spec = EncounterSpec::Slalom(slalom.clone());
        assert_eq!(spec.token(), "slalom");
        assert_eq!(spec.length_m(), 5.0 * 65.0 + 120.0);
        assert!(spec.validate(1).is_ok());
        assert!(EncounterSpec::Slalom(SlalomSpec {
            lane_sequence: Vec::new(),
            ..slalom.clone()
        })
        .validate(1)
        .is_err());
        assert!(EncounterSpec::Slalom(SlalomSpec {
            blockers: 0,
            ..slalom.clone()
        })
        .validate(1)
        .is_err());
        assert_eq!(
            EncounterSpec::Slalom(SlalomSpec {
                lane_sequence: vec![-3, 3],
                ..slalom
            })
            .validate(1)
            .unwrap_err()
            .code,
            CourseErrorCode::InvalidEncounterLane
        );
    }

    #[test]
    fn a_zone_validates_everything_it_holds() {
        let mut zone = TrafficZoneSpec::default();
        assert!(zone.is_empty());
        assert!(zone.validate(2).is_ok());
        zone.flow = Some(TrafficFlowSpec::at_density(20.0));
        zone.encounters.push(EncounterSpec::Zipper(ZipperSpec::of_length(200.0)));
        zone.near_miss_windows.push(NearMissWindowSpec {
            start_offset_m: 0.0,
            length_m: 200.0,
            clearance_m: ScalarRange::new(0.4, 1.4),
            side: PassingSide::Either,
            minimum_relative_speed_mps: 8.0,
            intended_opportunities: 4,
            difficulty_weight: 0.5,
        });
        assert!(!zone.is_empty());
        assert!(zone.validate(2).is_ok());
        zone.near_miss_windows[0].length_m = 0.0;
        assert!(zone.validate(2).is_err());
    }

    #[test]
    fn passing_sides_and_archetypes_round_trip_through_their_tokens() {
        for side in [PassingSide::Left, PassingSide::Right, PassingSide::Either] {
            assert_eq!(PassingSide::parse(side.token()), Some(side));
        }
        assert_eq!(PassingSide::parse("upwards"), None);
        assert!(PassingSide::Left.accepts(1));
        assert!(!PassingSide::Left.accepts(-1));
        assert!(PassingSide::Right.accepts(-1));
        assert!(!PassingSide::Right.accepts(1));
        assert!(PassingSide::Either.accepts(1));
        assert!(PassingSide::Either.accepts(-1));
        assert!(!PassingSide::Either.accepts(0));

        for a in VehicleArchetype::ALL {
            assert_eq!(VehicleArchetype::parse(a.token()), Some(a));
        }
        assert_eq!(VehicleArchetype::parse("tractor"), None);
        let variants: Vec<u8> = VehicleArchetype::ALL.iter().map(|a| a.variant()).collect();
        assert_eq!(variants, vec![0, 1, 2, 3]);
        assert!(variants
            .iter()
            .all(|v| *v < crate::sim::traffic::TRAFFIC_VARIANTS));
    }

    #[test]
    fn the_zipper_alternation_direction_is_carried_through() {
        let z = ZipperSpec {
            alternation: TurnDirection::Left,
            ..ZipperSpec::of_length(100.0)
        };
        assert_eq!(z.alternation.sign(), -1.0);
    }
}
