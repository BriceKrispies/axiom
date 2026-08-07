//! **The unit convention**, and the two range types the authored model draws
//! deterministic values from.
//!
//! # The convention
//!
//! Burnt Rubber's simulation is SI throughout (`crate::tuning`), and the course
//! specification keeps that and adds one rule: **every scalar field names its
//! unit in its own name**.
//!
//! | Suffix | Unit |
//! |---|---|
//! | `_m` | metres |
//! | `_km` | kilometres |
//! | `_mps` | metres per second |
//! | `_rad` | radians |
//! | `_s` | seconds |
//! | `_weight` | a non-negative dimensionless weight |
//! | `_probability` | `0..1` |
//! | (none) | a count, an index or an identity |
//!
//! There is deliberately **no dimensioned-quantity framework** here. The engine
//! kernel already has `Meters`/`Radians`/`Ratio`, but the course model is a wide
//! record of plain authored numbers that has to round-trip through a text DSL
//! and a validator, and wrapping several hundred fields in newtypes would buy
//! nothing the naming rule does not already buy: `amplitude_m` and
//! `wavelength_m` cannot be swapped by accident any more than `Meters` values
//! could, because they are named for what they are. The one place a unit is
//! genuinely *ambiguous* — a DSL literal — is exactly where a unit suffix is
//! **mandatory** (`700m`, `18deg`, `180mph`, `0.75s`), and [`Unit`] resolves it
//! to the SI field the record stores.

use crate::course::error::{CourseError, CourseErrorCode, CourseResult};
use crate::draw::Draw;

/// Metres per second in one mile per hour.
pub const MPH_TO_MPS: f32 = 0.447_04;
/// Metres per second in one kilometre per hour.
pub const KMH_TO_MPS: f32 = 1.0 / 3.6;

/// A unit a DSL literal may carry, and what it converts to.
///
/// The set is closed: an unrecognised suffix is [`CourseErrorCode::InvalidUnit`]
/// rather than a silently-accepted bare number, because "180" with no unit in a
/// speed field is the single most expensive typo this grammar can contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Metres.
    Metres,
    /// Kilometres.
    Kilometres,
    /// Degrees (stored as radians).
    Degrees,
    /// Radians.
    Radians,
    /// Seconds.
    Seconds,
    /// Metres per second.
    MetresPerSecond,
    /// Kilometres per hour (stored as m/s).
    KilometresPerHour,
    /// Miles per hour (stored as m/s).
    MilesPerHour,
}

impl Unit {
    /// Resolve a literal suffix. `None` is not an error here — the caller
    /// decides whether a bare number is legal for the field it is filling.
    pub fn parse(suffix: &str) -> Option<Unit> {
        match suffix {
            "m" => Some(Unit::Metres),
            "km" => Some(Unit::Kilometres),
            "deg" => Some(Unit::Degrees),
            "rad" => Some(Unit::Radians),
            "s" => Some(Unit::Seconds),
            "mps" => Some(Unit::MetresPerSecond),
            "kmh" => Some(Unit::KilometresPerHour),
            "mph" => Some(Unit::MilesPerHour),
            _ => None,
        }
    }

    /// Convert a literal in this unit to the SI value the record stores.
    pub fn to_si(self, value: f32) -> f32 {
        match self {
            Unit::Metres | Unit::Radians | Unit::Seconds | Unit::MetresPerSecond => value,
            Unit::Kilometres => value * 1_000.0,
            Unit::Degrees => value.to_radians(),
            Unit::KilometresPerHour => value * KMH_TO_MPS,
            Unit::MilesPerHour => value * MPH_TO_MPS,
        }
    }

    /// The dimension this unit measures — how a field declares what it will
    /// accept.
    pub const fn dimension(self) -> Dimension {
        match self {
            Unit::Metres | Unit::Kilometres => Dimension::Length,
            Unit::Degrees | Unit::Radians => Dimension::Angle,
            Unit::Seconds => Dimension::Time,
            Unit::MetresPerSecond | Unit::KilometresPerHour | Unit::MilesPerHour => {
                Dimension::Speed
            }
        }
    }
}

/// What a field measures. A literal whose unit is of the wrong dimension is
/// rejected — this is what stops `length = 4s` from compiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    /// A length, stored in metres.
    Length,
    /// An angle, stored in radians.
    Angle,
    /// A duration, stored in seconds.
    Time,
    /// A speed, stored in metres per second.
    Speed,
    /// A dimensionless count, weight, ratio or probability.
    Scalar,
}

impl Dimension {
    /// The name used in diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            Dimension::Length => "a length",
            Dimension::Angle => "an angle",
            Dimension::Time => "a duration",
            Dimension::Speed => "a speed",
            Dimension::Scalar => "a plain number",
        }
    }
}

/// An inclusive range of authored scalars a generator draws a value from.
///
/// A single value is the degenerate range `lo == hi`, so a field can accept
/// either `90m` or `90m..150m` without two types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalarRange {
    /// The low bound (inclusive).
    pub lo: f32,
    /// The high bound (inclusive).
    pub hi: f32,
}

impl ScalarRange {
    /// The degenerate range that is exactly `value`.
    pub const fn exact(value: f32) -> ScalarRange {
        ScalarRange { lo: value, hi: value }
    }

    /// A range from `lo` to `hi`.
    pub const fn new(lo: f32, hi: f32) -> ScalarRange {
        ScalarRange { lo, hi }
    }

    /// The midpoint — what a validator uses where it wants one representative
    /// value rather than a draw.
    pub fn midpoint(self) -> f32 {
        (self.lo + self.hi) * 0.5
    }

    /// Draw a value deterministically from `draw`.
    pub fn sample(self, draw: &mut Draw) -> f32 {
        draw.range(self.lo, self.hi)
    }

    /// Reject a reversed, non-finite or (optionally) non-positive range.
    pub fn validate(self, field: &str, positive: bool) -> CourseResult<ScalarRange> {
        crate::course::error::finite(self.lo, field)?;
        crate::course::error::finite(self.hi, field)?;
        (self.hi >= self.lo).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidSpeedRange,
                format!(
                    "`{field}` is reversed: {}..{} — the low bound must not exceed the high one",
                    self.lo, self.hi
                ),
            )
            .in_field(field)
        })?;
        (!positive || self.lo > 0.0).then_some(self).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidSpeedRange,
                format!("`{field}` must be positive, got {}..{}", self.lo, self.hi),
            )
            .in_field(field)
        })
    }
}

/// An inclusive range of authored counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountRange {
    /// The low bound (inclusive).
    pub lo: u32,
    /// The high bound (inclusive).
    pub hi: u32,
}

impl CountRange {
    /// The degenerate range that is exactly `value`.
    pub const fn exact(value: u32) -> CountRange {
        CountRange { lo: value, hi: value }
    }

    /// A range from `lo` to `hi`.
    pub const fn new(lo: u32, hi: u32) -> CountRange {
        CountRange { lo, hi }
    }

    /// Draw a count deterministically from `draw`.
    pub fn sample(self, draw: &mut Draw) -> u32 {
        draw.range_u32(self.lo, self.hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unit_resolves_and_converts_to_si() {
        assert_eq!(Unit::parse("m"), Some(Unit::Metres));
        assert_eq!(Unit::parse("km"), Some(Unit::Kilometres));
        assert_eq!(Unit::parse("deg"), Some(Unit::Degrees));
        assert_eq!(Unit::parse("rad"), Some(Unit::Radians));
        assert_eq!(Unit::parse("s"), Some(Unit::Seconds));
        assert_eq!(Unit::parse("mps"), Some(Unit::MetresPerSecond));
        assert_eq!(Unit::parse("kmh"), Some(Unit::KilometresPerHour));
        assert_eq!(Unit::parse("mph"), Some(Unit::MilesPerHour));
        assert_eq!(Unit::parse("furlongs"), None);

        assert_eq!(Unit::Metres.to_si(700.0), 700.0);
        assert_eq!(Unit::Kilometres.to_si(1.5), 1_500.0);
        assert!((Unit::Degrees.to_si(180.0) - std::f32::consts::PI).abs() < 1.0e-6);
        assert_eq!(Unit::Radians.to_si(1.0), 1.0);
        assert_eq!(Unit::Seconds.to_si(0.75), 0.75);
        assert_eq!(Unit::MetresPerSecond.to_si(30.0), 30.0);
        assert!((Unit::KilometresPerHour.to_si(360.0) - 100.0).abs() < 1.0e-4);
        assert!((Unit::MilesPerHour.to_si(180.0) - 80.467_2).abs() < 1.0e-3);
    }

    #[test]
    fn units_report_the_dimension_they_measure() {
        assert_eq!(Unit::Metres.dimension(), Dimension::Length);
        assert_eq!(Unit::Kilometres.dimension(), Dimension::Length);
        assert_eq!(Unit::Degrees.dimension(), Dimension::Angle);
        assert_eq!(Unit::Radians.dimension(), Dimension::Angle);
        assert_eq!(Unit::Seconds.dimension(), Dimension::Time);
        assert_eq!(Unit::MilesPerHour.dimension(), Dimension::Speed);
        assert_eq!(Unit::KilometresPerHour.dimension(), Dimension::Speed);
        assert_eq!(Unit::MetresPerSecond.dimension(), Dimension::Speed);
        for d in [
            Dimension::Length,
            Dimension::Angle,
            Dimension::Time,
            Dimension::Speed,
            Dimension::Scalar,
        ] {
            assert!(!d.name().is_empty());
        }
    }

    #[test]
    fn a_scalar_range_draws_inside_itself_and_repeats_for_a_seed() {
        let range = ScalarRange::new(90.0, 150.0);
        let take = || {
            let mut draw = Draw::seeded(9);
            (0..64).map(|_| range.sample(&mut draw)).collect::<Vec<f32>>()
        };
        let values = take();
        assert_eq!(values, take(), "the same seed draws the same values");
        assert!(values.iter().all(|v| (90.0..=150.0).contains(v)));
        assert_eq!(range.midpoint(), 120.0);
        assert_eq!(ScalarRange::exact(4.0).sample(&mut Draw::seeded(1)), 4.0);
    }

    #[test]
    fn a_count_range_draws_inside_itself() {
        let range = CountRange::new(3, 6);
        let mut draw = Draw::seeded(3);
        for _ in 0..64 {
            assert!((3..=6).contains(&range.sample(&mut draw)));
        }
        assert_eq!(CountRange::exact(4).sample(&mut Draw::seeded(1)), 4);
    }

    #[test]
    fn a_reversed_or_non_finite_range_is_rejected() {
        assert!(ScalarRange::new(10.0, 20.0).validate("speed_mps", true).is_ok());
        let reversed = ScalarRange::new(20.0, 10.0)
            .validate("speed_mps", true)
            .unwrap_err();
        assert_eq!(reversed.code, CourseErrorCode::InvalidSpeedRange);
        let negative = ScalarRange::new(-1.0, 10.0)
            .validate("speed_mps", true)
            .unwrap_err();
        assert_eq!(negative.code, CourseErrorCode::InvalidSpeedRange);
        assert!(ScalarRange::new(-1.0, 10.0).validate("bank_rad", false).is_ok());
        let nan = ScalarRange::new(f32::NAN, 10.0)
            .validate("speed_mps", true)
            .unwrap_err();
        assert_eq!(nan.code, CourseErrorCode::InvalidFiniteScalar);
        let nan_hi = ScalarRange::new(1.0, f32::NAN)
            .validate("speed_mps", true)
            .unwrap_err();
        assert_eq!(nan_hi.code, CourseErrorCode::InvalidFiniteScalar);
    }
}
