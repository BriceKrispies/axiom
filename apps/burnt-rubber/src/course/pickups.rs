//! **Compiled boost pickups**: the concrete, immutable charge the course offers.
//!
//! An authored row ([`BoostPickupSpec`]) becomes a flat list of
//! [`BoostPickup`]s, one per pickup, sorted by course distance and indexed the
//! same way traffic is. By the time the game runs there is no row left — the
//! runtime sees only "there is a large pickup at 4 218 m in lane −1", which is
//! all it ever needed to know.
//!
//! # What is *not* here
//!
//! A pickup carries no lateral offset, no world position and no visual state. It
//! carries a **lane**, and [`crate::track::Track::lane_lateral`] answers where
//! that is at any distance — the same discipline
//! [`crate::course::traffic::TrafficPlan`] keeps, and for the same reason: a road
//! that widens must move the pickup with its lane, not leave it where the
//! centreline used to be.
//!
//! It also carries no "taken" flag. Whether the player has collected one is a
//! property of *this run*, not of the course, and the course plan is shared by
//! `Arc` between the live race, the ghost and any replay — a mutable flag on it
//! would be one run reaching into another's.

use crate::course::specification::{BoostPickupSpec, BoostTier, PickupId};

/// One concrete pickup, fully determined before the game starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoostPickup {
    /// Stable identity. Dense, and ordered by [`Self::at_m`].
    pub id: PickupId,
    /// Where it stands along the course (m).
    pub at_m: f32,
    /// The lane it stands in, numbered out from the centreline.
    pub lane: i32,
    /// What it pays, and what colour it is.
    pub tier: BoostTier,
    /// The section it falls in.
    pub section: u16,
}

/// Expand one authored row into its concrete pickups.
///
/// `zone_start_m` is where the row's offsets are measured from, `next_id` mints
/// the identities in course order, and `section_of` resolves which compiled
/// section each one landed in. Pure: the same row at the same zone start always
/// produces the same pickups, which is what makes a recompiled course
/// byte-identical.
pub fn expand_row(
    spec: &BoostPickupSpec,
    zone_start_m: f32,
    next_id: &mut u32,
    section_of: &impl Fn(f32) -> u16,
) -> Vec<BoostPickup> {
    (0..spec.count.max(1))
        .map(|k| {
            let at_m = zone_start_m + spec.start_offset_m + spec.spacing_m * k as f32;
            let id = PickupId(*next_id);
            *next_id += 1;
            BoostPickup {
                id,
                at_m,
                lane: spec.lane,
                tier: spec.tier,
                section: section_of(at_m),
            }
        })
        .collect()
}

/// How much road one pickup occupies either side of its centre, for the
/// purposes of "two of these are on top of each other" (m).
///
/// Not a collect radius — that is [`crate::tuning::RaceTuning::pickup_reach_m`],
/// which is about the *car* and lives with the rest of the race rules. This is
/// purely the authoring question of whether two placements are distinguishable,
/// and it is deliberately generous: two pickups four metres apart in one lane are
/// one pickup as far as any player can tell, and taking the first takes the
/// second.
pub const PICKUP_FOOTPRINT_M: f32 = 6.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn section_of(_: f32) -> u16 {
        3
    }

    #[test]
    fn a_single_pickup_expands_to_one_at_its_offset() {
        let mut next = 0;
        let out = expand_row(
            &BoostPickupSpec::single(120.0, -1, BoostTier::Small),
            1_000.0,
            &mut next,
            &section_of,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].at_m, 1_120.0);
        assert_eq!(out[0].lane, -1);
        assert_eq!(out[0].tier, BoostTier::Small);
        assert_eq!(out[0].id, PickupId(0));
        assert_eq!(out[0].section, 3);
        assert_eq!(next, 1, "the id counter advanced exactly once");
    }

    #[test]
    fn a_row_expands_in_course_order_with_dense_identities() {
        let mut next = 5;
        let out = expand_row(
            &BoostPickupSpec::row(200.0, 2, BoostTier::Large, 4, 30.0),
            0.0,
            &mut next,
            &section_of,
        );
        assert_eq!(
            out.iter().map(|p| p.at_m).collect::<Vec<_>>(),
            vec![200.0, 230.0, 260.0, 290.0]
        );
        assert_eq!(
            out.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![PickupId(5), PickupId(6), PickupId(7), PickupId(8)]
        );
        assert!(out.iter().all(|p| p.tier == BoostTier::Large));
        assert!(out.iter().all(|p| p.lane == 2));
        assert_eq!(next, 9);
    }

    /// A count of zero is rejected at the specification boundary, but expansion
    /// must not be able to produce nothing from a row that reached it anyway —
    /// an empty row would leave an id gap and a silently-missing pickup.
    #[test]
    fn a_degenerate_count_still_places_one() {
        let mut next = 0;
        let out = expand_row(
            &BoostPickupSpec {
                count: 0,
                ..BoostPickupSpec::single(10.0, 0, BoostTier::Medium)
            },
            0.0,
            &mut next,
            &section_of,
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn expansion_is_a_pure_function_of_the_row_and_its_zone() {
        let row = BoostPickupSpec::row(80.0, 1, BoostTier::Medium, 3, 25.0);
        let once = expand_row(&row, 500.0, &mut 0, &section_of);
        let twice = expand_row(&row, 500.0, &mut 0, &section_of);
        assert_eq!(once, twice);
        let elsewhere = expand_row(&row, 900.0, &mut 0, &section_of);
        assert_ne!(once, elsewhere);
    }
}
