//! The boost meter — the game's entire reward loop in one number.
//!
//! Boost is the only resource in Burnt Rubber, and it is deliberately earned by
//! doing the risky thing rather than by collecting anything:
//!
//! * **near misses** — threading traffic at a real closing speed;
//! * **drifting** — holding a slide rather than tidying it up;
//! * **speed** — simply staying above a high threshold.
//!
//! All three are read from simulation state that already exists, which is the
//! point: there is no separate "combo system" to keep in sync with the driving,
//! and there is no timer anywhere. If the car is not doing something dangerous,
//! the meter is not filling.
//!
//! Spending is gated by a minimum charge so a nearly-empty meter cannot be
//! tapped into a stutter, and by an "already released" latch so a boost that
//! empties does not silently re-engage while the key is still held.

use crate::tuning::{RaceTuning, DT};

use super::car::CarState;

/// The boost meter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoostMeter {
    /// Charge remaining, `0..1`.
    charge: f32,
    /// Whether boost is being spent this step.
    active: bool,
    /// Set when a boost runs the meter dry; cleared when the key is released, so
    /// an empty meter cannot re-engage under a held key.
    exhausted: bool,
    /// Charge earned since the last drain, for the HUD's "+" flash.
    recent_gain: f32,
}

impl BoostMeter {
    /// A meter at its starting charge.
    pub const fn new() -> BoostMeter {
        BoostMeter {
            charge: STARTING_CHARGE,
            active: false,
            exhausted: false,
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

        // Releasing the key always clears the exhaustion latch.
        self.exhausted &= held;

        let can_start = self.ready(race) && !self.exhausted;
        let can_continue = self.active && self.charge > 0.0 && !self.exhausted;
        self.active = held && (can_continue || can_start);

        if self.active {
            self.charge = (self.charge - race.boost_drain_rate * DT).max(0.0);
            // Running dry latches until the key is let go.
            self.exhausted = self.charge <= 0.0;
            self.active = !self.exhausted;
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

    #[test]
    fn an_empty_meter_does_not_re_engage_under_a_held_key() {
        let mut m = BoostMeter::new();
        let r = RaceTuning::DEFAULT;
        // A car that is earning: flat out.
        let fast = car_at(r.high_speed_threshold + 5.0, false);
        while m.step(true, &fast, &r) {}
        assert!(!m.active());
        // Keep holding while it trickles back up — it must stay off.
        for _ in 0..600 {
            assert!(!m.step(true, &fast, &r), "still latched off");
        }
        assert!(m.charge() > r.boost_min_to_start, "even though it has recharged");
        // Release, then press again.
        assert!(!m.step(false, &fast, &r));
        assert!(m.step(true, &fast, &r), "a fresh press works");
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
