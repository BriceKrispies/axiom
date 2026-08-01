//! Where the car's mass actually sits, and what that costs it.
//!
//! Until this module existed the car was a *point*: a position, a heading and
//! two speeds. It had no mass, no wheelbase and no centre of gravity, so the
//! question "is the centre of gravity too high?" had no answer — there was
//! nothing in the model for a centre of gravity to be a property *of*. Handling
//! came entirely from `steering_authority` and a constant per-surface grip.
//!
//! This is the smallest structure that makes the question answerable. It is
//! deliberately **not** a tyre, drivetrain or suspension model — the app's
//! contract rules those out, and they would be a far larger thing than the game
//! needs. What it is instead is the handful of *real* rigid-body quantities that
//! a centre of gravity genuinely determines, each wired to a handling knob that
//! already existed:
//!
//! | Real quantity | What it drives |
//! |---|---|
//! | Longitudinal CoG offset from the wheelbase midpoint | the point the chassis yaws about |
//! | Static front load fraction | turn-in authority |
//! | `cog_height / half-track` (the rollover ratio) | lateral load transfer, and so grip |
//! | `cog_height / wheelbase` | longitudinal transfer under power and braking |
//!
//! Every one of those is a textbook rigid-body relation, not a fudge factor, so
//! raising the centre of gravity makes the car tippy and vague for the reason a
//! real car goes tippy and vague, and lowering it plants the car for the same
//! reason. The *magnitudes* are still arcade — see [`LOAD_SENSITIVITY`].

/// The car's mass geometry: where the wheels are, and where the mass sits
/// between them.
///
/// Distances are metres. [`Self::cog_from_front`] is a fraction of the wheelbase
/// rather than a distance so that the front/rear split stays meaningful if the
/// wheelbase changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChassisGeometry {
    /// Front axle to rear axle (m).
    pub wheelbase: f32,
    /// Left wheel to right wheel (m). This is the lever the car rolls over.
    pub track_width: f32,
    /// Height of the centre of gravity above the road (m).
    ///
    /// The single most consequential number here. It appears in the numerator of
    /// both load-transfer terms, so everything about how planted the car feels
    /// scales with it.
    pub cog_height: f32,
    /// Where the centre of gravity sits along the wheelbase, as a fraction:
    /// `0.0` is directly over the front axle, `1.0` over the rear, `0.5` dead
    /// centre. Below `0.5` is a front-biased car.
    pub cog_from_front: f32,
}

impl ChassisGeometry {
    /// The shipping car: a low, front-biased mass.
    ///
    /// `cog_height` of 30 cm under a body a metre and a bit tall is a mass
    /// carried as low as the bodywork allows, and a 1.9 m track is deliberately
    /// wide. Together they put the rollover threshold at about **3.2 g**, which
    /// matters because this car corners at roughly 2.25 g at speed: an earlier,
    /// taller draft sat at a 2.26 g threshold, so every hard corner pinned the
    /// load transfer at its maximum and the geometry stopped discriminating at
    /// exactly the moment it should have mattered most. The car needs headroom
    /// above its own cornering envelope for the centre of gravity to be a live
    /// control rather than a saturated one.
    ///
    /// `cog_from_front` of `0.44` puts 56% of the weight on the front axle,
    /// which buys turn-in bite and makes the tail the end that rotates.
    pub const DEFAULT: ChassisGeometry = ChassisGeometry {
        wheelbase: 2.7,
        track_width: 1.9,
        cog_height: 0.3,
        cog_from_front: 0.44,
    };

    /// Fraction of the car's weight carried by the front axle at rest.
    ///
    /// The mass sits `cog_from_front` of the way back from the front axle, so
    /// the *front* carries the complement — a CoG near the front axle puts
    /// almost all the load on it.
    pub fn front_load(&self) -> f32 {
        (1.0 - self.cog_from_front).clamp(0.0, 1.0)
    }

    /// Fraction of the car's weight carried by the rear axle at rest.
    pub fn rear_load(&self) -> f32 {
        1.0 - self.front_load()
    }

    /// How far ahead of the wheelbase midpoint the centre of gravity sits (m).
    /// Negative for a rear-biased car.
    ///
    /// This is the offset the chassis actually rotates about. A car does not
    /// pivot about its geometric middle; it pivots about its mass.
    pub fn forward_offset(&self) -> f32 {
        (0.5 - self.cog_from_front) * self.wheelbase
    }

    /// The rollover ratio: lateral load transferred per `g` of lateral
    /// acceleration.
    ///
    /// Straight out of the rigid-body free-body diagram — the overturning moment
    /// is `m·a·h` and the restoring moment is `m·g·(t/2)`, so the transfer per
    /// `g` is `h / (t/2)`. Its reciprocal is the lateral acceleration at which
    /// the inside wheels leave the ground.
    pub fn roll_leverage(&self) -> f32 {
        self.cog_height / (self.track_width * 0.5).max(1.0e-3)
    }

    /// Lateral acceleration (in `g`) at which the inside wheels lift — the
    /// classic static rollover threshold.
    pub fn rollover_threshold(&self) -> f32 {
        1.0 / self.roll_leverage().max(1.0e-3)
    }

    /// Longitudinal load transferred per `g` of forward acceleration, as a
    /// fraction of the car's weight. Same free-body argument as
    /// [`Self::roll_leverage`], about the axles instead of the wheels.
    pub fn pitch_leverage(&self) -> f32 {
        self.cog_height / self.wheelbase.max(1.0e-3)
    }

    /// The fraction of the car's load that has moved to the outside wheels at
    /// `lateral_g` of cornering, clamped at `1.0` (everything on the outside —
    /// the inside wheels are airborne and there is nothing left to transfer).
    pub fn lateral_transfer(&self, lateral_g: f32) -> f32 {
        (lateral_g.abs() * self.roll_leverage()).clamp(0.0, 1.0)
    }

    /// Grip remaining, as a multiplier, once `transfer` of the load has moved
    /// onto the outside wheels.
    ///
    /// This is tyre **load sensitivity**: a tyre's grip grows less than linearly
    /// with the load on it, so a pair of wheels sharing the load evenly grips
    /// harder than one overloaded wheel and one unloaded one. That is *why* a
    /// low centre of gravity corners better, and it is the mechanism this whole
    /// module exists to give the car. Quadratic because the loss is negligible
    /// for gentle cornering and bites hard near the rollover limit.
    /// Because `transfer` is a fraction and saturates at `1.0`, the worst this
    /// can return is `1 - LOAD_SENSITIVITY` — grip degrades, it never vanishes,
    /// and that floor is a consequence of the arithmetic rather than a clamp
    /// bolted on top of it.
    pub fn grip_scale(&self, transfer: f32) -> f32 {
        let t = transfer.clamp(0.0, 1.0);
        1.0 - LOAD_SENSITIVITY * t * t
    }

    /// Turn-in authority, as a multiplier, from the static front load.
    ///
    /// The front tyres are what point the car, so their share of the weight sets
    /// how hard it bites on entry. Normalised so a 50/50 car is exactly `1.0`
    /// and the geometry is a pure *bias*, never a free grant of authority.
    pub fn turn_in_scale(&self) -> f32 {
        (1.0 + TURN_IN_GAIN * (self.front_load() - 0.5)).max(0.1)
    }
}

/// How much grip is lost at full lateral load transfer.
///
/// The one honestly arcade number in the module. Real tyre load sensitivity is
/// steeper than this; at these speeds a faithful value would make the car
/// undriveable, so the *shape* is physical and the *depth* is tuned.
const LOAD_SENSITIVITY: f32 = 0.3;

/// Turn-in authority gained per unit of front load bias.
const TURN_IN_GAIN: f32 = 0.8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipping_car_is_low_and_front_biased() {
        let g = ChassisGeometry::DEFAULT;
        assert!(
            g.cog_height < g.track_width * 0.5,
            "a centre of gravity above the half-track tips before it slides"
        );
        assert!(
            g.front_load() > 0.5,
            "the shipping car carries more weight on the front axle: {}",
            g.front_load()
        );
        assert!(
            g.forward_offset() > 0.0,
            "so its mass sits ahead of the wheelbase midpoint"
        );
        assert!((g.front_load() + g.rear_load() - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn a_lower_centre_of_gravity_transfers_less_load_and_keeps_more_grip() {
        let low = ChassisGeometry { cog_height: 0.35, ..ChassisGeometry::DEFAULT };
        let high = ChassisGeometry { cog_height: 0.80, ..ChassisGeometry::DEFAULT };
        let corner = 1.2;
        assert!(low.lateral_transfer(corner) < high.lateral_transfer(corner));
        assert!(
            low.grip_scale(low.lateral_transfer(corner))
                > high.grip_scale(high.lateral_transfer(corner)),
            "the low car corners harder — this is the whole point of the module"
        );
        assert!(low.rollover_threshold() > high.rollover_threshold());
    }

    #[test]
    fn transfer_saturates_when_the_inside_wheels_lift() {
        let g = ChassisGeometry::DEFAULT;
        let threshold = g.rollover_threshold();
        assert!((g.lateral_transfer(threshold) - 1.0).abs() < 1.0e-5);
        assert_eq!(g.lateral_transfer(threshold * 4.0), 1.0, "and stays there");
        // Symmetric: a left-hand corner transfers as much as a right-hand one.
        assert_eq!(g.lateral_transfer(-0.8), g.lateral_transfer(0.8));
    }

    #[test]
    fn straight_line_running_is_untouched_by_the_geometry() {
        let g = ChassisGeometry::DEFAULT;
        assert_eq!(g.lateral_transfer(0.0), 0.0);
        assert_eq!(g.grip_scale(0.0), 1.0, "no cornering, no penalty");
    }

    /// Even a preposterously tall car keeps most of its grip: the transfer
    /// fraction saturates, so the loss is bounded by construction.
    #[test]
    fn grip_never_falls_away_completely() {
        let g = ChassisGeometry { cog_height: 4.0, ..ChassisGeometry::DEFAULT };
        let worst = g.grip_scale(g.lateral_transfer(9.0));
        assert!((worst - (1.0 - LOAD_SENSITIVITY)).abs() < 1.0e-6);
        assert!(worst > 0.0, "grip degrades, it never vanishes");
        // And no input can push it below that bound.
        for t in [-5.0, 0.0, 0.5, 1.0, 40.0] {
            assert!(g.grip_scale(t) >= 1.0 - LOAD_SENSITIVITY);
            assert!(g.grip_scale(t) <= 1.0);
        }
    }

    #[test]
    fn a_balanced_car_gets_no_turn_in_bias_either_way() {
        let neutral = ChassisGeometry { cog_from_front: 0.5, ..ChassisGeometry::DEFAULT };
        assert!((neutral.turn_in_scale() - 1.0).abs() < 1.0e-6);
        assert!((neutral.forward_offset()).abs() < 1.0e-6);

        let front = ChassisGeometry { cog_from_front: 0.4, ..ChassisGeometry::DEFAULT };
        let rear = ChassisGeometry { cog_from_front: 0.6, ..ChassisGeometry::DEFAULT };
        assert!(front.turn_in_scale() > 1.0, "front weight bites");
        assert!(rear.turn_in_scale() < 1.0);
        assert!(front.forward_offset() > 0.0 && rear.forward_offset() < 0.0);
    }

    #[test]
    fn the_leverages_are_the_textbook_ratios() {
        let g = ChassisGeometry::DEFAULT;
        assert!((g.roll_leverage() - g.cog_height / (g.track_width * 0.5)).abs() < 1.0e-6);
        assert!((g.pitch_leverage() - g.cog_height / g.wheelbase).abs() < 1.0e-6);
        // A degenerate car cannot divide by zero.
        let flat = ChassisGeometry { track_width: 0.0, wheelbase: 0.0, ..g };
        assert!(flat.roll_leverage().is_finite() && flat.pitch_leverage().is_finite());
    }
}
