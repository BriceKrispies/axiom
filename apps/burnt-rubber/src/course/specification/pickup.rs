//! **Authored boost pickups**: charge the course hands out for driving a line.
//!
//! A pickup is the one thing on the course that is *placed* rather than
//! *generated*. Traffic is a density description, an encounter is a figure, a
//! near-miss window is a projection — all three compile into something the
//! author did not write out car by car. A pickup is written out: this tier, in
//! this lane, this far along. There is nothing to draw for it and no seed stream
//! behind it, which is why [`crate::course::compiler::seeds`] gains no domain
//! here.
//!
//! # Why a tier and not an amount
//!
//! A pickup could carry `boost = 0.3` and be done with. It carries a
//! [`BoostTier`] instead, because the amount and the colour have to agree: green
//! is small, blue is medium, red is large, and a player learns that mapping in
//! the first thirty seconds and then reads the road with it. If the amount were
//! authored per pickup, two pickups the same colour could pay differently, and
//! the colour would stop being information. The tier is the identity; the amount
//! ([`crate::tuning::RaceTuning::pickup_boost`]) and the material
//! (`render::pickups`) are both derived from it, in one place each.
//!
//! # Rows
//!
//! Pickups come in runs far more often than they come alone — a line of three
//! down the inside of a bend is the shape that reads as "take this line". That
//! is [`BoostPickupSpec::count`] and [`BoostPickupSpec::spacing_m`], bounded by
//! [`MAX_PICKUP_ROW`], rather than three authored entries: the row is one
//! authored intention and editing it should be one edit.

use crate::course::error::{finite, positive, CourseError, CourseErrorCode, CourseResult};

/// How much charge a pickup is worth, as a named tier.
///
/// The set is closed and small on purpose. Three tiers is as many distinct
/// colours as a player can tell apart at 300 km/h through a windscreen, and a
/// fourth would be a colour nobody could name under motion blur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BoostTier {
    /// A sip — green. Worth roughly one near miss.
    Small,
    /// A useful top-up — blue.
    Medium,
    /// A real shove — red.
    Large,
}

impl BoostTier {
    /// Every tier, weakest first. The order is meaningful: it is the order the
    /// amounts increase in, and `the_tiers_are_ordered_by_what_they_pay` pins
    /// that the tuning agrees.
    pub const ALL: [BoostTier; 3] = [BoostTier::Small, BoostTier::Medium, BoostTier::Large];

    /// The DSL token and dump keyword.
    pub const fn token(self) -> &'static str {
        match self {
            BoostTier::Small => "small",
            BoostTier::Medium => "medium",
            BoostTier::Large => "large",
        }
    }

    /// A dense index, for looking the tier up in a per-tier table (the amounts,
    /// the three material handles, the three visual pools).
    pub const fn index(self) -> usize {
        match self {
            BoostTier::Small => 0,
            BoostTier::Medium => 1,
            BoostTier::Large => 2,
        }
    }

    /// Resolve a DSL token, naming what does exist when it is not one.
    pub fn parse(token: &str) -> CourseResult<BoostTier> {
        BoostTier::ALL
            .into_iter()
            .find(|t| t.token() == token)
            .ok_or_else(|| {
                let known = BoostTier::ALL.map(BoostTier::token).join(", ");
                CourseError::new(
                    CourseErrorCode::UnknownField,
                    format!("`{token}` is not a boost tier — the tiers are: {known}"),
                )
                .in_field("boost")
            })
    }
}

/// The most pickups one authored row may place.
///
/// A bound, not a suggestion, for the same reason [`super::MAX_MOTIF_COUNT`] is
/// one: the row is expanded eagerly into concrete pickups at compile time, so an
/// unbounded count is an unbounded compile. Sixteen at any sane spacing is
/// already most of a section.
pub const MAX_PICKUP_ROW: u32 = 16;

/// One authored pickup, or one authored row of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoostPickupSpec {
    /// Where the first one sits, from the start of its zone (m).
    ///
    /// Zone-relative, exactly like an encounter's `at`, so moving a section does
    /// not mean editing every pickup inside it.
    pub start_offset_m: f32,
    /// The lane it sits in, numbered out from the centreline.
    ///
    /// A **lane**, never a lateral offset. The compiled pickup keeps the lane
    /// too and resolves the offset at runtime, so a pickup authored in lane `+2`
    /// stays in lane `+2` through a width transition instead of sliding onto the
    /// verge.
    pub lane: i32,
    /// What it pays, and therefore what colour it is.
    pub tier: BoostTier,
    /// How many, counting the first. `1` is a single pickup.
    pub count: u32,
    /// The gap between consecutive pickups in the row (m). Ignored when
    /// [`Self::count`] is `1`.
    pub spacing_m: f32,
}

impl BoostPickupSpec {
    /// One pickup of `tier` in `lane`, `start_offset_m` into its zone.
    pub const fn single(start_offset_m: f32, lane: i32, tier: BoostTier) -> BoostPickupSpec {
        BoostPickupSpec {
            start_offset_m,
            lane,
            tier,
            count: 1,
            spacing_m: DEFAULT_ROW_SPACING_M,
        }
    }

    /// A row of `count`, `spacing_m` apart.
    pub const fn row(
        start_offset_m: f32,
        lane: i32,
        tier: BoostTier,
        count: u32,
        spacing_m: f32,
    ) -> BoostPickupSpec {
        BoostPickupSpec {
            count,
            spacing_m,
            ..BoostPickupSpec::single(start_offset_m, lane, tier)
        }
    }

    /// How much road the whole row covers (m).
    pub fn length_m(&self) -> f32 {
        self.spacing_m * self.count.saturating_sub(1) as f32
    }

    /// Reject a row that cannot be placed. `lane_reach` is the widest lane index
    /// the course could ever offer, which is a *structural* check — whether the
    /// road actually has that lane **here** is a question about compiled
    /// geometry and belongs to the validator.
    pub fn validate(&self, lane_reach: i32) -> CourseResult<()> {
        finite(self.start_offset_m, "at")?;
        ((self.count >= 1) & (self.count <= MAX_PICKUP_ROW))
            .then_some(())
            .ok_or_else(|| {
                CourseError::new(
                    CourseErrorCode::RepeatLimitExceeded,
                    format!(
                        "a pickup row places 1..{MAX_PICKUP_ROW} pickups, not {}",
                        self.count
                    ),
                )
                .in_field("count")
            })?;
        // Spacing only has to be real when there is a gap to space.
        (self.count == 1)
            .then_some(0.0)
            .map(Ok)
            .unwrap_or_else(|| {
                positive(
                    self.spacing_m,
                    "spacing",
                    CourseErrorCode::InvalidSectionLength,
                )
            })?;
        (self.lane.abs() <= lane_reach)
            .then_some(())
            .ok_or_else(|| {
                CourseError::new(
                    CourseErrorCode::InvalidPickupLane,
                    format!(
                        "a pickup is authored in lane {} on a course that reaches {lane_reach} \
                         lanes either side of the centreline",
                        self.lane
                    ),
                )
                .in_field("lane")
            })?;
        Ok(())
    }
}

/// The gap a row uses when it does not author one (m).
///
/// Sized so a row reads as a *line* rather than as separate pickups: at the
/// course's expected speed, 34 m is a little under half a second apart, which is
/// close enough that taking the first commits you to the rest.
pub const DEFAULT_ROW_SPACING_M: f32 = 34.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tier_round_trips_through_its_token_and_has_a_dense_index() {
        for tier in BoostTier::ALL {
            assert_eq!(BoostTier::parse(tier.token()).unwrap(), tier);
        }
        let indices: Vec<usize> = BoostTier::ALL.iter().map(|t| t.index()).collect();
        assert_eq!(indices, vec![0, 1, 2], "the index is dense and in order");
        let mut tokens: Vec<&str> = BoostTier::ALL.iter().map(|t| t.token()).collect();
        tokens.sort_unstable();
        let count = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), count, "two tiers share a token");
    }

    #[test]
    fn the_tiers_are_ordered_weakest_first() {
        assert!(BoostTier::Small < BoostTier::Medium);
        assert!(BoostTier::Medium < BoostTier::Large);
    }

    #[test]
    fn an_unknown_tier_is_named_and_lists_the_real_ones() {
        let err = BoostTier::parse("enormous").unwrap_err();
        assert_eq!(err.code, CourseErrorCode::UnknownField);
        assert!(err.message.contains("enormous"), "{}", err.message);
        assert!(err.message.contains("medium"), "{}", err.message);
    }

    #[test]
    fn a_single_pickup_is_a_row_of_one_and_covers_no_road() {
        let one = BoostPickupSpec::single(120.0, -1, BoostTier::Small);
        assert_eq!(one.count, 1);
        assert_eq!(one.length_m(), 0.0);
        assert!(one.validate(2).is_ok());
    }

    #[test]
    fn a_row_covers_the_gaps_between_its_pickups_not_one_per_pickup() {
        let row = BoostPickupSpec::row(100.0, 0, BoostTier::Medium, 4, 30.0);
        // Four pickups have three gaps between them.
        assert_eq!(row.length_m(), 90.0);
        assert!(row.validate(2).is_ok());
    }

    #[test]
    fn an_unplaceable_row_is_rejected_with_the_right_code() {
        let base = BoostPickupSpec::row(100.0, 0, BoostTier::Large, 3, 30.0);
        assert_eq!(
            BoostPickupSpec { start_offset_m: f32::NAN, ..base }
                .validate(2)
                .unwrap_err()
                .code,
            CourseErrorCode::InvalidFiniteScalar
        );
        assert_eq!(
            BoostPickupSpec { count: 0, ..base }.validate(2).unwrap_err().code,
            CourseErrorCode::RepeatLimitExceeded
        );
        assert_eq!(
            BoostPickupSpec { count: MAX_PICKUP_ROW + 1, ..base }
                .validate(2)
                .unwrap_err()
                .code,
            CourseErrorCode::RepeatLimitExceeded
        );
        assert_eq!(
            BoostPickupSpec { spacing_m: 0.0, ..base }
                .validate(2)
                .unwrap_err()
                .code,
            CourseErrorCode::InvalidSectionLength
        );
        assert_eq!(
            BoostPickupSpec { lane: 4, ..base }.validate(2).unwrap_err().code,
            CourseErrorCode::InvalidPickupLane
        );
        // A single pickup does not need a spacing at all, so a zero one is not a
        // failure — there is no gap for it to describe.
        assert!(BoostPickupSpec { count: 1, spacing_m: 0.0, ..base }
            .validate(2)
            .is_ok());
    }
}
