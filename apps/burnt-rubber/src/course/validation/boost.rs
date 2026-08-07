//! **Boost-sustain analysis**: can a skilled route keep the meter alive?
//!
//! Burnt Rubber's whole reward loop is one number, and a course that offers no
//! way to refill it is not a hard course — it is a broken one. So the validator
//! puts a budget on every section:
//!
//! ```text
//! seconds = section_length / expected_speed
//! earned  = chances · near_miss_boost · conversion · conversion_weight
//!         + seconds · high_speed_boost_rate · high_speed_share
//! spent   = seconds · boost_drain_rate · target_boost_duty
//! ratio   = earned / spent
//! ```
//!
//! Both halves of `earned` are real: threading traffic is the *interesting*
//! source of boost, and simply holding a high speed is the other one (see
//! `BoostMeter::step`). Leaving the second out is what made an early version of
//! this analysis call every section of the shipping course starved.
//!
//! and classifies the section by the ratio against the authored thresholds.
//!
//! # What this is, and what it is not
//!
//! It is a **reproducible approximation**, and it says so. It does not prove
//! that every possible skilled player can boost continuously; it measures the
//! opportunities the course actually compiled against the boost the intended
//! duty cycle actually costs, using the game's own numbers
//! (`RaceTuning::near_miss_boost`, `RaceTuning::boost_drain_rate`) rather than
//! invented ones. Where a ghost run is available its measured boost is folded in
//! ([`fold_ghost`]), which turns the estimate into a measurement for the one
//! route the ghost actually drove.
//!
//! Every threshold is authored in
//! [`ValidationThresholds`](crate::course::specification::ValidationThresholds),
//! not buried here: a course that wants a harsher economy says so in its own
//! source.

use crate::course::geometry::CompiledSection;
use crate::course::specification::ValidationThresholds;
use crate::course::traffic::NearMissWindow;
use crate::tuning::RaceTuning;

use super::report::{BoostStatus, SectionVerdict};

/// Classify one section's boost economy.
pub fn classify(
    section: &CompiledSection,
    windows: &[NearMissWindow],
    traversable: bool,
    narrowest_corridor_lanes: u32,
    thresholds: &ValidationThresholds,
    race: &RaceTuning,
) -> SectionVerdict {
    // Opportunities are weighted by how hard the window is: a chance that only
    // a very good player takes is not worth a whole near miss in a budget.
    let (opportunities, earned) = windows
        .iter()
        .filter(|w| overlaps(w, section))
        .fold((0u32, 0.0f32), |(count, boost), w| {
            let chances = w.intended_opportunities.max(1);
            (
                count + chances,
                boost
                    + chances as f32
                        * race.near_miss_boost
                        * thresholds.near_miss_conversion
                        * w.difficulty_weight.clamp(0.0, 1.0),
            )
        });

    let seconds = section.length_m() / section.expected_speed_mps.max(1.0);
    // The passive half: the meter fills on its own above the high-speed
    // threshold, and a skilled route is above it most of the time.
    let earned = earned
        + seconds * race.high_speed_boost_rate * thresholds.high_speed_share.clamp(0.0, 1.0);
    let spent = seconds * race.boost_drain_rate * thresholds.target_boost_duty.clamp(0.0, 1.0);
    let ratio = (spent > 1.0e-6)
        .then(|| earned / spent)
        .unwrap_or(f32::INFINITY);

    let status = (!traversable)
        .then_some(BoostStatus::Invalid)
        .unwrap_or_else(|| {
            (ratio < thresholds.starved_ratio)
                .then_some(BoostStatus::Starved)
                .unwrap_or_else(|| {
                    ((ratio >= thresholds.excellent_ratio)
                        & (narrowest_corridor_lanes >= thresholds.excellent_route_width))
                        .then_some(BoostStatus::Excellent)
                        .unwrap_or(BoostStatus::Acceptable)
                })
        });

    SectionVerdict {
        id: section.id.clone(),
        index: section.index,
        start_m: section.start_m,
        end_m: section.end_m,
        traversable,
        narrowest_corridor_lanes,
        opportunities,
        boost_earned: earned,
        boost_spent: spent,
        status,
    }
}

/// The course's own verdict, from the **whole** budget rather than the worst
/// section.
///
/// Folding the per-section statuses would let a sixty-metre link between two
/// bends -- which is too short to hold an opportunity and is not meant to --
/// condemn a nine-kilometre course. Boost is carried *across* sections: what
/// matters is whether the route as a whole can fund the duty cycle it intends,
/// and the per-section verdicts are the detail underneath that.
///
/// Traversability is the exception and is not averaged: one section with no
/// route through it makes the course invalid however good the economy is.
pub fn classify_course(
    sections: &[SectionVerdict],
    thresholds: &ValidationThresholds,
) -> BoostStatus {
    let blocked = sections.iter().any(|s| !s.traversable);
    let earned: f32 = sections.iter().map(|s| s.boost_earned).sum();
    let spent: f32 = sections.iter().map(|s| s.boost_spent).sum();
    let corridor = sections
        .iter()
        .map(|s| s.narrowest_corridor_lanes)
        .max()
        .unwrap_or(0);
    let ratio = (spent > 1.0e-6)
        .then(|| earned / spent)
        .unwrap_or(f32::INFINITY);
    blocked.then_some(BoostStatus::Invalid).unwrap_or_else(|| {
        (ratio < thresholds.starved_ratio)
            .then_some(BoostStatus::Starved)
            .unwrap_or_else(|| {
                ((ratio >= thresholds.excellent_ratio)
                    & (corridor >= thresholds.excellent_route_width))
                    .then_some(BoostStatus::Excellent)
                    .unwrap_or(BoostStatus::Acceptable)
            })
    })
}

/// Whether a window offers any of its chances inside a section.
fn overlaps(window: &NearMissWindow, section: &CompiledSection) -> bool {
    (window.end_m > section.start_m) & (window.start_m < section.end_m)
}

/// Fold a ghost run's measured boost into a course-level verdict.
///
/// A ghost that genuinely held boost for the target duty proves the estimate
/// was not optimistic; a ghost that ran dry demotes the course however good the
/// arithmetic looked. This is the one place the analysis stops being an estimate
/// — for the single route the ghost drove.
pub fn fold_ghost(
    estimate: BoostStatus,
    boost_fraction: f32,
    thresholds: &ValidationThresholds,
) -> BoostStatus {
    let target = thresholds.target_boost_duty.clamp(0.0, 1.0);
    // A measured duty well under the target is a starved course whatever the
    // opportunity count says.
    (boost_fraction < target * GHOST_STARVED_MARGIN)
        .then_some(estimate.worst(BoostStatus::Starved))
        .unwrap_or(estimate)
}

/// How far below the target duty a ghost may fall before the course counts as
/// starved.
///
/// Not 1.0: the ghost is one route driven by one technique, and holding *most*
/// of the intended duty is evidence the economy works, not evidence it does
/// not.
pub const GHOST_STARVED_MARGIN: f32 = 0.6;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::{
        EncounterId, PassingSide, ScalarRange, SectionId, SectionKind, VehicleId,
    };

    fn section(length_m: f32) -> CompiledSection {
        CompiledSection {
            id: SectionId::new("s"),
            index: 0,
            start_m: 0.0,
            end_m: length_m,
            environment: SectionKind::StartStraight,
            expected_speed_mps: 80.0,
            lanes: 5,
            primitive: "straight",
        }
    }

    fn window(start_m: f32, end_m: f32, chances: u32, difficulty: f32) -> NearMissWindow {
        NearMissWindow {
            encounter: Some(EncounterId(0)),
            start_m,
            end_m,
            vehicles: vec![VehicleId(0)],
            clearance_m: ScalarRange::new(0.4, 1.4),
            side: PassingSide::Either,
            minimum_relative_speed_mps: 8.0,
            intended_opportunities: chances,
            difficulty_weight: difficulty,
            section: 0,
        }
    }

    #[test]
    fn a_section_with_no_opportunities_is_starved() {
        let verdict = classify(
            &section(1_000.0),
            &[],
            true,
            3,
            &ValidationThresholds::DEFAULT,
            &RaceTuning::DEFAULT,
        );
        assert_eq!(verdict.status, BoostStatus::Starved);
        assert_eq!(verdict.opportunities, 0);
        assert!(verdict.boost_spent > 0.0);
        // Not zero: holding a high speed fills the meter on its own. It is just
        // nowhere near enough to fund the intended duty cycle.
        assert!(verdict.boost_earned > 0.0);
        assert!(verdict.ratio() < 1.0, "ratio {}", verdict.ratio());
    }

    #[test]
    fn an_untraversable_section_is_invalid_however_rich_it_is() {
        let verdict = classify(
            &section(1_000.0),
            &[window(0.0, 1_000.0, 50, 1.0)],
            false,
            0,
            &ValidationThresholds::DEFAULT,
            &RaceTuning::DEFAULT,
        );
        assert_eq!(verdict.status, BoostStatus::Invalid);
        assert!(verdict.boost_earned > verdict.boost_spent);
    }

    #[test]
    fn the_classification_walks_starved_acceptable_excellent_as_chances_are_added() {
        let s = section(1_000.0);
        let t = ValidationThresholds::DEFAULT;
        let r = RaceTuning::DEFAULT;
        // 1000 m at 80 m/s is 12.5 s. Spent: 12.5 * 0.36 * 0.35 = 1.575 of the
        // meter. Earned passively: 12.5 * 0.075 * 0.8 = 0.75. A near miss pays
        // 0.13, converted at 0.72, so each chance adds 0.0936 — about nine
        // chances to break even, and about nineteen to reach the excellent bar.
        let status_for = |chances: u32, corridor: u32| {
            classify(&s, &[window(0.0, 1_000.0, chances, 1.0)], true, corridor, &t, &r).status
        };
        assert_eq!(status_for(4, 3), BoostStatus::Starved);
        assert_eq!(status_for(12, 3), BoostStatus::Acceptable);
        assert_eq!(status_for(30, 3), BoostStatus::Excellent);
        // Excellent needs more than one route as well as surplus boost.
        assert_eq!(status_for(30, 1), BoostStatus::Acceptable);
    }

    #[test]
    fn difficulty_weight_discounts_a_hard_chance() {
        let s = section(1_000.0);
        let t = ValidationThresholds::DEFAULT;
        let r = RaceTuning::DEFAULT;
        let easy = classify(&s, &[window(0.0, 1_000.0, 20, 1.0)], true, 3, &t, &r);
        let hard = classify(&s, &[window(0.0, 1_000.0, 20, 0.4)], true, 3, &t, &r);
        assert_eq!(easy.opportunities, hard.opportunities);
        assert!(hard.boost_earned < easy.boost_earned);
    }

    #[test]
    fn only_windows_that_overlap_the_section_count() {
        let s = CompiledSection {
            start_m: 1_000.0,
            end_m: 2_000.0,
            ..section(1_000.0)
        };
        let t = ValidationThresholds::DEFAULT;
        let r = RaceTuning::DEFAULT;
        let inside = classify(&s, &[window(1_400.0, 1_500.0, 5, 1.0)], true, 3, &t, &r);
        assert_eq!(inside.opportunities, 5);
        let before = classify(&s, &[window(0.0, 999.0, 5, 1.0)], true, 3, &t, &r);
        assert_eq!(before.opportunities, 0);
        let after = classify(&s, &[window(2_001.0, 2_500.0, 5, 1.0)], true, 3, &t, &r);
        assert_eq!(after.opportunities, 0);
        // A window that straddles the boundary does count.
        let straddling = classify(&s, &[window(900.0, 1_100.0, 5, 1.0)], true, 3, &t, &r);
        assert_eq!(straddling.opportunities, 5);
    }

    #[test]
    fn a_zero_length_section_spends_nothing_and_is_never_starved() {
        let s = CompiledSection {
            start_m: 100.0,
            end_m: 100.0,
            ..section(0.0)
        };
        let verdict = classify(
            &s,
            &[],
            true,
            3,
            &ValidationThresholds::DEFAULT,
            &RaceTuning::DEFAULT,
        );
        assert_eq!(verdict.boost_spent, 0.0);
        assert_eq!(verdict.ratio(), f32::INFINITY);
        assert_ne!(verdict.status, BoostStatus::Starved);
    }

    #[test]
    fn a_ghost_that_ran_dry_demotes_the_estimate() {
        let t = ValidationThresholds::DEFAULT;
        // The target duty is 0.55; a ghost holding 0.5 is fine.
        assert_eq!(
            fold_ghost(BoostStatus::Excellent, 0.5, &t),
            BoostStatus::Excellent
        );
        // One holding 0.1 is not, however good the arithmetic looked.
        assert_eq!(
            fold_ghost(BoostStatus::Excellent, 0.1, &t),
            BoostStatus::Starved
        );
        // And it can never *promote* a course.
        assert_eq!(
            fold_ghost(BoostStatus::Invalid, 0.9, &t),
            BoostStatus::Invalid
        );
        assert_eq!(
            fold_ghost(BoostStatus::Starved, 0.9, &t),
            BoostStatus::Starved
        );
    }

    #[test]
    fn the_classification_is_stable_for_the_same_input() {
        let s = section(1_000.0);
        let w = [window(0.0, 1_000.0, 25, 0.8)];
        let t = ValidationThresholds::DEFAULT;
        let r = RaceTuning::DEFAULT;
        let a = classify(&s, &w, true, 3, &t, &r);
        let b = classify(&s, &w, true, 3, &t, &r);
        assert_eq!(a, b);
    }
}
