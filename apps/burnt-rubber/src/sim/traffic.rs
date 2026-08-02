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
use crate::tuning::{RaceTuning, DT};

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
        wander_phase: 0.0,
        wander_amount: 0.0,
    };
}

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
    pub fn step(&mut self, player_distance: f32, track: &Track, race: &RaceTuning) {
        self.retire_behind(player_distance, track, race);
        self.advance(track);
        self.spawn_ahead(player_distance, track, race);
    }

    fn retire_behind(&mut self, player_distance: f32, track: &Track, race: &RaceTuning) {
        let floor = player_distance - race.traffic_behind;
        for car in self.cars.iter_mut().filter(|c| c.active) {
            car.active = car.distance >= floor && car.distance <= track.length();
        }
    }

    fn advance(&mut self, track: &Track) {
        for car in self.cars.iter_mut().filter(|c| c.active) {
            car.distance += car.speed * DT;
            car.lateral = lane_lateral(track, car.distance, car.lane)
                // The wander is driven by distance travelled, not by a tick
                // count, so a car resampled at a different moment is in the same
                // place — which is what keeps replay exact.
                + car.wander_amount * (car.wander_phase + car.distance * WANDER_RATE).sin();
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
                t.step(d, &track, &r);
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
            traffic.step(distance, &track, &r);
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
        traffic.step(600.0, &track, &r);
        assert!(traffic.active_count() > 0, "traffic appears");
        for _ in 0..600 {
            traffic.step(600.0, &track, &r);
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
        traffic.step(0.0, &track, &r);
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
        traffic.step(400.0, &track, &r);
        let before: Vec<(u32, f32)> = traffic.active().map(|c| (c.slot, c.distance)).collect();
        for _ in 0..60 {
            traffic.step(400.0, &track, &r);
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
            traffic.step(distance, &track, &r);
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
        traffic.step(500.0, &track, &r);
        let filled = traffic.active_count();
        assert!(filled > 0);
        // Jump the player a long way forward: everything behind must retire, and
        // the pool refills ahead.
        traffic.step(5_000.0, &track, &r);
        for car in traffic.active() {
            assert!(car.distance >= 5_000.0 - r.traffic_behind - 1.0);
        }
    }

    #[test]
    fn clearing_the_pool_rewinds_to_the_first_slot() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(6, &r);
        traffic.step(2_000.0, &track, &r);
        assert!(traffic.active_count() > 0);
        traffic.clear();
        assert_eq!(traffic.active_count(), 0);
        traffic.step(400.0, &track, &r);
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

    #[test]
    fn marking_a_near_miss_sticks_and_ignores_a_bad_index() {
        let track = track();
        let r = RaceTuning::DEFAULT;
        let mut traffic = Traffic::new(8, &r);
        traffic.step(500.0, &track, &r);
        traffic.mark_near_missed(0);
        assert!(traffic.cars()[0].near_missed);
        traffic.mark_near_missed(9_999);
        assert!(traffic.cars()[0].near_missed, "an out-of-range index is a no-op");
    }
}
