//! The numbers validation judges a course against — **authored, not buried**.
//!
//! Every threshold the validator uses lives in one record that is part of the
//! course specification, so "this course is starved of boost" is a statement
//! about a number the author can see and change, not about a constant hidden in
//! the analysis. A course that wants a harder boost economy says so.

use crate::course::error::{finite, positive, CourseErrorCode, CourseResult};

/// The bounds and targets validation measures a compiled course against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValidationThresholds {
    /// The tightest turn radius any primitive may author (m).
    pub min_turn_radius_m: f32,
    /// The steepest grade (rise over run) the compiled road may reach.
    ///
    /// A course-level envelope rather than an engine constant: a rolling
    /// motorway and a road that screws its way down a ridge want different
    /// answers, and the author of each is the one who knows which. Geometry that
    /// exceeds it is clamped and the section is named in a warning.
    pub max_grade: f32,
    /// The steepest banking the compiled road may reach (rad).
    ///
    /// Same reasoning as [`Self::max_grade`]. `bank_per_curvature` — how hard
    /// the road leans per unit of corner — stays in
    /// [`CourseTuning`](crate::tuning::CourseTuning), because that is a property
    /// of the game's roads; this is how far a *particular* course lets that go.
    pub max_bank_rad: f32,
    /// Largest curvature step between adjacent samples the geometry may contain
    /// (rad/m). Above this the road kinks visibly.
    pub max_curvature_step: f32,
    /// Largest grade step between adjacent samples (rise/run).
    pub max_grade_step: f32,
    /// Largest bank step between adjacent samples (rad).
    pub max_bank_step: f32,
    /// How far apart the traversability grid samples the course (m).
    ///
    /// It has to be coarse enough that **one column can contain one lane
    /// change**: a 3.5 m lane at [`Self::lateral_speed_mps`] takes about 0.3 s,
    /// which is 24 m at racing speed. A finer grid cannot express a lane change
    /// at all, and the validator reports that as a configuration error rather
    /// than silently deciding nothing is passable.
    pub traversal_step_m: f32,
    /// How fast the player can move sideways when threading (m/s). Sets how
    /// many lanes a grid step can cross.
    pub lateral_speed_mps: f32,
    /// Extra lateral margin beyond the two half-widths before a cell counts as
    /// passable (m).
    pub lateral_margin_m: f32,
    /// The least warning the player must have of any vehicle (s).
    pub min_reaction_time_s: f32,
    /// The fraction of compiled near-miss opportunities a skilled route is
    /// assumed to convert, `0..1`.
    pub near_miss_conversion: f32,
    /// The fraction of a section a skilled player intends to spend boosting,
    /// `0..1` — what the boost budget is measured against.
    ///
    /// Not 1.0, and it never could be: the meter drains at 0.36/s and a near
    /// miss pays 0.13, so holding boost continuously would need three passes a
    /// second. A third of the time is what a good run actually sustains.
    pub target_boost_duty: f32,
    /// The fraction of a section a skilled route spends above the speed at
    /// which the meter fills on its own, `0..1`.
    ///
    /// The passive half of the economy, and leaving it out is what made an
    /// early version of this analysis call the whole course starved: near
    /// misses are the *interesting* source of boost, not the only one.
    pub high_speed_share: f32,
    /// Below this earned-over-spent ratio a section is **starved**.
    pub starved_ratio: f32,
    /// At or above this ratio a section is **excellent**.
    pub excellent_ratio: f32,
    /// How many distinct lanes must stay reachable for a section to count as
    /// offering multiple routes.
    pub excellent_route_width: u32,
}

impl ValidationThresholds {
    /// The shipping bar. These are the numbers the demo course is judged
    /// against, and the defaults a source that says nothing inherits.
    pub const DEFAULT: ValidationThresholds = ValidationThresholds {
        min_turn_radius_m: 90.0,
        max_grade: 0.10,
        // ~15 degrees. Well clear of what an ordinary corner asks for
        // (`bank_per_curvature` times a 350 m radius is 4.3 degrees), so this
        // only ever binds on the tight, deliberately-leaned figures — which is
        // the point of it being here rather than being a fixed 8 degrees
        // nothing could author its way past.
        max_bank_rad: 0.26,
        max_curvature_step: 0.0025,
        max_grade_step: 0.004,
        max_bank_step: 0.006,
        traversal_step_m: 30.0,
        lateral_speed_mps: 12.0,
        lateral_margin_m: 0.35,
        min_reaction_time_s: 0.6,
        near_miss_conversion: 0.72,
        target_boost_duty: 0.35,
        high_speed_share: 0.8,
        starved_ratio: 1.0,
        excellent_ratio: 1.6,
        excellent_route_width: 2,
    };

    /// Reject thresholds that cannot be measured against.
    pub fn validate(&self) -> CourseResult<()> {
        positive(
            self.min_turn_radius_m,
            "min_turn_radius_m",
            CourseErrorCode::InvalidRadius,
        )?;
        positive(self.max_grade, "max_grade", CourseErrorCode::InvalidFiniteScalar)?;
        positive(self.max_bank_rad, "max_bank", CourseErrorCode::InvalidFiniteScalar)?;
        positive(
            self.max_curvature_step,
            "max_curvature_step",
            CourseErrorCode::NonContinuousCourse,
        )?;
        positive(
            self.max_grade_step,
            "max_grade_step",
            CourseErrorCode::NonContinuousCourse,
        )?;
        positive(
            self.max_bank_step,
            "max_bank_step",
            CourseErrorCode::NonContinuousCourse,
        )?;
        positive(
            self.traversal_step_m,
            "traversal_step_m",
            CourseErrorCode::InvalidSectionLength,
        )?;
        positive(
            self.lateral_speed_mps,
            "lateral_speed_mps",
            CourseErrorCode::InvalidSpeedRange,
        )?;
        finite(self.lateral_margin_m, "lateral_margin_m")?;
        positive(
            self.min_reaction_time_s,
            "min_reaction_time_s",
            CourseErrorCode::ImpossibleReactionTime,
        )?;
        finite(self.near_miss_conversion, "near_miss_conversion")?;
        finite(self.target_boost_duty, "target_boost_duty")?;
        finite(self.high_speed_share, "high_speed_share")?;
        positive(
            self.starved_ratio,
            "starved_ratio",
            CourseErrorCode::InvalidFiniteScalar,
        )?;
        positive(
            self.excellent_ratio,
            "excellent_ratio",
            CourseErrorCode::InvalidFiniteScalar,
        )?;
        Ok(())
    }
}

impl Default for ValidationThresholds {
    fn default() -> Self {
        ValidationThresholds::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipping_thresholds_are_self_consistent_and_valid() {
        let t = ValidationThresholds::DEFAULT;
        assert!(t.validate().is_ok());
        assert_eq!(ValidationThresholds::default(), t);
        assert!(
            t.excellent_ratio > t.starved_ratio,
            "excellent has to be a higher bar than starved"
        );
        assert!((0.0..=1.0).contains(&t.near_miss_conversion));
        // A limit has to be looser than the step allowed toward it, or the road
        // can never reach it.
        assert!(t.max_grade > t.max_grade_step);
        assert!(t.max_bank_rad > t.max_bank_step);
        assert!((0.0..=1.0).contains(&t.target_boost_duty));
    }

    #[test]
    fn an_unmeasurable_threshold_set_is_rejected() {
        let base = ValidationThresholds::DEFAULT;
        for broken in [
            ValidationThresholds { min_turn_radius_m: 0.0, ..base },
            ValidationThresholds { max_grade: 0.0, ..base },
            ValidationThresholds { max_bank_rad: -1.0, ..base },
            ValidationThresholds { max_curvature_step: 0.0, ..base },
            ValidationThresholds { max_grade_step: -1.0, ..base },
            ValidationThresholds { max_bank_step: 0.0, ..base },
            ValidationThresholds { traversal_step_m: 0.0, ..base },
            ValidationThresholds { lateral_speed_mps: 0.0, ..base },
            ValidationThresholds { lateral_margin_m: f32::NAN, ..base },
            ValidationThresholds { min_reaction_time_s: 0.0, ..base },
            ValidationThresholds { near_miss_conversion: f32::NAN, ..base },
            ValidationThresholds { target_boost_duty: f32::INFINITY, ..base },
            ValidationThresholds { high_speed_share: f32::NAN, ..base },
            ValidationThresholds { starved_ratio: 0.0, ..base },
            ValidationThresholds { excellent_ratio: -1.0, ..base },
        ] {
            assert!(broken.validate().is_err(), "{broken:?} should not validate");
        }
    }
}
