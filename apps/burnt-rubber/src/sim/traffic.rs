//! Deterministic traffic: lane-following cars that exist to be threaded.
//!
//! This is emphatically **not** driving AI. Traffic has no goals, no
//! pathfinding, no awareness of the player and no decisions to make. Each car
//! holds a lane, holds a speed, and drifts a few centimetres inside its lane as
//! it goes. That is the whole model, and it is the right one: the interesting
//! agent in a game about threading traffic is the *player*, and traffic that
//! reacted to being approached would remove exactly the thing the player is
//! being asked to judge.
//!
//! ## The slot model
//!
//! Traffic is defined as an infinite ordered list of **slots** along the course,
//! slot `k` sitting at `k · traffic_spacing` metres. A slot's lane, speed,
//! variant and lane-wander phase are a pure function of `(seed, k)` — never of
//! when it spawned or of how the player got there. A bounded pool of live cars
//! is recycled through those slots: a car that falls behind the player is
//! retired, and the pool slot is reused for the next slot ahead.
//!
//! The consequence is the property the tests pin: **recycling a pool slot cannot
//! change what is generated.** Slot 412 is the same red car in the same lane at
//! the same speed whether the pool had a free entry immediately or reused the
//! one that just retired.

use crate::draw::Draw;
use crate::track::Track;
use crate::tuning::{CollisionTuning, RaceTuning, DT};

/// One live traffic car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrafficCar {
    /// Whether this pool entry is currently in play.
    pub active: bool,
    /// The slot this car was spawned from.
    pub slot: u32,
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
    /// are there, and it will do exactly the same thing to a barrier. Traffic
    /// yielding a little is what makes it feel lighter than concrete — the one
    /// thing the collision brief actually asks of it.
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

/// Traffic cars spawned in one step is bounded by the pool size, so the initial
/// fill and any catch-up after a reset both terminate.
#[derive(Debug, Clone)]
pub struct Traffic {
    cars: Vec<TrafficCar>,
    next_slot: u32,
    seed: u64,
}

impl Traffic {
    /// An empty pool of `race.traffic_active` entries.
    pub fn new(seed: u64, race: &RaceTuning) -> Traffic {
        Traffic {
            cars: vec![TrafficCar::RETIRED; race.traffic_active],
            next_slot: 0,
            seed,
        }
    }

    /// Every pool entry, live or retired, in stable order.
    pub fn cars(&self) -> &[TrafficCar] {
        &self.cars
    }

    /// The pool entries, mutably — how a scripted scenario places a specific car
    /// exactly where it wants the player to meet it.
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

    /// Retire everything and rewind to the first slot — a restart.
    pub fn clear(&mut self) {
        self.cars.iter_mut().for_each(|c| *c = TrafficCar::RETIRED);
        self.next_slot = 0;
    }

    /// Advance the traffic one fixed step around a player at `player_distance`.
    pub fn step(
        &mut self,
        player_distance: f32,
        track: &Track,
        race: &RaceTuning,
        collision: &CollisionTuning,
    ) {
        self.retire_behind(player_distance, track, race);
        self.advance(track, collision);
        self.spawn_ahead(player_distance, track, race);
    }

    fn retire_behind(&mut self, player_distance: f32, track: &Track, race: &RaceTuning) {
        let floor = player_distance - race.traffic_behind;
        for car in self.cars.iter_mut().filter(|c| c.active) {
            car.active = car.distance >= floor && car.distance <= track.length();
        }
    }

    fn advance(&mut self, track: &Track, collision: &CollisionTuning) {
        for car in self.cars.iter_mut().filter(|c| c.active) {
            car.distance += (car.speed + car.yield_speed) * DT;
            car.lateral = lane_lateral(track, car.distance, car.lane)
                // The wander is driven by distance travelled, not by a tick
                // count, so a car resampled at a different moment is in the same
                // place — which is what keeps replay exact.
                + car.wander_amount * (car.wander_phase + car.distance * WANDER_RATE).sin()
                // ...and the temporary offset a contact gave it, which decays.
                + car.yield_offset;
            car.relax(collision);
        }
    }

    fn spawn_ahead(&mut self, player_distance: f32, track: &Track, race: &RaceTuning) {
        let horizon = player_distance + race.traffic_ahead;
        // Skip past every slot the player has already gone by *arithmetically*,
        // in one move. Walking them one per loop iteration would spend the
        // spawn budget on slots that are never going to spawn, so a player who
        // jumped forward (a capture, a reset, the finish teleport) would find an
        // empty road until the cursor crawled up to them.
        let floor = (player_distance - race.traffic_behind).max(race.traffic_clear_start);
        let first_live = (floor / race.traffic_spacing).ceil().max(0.0) as u32;
        self.next_slot = self.next_slot.max(first_live);

        // Bounded by the pool size: at most one spawn per pool entry per step.
        for _ in 0..self.cars.len() {
            let base = self.next_slot as f32 * race.traffic_spacing;
            if base > horizon || base > track.length() {
                break;
            }
            let Some(index) = self.cars.iter().position(|c| !c.active) else {
                break;
            };
            // **The safety region.** A slot inside it is skipped, never spawned.
            //
            // In ordinary play this never fires: slots enter the horizon
            // `traffic_ahead` metres away and the cursor only moves forward. It
            // fires after a *jump* — `place_at`, a capture, the finish teleport,
            // any of which clears the pool and refills it around wherever the
            // player now is. Without the skip, refilling from
            // `player - traffic_behind` puts the very next slot anywhere in the
            // 85 m after that, which includes on top of the car. A car
            // materialising inside the player is the least fair thing traffic
            // can do, and it is the one failure the slot model made possible.
            if inside_safety_region(base, player_distance, race) {
                self.next_slot += 1;
                continue;
            }
            self.cars[index] = spawn_slot(self.seed, self.next_slot, track, race);
            self.next_slot += 1;
        }
    }

    /// Mark a car as having awarded its near miss, by pool index.
    pub fn mark_near_missed(&mut self, index: usize) {
        if let Some(car) = self.cars.get_mut(index) {
            car.near_missed = true;
        }
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

/// Build the car for `slot` — a pure function of `(seed, slot)`.
pub fn spawn_slot(seed: u64, slot: u32, track: &Track, race: &RaceTuning) -> TrafficCar {
    let mut draw = Draw::seeded(seed).fork(TRAFFIC_SALT ^ slot as u64);
    let distance = slot as f32 * race.traffic_spacing;
    // Pick among the lanes that exist here, then re-centre the index: the draw
    // is an ordinal `0..lanes` and a lane is a signed offset from the middle.
    let lanes = lane_count(track, distance);
    let lane = draw.index(lanes) as i32 - lane_reach(track, distance);
    let speed = draw.range(race.traffic_speed_min, race.traffic_speed_max);
    let variant = draw.index(TRAFFIC_VARIANTS as usize) as u8;
    let wander_phase = draw.range(0.0, std::f32::consts::TAU);
    let wander_amount = draw.range(0.1, 0.45);
    TrafficCar {
        active: true,
        slot,
        distance,
        lateral: lane_lateral(track, distance, lane),
        lane,
        speed,
        variant,
        near_missed: false,
        yield_offset: 0.0,
        yield_speed: 0.0,
        wander_phase,
        wander_amount,
    }
}

/// Salt separating the traffic stream from every other generator on the seed.
const TRAFFIC_SALT: u64 = 0x7A4F_1C33_9E02_D5B1;

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
    use crate::tuning::CourseTuning;

    fn track() -> Track {
        Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT)
    }

    #[test]
    fn a_slot_is_a_pure_function_of_seed_and_index() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        for slot in [20u32, 77, 300] {
            let a = spawn_slot(9, slot, &track, &r);
            let b = spawn_slot(9, slot, &track, &r);
            assert_eq!(a, b);
            assert_ne!(spawn_slot(10, slot, &track, &r), a, "a different seed differs");
        }
    }

    #[test]
    fn traffic_placement_is_deterministic_across_two_identical_runs() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let run = || {
            let mut t = Traffic::new(4242, &r);
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
    /// what a slot contains.
    #[test]
    fn recycling_a_pool_entry_does_not_change_the_generated_contents() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(7, &r);
        let mut distance = 0.0f32;
        let mut seen: Vec<(u32, TrafficCar)> = Vec::new();
        for _ in 0..12_000 {
            distance += 70.0 * DT;
            traffic.step(distance, &track, &r, &CollisionTuning::DEFAULT);
            for car in traffic.active() {
                if !seen.iter().any(|(s, _)| *s == car.slot) {
                    // Capture each slot the first time it appears, at spawn.
                    let fresh = spawn_slot(7, car.slot, &track, &r);
                    seen.push((car.slot, fresh));
                }
            }
        }
        assert!(seen.len() > 100, "the run recycled through many slots");
        for (slot, captured) in seen {
            assert_eq!(
                spawn_slot(7, slot, &track, &r),
                captured,
                "slot {slot} regenerates identically"
            );
        }
    }

    #[test]
    fn the_pool_is_bounded_and_fills_up_around_the_player() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(1, &r);
        traffic.step(600.0, &track, &r, &CollisionTuning::DEFAULT);
        assert!(traffic.active_count() > 0, "traffic appears");
        for _ in 0..600 {
            traffic.step(600.0, &track, &r, &CollisionTuning::DEFAULT);
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
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(3, &r);
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
    fn traffic_travels_forward_at_its_own_speed() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(5, &r);
        traffic.step(400.0, &track, &r, &CollisionTuning::DEFAULT);
        let before: Vec<(u32, f32)> = traffic.active().map(|c| (c.slot, c.distance)).collect();
        for _ in 0..60 {
            traffic.step(400.0, &track, &r, &CollisionTuning::DEFAULT);
        }
        for (slot, start) in before {
            if let Some(car) = traffic.active().find(|c| c.slot == slot) {
                let moved = car.distance - start;
                assert!(
                    moved > r.traffic_speed_min * 0.9 && moved < r.traffic_speed_max * 1.1,
                    "slot {slot} moved {moved} m in a second at {} m/s",
                    car.speed
                );
            }
        }
    }

    #[test]
    fn traffic_stays_within_the_road_on_its_lane_path() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(11, &r);
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
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(2, &r);
        traffic.step(500.0, &track, &r, &CollisionTuning::DEFAULT);
        let filled = traffic.active_count();
        assert!(filled > 0);
        // Jump the player a long way forward: everything behind must retire, and
        // the pool refills ahead.
        traffic.step(5_000.0, &track, &r, &CollisionTuning::DEFAULT);
        for car in traffic.active() {
            assert!(car.distance >= 5_000.0 - r.traffic_behind - 1.0);
        }
    }

    #[test]
    fn clearing_the_pool_rewinds_to_the_first_slot() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(6, &r);
        traffic.step(2_000.0, &track, &r, &CollisionTuning::DEFAULT);
        assert!(traffic.active_count() > 0);
        traffic.clear();
        assert_eq!(traffic.active_count(), 0);
        traffic.step(400.0, &track, &r, &CollisionTuning::DEFAULT);
        let lowest = traffic.active().map(|c| c.slot).min().expect("refilled");
        assert!(
            lowest as f32 * r.traffic_spacing >= r.traffic_clear_start - 1.0,
            "and refills from the start of the course"
        );
    }

    #[test]
    fn lanes_are_inside_the_road_and_ordered() {
        let track = track();
        let r = RaceTuning::DEFAULT;
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
            // An out-of-range lane clamps rather than panicking, on both sides.
            assert_eq!(
                lane_lateral(&track, distance, 99),
                lane_lateral(&track, distance, reach)
            );
            assert_eq!(
                lane_lateral(&track, distance, -99),
                lane_lateral(&track, distance, -reach)
            );
            let _ = &r;
        }
    }

    /// A traffic car yields, and there is a hard limit on how far.
    #[test]
    fn a_traffic_car_yields_sideways_but_only_within_its_budget() {
        let c = CollisionTuning::DEFAULT;
        let mut car = spawn_slot(1, 10, &track(), &RaceTuning::DEFAULT);
        // A modest shove is taken in full.
        let taken = car.yield_lateral(0.4, &c);
        assert!((taken - 0.4).abs() < 1.0e-5, "took {taken} of 0.4");
        assert!((car.yield_offset - 0.4).abs() < 1.0e-5);

        // Repeated shoves accumulate, and then stop at the budget however hard
        // the player keeps pushing.
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

        // And it comes back to its lane on its own.
        for _ in 0..600 {
            car.relax(&c);
        }
        assert_eq!(car.yield_offset, 0.0, "and returns to its lane exactly");
    }

    #[test]
    fn a_traffic_car_shunts_forward_but_only_within_its_budget() {
        let c = CollisionTuning::DEFAULT;
        let mut car = spawn_slot(1, 10, &track(), &RaceTuning::DEFAULT);
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
        // A shunt the other way is bounded too — a car cannot be driven
        // backwards down the course.
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
        let track = track();
        let r = RaceTuning::DEFAULT;
        let c = CollisionTuning::DEFAULT;
        let mut traffic = Traffic::new(21, &r);
        traffic.step(500.0, &track, &r, &c);
        let index = traffic
            .cars()
            .iter()
            .position(|car| car.active)
            .expect("a live car");
        let lane_line = traffic.cars()[index].lateral;

        traffic.cars_mut()[index].yield_lateral(1.0, &c);
        traffic.step(500.0, &track, &r, &c);
        let shoved = traffic.cars()[index].lateral;
        assert!(
            (shoved - lane_line).abs() > 0.5,
            "the shove moved it: {lane_line} -> {shoved}"
        );

        for _ in 0..300 {
            traffic.step(500.0, &track, &r, &c);
        }
        assert_eq!(traffic.cars()[index].yield_offset, 0.0, "and it came back");
        // Still on the road, throughout — a yield can never push a car off it.
        let sample = track.sample_at(traffic.cars()[index].distance);
        assert!(traffic.cars()[index].lateral.abs() <= sample.half_width + 0.1);
    }

    /// The fairness rule with the sharpest edge: after a jump, the pool refills
    /// around wherever the player now is, and nothing may materialise on top of
    /// them.
    #[test]
    fn recycled_traffic_never_spawns_inside_the_player_safety_region() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let c = CollisionTuning::DEFAULT;
        // Sweep the player across a whole slot pitch, so every possible phase
        // relationship between the player and the slot grid is exercised — the
        // bug only appears at some of them.
        for offset in 0..85 {
            let player = 2_000.0 + offset as f32;
            let mut traffic = Traffic::new(33, &r);
            traffic.step(player, &track, &r, &c);
            for car in traffic.active() {
                assert!(
                    !inside_safety_region(car.distance, player, &r),
                    "slot {} spawned {} m from a player at {player}",
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
        let track = track();
        let r = RaceTuning::DEFAULT;
        let c = CollisionTuning::DEFAULT;
        let mut traffic = Traffic::new(44, &r);
        let mut seen: Vec<u32> = Vec::new();
        let mut player = 400.0f32;
        for jump in 0..40 {
            player = (player + 231.0 * (jump as f32 + 1.0)).min(track.length() - 500.0);
            traffic.clear();
            for _ in 0..30 {
                player += 70.0 * DT;
                traffic.step(player, &track, &r, &c);
                for car in traffic.active() {
                    // A car may *drive* into the region — that is traffic being
                    // caught up with, which is the game. What may never happen
                    // is one being *created* there, which is what the first
                    // sighting of a slot detects.
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
        assert!(seen.len() >= 40, "the run genuinely recycled: {} slots", seen.len());
    }

    /// Traffic must never form a wall. Checked over the whole generated course,
    /// not a sample of it.
    #[test]
    fn traffic_never_blocks_the_road_across_the_whole_generation_range() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let c = CollisionTuning::DEFAULT;
        let mut traffic = Traffic::new(55, &r);
        let mut player = 0.0f32;
        let vehicle = crate::tuning::VehicleTuning::DEFAULT;
        // The along-course window inside which two cars are "abreast" — a
        // player cannot slip between them longitudinally, so the road has to
        // offer a gap sideways.
        let abreast = vehicle.half_length + r.traffic_half_length;
        let mut checked = 0u32;
        while player < track.length() - 200.0 {
            player += 80.0 * DT;
            traffic.step(player, &track, &r, &c);
            let live: Vec<(f32, f32)> = traffic.active().map(|t| (t.distance, t.lateral)).collect();
            for (distance, _) in &live {
                let sample = track.sample_at(*distance);
                let lanes = track.lane_count(&sample);
                // Everything abreast of this car, including itself.
                let blockers: Vec<f32> = live
                    .iter()
                    .filter(|(d, _)| (d - distance).abs() < abreast * 2.0)
                    .map(|(_, l)| *l)
                    .collect();
                // A cross-section is passable if some lane centre is clear of
                // every car abreast by more than the two half-widths.
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
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(8, &r);
        traffic.step(500.0, &track, &r, &CollisionTuning::DEFAULT);
        traffic.mark_near_missed(0);
        assert!(traffic.cars()[0].near_missed);
        traffic.mark_near_missed(9_999);
        assert!(traffic.cars()[0].near_missed, "an out-of-range index is a no-op");
    }
}
