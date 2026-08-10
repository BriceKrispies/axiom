//! **Collecting boost pickups**: the swept test, and the one-per-run ledger.
//!
//! A pickup does not move, has no state of its own, and cannot be hit — which
//! means it needs none of the machinery traffic needs. There is no pool, no
//! activation, no advance and no retirement. There are two facts:
//!
//! * where the pickups are — the compiled [`CoursePlan`], immutable and shared;
//! * which ones *this run* has already taken — [`PickupField`], one bit each.
//!
//! Keeping the second out of the plan is not tidiness. The plan is shared by
//! `Arc` between the live race, the ghost and any replay, and a "taken" flag on
//! it would be one run reaching into another's: the ghost driving over a pickup
//! would silently remove it from the player's road.
//!
//! # Why the test is swept
//!
//! The naive test is "is a pickup within *x* metres of where the car is now",
//! and it is wrong in a way that only shows up at speed. At the boosted top
//! speed the car covers about 1.6 m per fixed step, so a point test against a
//! pickup needs a window at least that wide just to be *hit*, and any window
//! wide enough is also wide enough to collect a pickup the car has not reached.
//!
//! So the test asks the question that is actually being asked: **did the car
//! pass this pickup during this step**. That is `(at > from) & (at <= to)` over
//! the interval the car travelled — the same `crossed` idiom
//! [`crate::sim::traffic`] uses for a plan's scheduled lane and speed changes,
//! and for the same reason. It cannot be tunnelled through at any speed, it
//! cannot double-collect at any frame rate, and it does not need a magic radius.
//!
//! The *lateral* half is an ordinary proximity test, because the car does not
//! sweep sideways in any meaningful sense within one step.

use std::sync::Arc;

use crate::course::pickups::BoostPickup;
use crate::course::runtime::CoursePlan;
use crate::track::Track;
use crate::tuning::{RaceTuning, VehicleTuning};

use super::car::CarState;

/// The pickups on this course, and which of them this run has taken.
#[derive(Debug, Clone)]
pub struct PickupField {
    plan: Arc<CoursePlan>,
    /// One flag per compiled pickup, indexed by [`crate::course::specification::PickupId`].
    ///
    /// A flat `Vec<bool>` rather than a set: identities are dense and minted at
    /// compile time, so the index is free and there is nothing to hash.
    taken: Vec<bool>,
    /// How many have been taken, for the HUD and the ghost's measurements.
    collected: u32,
}

impl PickupField {
    /// An untouched field over `plan`.
    pub fn new(plan: Arc<CoursePlan>) -> PickupField {
        let taken = vec![false; plan.pickups().len()];
        PickupField {
            plan,
            taken,
            collected: 0,
        }
    }

    /// How many pickups this run has taken.
    pub const fn collected(&self) -> u32 {
        self.collected
    }

    /// How many the course holds in total.
    pub fn total(&self) -> usize {
        self.plan.pickups().len()
    }

    /// Whether `pickup` has already been taken this run.
    pub fn is_taken(&self, pickup: &BoostPickup) -> bool {
        self.taken
            .get(pickup.id.0 as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Collect everything the car passed moving from `from_m` to its current
    /// distance, returning the tiers taken in course order.
    ///
    /// Returns the *tiers* rather than awarding directly: what a tier is worth
    /// is a rule of the race ([`RaceTuning::pickup_boost`]) and how the meter
    /// takes it is the meter's business. This decides only *which* pickups were
    /// taken, which is the one question it is in a position to answer.
    pub fn collect(
        &mut self,
        from_m: f32,
        car: &CarState,
        track: &Track,
        race: &RaceTuning,
        vehicle: &VehicleTuning,
    ) -> Vec<Collected> {
        // A step that went nowhere, or backwards (a reset, a teleport), collects
        // nothing. A backwards sweep would otherwise re-cross pickups the car
        // has already passed, and the ledger is what stops that mattering — but
        // it is cheaper and clearer to not ask.
        let to_m = car.distance;
        if to_m <= from_m {
            return Vec::new();
        }
        let reach = vehicle.half_width + race.pickup_reach_m;
        let start = self.plan.first_pickup_at(from_m);
        // Walk forward from the index while the pickups are still inside the
        // swept interval. Bounded by how far the car moved in one step, which is
        // a couple of metres — so this is a handful of comparisons, not a scan.
        let mut taken: Vec<Collected> = Vec::new();
        for pickup in &self.plan.pickups()[start..] {
            if pickup.at_m > to_m {
                break;
            }
            // `first_pickup_at` is a lower bound and may point slightly behind
            // `from_m`; the sweep's own lower bound is exclusive, so a pickup
            // exactly at `from_m` was collected by the previous step.
            if pickup.at_m <= from_m {
                continue;
            }
            let already = self
                .taken
                .get(pickup.id.0 as usize)
                .copied()
                .unwrap_or(true);
            if already {
                continue;
            }
            let sample = track.sample_at(pickup.at_m);
            let lateral = track.lane_lateral(&sample, pickup.lane);
            if (car.lateral - lateral).abs() > reach {
                continue;
            }
            self.taken[pickup.id.0 as usize] = true;
            self.collected += 1;
            taken.push(Collected {
                tier: pickup.tier,
                at_m: pickup.at_m,
                boost: race.pickup_boost(pickup.tier),
            });
        }
        taken
    }

    /// Forget every collection — a restart.
    pub fn reset(&mut self) {
        self.taken.iter_mut().for_each(|t| *t = false);
        self.collected = 0;
    }
}

/// One pickup taken this step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Collected {
    /// Which tier it was — what the cue and the notification key off.
    pub tier: crate::course::specification::BoostTier,
    /// Where it stood (m).
    pub at_m: f32,
    /// What it paid (fraction of the meter).
    pub boost: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::BoostTier;
    use crate::sim::RaceSim;
    use crate::tuning::Tuning;
    use axiom_math::Vec3;

    fn field() -> (PickupField, Arc<CoursePlan>) {
        let sim = RaceSim::shipping();
        let plan = sim.plan().clone();
        (PickupField::new(plan.clone()), plan)
    }

    fn car_at(distance: f32, lateral: f32) -> CarState {
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        car.distance = distance;
        car.lateral = lateral;
        car
    }

    /// The car must be *on* the pickup's lane, and must have crossed it.
    #[test]
    fn driving_over_a_pickup_in_its_lane_collects_it_exactly_once() {
        let (mut field, plan) = field();
        let tuning = Tuning::DEFAULT;
        let target = plan.pickups()[0];
        let track = plan.track();
        let sample = track.sample_at(target.at_m);
        let lateral = track.lane_lateral(&sample, target.lane);

        let car = car_at(target.at_m + 1.0, lateral);
        let taken = field.collect(
            target.at_m - 1.0,
            &car,
            track,
            &tuning.race,
            &tuning.vehicle,
        );
        assert_eq!(taken.len(), 1, "the pickup was not collected");
        assert_eq!(taken[0].tier, target.tier);
        assert_eq!(taken[0].boost, tuning.race.pickup_boost(target.tier));
        assert_eq!(field.collected(), 1);
        assert!(field.is_taken(&target));

        // Crossing the same ground again pays nothing.
        let again = field.collect(
            target.at_m - 1.0,
            &car,
            track,
            &tuning.race,
            &tuning.vehicle,
        );
        assert!(again.is_empty(), "a pickup paid twice");
        assert_eq!(field.collected(), 1);
    }

    #[test]
    fn a_pickup_in_another_lane_is_not_collected() {
        let (mut field, plan) = field();
        let tuning = Tuning::DEFAULT;
        let target = plan.pickups()[0];
        let track = plan.track();
        let sample = track.sample_at(target.at_m);
        let lateral = track.lane_lateral(&sample, target.lane);

        // Two lanes away is comfortably outside any reach.
        let car = car_at(target.at_m + 1.0, lateral + track.lane_width() * 2.0);
        let taken = field.collect(
            target.at_m - 1.0,
            &car,
            track,
            &tuning.race,
            &tuning.vehicle,
        );
        assert!(taken.is_empty(), "collected from another lane");
        assert_eq!(field.collected(), 0);
    }

    /// **The reason the test is swept.** One step that jumps clean over a
    /// pickup — further than any real step, so the point test it replaced would
    /// certainly have missed — still collects it.
    #[test]
    fn a_pickup_cannot_be_tunnelled_through_at_any_speed() {
        let (mut field, plan) = field();
        let tuning = Tuning::DEFAULT;
        let target = plan.pickups()[0];
        let track = plan.track();
        let sample = track.sample_at(target.at_m);
        let lateral = track.lane_lateral(&sample, target.lane);

        let car = car_at(target.at_m + 40.0, lateral);
        let taken = field.collect(
            target.at_m - 40.0,
            &car,
            track,
            &tuning.race,
            &tuning.vehicle,
        );
        assert_eq!(taken.len(), 1, "an 80 m step stepped over a pickup");
    }

    #[test]
    fn a_backwards_or_stationary_step_collects_nothing() {
        let (mut field, plan) = field();
        let tuning = Tuning::DEFAULT;
        let target = plan.pickups()[0];
        let track = plan.track();
        let sample = track.sample_at(target.at_m);
        let lateral = track.lane_lateral(&sample, target.lane);

        let car = car_at(target.at_m - 5.0, lateral);
        assert!(field
            .collect(target.at_m + 5.0, &car, track, &tuning.race, &tuning.vehicle)
            .is_empty());
        assert!(field
            .collect(car.distance, &car, track, &tuning.race, &tuning.vehicle)
            .is_empty());
    }

    /// A sweep that spans a whole row takes every pickup in it, in course order.
    #[test]
    fn a_sweep_across_a_row_takes_all_of_it_in_order() {
        let (mut field, plan) = field();
        let tuning = Tuning::DEFAULT;
        let track = plan.track();
        // The opening row: three in lane 0 on the start straight.
        let row: Vec<&BoostPickup> = plan
            .pickups()
            .iter()
            .filter(|p| p.lane == 0)
            .take(3)
            .collect();
        assert_eq!(row.len(), 3, "the shipping course has an opening row");
        let first = row[0].at_m;
        let last = row[2].at_m;
        let sample = track.sample_at(first);
        let lateral = track.lane_lateral(&sample, 0);

        let car = car_at(last + 1.0, lateral);
        let taken = field.collect(first - 1.0, &car, track, &tuning.race, &tuning.vehicle);
        assert_eq!(taken.len(), 3);
        assert!(
            taken.windows(2).all(|w| w[0].at_m < w[1].at_m),
            "collections are not in course order"
        );
    }

    #[test]
    fn a_reset_forgets_every_collection() {
        let (mut field, plan) = field();
        let tuning = Tuning::DEFAULT;
        let target = plan.pickups()[0];
        let track = plan.track();
        let sample = track.sample_at(target.at_m);
        let lateral = track.lane_lateral(&sample, target.lane);
        let car = car_at(target.at_m + 1.0, lateral);
        field.collect(target.at_m - 1.0, &car, track, &tuning.race, &tuning.vehicle);
        assert_eq!(field.collected(), 1);

        field.reset();
        assert_eq!(field.collected(), 0);
        assert!(!field.is_taken(&target));
        assert_eq!(field.total(), plan.pickups().len());
        // And it can be collected again.
        assert_eq!(
            field
                .collect(target.at_m - 1.0, &car, track, &tuning.race, &tuning.vehicle)
                .len(),
            1
        );
    }

    /// Every tier pays what the tuning says it pays, and the ladder climbs.
    #[test]
    fn the_pickup_ladder_climbs() {
        let race = Tuning::DEFAULT.race;
        let paid: Vec<f32> = BoostTier::ALL
            .iter()
            .map(|t| race.pickup_boost(*t))
            .collect();
        assert!(paid[0] < paid[1], "medium pays more than small");
        assert!(paid[1] < paid[2], "large pays more than medium");
        assert!(
            paid[0] >= race.near_miss_boost,
            "the smallest pickup is at least worth a pass"
        );
        assert!(
            paid[2] < 1.0,
            "no single pickup fills the bar: {}",
            paid[2]
        );
    }
}
