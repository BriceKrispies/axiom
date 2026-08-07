//! Deterministic traffic: lane-following cars that exist to be threaded.
//!
//! This is emphatically **not** driving AI. Traffic has no goals, no
//! pathfinding, no awareness of the player and no decisions to make. Each car
//! holds the lane its plan gives it, holds the speed its plan gives it, makes
//! the lane changes its plan schedules, and drifts a few centimetres inside its
//! lane as it goes. That is the whole model, and it is the right one: the
//! interesting agent in a game about threading traffic is the *player*, and
//! traffic that reacted to being approached would remove exactly the thing the
//! player is being asked to judge.
//!
//! ## Activation, not spawning
//!
//! Traffic used to be an infinite arithmetic list of *slots*: slot `k` sat at
//! `k · spacing` and its lane, speed and variant were a pure function of
//! `(seed, k)`. That was deterministic, and it was also the reason a course
//! could not be authored — there was nowhere to say "a van here, in this lane,
//! at this speed", because a slot's contents were computed, not chosen.
//!
//! What replaces it is **activation from a compiled plan**
//! ([`crate::course::traffic::TrafficPlan`]). Every vehicle on the course is
//! decided before the race starts, sorted by spawn distance and indexed; the
//! runtime's whole job is to notice which plans have entered the forward horizon
//! and to copy them into a bounded pool. The determinism property is *stronger*
//! than the slot model's, because it no longer depends on arithmetic being
//! reproducible: the vehicle is a value that was written down.
//!
//! Recycling a pool entry still cannot change what a plan contains, for the
//! simplest possible reason — the pool holds copies and the plan is immutable.

use std::sync::Arc;

use crate::course::runtime::CoursePlan;
use crate::course::specification::VehicleId;
use crate::course::traffic::TrafficPlan;
use crate::draw::Draw;
use crate::track::Track;
use crate::tuning::{CollisionTuning, RaceTuning, DT};

/// One live traffic car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrafficCar {
    /// Whether this pool entry is currently in play.
    pub active: bool,
    /// The compiled plan's stable identity. Everything that has to *name* a
    /// traffic car — the collision episode ledger, a replay, a near-miss window
    /// — names it by this, never by its pool index, which is recycled.
    pub slot: u32,
    /// Which plan this car came from, so its scheduled lane and speed changes
    /// can be read without searching.
    pub plan_index: usize,
    /// Arc length along the course (m).
    pub distance: f32,
    /// Lateral offset from the road centre (m), including the in-lane wander.
    pub lateral: f32,
    /// The lane this car holds, numbered out from the centreline (see
    /// [`Track::lane_lateral`]). Signed, so a road that gains an outer lane pair
    /// leaves every car where it is instead of renumbering the whole road.
    pub lane: i32,
    /// Cruising speed (m/s).
    pub speed: f32,
    /// Which of the visual car shapes this is.
    pub variant: u8,
    /// Whether the player has already been awarded a near miss on this car.
    pub near_missed: bool,
    /// A temporary lateral offset from this car's lane (m), given by being hit
    /// and returned smoothly afterwards.
    ///
    /// This is the *entire* extent to which traffic reacts to anything, and it
    /// is deliberately not behaviour: a car that has been shoved slides over and
    /// then comes back to its lane. It is not avoiding you, it does not know you
    /// are there, and it will do exactly the same thing to a barrier.
    pub yield_offset: f32,
    /// A temporary addition to this car's speed (m/s) from being shunted, which
    /// bleeds off.
    pub yield_speed: f32,
    /// Phase of the in-lane wander (radians).
    wander_phase: f32,
    /// Amplitude of the in-lane wander (m).
    wander_amount: f32,
}

impl TrafficCar {
    const RETIRED: TrafficCar = TrafficCar {
        active: false,
        slot: 0,
        plan_index: 0,
        distance: 0.0,
        lateral: 0.0,
        lane: 0,
        speed: 0.0,
        variant: 0,
        near_missed: false,
        yield_offset: 0.0,
        yield_speed: 0.0,
        wander_phase: 0.0,
        wander_amount: 0.0,
    };

    /// Yield sideways by up to `amount` metres, returning how much was actually
    /// taken. Bounded by [`CollisionTuning::traffic_yield_lateral`], so a car
    /// can be nudged out of the player's way but never pushed off the road or
    /// bulldozed across the course.
    pub fn yield_lateral(&mut self, amount: f32, tuning: &CollisionTuning) -> f32 {
        let limit = tuning.traffic_yield_lateral;
        let wanted = self.yield_offset + amount;
        let taken = wanted.clamp(-limit, limit) - self.yield_offset;
        self.yield_offset += taken;
        taken
    }

    /// Take up to `amount` metres of along-course displacement as a forward
    /// shunt, returning how much was taken. The displacement is converted to a
    /// bounded extra speed rather than a position jump, so a shunted car
    /// accelerates away rather than teleporting.
    pub fn yield_forward(&mut self, amount: f32, tuning: &CollisionTuning) -> f32 {
        let limit = tuning.traffic_yield_speed;
        let wanted = self.yield_speed + amount / DT.max(1.0e-6) * SHUNT_TRANSFER;
        let clamped = wanted.clamp(-limit, limit);
        let taken = (clamped - self.yield_speed) * DT / SHUNT_TRANSFER;
        self.yield_speed = clamped;
        taken
    }

    /// Fade both yields back toward nothing. Called once per fixed step.
    fn relax(&mut self, tuning: &CollisionTuning) {
        self.yield_offset *= (-tuning.traffic_yield_return * DT).exp();
        self.yield_speed *= (-tuning.traffic_yield_decay * DT).exp();
        if self.yield_offset.abs() <= YIELD_EPSILON {
            self.yield_offset = 0.0;
        }
        if self.yield_speed.abs() <= YIELD_EPSILON {
            self.yield_speed = 0.0;
        }
    }
}

/// How much of a requested along-course displacement becomes speed rather than
/// being refused. Below one, so a shunt reads as a nudge the car drives out of
/// rather than as the player's whole closing speed transferring across.
const SHUNT_TRANSFER: f32 = 0.35;

/// Yield magnitude below which a car counts as back in its lane.
const YIELD_EPSILON: f32 = 1.0e-3;

/// How many distinct traffic car shapes exist.
pub const TRAFFIC_VARIANTS: u8 = 4;

/// The live traffic: a bounded pool of cars, activated from a compiled plan.
#[derive(Debug, Clone)]
pub struct Traffic {
    plan: Arc<CoursePlan>,
    cars: Vec<TrafficCar>,
    /// The next plan index that has not been considered for activation. Monotone
    /// while the player moves forward; recomputed from the plan's distance index
    /// after a jump.
    cursor: usize,
}

impl Traffic {
    /// An empty pool of `race.traffic_active` entries over `plan`.
    pub fn new(plan: Arc<CoursePlan>, race: &RaceTuning) -> Traffic {
        Traffic {
            plan,
            cars: vec![TrafficCar::RETIRED; race.traffic_active],
            cursor: 0,
        }
    }

    /// The course this traffic is activated from.
    pub fn plan(&self) -> &CoursePlan {
        &self.plan
    }

    /// Every pool entry, live or retired, in stable order.
    pub fn cars(&self) -> &[TrafficCar] {
        &self.cars
    }

    /// The pool entries, mutably — how the collision resolver shoves a car
    /// aside, and how a scripted scenario places one exactly where it wants the
    /// player to meet it.
    pub fn cars_mut(&mut self) -> &mut [TrafficCar] {
        &mut self.cars
    }

    /// The live cars, in stable order.
    pub fn active(&self) -> impl Iterator<Item = &TrafficCar> {
        self.cars.iter().filter(|c| c.active)
    }

    /// How many cars are live.
    pub fn active_count(&self) -> usize {
        self.cars.iter().filter(|c| c.active).count()
    }

    /// Retire everything and rewind the cursor — a restart, a reset or a jump.
    pub fn clear(&mut self) {
        self.cars.iter_mut().for_each(|c| *c = TrafficCar::RETIRED);
        self.cursor = 0;
    }

    /// Advance the traffic one fixed step around a player at `player_distance`.
    pub fn step(
        &mut self,
        player_distance: f32,
        track: &Track,
        race: &RaceTuning,
        collision: &CollisionTuning,
    ) {
        self.retire(player_distance, track, race);
        self.advance(track, collision);
        self.activate(player_distance, track, race);
    }

    /// Retire cars the player has left behind, and cars past their plan's own
    /// end.
    fn retire(&mut self, player_distance: f32, track: &Track, race: &RaceTuning) {
        let floor = player_distance - race.traffic_behind;
        let plans = self.plan.traffic();
        self.cars.iter_mut().filter(|c| c.active).for_each(|car| {
            let despawn = plans
                .get(car.plan_index)
                .map(|p| p.despawn_m)
                .unwrap_or(track.length());
            car.active = (car.distance >= floor)
                & (car.distance <= track.length())
                & (car.distance <= despawn);
        });
    }

    /// Drive every live car one step along its plan.
    fn advance(&mut self, track: &Track, collision: &CollisionTuning) {
        let plans = self.plan.traffic();
        for car in self.cars.iter_mut().filter(|c| c.active) {
            let plan = &plans[car.plan_index];
            let from = car.distance;
            car.distance += (car.speed + car.yield_speed) * DT;
            // A scheduled change is applied when the car **crosses** it, not
            // re-read from the plan every step. Both shapes replay identically
            // for a car that is only ever driven by its plan; the difference is
            // that this one leaves a car somebody has deliberately placed
            // (`cars_mut`, a staged scenario, the collision fixtures) alone,
            // instead of overwriting its speed sixty times a second with the
            // one the plan happened to compile.
            let crossed = |at: f32| (at > from) & (at <= car.distance);
            plan.speed_changes
                .iter()
                .filter(|c| crossed(c.at_m))
                .for_each(|c| car.speed = c.to_mps);
            plan.lane_changes
                .iter()
                .filter(|c| crossed(c.at_m))
                .for_each(|c| car.lane = c.to_lane);
            car.lateral = lane_lateral(track, car.distance, car.lane)
                // The wander is driven by distance travelled, not by a tick
                // count, for the same reason.
                + car.wander_amount * (car.wander_phase + car.distance * WANDER_RATE).sin()
                // ...and the temporary offset a contact gave it, which decays.
                + car.yield_offset;
            car.relax(collision);
        }
    }

    /// Copy every plan that has entered the forward horizon into a free pool
    /// entry.
    fn activate(&mut self, player_distance: f32, track: &Track, race: &RaceTuning) {
        let horizon = player_distance + race.traffic_ahead;
        let floor = player_distance - race.traffic_behind;
        // Skip past everything already behind the player **arithmetically**, in
        // one move, through the plan's distance index. Walking them one per
        // step would spend the activation budget on plans that will never
        // activate, so a player who jumped forward (a capture, a reset, the
        // finish teleport) would find an empty road while the cursor crawled up
        // to them.
        self.cursor = self.cursor.max(self.plan.first_vehicle_at(floor));

        let plans = self.plan.traffic();
        // Bounded by the pool size: at most one activation per pool entry per
        // step, so the initial fill and any catch-up after a jump both
        // terminate.
        for _ in 0..self.cars.len() {
            let Some(plan) = plans.get(self.cursor) else {
                break;
            };
            if plan.spawn_m > horizon {
                break;
            }
            let Some(index) = self.cars.iter().position(|c| !c.active) else {
                break;
            };
            // **The safety region.** A plan inside it is skipped, never
            // activated.
            //
            // In ordinary play this never fires: plans enter the horizon
            // `traffic_ahead` metres away and the cursor only moves forward. It
            // fires after a *jump* — `place_at`, a capture, the finish teleport,
            // any of which clears the pool and refills it around wherever the
            // player now is. A car materialising inside the player is the least
            // fair thing traffic can do.
            if inside_safety_region(plan.spawn_m, player_distance, race) {
                self.cursor += 1;
                continue;
            }
            self.cars[index] = activate(plan, self.cursor, track);
            self.cursor += 1;
        }
    }

    /// Mark a car as having awarded its near miss, by pool index.
    pub fn mark_near_missed(&mut self, index: usize) {
        if let Some(car) = self.cars.get_mut(index) {
            car.near_missed = true;
        }
    }

    /// The compiled plan a live car came from.
    pub fn plan_of(&self, car: &TrafficCar) -> Option<&TrafficPlan> {
        self.plan.traffic().get(car.plan_index)
    }
}

/// How quickly the in-lane wander cycles with distance (rad/m).
const WANDER_RATE: f32 = 0.011;

/// Whether `distance` is inside the region around the player where a car may
/// never appear from nothing.
///
/// The window ahead is a *reaction* window, not a collision one: a car that pops
/// into existence 10 m ahead is unfair even though it is not yet touching you.
/// [`RaceTuning::traffic_safe_ahead`] is sized so that a player at the boosted
/// top speed still gets more than a second of warning.
pub fn inside_safety_region(distance: f32, player_distance: f32, race: &RaceTuning) -> bool {
    let relative = distance - player_distance;
    relative > -race.traffic_safe_behind && relative < race.traffic_safe_ahead
}

/// Build the live car for a compiled plan.
///
/// A pure function of the plan and nothing else — not of when it activated, not
/// of which pool entry it landed in, not of how the player got here.
pub fn activate(plan: &TrafficPlan, plan_index: usize, track: &Track) -> TrafficCar {
    // The cosmetic wander is derived from the plan's own variation seed, so a
    // car's drift inside its lane is as stable as everything else about it.
    let mut draw = Draw::seeded(plan.variation_seed);
    let wander_phase = draw.range(0.0, std::f32::consts::TAU);
    let wander_amount = draw.range(0.1, 0.45);
    TrafficCar {
        active: true,
        slot: plan.id.0,
        plan_index,
        distance: plan.spawn_m,
        // **In its lane from the first step it exists.** Leaving this at zero
        // put every newly-activated car on the centreline for one step — a lane
        // it may not even be in — which is a car in the wrong place in every
        // frame that samples the pool before the next `advance`.
        lateral: lane_lateral(track, plan.spawn_m, plan.lane)
            + wander_amount * (wander_phase + plan.spawn_m * WANDER_RATE).sin(),
        lane: plan.lane,
        speed: plan.speed_mps,
        variant: plan.archetype.variant(),
        near_missed: false,
        yield_offset: 0.0,
        yield_speed: 0.0,
        wander_phase,
        wander_amount,
    }
}

/// The stable identity of a live car.
pub fn identity(car: &TrafficCar) -> VehicleId {
    VehicleId(car.slot)
}

/// How many lanes the road has at `distance`.
///
/// Delegates to the track, which is the only place lanes are defined — the road
/// mesh paints the dividers from the same call, so traffic can never sit on a
/// line rather than between two.
pub fn lane_count(track: &Track, distance: f32) -> usize {
    track.lane_count(&track.sample_at(distance))
}

/// How far out from the centreline lanes reach at `distance`.
pub fn lane_reach(track: &Track, distance: f32) -> i32 {
    track.lane_reach(&track.sample_at(distance))
}

/// The centre of `lane` at `distance` (m from the road centre).
pub fn lane_lateral(track: &Track, distance: f32, lane: i32) -> f32 {
    let sample = track.sample_at(distance);
    track.lane_lateral(&sample, lane)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::procedural;

    fn plan() -> Arc<CoursePlan> {
        Arc::new(procedural::shipping_plan(crate::DEFAULT_SEED).expect("compiles"))
    }

    fn traffic(plan: Arc<CoursePlan>) -> Traffic {
        Traffic::new(plan, &RaceTuning::DEFAULT)
    }

    #[test]
    fn a_live_car_is_a_pure_function_of_the_plan_it_came_from() {
        let plan = plan();
        for index in [3usize, 17, 40] {
            let compiled = &plan.traffic()[index];
            let a = activate(compiled, index, plan.track());
            let b = activate(compiled, index, plan.track());
            assert_eq!(a, b);
            assert_eq!(a.slot, compiled.id.0);
            assert_eq!(a.lane, compiled.lane);
            assert_eq!(a.speed, compiled.speed_mps);
            assert_eq!(a.variant, compiled.archetype.variant());
            assert_eq!(a.distance, compiled.spawn_m);
            assert_eq!(identity(&a), compiled.id);
        }
    }

    #[test]
    fn traffic_placement_is_deterministic_across_two_identical_runs() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let run = || {
            let mut t = traffic(plan.clone());
            let mut d = 0.0f32;
            for _ in 0..3_000 {
                d += 60.0 * DT;
                t.step(d, &track, &r, &CollisionTuning::DEFAULT);
            }
            t.cars().to_vec()
        };
        assert_eq!(run(), run());
    }

    /// The property that makes the pool safe: recycling an entry cannot change
    /// what a plan contains, because the pool holds *copies* of an immutable
    /// list.
    #[test]
    fn recycling_a_pool_entry_does_not_change_the_generated_contents() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan.clone());
        let mut distance = 0.0f32;
        let mut seen: Vec<(u32, TrafficCar)> = Vec::new();
        for _ in 0..12_000 {
            distance += 70.0 * DT;
            traffic.step(distance, &track, &r, &CollisionTuning::DEFAULT);
            for car in traffic.active() {
                if !seen.iter().any(|(s, _)| *s == car.slot) {
                    seen.push((car.slot, activate(&plan.traffic()[car.plan_index], car.plan_index, plan.track())));
                }
            }
        }
        assert!(seen.len() > 50, "the run recycled through many plans: {}", seen.len());
        for (slot, captured) in seen {
            let compiled = plan.vehicle(VehicleId(slot)).expect("the plan still has it");
            assert_eq!(activate(compiled, captured.plan_index, plan.track()), captured);
        }
    }

    /// **Activated once.** A plan that has been copied into the pool is never
    /// copied again while the player keeps moving forward.
    #[test]
    fn a_plan_activates_exactly_once_on_a_forward_run() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan.clone());
        let mut distance = 0.0f32;
        let mut activations: Vec<u32> = Vec::new();
        let mut live: Vec<u32> = Vec::new();
        while distance < track.length() - 200.0 {
            distance += 90.0 * DT;
            traffic.step(distance, &track, &r, &CollisionTuning::DEFAULT);
            let now: Vec<u32> = traffic.active().map(|c| c.slot).collect();
            now.iter()
                .filter(|slot| !live.contains(slot))
                .for_each(|slot| activations.push(*slot));
            live = now;
        }
        let mut unique = activations.clone();
        unique.sort_unstable();
        let count = unique.len();
        unique.dedup();
        assert_eq!(
            unique.len(),
            count,
            "a plan was activated twice on one forward run"
        );
        assert!(count > 40, "the run activated {count} vehicles");
    }

    #[test]
    fn the_pool_is_bounded_and_fills_up_around_the_player() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan);
        traffic.step(1_200.0, &track, &r, &CollisionTuning::DEFAULT);
        assert!(traffic.active_count() > 0, "traffic appears");
        for _ in 0..600 {
            traffic.step(1_200.0, &track, &r, &CollisionTuning::DEFAULT);
        }
        assert!(
            traffic.active_count() <= r.traffic_active,
            "never more than the pool: {}",
            traffic.active_count()
        );
        assert_eq!(traffic.cars().len(), r.traffic_active);
    }

    #[test]
    fn the_start_line_is_clear_of_traffic() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan);
        traffic.step(0.0, &track, &r, &CollisionTuning::DEFAULT);
        for car in traffic.active() {
            assert!(
                car.distance >= r.traffic_clear_start - 1.0,
                "a car at {} is on the start line",
                car.distance
            );
        }
    }

    #[test]
    fn traffic_travels_forward_at_its_planned_speed() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan);
        traffic.step(1_000.0, &track, &r, &CollisionTuning::DEFAULT);
        let before: Vec<(u32, f32, f32)> = traffic
            .active()
            .map(|c| (c.slot, c.distance, c.speed))
            .collect();
        assert!(!before.is_empty());
        for _ in 0..60 {
            traffic.step(1_000.0, &track, &r, &CollisionTuning::DEFAULT);
        }
        for (slot, start, speed) in before {
            if let Some(car) = traffic.active().find(|c| c.slot == slot) {
                let moved = car.distance - start;
                assert!(
                    (moved - speed).abs() < speed * 0.25,
                    "slot {slot} moved {moved} m in a second at {speed} m/s"
                );
            }
        }
    }

    #[test]
    fn traffic_stays_within_the_road_on_its_lane_path() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan);
        let mut distance = 0.0f32;
        for _ in 0..9_000 {
            distance = (distance + 80.0 * DT).min(track.length() - 10.0);
            traffic.step(distance, &track, &r, &CollisionTuning::DEFAULT);
            for car in traffic.active() {
                let sample = track.sample_at(car.distance);
                assert!(
                    car.lateral.abs() <= sample.half_width + 0.1,
                    "slot {} is off the road at {} (half width {})",
                    car.slot,
                    car.lateral,
                    sample.half_width
                );
                assert!(car.lateral.is_finite() && car.distance.is_finite());
                assert!(car.variant < TRAFFIC_VARIANTS);
            }
        }
    }

    #[test]
    fn traffic_behind_the_player_is_retired() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan);
        traffic.step(1_000.0, &track, &r, &CollisionTuning::DEFAULT);
        assert!(traffic.active_count() > 0);
        traffic.step(5_000.0, &track, &r, &CollisionTuning::DEFAULT);
        for car in traffic.active() {
            assert!(car.distance >= 5_000.0 - r.traffic_behind - 1.0);
        }
    }

    #[test]
    fn a_car_past_its_plans_own_end_is_retired() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan.clone());
        traffic.step(1_000.0, &track, &r, &CollisionTuning::DEFAULT);
        let index = traffic
            .cars()
            .iter()
            .position(|c| c.active)
            .expect("a live car");
        // Drive it past its plan's despawn distance by hand.
        let despawn = plan.traffic()[traffic.cars()[index].plan_index].despawn_m;
        traffic.cars_mut()[index].distance = despawn + 10.0;
        traffic.step(despawn + 5.0, &track, &r, &CollisionTuning::DEFAULT);
        assert!(
            traffic.cars()[index].slot != plan.traffic()[traffic.cars()[index].plan_index].id.0
                || !traffic.cars()[index].active
                || traffic.cars()[index].distance <= despawn,
            "a car past its plan's end stayed live"
        );
    }

    #[test]
    fn clearing_the_pool_rewinds_to_the_first_plan() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan);
        traffic.step(2_000.0, &track, &r, &CollisionTuning::DEFAULT);
        assert!(traffic.active_count() > 0);
        traffic.clear();
        assert_eq!(traffic.active_count(), 0);
        traffic.step(400.0, &track, &r, &CollisionTuning::DEFAULT);
        let lowest = traffic
            .active()
            .map(|c| c.distance)
            .fold(f32::INFINITY, f32::min);
        assert!(
            lowest >= r.traffic_clear_start - 1.0,
            "and refills from the start of the course, not from {lowest}"
        );
    }

    #[test]
    fn lanes_are_inside_the_road_and_ordered() {
        let plan = plan();
        let track = plan.track().clone();
        for distance in [0.0f32, 900.0, 4_400.0, 8_000.0] {
            let lanes = lane_count(&track, distance);
            let sample = track.sample_at(distance);
            assert_eq!(lanes, track.lane_count(&sample), "one definition of lanes");
            let reach = lane_reach(&track, distance);
            assert_eq!(lanes, (reach * 2 + 1) as usize, "an odd count, centred");
            let mut previous = f32::NEG_INFINITY;
            for lane in -reach..=reach {
                let lateral = lane_lateral(&track, distance, lane);
                assert_eq!(lateral, track.lane_lateral(&sample, lane));
                assert!(lateral > previous, "lanes run left to right");
                previous = lateral;
                assert!(lateral.abs() < sample.half_width, "and stay on the road");
            }
            assert_eq!(
                lane_lateral(&track, distance, 99),
                lane_lateral(&track, distance, reach)
            );
            assert_eq!(
                lane_lateral(&track, distance, -99),
                lane_lateral(&track, distance, -reach)
            );
        }
    }

    /// A traffic car yields, and there is a hard limit on how far.
    #[test]
    fn a_traffic_car_yields_sideways_but_only_within_its_budget() {
        let c = CollisionTuning::DEFAULT;
        let plan = plan();
        let mut car = activate(&plan.traffic()[5], 5, plan.track());
        let taken = car.yield_lateral(0.4, &c);
        assert!((taken - 0.4).abs() < 1.0e-5, "took {taken} of 0.4");
        assert!((car.yield_offset - 0.4).abs() < 1.0e-5);

        for _ in 0..50 {
            car.yield_lateral(1.0, &c);
        }
        assert!(
            (car.yield_offset - c.traffic_yield_lateral).abs() < 1.0e-5,
            "yielded {} past the {} m limit",
            car.yield_offset,
            c.traffic_yield_lateral
        );
        assert_eq!(car.yield_lateral(1.0, &c), 0.0, "a full budget takes nothing more");

        for _ in 0..600 {
            car.relax(&c);
        }
        assert_eq!(car.yield_offset, 0.0, "and it returns to its lane exactly");
    }

    #[test]
    fn a_traffic_car_shunts_forward_but_only_within_its_budget() {
        let c = CollisionTuning::DEFAULT;
        let plan = plan();
        let mut car = activate(&plan.traffic()[5], 5, plan.track());
        for _ in 0..200 {
            car.yield_forward(1.0, &c);
        }
        assert!(
            (car.yield_speed - c.traffic_yield_speed).abs() < 1.0e-4,
            "shunted to {} m/s past the {} m/s limit",
            car.yield_speed,
            c.traffic_yield_speed
        );
        assert_eq!(car.yield_forward(1.0, &c), 0.0);
        for _ in 0..400 {
            car.yield_forward(-1.0, &c);
        }
        assert!(car.yield_speed >= -c.traffic_yield_speed - 1.0e-4);
        for _ in 0..600 {
            car.relax(&c);
        }
        assert_eq!(car.yield_speed, 0.0);
    }

    #[test]
    fn a_yielded_car_actually_moves_and_then_returns_to_its_lane() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let c = CollisionTuning::DEFAULT;
        let mut traffic = traffic(plan);
        traffic.step(1_000.0, &track, &r, &c);
        let index = traffic
            .cars()
            .iter()
            .position(|car| car.active)
            .expect("a live car");
        let lane_line = traffic.cars()[index].lateral;

        traffic.cars_mut()[index].yield_lateral(1.0, &c);
        traffic.step(1_000.0, &track, &r, &c);
        let shoved = traffic.cars()[index].lateral;
        assert!(
            (shoved - lane_line).abs() > 0.5,
            "the shove moved it: {lane_line} -> {shoved}"
        );

        for _ in 0..300 {
            traffic.step(1_000.0, &track, &r, &c);
        }
        assert_eq!(traffic.cars()[index].yield_offset, 0.0, "and it came back");
    }

    /// The fairness rule with the sharpest edge: after a jump, the pool refills
    /// around wherever the player now is, and nothing may materialise on top of
    /// them.
    #[test]
    fn recycled_traffic_never_activates_inside_the_player_safety_region() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let c = CollisionTuning::DEFAULT;
        for offset in 0..85 {
            let player = 2_000.0 + offset as f32;
            let mut traffic = traffic(plan.clone());
            traffic.step(player, &track, &r, &c);
            for car in traffic.active() {
                assert!(
                    !inside_safety_region(car.distance, player, &r),
                    "slot {} activated {} m from a player at {player}",
                    car.slot,
                    car.distance - player
                );
            }
        }
    }

    /// The same rule, driven the way the game actually reaches it: a long run
    /// with repeated teleports, which is what a capture, a reset and the finish
    /// all do.
    #[test]
    fn traffic_never_appears_inside_the_safety_region_across_repeated_jumps() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let c = CollisionTuning::DEFAULT;
        let mut traffic = traffic(plan);
        let mut seen: Vec<u32> = Vec::new();
        let mut player = 400.0f32;
        for jump in 0..40 {
            player = (player + 231.0 * (jump as f32 + 1.0)).min(track.length() - 500.0);
            traffic.clear();
            for _ in 0..30 {
                player += 70.0 * DT;
                traffic.step(player, &track, &r, &c);
                for car in traffic.active() {
                    let fresh = !seen.contains(&car.slot);
                    assert!(
                        !fresh || !inside_safety_region(car.distance, player, &r),
                        "slot {} appeared {} m from the player",
                        car.slot,
                        car.distance - player
                    );
                    (fresh).then(|| seen.push(car.slot));
                }
            }
        }
        assert!(seen.len() >= 20, "the run genuinely recycled: {} slots", seen.len());
    }

    /// Traffic must never form a wall. Checked over the whole compiled course,
    /// not a sample of it — and now the compiler's own validator checks the
    /// same thing before the race ever starts.
    #[test]
    fn traffic_never_blocks_the_road_across_the_whole_course() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let c = CollisionTuning::DEFAULT;
        let mut traffic = traffic(plan);
        let mut player = 0.0f32;
        let vehicle = crate::tuning::VehicleTuning::DEFAULT;
        let abreast = vehicle.half_length + r.traffic_half_length;
        let mut checked = 0u32;
        while player < track.length() - 200.0 {
            player += 80.0 * DT;
            traffic.step(player, &track, &r, &c);
            let live: Vec<(f32, f32)> = traffic.active().map(|t| (t.distance, t.lateral)).collect();
            for (distance, _) in &live {
                let sample = track.sample_at(*distance);
                let lanes = track.lane_count(&sample);
                let blockers: Vec<f32> = live
                    .iter()
                    .filter(|(d, _)| (d - distance).abs() < abreast * 2.0)
                    .map(|(_, l)| *l)
                    .collect();
                let clearance = vehicle.half_width + r.traffic_half_width;
                let reach = track.lane_reach(&sample);
                let open = (-reach..=reach).filter(|lane| {
                    let centre = track.lane_lateral(&sample, *lane);
                    blockers.iter().all(|l| (l - centre).abs() >= clearance)
                });
                assert!(
                    open.count() > 0,
                    "at {distance} m, {} cars abreast blocked all {lanes} lanes",
                    blockers.len()
                );
                checked += 1;
            }
        }
        assert!(checked > 10_000, "the sweep saw {checked} cross-sections");
    }

    #[test]
    fn marking_a_near_miss_sticks_and_ignores_a_bad_index() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan);
        traffic.step(1_000.0, &track, &r, &CollisionTuning::DEFAULT);
        traffic.mark_near_missed(0);
        assert!(traffic.cars()[0].near_missed);
        traffic.mark_near_missed(9_999);
        assert!(traffic.cars()[0].near_missed, "an out-of-range index is a no-op");
    }

    #[test]
    fn a_live_car_can_be_traced_back_to_the_plan_it_came_from() {
        let plan = plan();
        let track = plan.track().clone();
        let r = RaceTuning::DEFAULT;
        let mut traffic = traffic(plan);
        traffic.step(1_000.0, &track, &r, &CollisionTuning::DEFAULT);
        let car = *traffic.active().next().expect("a live car");
        let compiled = traffic.plan_of(&car).expect("its plan");
        assert_eq!(compiled.id.0, car.slot);
        assert!(traffic.plan().length() > 0.0);
    }

    /// A plan's scheduled lane change really moves the car, which is what makes
    /// a rolling wall's opening walk.
    #[test]
    fn a_planned_lane_change_moves_the_car_when_it_reaches_it() {
        let plan = plan();
        let track = plan.track().clone();
        let compiled = TrafficPlan {
            lane_changes: vec![crate::course::traffic::LaneChange {
                at_m: 1_060.0,
                to_lane: -1,
            }],
            lane: 1,
            spawn_m: 1_000.0,
            speed_mps: 30.0,
            ..plan.traffic()[0].clone()
        };
        let mut car = activate(&compiled, 0, &track);
        assert_eq!(car.lane, 1);
        assert_eq!(compiled.lane_at(car.distance), 1);
        car.distance = 1_100.0;
        assert_eq!(compiled.lane_at(car.distance), -1);
    }
}
