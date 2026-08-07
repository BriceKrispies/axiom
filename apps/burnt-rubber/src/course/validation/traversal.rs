//! **Traversability**: is there a route through this traffic at all?
//!
//! The model is a bounded distance–lane occupancy grid and a forward
//! reachability sweep over it. One axis is course distance, the other is lane;
//! a cell is blocked where a projected vehicle (expanded by the player's width
//! and the collision margin) sits, or where the road has no lane; and a
//! transition between adjacent columns is legal only if the player could
//! physically cross that many lanes in the time the column takes.
//!
//! ```text
//!  lane +2 │ · · ▓ ▓ · · · · ·
//!  lane +1 │ · · · ▓ ▓ · ▓ ▓ ·
//!  lane  0 │ ▓ ▓ · · ▓ ▓ ▓ · ·      ▓ = blocked   · = free
//!  lane −1 │ · · · · · ▓ · · ·
//!  lane −2 │ · ▓ ▓ · · · · ▓ ▓
//!          └────────────────────▶  course distance
//! ```
//!
//! This is deliberately **not** a driving model. It answers one question — does
//! a physically-possible lane sequence exist — and it answers it the same way
//! every time. It does not model braking, racing lines, drifting or the player
//! choosing to slow down (all of which only ever *add* routes), so a course it
//! passes may still be hard, and a course it fails is genuinely impossible for a
//! player holding the expected speed.
//!
//! # Why the sweep resets after a failure
//!
//! When the reachable set empties, the sweep records the blockage and then
//! restarts from every free lane in the next column. A validator that stopped at
//! the first wall would report one problem per run, and an author would fix it
//! only to find the next one. Restarting means a single pass lists every blocked
//! stretch on the course.

use crate::course::specification::ValidationThresholds;
use crate::course::traffic::TrafficPlan;
use crate::track::{Track, MAX_LANE_REACH};
use crate::tuning::{RaceTuning, VehicleTuning};

/// How the player and the traffic are sized for the grid.
#[derive(Debug, Clone, Copy)]
pub struct OccupancyModel {
    /// Along-course half-extent a contact needs (m): both half-lengths plus a
    /// margin.
    pub along_clearance_m: f32,
    /// Lateral half-extent a contact needs (m): both half-widths plus the
    /// authored margin.
    pub lateral_clearance_m: f32,
    /// How far ahead of the player a vehicle is placed when it activates (m).
    pub activation_horizon_m: f32,
    /// How far behind the player a vehicle survives (m).
    pub retire_behind_m: f32,
}

impl OccupancyModel {
    /// Resolve the model from the game's own tuning, so the validator measures
    /// the boxes the collision resolver actually uses.
    pub fn resolve(
        vehicle: &VehicleTuning,
        race: &RaceTuning,
        thresholds: &ValidationThresholds,
    ) -> OccupancyModel {
        OccupancyModel {
            along_clearance_m: vehicle.half_length + race.traffic_half_length,
            lateral_clearance_m: vehicle.half_width
                + race.traffic_half_width
                + thresholds.lateral_margin_m,
            activation_horizon_m: race.traffic_ahead,
            retire_behind_m: race.traffic_behind,
        }
    }
}

/// The compiled grid and what the sweep found in it.
#[derive(Debug, Clone, PartialEq)]
pub struct TraversalGrid {
    /// Distance between columns (m).
    pub step_m: f32,
    /// Number of columns.
    pub columns: usize,
    /// Lanes per column (`2·MAX_LANE_REACH + 1`).
    pub lanes: usize,
    /// How many lanes the player may cross between adjacent columns.
    pub max_lane_shift: i32,
    /// Blocked cells, column-major: `blocked[column * lanes + lane_index]`.
    pub blocked: Vec<bool>,
    /// Reachable cells after the forward sweep, same layout.
    pub reachable: Vec<bool>,
    /// Distances (m) at which the route was cut off entirely.
    pub blockages: Vec<f32>,
    /// The tightest lateral gap the route ever had to take (m). Infinite if the
    /// course has no traffic at all.
    pub tightest_corridor_m: f32,
}

impl TraversalGrid {
    /// The lane index a signed lane number maps to.
    fn slot(lane: i32) -> usize {
        (lane + MAX_LANE_REACH) as usize
    }

    /// Whether cell `(column, lane)` is blocked.
    pub fn is_blocked(&self, column: usize, lane: i32) -> bool {
        self.blocked
            .get(column * self.lanes + Self::slot(lane))
            .copied()
            .unwrap_or(true)
    }

    /// Whether cell `(column, lane)` is reachable by a legal route.
    pub fn is_reachable(&self, column: usize, lane: i32) -> bool {
        self.reachable
            .get(column * self.lanes + Self::slot(lane))
            .copied()
            .unwrap_or(false)
    }

    /// How many lanes are reachable in `column`.
    pub fn corridor_width(&self, column: usize) -> u32 {
        (-MAX_LANE_REACH..=MAX_LANE_REACH)
            .filter(|lane| self.is_reachable(column, *lane))
            .count() as u32
    }

    /// The column covering `distance_m`.
    ///
    /// Columns sit *at* multiples of the step and each covers half a step
    /// either side of itself — which is exactly the window `analyse` marks
    /// occupancy over — so the covering column is the **nearest** one, not the
    /// one below.
    pub fn column_at(&self, distance_m: f32) -> usize {
        ((distance_m / self.step_m).round().max(0.0) as usize).min(self.columns.saturating_sub(1))
    }

    /// The narrowest corridor anywhere in `[start_m, end_m)`.
    pub fn narrowest_corridor(&self, start_m: f32, end_m: f32) -> u32 {
        let first = self.column_at(start_m);
        let last = self.column_at((end_m - self.step_m * 0.5).max(start_m));
        (first..=last)
            .map(|c| self.corridor_width(c))
            .min()
            .unwrap_or(0)
    }

    /// Whether a route exists all the way through `[start_m, end_m)`.
    pub fn is_traversable(&self, start_m: f32, end_m: f32) -> bool {
        self.narrowest_corridor(start_m, end_m) > 0
    }

    /// Total cells.
    pub fn cells(&self) -> usize {
        self.columns * self.lanes
    }

    /// Blocked cells.
    pub fn blocked_cells(&self) -> usize {
        self.blocked.iter().filter(|b| **b).count()
    }
}

/// Where a vehicle is when the player has reached `player_m`, or `None` if it
/// has not activated yet or has already been left behind.
///
/// This is the runtime's activation rule, expressed as arithmetic: a plan is
/// placed at its spawn distance the moment the player comes within the
/// activation horizon of it, and it drives forward from there while the player
/// closes at the expected speed.
pub fn projected_position(
    plan: &TrafficPlan,
    player_m: f32,
    expected_speed_mps: f32,
    model: &OccupancyModel,
) -> Option<f32> {
    let activate_at = (plan.spawn_m - model.activation_horizon_m).max(0.0);
    (player_m >= activate_at).then_some(())?;
    let travelled = (player_m - activate_at) / expected_speed_mps.max(1.0);
    let position = plan.spawn_m + plan.speed_at(plan.spawn_m) * travelled;
    ((position <= plan.despawn_m) & (position >= player_m - model.retire_behind_m))
        .then_some(position)
}

/// Build the grid and sweep it.
pub fn analyse(
    track: &Track,
    plans: &[TrafficPlan],
    thresholds: &ValidationThresholds,
    model: &OccupancyModel,
) -> TraversalGrid {
    let step_m = thresholds.traversal_step_m.max(1.0);
    let columns = ((track.length() / step_m).ceil().max(1.0) as usize) + 1;
    let lanes = (MAX_LANE_REACH * 2 + 1) as usize;
    // How many lanes a player holding the expected speed can cross in the time
    // one column takes. Zero means the grid is too fine to express a lane change
    // at all, which the caller reports as a configuration error.
    let mut blocked = vec![false; columns * lanes];
    let mut tightest = f32::INFINITY;
    let mut shift_bound = i32::MAX;

    for column in 0..columns {
        let distance_m = column as f32 * step_m;
        let sample = track.interpolated_at(distance_m);
        let reach = track.lane_reach(&sample);
        let expected = sample.expected_speed.max(1.0);
        shift_bound = shift_bound.min(
            (thresholds.lateral_speed_mps * (step_m / expected) / track.lane_width().max(0.1))
                .floor() as i32,
        );

        // Lanes the road does not have are blocked outright.
        (-MAX_LANE_REACH..=MAX_LANE_REACH).for_each(|lane| {
            (lane.abs() > reach).then(|| {
                blocked[column * lanes + TraversalGrid::slot(lane)] = true;
            });
        });

        // Every vehicle abreast of this column, projected to where it is when
        // the player arrives.
        let window = model.along_clearance_m + step_m * 0.5;
        let abreast: Vec<f32> = plans
            .iter()
            .filter_map(|plan| {
                let position = projected_position(plan, distance_m, expected, model)?;
                ((position - distance_m).abs() < window).then(|| {
                    track.lane_lateral(&sample, plan.lane_at(position))
                })
            })
            .collect();

        let mut widest = 0.0f32;
        (-MAX_LANE_REACH..=MAX_LANE_REACH).for_each(|lane| {
            let index = column * lanes + TraversalGrid::slot(lane);
            let centre = track.lane_lateral(&sample, lane);
            let nearest = abreast
                .iter()
                .map(|l| (l - centre).abs())
                .fold(f32::INFINITY, f32::min);
            blocked[index] |= nearest < model.lateral_clearance_m;
            (!blocked[index]).then(|| widest = widest.max(nearest));
        });
        (!abreast.is_empty()).then(|| tightest = tightest.min(widest));
    }

    let max_lane_shift = shift_bound.max(0);
    let (reachable, blockages) = sweep(&blocked, columns, lanes, max_lane_shift, step_m);
    TraversalGrid {
        step_m,
        columns,
        lanes,
        max_lane_shift,
        blocked,
        reachable,
        blockages,
        tightest_corridor_m: tightest,
    }
}

/// The forward reachability sweep.
fn sweep(
    blocked: &[bool],
    columns: usize,
    lanes: usize,
    max_lane_shift: i32,
    step_m: f32,
) -> (Vec<bool>, Vec<f32>) {
    let mut reachable = vec![false; columns * lanes];
    let mut blockages = Vec::new();
    let mut previous: Vec<bool> = (0..lanes).map(|slot| !blocked[slot]).collect();
    previous
        .iter()
        .enumerate()
        .for_each(|(slot, open)| reachable[slot] = *open);

    for column in 1..columns {
        let base = column * lanes;
        let current: Vec<bool> = (0..lanes)
            .map(|slot| {
                (!blocked[base + slot])
                    & (0..lanes).any(|from| {
                        previous[from]
                            & ((slot as i32 - from as i32).abs() <= max_lane_shift)
                    })
            })
            .collect();
        let dead = !current.iter().any(|open| *open);
        // A dead column is a wall. Record it and restart from whatever is free
        // in the next column, so one pass lists every blockage rather than the
        // first one.
        let current = dead
            .then(|| {
                blockages.push(column as f32 * step_m);
                (0..lanes).map(|slot| !blocked[base + slot]).collect()
            })
            .unwrap_or(current);
        current
            .iter()
            .enumerate()
            .for_each(|(slot, open)| reachable[base + slot] = *open);
        previous = current;
    }
    (reachable, blockages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::{VehicleArchetype, VehicleId};
    use crate::course::traffic::PLAN_LIFETIME_M;

    fn track() -> Track {
        crate::course::procedural::shipping_plan(crate::DEFAULT_SEED)
            .expect("the shipping course compiles")
            .track()
            .clone()
    }

    fn model() -> OccupancyModel {
        OccupancyModel::resolve(
            &VehicleTuning::DEFAULT,
            &RaceTuning::DEFAULT,
            &ValidationThresholds::DEFAULT,
        )
    }

    /// A vehicle placed so that it is exactly abreast of the player at
    /// `meet_m`: the grid's own projection run backwards.
    fn blocker(id: u32, meet_m: f32, lane: i32, model: &OccupancyModel) -> TrafficPlan {
        // A stationary blocker is met exactly where it stands, which makes a
        // fixture readable — the wall is at the metre the test names.
        let _ = model;
        TrafficPlan {
            id: VehicleId(id),
            spawn_m: meet_m,
            despawn_m: meet_m + PLAN_LIFETIME_M,
            lane,
            speed_mps: 0.01,
            archetype: VehicleArchetype::Saloon,
            lane_changes: Vec::new(),
            speed_changes: Vec::new(),
            encounter: None,
            section: 0,
            variation_seed: 1,
        }
    }

    #[test]
    fn an_open_road_is_traversable_everywhere() {
        let track = track();
        let grid = analyse(&track, &[], &ValidationThresholds::DEFAULT, &model());
        assert!(grid.blockages.is_empty(), "{:?}", grid.blockages);
        assert!(grid.is_traversable(0.0, track.length()));
        assert!(grid.corridor_width(0) >= 3, "the road is at least three lanes");
        assert_eq!(grid.tightest_corridor_m, f32::INFINITY, "no traffic, no gap");
        assert_eq!(grid.cells(), grid.columns * grid.lanes);
        assert!(grid.blocked_cells() > 0, "lanes the road lacks are blocked");
        assert!(
            grid.max_lane_shift >= 1,
            "the grid step must be able to express a lane change"
        );
    }

    #[test]
    fn a_wall_across_every_lane_fails() {
        let track = track();
        let at = 2_000.0;
        let plans: Vec<TrafficPlan> = (-MAX_LANE_REACH..=MAX_LANE_REACH)
            .enumerate()
            .map(|(i, lane)| blocker(i as u32, at, lane, &model()))
            .collect();
        let grid = analyse(&track, &plans, &ValidationThresholds::DEFAULT, &model());
        assert!(!grid.blockages.is_empty(), "a full wall was not detected");
        assert!(
            grid.blockages.iter().any(|d| (d - at).abs() < 60.0),
            "the blockage was reported at {:?}, not near {at}",
            grid.blockages
        );
        assert!(!grid.is_traversable(at - 40.0, at + 40.0));
        // And the sweep recovered afterwards, so the rest of the course is
        // still analysed.
        assert!(grid.is_traversable(at + 400.0, at + 800.0));
    }

    #[test]
    fn a_gap_in_a_wall_is_found_and_the_route_takes_it() {
        let track = track();
        let at = 2_000.0;
        let open = 0;
        let reach = track.lane_reach(&track.sample_at(at));
        let plans: Vec<TrafficPlan> = (-reach..=reach)
            .filter(|lane| *lane != open)
            .enumerate()
            .map(|(i, lane)| blocker(i as u32, at, lane, &model()))
            .collect();
        let grid = analyse(&track, &plans, &ValidationThresholds::DEFAULT, &model());
        assert!(grid.blockages.is_empty(), "{:?}", grid.blockages);
        let column = grid.column_at(at);
        assert!(grid.is_reachable(column, open), "the gap is not reachable");
        assert!(
            grid.tightest_corridor_m.is_finite(),
            "the tightest corridor was not measured"
        );
    }

    #[test]
    fn a_lane_change_the_player_cannot_physically_make_is_rejected() {
        let track = track();
        // Two walls, one column apart, whose gaps are as far apart as the road
        // allows. Crossing that many lanes in one column is not possible.
        let reach = track.lane_reach(&track.sample_at(2_000.0));
        assert!(reach >= 2, "this fixture needs a five-lane stretch");
        let step = ValidationThresholds::DEFAULT.traversal_step_m;
        let mut plans = Vec::new();
        let mut id = 0u32;
        (-reach..=reach).filter(|l| *l != -reach).for_each(|lane| {
            plans.push(blocker(id, 2_000.0, lane, &model()));
            id += 1;
        });
        (-reach..=reach).filter(|l| *l != reach).for_each(|lane| {
            plans.push(blocker(id, 2_000.0 + step, lane, &model()));
            id += 1;
        });
        let grid = analyse(&track, &plans, &ValidationThresholds::DEFAULT, &model());
        assert!(
            grid.max_lane_shift < reach * 2,
            "the fixture is only meaningful if the jump exceeds one step's reach"
        );
        assert!(
            !grid.blockages.is_empty(),
            "a {} lane jump in one step was accepted",
            reach * 2
        );
    }

    #[test]
    fn a_grid_too_fine_to_change_lane_reports_a_zero_shift() {
        let track = track();
        let thresholds = ValidationThresholds {
            traversal_step_m: 4.0,
            ..ValidationThresholds::DEFAULT
        };
        let grid = analyse(&track, &[], &thresholds, &model());
        assert_eq!(
            grid.max_lane_shift, 0,
            "a 4 m column cannot contain a lane change at racing speed"
        );
        // Straight ahead is still legal, so an empty road still passes.
        assert!(grid.blockages.is_empty());
    }

    #[test]
    fn collision_margins_widen_a_blocker_beyond_its_own_lane() {
        let track = track();
        let at = 2_000.0;
        let model = model();
        // One car, in the middle lane.
        let grid = analyse(
            &track,
            &[blocker(0, at, 0, &model)],
            &ValidationThresholds::DEFAULT,
            &model,
        );
        let column = grid.column_at(at);
        assert!(grid.is_blocked(column, 0), "its own lane is blocked");
        // With a lane width of 3.5 m and a clearance of about 2.6 m, the lanes
        // either side stay open — but only just, and widening the margin past
        // the lane width closes them.
        assert!(model.lateral_clearance_m < track.lane_width());
        assert!(!grid.is_blocked(column, 1));
        let wide = OccupancyModel {
            lateral_clearance_m: track.lane_width() * 1.5,
            ..model
        };
        let grid = analyse(&track, &[blocker(0, at, 0, &wide)], &ValidationThresholds::DEFAULT, &wide);
        let column = grid.column_at(at);
        assert!(grid.is_blocked(column, 1), "a wide margin spills into the next lane");
        assert!(grid.is_blocked(column, -1));
    }

    #[test]
    fn the_projection_activates_and_retires_a_vehicle_at_the_right_distances() {
        let model = model();
        let plan = TrafficPlan {
            speed_mps: 30.0,
            ..blocker(0, 2_000.0, 0, &model)
        };
        // Before the activation horizon it does not exist.
        assert_eq!(
            projected_position(&plan, 2_000.0 - model.activation_horizon_m - 10.0, 80.0, &model),
            None
        );
        // At the horizon it is exactly at its spawn distance.
        let at_spawn =
            projected_position(&plan, 2_000.0 - model.activation_horizon_m, 80.0, &model).unwrap();
        assert!((at_spawn - 2_000.0).abs() < 1.0e-3);
        // And the player catches it 372 m past the spawn point, which is the
        // meeting projection the rest of the system uses.
        let meet = crate::course::traffic::meeting_distance(2_000.0, 30.0, 620.0, 80.0, 9_000.0);
        let there = projected_position(&plan, meet, 80.0, &model).unwrap();
        assert!((there - meet).abs() < 2.0, "met at {there}, expected {meet}");
        // Long after, it is behind the player and retired.
        assert_eq!(projected_position(&plan, meet + 4_000.0, 80.0, &model), None);
    }

    #[test]
    fn the_grid_answers_out_of_range_queries_conservatively() {
        let track = track();
        let grid = analyse(&track, &[], &ValidationThresholds::DEFAULT, &model());
        assert!(grid.is_blocked(grid.columns + 10, 0), "off the end is blocked");
        assert!(!grid.is_reachable(grid.columns + 10, 0));
        assert_eq!(grid.column_at(-100.0), 0);
        assert_eq!(grid.column_at(1.0e9), grid.columns - 1);
        assert!(grid.narrowest_corridor(500.0, 400.0) > 0, "a reversed span is not a panic");
    }
}
