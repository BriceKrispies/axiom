//! **Compiled traffic**: the concrete, immutable vehicle plans the runtime
//! activates.
//!
//! Ambient flow ([`flow`]) and authored encounters ([`encounters`]) both compile
//! to the same [`TrafficPlan`], and by the time the game runs there is nothing
//! but a list of them sorted by spawn distance. The runtime does not know which
//! vehicle came from a density description and which from a zipper, and must
//! not: "where is the next car" has to be one question with one answer.
//!
//! A plan carries everything needed to *reproduce* a vehicle's intended
//! behaviour: where it appears, in which lane, how fast, what shape it is, what
//! lane changes and speed changes it will make, and which encounter (if any)
//! owns it. Nothing about a vehicle is decided at spawn time — spawning reads
//! the plan, and that is what makes a reset reproduce the same road rather than
//! a statistically similar one.

pub mod encounters;
pub mod flow;

use crate::course::specification::{
    EncounterId, PassingSide, ScalarRange, VehicleArchetype, VehicleId,
};

/// A lane change a compiled vehicle will make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneChange {
    /// The vehicle's own course distance at which it starts moving (m).
    pub at_m: f32,
    /// The lane it moves to.
    pub to_lane: i32,
}

/// A change of cruising speed a compiled vehicle will make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedChange {
    /// The vehicle's own course distance at which the new speed applies (m).
    pub at_m: f32,
    /// The speed it settles to (m/s).
    pub to_mps: f32,
}

/// One concrete vehicle, fully determined before the game starts.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficPlan {
    /// Stable identity. Dense, and ordered by [`Self::spawn_m`].
    pub id: VehicleId,
    /// Where along the course the vehicle is placed when it activates (m).
    pub spawn_m: f32,
    /// The vehicle's own distance past which it is retired regardless of where
    /// the player is (m) — a backstop, since the ordinary retirement is the
    /// player leaving it behind.
    pub despawn_m: f32,
    /// The lane it holds at spawn.
    pub lane: i32,
    /// Its cruising speed at spawn (m/s).
    pub speed_mps: f32,
    /// Its shape.
    pub archetype: VehicleArchetype,
    /// Lane changes, in ascending distance order.
    pub lane_changes: Vec<LaneChange>,
    /// Speed changes, in ascending distance order.
    pub speed_changes: Vec<SpeedChange>,
    /// The encounter that placed it, if any.
    pub encounter: Option<EncounterId>,
    /// The section it was compiled in.
    pub section: u16,
    /// The seed its cosmetic variation (in-lane wander) is derived from.
    pub variation_seed: u64,
}

impl TrafficPlan {
    /// The lane this vehicle holds once it has driven `distance_m` along the
    /// course. Lane changes are ordered, so this is the last one it has passed.
    pub fn lane_at(&self, distance_m: f32) -> i32 {
        self.lane_changes
            .iter()
            .filter(|c| c.at_m <= distance_m)
            .next_back()
            .map(|c| c.to_lane)
            .unwrap_or(self.lane)
    }

    /// The cruising speed this vehicle holds at `distance_m`.
    pub fn speed_at(&self, distance_m: f32) -> f32 {
        self.speed_changes
            .iter()
            .filter(|c| c.at_m <= distance_m)
            .next_back()
            .map(|c| c.to_mps)
            .unwrap_or(self.speed_mps)
    }
}

/// One compiled encounter: its extent, the vehicles it owns and what it demands
/// of validation.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEncounter {
    /// Stable identity.
    pub id: EncounterId,
    /// Which template produced it.
    pub kind: &'static str,
    /// The section it starts in.
    pub section: u16,
    /// Where it starts along the course (m).
    pub start_m: f32,
    /// Where it ends (m).
    pub end_m: f32,
    /// The vehicles it placed, in course order.
    pub vehicles: Vec<VehicleId>,
    /// Whether validation must prove a continuous route through it.
    pub requires_route: bool,
    /// The least warning the player may be given (s).
    pub minimum_reaction_time_s: f32,
    /// The lateral gap the route is meant to leave (m).
    pub lateral_clearance_m: f32,
    /// How many near-miss opportunities the figure is meant to offer.
    pub target_near_misses: u32,
}

/// A compiled near-miss **opportunity**.
///
/// It awards nothing. The runtime's `sim::collision::is_near_miss` decides what
/// the player actually earned; this says where the course intended the chances
/// to be, which is what the boost-sustain analysis measures and what the
/// authoring overlay shows.
#[derive(Debug, Clone, PartialEq)]
pub struct NearMissWindow {
    /// The encounter that compiled it, if any.
    pub encounter: Option<EncounterId>,
    /// Where the window opens (m).
    pub start_m: f32,
    /// Where it closes (m).
    pub end_m: f32,
    /// The vehicles the opportunity is against.
    pub vehicles: Vec<VehicleId>,
    /// The clearance band the pass is meant to happen in (m).
    pub clearance_m: ScalarRange,
    /// Which side the pass is meant to be on.
    pub side: PassingSide,
    /// The least relative speed that counts (m/s).
    pub minimum_relative_speed_mps: f32,
    /// How many opportunities the window offers.
    pub intended_opportunities: u32,
    /// How hard it is, `0..1`.
    pub difficulty_weight: f32,
    /// The section it falls in.
    pub section: u16,
}

impl NearMissWindow {
    /// Whether `distance_m` falls inside the window.
    pub fn contains(&self, distance_m: f32) -> bool {
        (distance_m >= self.start_m) & (distance_m <= self.end_m)
    }
}

/// Where the player actually meets a vehicle that spawns at `spawn_m`.
///
/// This is the one projection the whole system shares, and it is worth stating
/// once rather than three times. A vehicle is placed at `spawn_m` at the moment
/// the player is `horizon_m` short of it; from then on the player closes at
/// `expected_speed - vehicle_speed`, and both keep moving. Solving for where
/// they are level:
///
/// ```text
/// meet = spawn_m + vehicle_speed · horizon / (expected_speed − vehicle_speed)
/// ```
///
/// A vehicle at or above the expected player speed is never caught, and the
/// meeting point is reported as the end of the course.
pub fn meeting_distance(
    spawn_m: f32,
    vehicle_speed_mps: f32,
    horizon_m: f32,
    expected_speed_mps: f32,
    course_length_m: f32,
) -> f32 {
    let closing = expected_speed_mps - vehicle_speed_mps;
    (closing > 0.1)
        .then(|| spawn_m + vehicle_speed_mps * horizon_m / closing)
        .unwrap_or(course_length_m)
        .clamp(0.0, course_length_m)
}

/// How much road a near-miss opportunity covers either side of the meeting
/// point (m).
///
/// The projection above assumes the player holds exactly the expected speed for
/// the whole approach, which no player does. This is the honest slop around it.
pub const MEETING_WINDOW_M: f32 = 45.0;

/// The vehicle's own distance past which its plan is retired regardless of the
/// player (m).
///
/// A backstop, not the ordinary retirement: at the expected closing speeds a
/// vehicle is left behind long before it has driven this far. It exists so a
/// vehicle whose player never arrives (a stalled run, a reset backwards) still
/// has an end.
pub const PLAN_LIFETIME_M: f32 = 1_600.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> TrafficPlan {
        TrafficPlan {
            id: VehicleId(3),
            spawn_m: 400.0,
            despawn_m: 2_000.0,
            lane: 1,
            speed_mps: 30.0,
            archetype: VehicleArchetype::Saloon,
            lane_changes: vec![
                LaneChange { at_m: 500.0, to_lane: 0 },
                LaneChange { at_m: 600.0, to_lane: -1 },
            ],
            speed_changes: vec![SpeedChange { at_m: 550.0, to_mps: 24.0 }],
            encounter: None,
            section: 2,
            variation_seed: 77,
        }
    }

    #[test]
    fn a_plan_resolves_its_lane_and_speed_at_any_distance() {
        let p = plan();
        assert_eq!(p.lane_at(400.0), 1, "the spawn lane holds until the first change");
        assert_eq!(p.lane_at(499.9), 1);
        assert_eq!(p.lane_at(500.0), 0);
        assert_eq!(p.lane_at(599.0), 0);
        assert_eq!(p.lane_at(600.0), -1);
        assert_eq!(p.lane_at(9_000.0), -1, "the last change is the final lane");

        assert_eq!(p.speed_at(400.0), 30.0);
        assert_eq!(p.speed_at(549.0), 30.0);
        assert_eq!(p.speed_at(550.0), 24.0);
        assert_eq!(p.speed_at(9_000.0), 24.0);
    }

    #[test]
    fn a_plan_with_no_changes_holds_what_it_spawned_with() {
        let p = TrafficPlan {
            lane_changes: Vec::new(),
            speed_changes: Vec::new(),
            ..plan()
        };
        assert_eq!(p.lane_at(0.0), 1);
        assert_eq!(p.lane_at(9_000.0), 1);
        assert_eq!(p.speed_at(9_000.0), 30.0);
    }

    #[test]
    fn the_meeting_projection_puts_a_slower_car_ahead_of_where_it_spawned() {
        // Player at 80 m/s, traffic at 30, spawned 620 m ahead: they are level
        // 372 m past the spawn point.
        let meet = meeting_distance(1_000.0, 30.0, 620.0, 80.0, 9_000.0);
        assert!((meet - 1_372.0).abs() < 1.0, "met at {meet}");
        // A car as fast as the player is never caught.
        assert_eq!(meeting_distance(1_000.0, 80.0, 620.0, 80.0, 9_000.0), 9_000.0);
        assert_eq!(meeting_distance(1_000.0, 95.0, 620.0, 80.0, 9_000.0), 9_000.0);
        // A stationary obstacle is met exactly where it stands.
        assert_eq!(meeting_distance(1_000.0, 0.0, 620.0, 80.0, 9_000.0), 1_000.0);
        // And nothing is ever projected off the end of the course.
        let far = meeting_distance(8_900.0, 60.0, 620.0, 80.0, 9_000.0);
        assert!((0.0..=9_000.0).contains(&far));
    }

    #[test]
    fn a_near_miss_window_answers_whether_a_distance_is_inside_it() {
        let w = NearMissWindow {
            encounter: Some(EncounterId(1)),
            start_m: 100.0,
            end_m: 200.0,
            vehicles: vec![VehicleId(1)],
            clearance_m: ScalarRange::new(0.4, 1.4),
            side: PassingSide::Either,
            minimum_relative_speed_mps: 8.0,
            intended_opportunities: 2,
            difficulty_weight: 0.5,
            section: 0,
        };
        assert!(w.contains(100.0));
        assert!(w.contains(150.0));
        assert!(w.contains(200.0));
        assert!(!w.contains(99.0));
        assert!(!w.contains(201.0));
    }
}
