//! The boost meter — the game's entire reward loop in one number.
//!
//! Boost is the only resource in Burnt Rubber. Three of its four sources are
//! *earned by driving dangerously*, and are read from simulation state that
//! already exists:
//!
//! * **near misses** — threading traffic at a real closing speed;
//! * **drifting** — holding a slide rather than tidying it up;
//! * **speed** — simply staying above a high threshold.
//!
//! There is no separate "combo system" to keep in sync with the driving, and
//! there is no timer anywhere. If the car is not doing something dangerous,
//! those three are not filling the meter.
//!
//! # The fourth source: authored pickups
//!
//! The fourth is different in kind, and this module used to say the game had no
//! such thing. It does: a **boost pickup** ([`crate::course::pickups`]) is
//! charge the *course* hands over, placed by an author at a distance and a lane
//! and collected by driving over it. It is income the driving does not have to
//! generate.
//!
//! That is a real change to the shape of the loop, and it is worth being honest
//! about what keeps it from undoing the other three. Two things, and neither is
//! in this file:
//!
//! * **Where a pickup is *is* its difficulty.** The tier ladder says what one
//!   pays; nothing says what it costs, because the cost is the line you have to
//!   take. The shipping course puts them on the outside of banked sweepers, over
//!   blind crests and in the tunnel's traffic (see
//!   [`crate::course::procedural`]), and a course that scattered them down the
//!   racing line would have removed the loop rather than fed it.
//! * **The ladder is sized against a pass, not against the bar.** The largest
//!   tier is worth about four near misses and still under a second and a half of
//!   boost; none of them fills the meter. A pickup tops up a run that is already
//!   threading traffic — it cannot replace one that is not.
//!
//! The meter itself does not know any of this. [`BoostMeter::award`] is the same
//! door a near miss comes through, which is deliberate: the meter's job is to
//! hold a number and answer a held button, not to have opinions about where
//! charge came from.
//!
//! # Holding the button *is* the request
//!
//! There is no latch. While the button is held the meter is trying to spend,
//! and the moment it has enough to spend it does — so a boost that runs dry
//! mid-corner comes back on its own as soon as the next near miss pays for it,
//! with the player's thumb never leaving the button.
//!
//! It used to work the other way: running dry set an "exhausted" flag that only
//! a *release* could clear, so a player holding the button through a pass
//! watched the meter refill and nothing happen. That is the wrong shape for a
//! held control. A held button is a continuous request, not an edge, and the
//! meter's job is to answer it whenever it can.
//!
//! What stops that becoming a stutter is the one gate that remains:
//! [`RaceTuning::boost_min_to_start`]. Starting a boost needs a meter with
//! something in it; once running it drains to nothing. That is ordinary
//! hysteresis, and it is what turns "spend whatever you have the instant you
//! have it" into "wait until it is worth spending, then spend it".

use crate::tuning::{RaceTuning, DT};

use super::car::CarState;

/// The boost meter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoostMeter {
    /// Charge remaining, `0..1`.
    charge: f32,
    /// Whether boost is being spent this step.
    active: bool,
    /// Charge earned since the last drain, for the HUD's "+" flash.
    recent_gain: f32,
}

impl BoostMeter {
    /// A meter at its starting charge.
    pub const fn new() -> BoostMeter {
        BoostMeter {
            charge: STARTING_CHARGE,
            active: false,
            recent_gain: 0.0,
        }
    }

    /// Charge remaining, `0..1`.
    pub const fn charge(&self) -> f32 {
        self.charge
    }

    /// Whether boost is being spent.
    pub const fn active(&self) -> bool {
        self.active
    }

    /// Charge earned in the most recent step (for the HUD flash).
    pub const fn recent_gain(&self) -> f32 {
        self.recent_gain
    }

    /// Whether there is enough charge to *start* a boost.
    pub fn ready(&self, race: &RaceTuning) -> bool {
        self.charge >= race.boost_min_to_start
    }

    /// Award charge — the near-miss reward, and anything else that earns.
    pub fn award(&mut self, amount: f32) {
        let before = self.charge;
        self.charge = (self.charge + amount.max(0.0)).clamp(0.0, 1.0);
        self.recent_gain += self.charge - before;
    }

    /// Advance the meter one fixed step.
    ///
    /// Returns whether boost is available to the controller this step. The
    /// controller asks; the meter decides.
    pub fn step(&mut self, held: bool, car: &CarState, race: &RaceTuning) -> bool {
        self.recent_gain = 0.0;

        // Passive earning, from what the car is already doing.
        let drifting = car.drifting && car.grounded;
        let flat_out = car.speed() >= race.high_speed_threshold;
        let earned = (if drifting { race.drift_boost_rate } else { 0.0 }
            + if flat_out { race.high_speed_boost_rate } else { 0.0 })
            * DT;
        self.award(earned);

        // The hysteresis, and the whole of it: **starting** needs a meter worth
        // spending, **continuing** only needs one that is not empty. Without the
        // gap between those two the meter would re-engage on the first
        // hundredth it earned and fire in single frames.
        let can_start = self.ready(race);
        let can_continue = self.active && self.charge > 0.0;
        self.active = held && (can_continue || can_start);

        if self.active {
            self.charge = (self.charge - race.boost_drain_rate * DT).max(0.0);
            // Running dry ends *this* boost. It does not lock the button out:
            // the next step asks the same question again, and the moment the
            // meter is back above the starting gate the answer is yes.
            self.active = self.charge > 0.0;
        }
        self.active
    }

    /// Reset to the starting charge — a restart.
    pub fn reset(&mut self) {
        *self = BoostMeter::new();
    }
}

impl Default for BoostMeter {
    fn default() -> Self {
        BoostMeter::new()
    }
}

/// The charge the car starts a race with: enough for one satisfying shove off
/// the line, not enough to carry the first section.
pub const STARTING_CHARGE: f32 = 0.35;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    fn car_at(speed: f32, drifting: bool) -> CarState {
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        car.forward_speed = speed;
        car.drifting = drifting;
        car.grounded = true;
        car
    }

    #[test]
    fn a_new_meter_starts_partly_charged_and_idle() {
        let m = BoostMeter::new();
        assert_eq!(m.charge(), STARTING_CHARGE);
        assert!(!m.active());
        assert_eq!(m.recent_gain(), 0.0);
        assert_eq!(BoostMeter::default(), m);
    }

    #[test]
    fn holding_boost_drains_the_meter_and_stops_cleanly_when_empty() {
        let mut m = BoostMeter::new();
        let r = RaceTuning::DEFAULT;
        let idle = car_at(10.0, false);
        assert!(m.step(true, &idle, &r), "it engages");
        let after_one = m.charge();
        assert!(after_one < STARTING_CHARGE, "and drains");

        let mut steps = 0;
        while m.step(true, &idle, &r) {
            steps += 1;
            assert!(steps < 10_000, "it must eventually run out");
        }
        assert_eq!(m.charge(), 0.0);
        assert!(!m.active());
    }

    /// **The thing a held button means.** Run the meter dry with the button
    /// still down, keep holding, and boost comes back on its own the moment the
    /// meter is worth spending again — no release, no re-press.
    #[test]
    fn a_held_button_re_engages_as_soon_as_the_meter_can_pay() {
        let mut m = BoostMeter::new();
        let r = RaceTuning::DEFAULT;
        let fast = car_at(r.high_speed_threshold + 5.0, false);

        // Hold until it runs out.
        while m.step(true, &fast, &r) {}
        assert!(!m.active());
        assert!(m.charge() < r.boost_min_to_start);

        // Keep holding. It must come back by itself.
        let mut steps = 0;
        while !m.step(true, &fast, &r) {
            steps += 1;
            assert!(steps < 3_000, "a held button never re-engaged");
        }
        assert!(m.active(), "boost came back without the button being released");
        assert!(
            m.charge() >= r.boost_min_to_start - r.boost_drain_rate * DT,
            "and it waited until the meter was worth spending: {}",
            m.charge()
        );
    }

    /// A near miss is the case this exists for: dry, still holding, one pass
    /// pays for the next boost and it fires on the spot.
    #[test]
    fn a_near_miss_re_lights_the_boost_under_a_held_button() {
        let mut m = BoostMeter::new();
        let r = RaceTuning::DEFAULT;
        // A car earning nothing passively, so the award is the only income.
        let idle = car_at(10.0, false);
        while m.step(true, &idle, &r) {}
        assert!(!m.active());

        m.award(r.near_miss_boost);
        assert!(
            m.step(true, &idle, &r),
            "a near miss did not re-light a held boost"
        );
    }

    /// The whole point, end to end: hold the button, run dry, thread one car,
    /// and the boost is back — without the thumb moving.
    #[test]
    fn one_pass_is_enough_to_relight_a_held_boost() {
        let r = RaceTuning::DEFAULT;
        assert!(
            r.near_miss_boost > r.boost_min_to_start,
            "the gate is not payable by one pass"
        );
        let mut m = BoostMeter::new();
        let idle = car_at(10.0, false);
        while m.step(true, &idle, &r) {}
        assert!(!m.active(), "dry");
        m.award(r.near_miss_boost);
        assert!(m.step(true, &idle, &r));
        // And it is a real boost, not a single frame: it runs until the meter
        // is spent.
        let mut steps = 0;
        while m.step(true, &idle, &r) {
            steps += 1;
        }
        assert!(
            steps > 10,
            "the re-lit boost lasted {steps} steps, which is a flicker"
        );
    }

    /// The gate that replaced the latch has to be doing its job: a meter with
    /// almost nothing in it may not fire, or a held button would flicker every
    /// frame it earned a hundredth.
    #[test]
    fn a_nearly_empty_meter_still_will_not_start_under_a_held_button() {
        let mut m = BoostMeter::new();
        let r = RaceTuning::DEFAULT;
        let idle = car_at(10.0, false);
        while m.step(true, &idle, &r) {}

        m.award(r.boost_min_to_start * 0.5);
        assert!(!m.step(true, &idle, &r), "half the gate is not enough");
        m.award(r.boost_min_to_start);
        assert!(m.step(true, &idle, &r), "and over the gate it fires");
    }

    #[test]
    fn boost_will_not_start_below_the_minimum_charge() {
        let mut m = BoostMeter::new();
        let r = RaceTuning::DEFAULT;
        let idle = car_at(0.0, false);
        while m.step(true, &idle, &r) {}
        m.step(false, &idle, &r);
        m.award(r.boost_min_to_start * 0.5);
        assert!(!m.ready(&r));
        assert!(!m.step(true, &idle, &r), "too little to start");
        m.award(r.boost_min_to_start);
        assert!(m.step(true, &idle, &r), "enough now");
    }

    #[test]
    fn drifting_earns_boost() {
        let r = RaceTuning::DEFAULT;
        let mut drifting = BoostMeter::new();
        let mut gripping = BoostMeter::new();
        for _ in 0..120 {
            drifting.step(false, &car_at(30.0, true), &r);
            gripping.step(false, &car_at(30.0, false), &r);
        }
        assert!(
            drifting.charge() > gripping.charge(),
            "a drift pays: {} vs {}",
            drifting.charge(),
            gripping.charge()
        );
    }

    #[test]
    fn sustained_high_speed_earns_boost() {
        let r = RaceTuning::DEFAULT;
        let mut fast = BoostMeter::new();
        let mut slow = BoostMeter::new();
        for _ in 0..120 {
            fast.step(false, &car_at(r.high_speed_threshold + 5.0, false), &r);
            slow.step(false, &car_at(r.high_speed_threshold - 20.0, false), &r);
        }
        assert!(fast.charge() > slow.charge(), "speed pays too");
    }

    #[test]
    fn an_airborne_drift_does_not_pay() {
        let r = RaceTuning::DEFAULT;
        let mut m = BoostMeter::new();
        let mut airborne = car_at(30.0, true);
        airborne.grounded = false;
        let before = m.charge();
        m.step(false, &airborne, &r);
        assert_eq!(m.charge(), before, "there is no drift without a road");
    }

    #[test]
    fn a_near_miss_award_lands_immediately_and_is_reported() {
        let r = RaceTuning::DEFAULT;
        let mut m = BoostMeter::new();
        let idle = car_at(0.0, false);
        m.step(false, &idle, &r);
        let before = m.charge();
        m.award(r.near_miss_boost);
        assert!((m.charge() - before - r.near_miss_boost).abs() < 1.0e-5);
        assert!(m.recent_gain() >= r.near_miss_boost - 1.0e-5);
    }

    #[test]
    fn the_meter_never_leaves_its_range() {
        let r = RaceTuning::DEFAULT;
        let mut m = BoostMeter::new();
        m.award(50.0);
        assert_eq!(m.charge(), 1.0);
        m.award(-50.0);
        assert_eq!(m.charge(), 1.0, "a negative award is ignored");
        let idle = car_at(0.0, false);
        for _ in 0..10_000 {
            m.step(true, &idle, &r);
            assert!((0.0..=1.0).contains(&m.charge()));
        }
    }

    #[test]
    fn resetting_returns_the_starting_charge() {
        let r = RaceTuning::DEFAULT;
        let mut m = BoostMeter::new();
        let idle = car_at(0.0, false);
        for _ in 0..30 {
            m.step(true, &idle, &r);
        }
        assert_ne!(m.charge(), STARTING_CHARGE);
        m.reset();
        assert_eq!(m, BoostMeter::new());
    }
}
