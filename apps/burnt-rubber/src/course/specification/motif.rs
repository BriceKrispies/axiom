//! The **authored form** of a motif: which motif, with what parameters.
//!
//! Expanding one into ordinary sections is [`crate::course::motifs`]'s job. The
//! split is the point: a motif exists only between the source and the expanded
//! course, and by the time anything is compiled it has become an inspectable
//! list of plain [`SectionSpec`](super::SectionSpec)s. The runtime never learns
//! that a motif existed.

use crate::course::error::{CourseError, CourseErrorCode, CourseResult};

use super::ids::SectionId;
use super::units::{CountRange, ScalarRange};

/// A built-in motif.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotifKind {
    /// A run of long, fast, alternating sweepers.
    HighSpeedSweeps,
    /// Tight alternating bends with almost no straight between them.
    AlternatingSlalom,
    /// Gently rolling, gently weaving open road.
    RollingFreeway,
    /// A narrowing corridor: lanes collapse, then hold, then open again.
    TunnelSqueeze,
    /// A rise that hides what is on the other side of it.
    BlindCrest,
    /// A staged loss of lanes with no recovery.
    LaneCollapse,
}

impl MotifKind {
    /// Every motif, in a stable order.
    pub const ALL: [MotifKind; 6] = [
        MotifKind::HighSpeedSweeps,
        MotifKind::AlternatingSlalom,
        MotifKind::RollingFreeway,
        MotifKind::TunnelSqueeze,
        MotifKind::BlindCrest,
        MotifKind::LaneCollapse,
    ];

    /// The DSL token / dump keyword.
    pub const fn token(self) -> &'static str {
        match self {
            MotifKind::HighSpeedSweeps => "high_speed_sweeps",
            MotifKind::AlternatingSlalom => "alternating_slalom",
            MotifKind::RollingFreeway => "rolling_freeway",
            MotifKind::TunnelSqueeze => "tunnel_squeeze",
            MotifKind::BlindCrest => "blind_crest",
            MotifKind::LaneCollapse => "lane_collapse",
        }
    }

    /// Resolve a DSL token, or say which motif was asked for and does not exist.
    pub fn parse(token: &str) -> CourseResult<MotifKind> {
        MotifKind::ALL
            .into_iter()
            .find(|m| m.token() == token)
            .ok_or_else(|| {
                let known = MotifKind::ALL
                    .map(|m| m.token())
                    .join(", ");
                CourseError::new(
                    CourseErrorCode::UnknownMotif,
                    format!("no motif called `{token}` — the built-in motifs are: {known}"),
                )
                .in_field("motif")
            })
    }
}

/// The parameters a motif is expanded with.
///
/// One record for every motif rather than one per kind: the fields overlap
/// heavily (nearly all of them want a length and a count), and a motif simply
/// ignores what it has no use for. What it *cannot* do is read a field nobody
/// set — every field has a default, and [`MotifParams::DEFAULT`] is what a
/// source that mentions none of them gets.
#[derive(Debug, Clone, PartialEq)]
pub struct MotifParams {
    /// How many repetitions the motif runs for. Bounded by [`MAX_MOTIF_COUNT`].
    pub count: u32,
    /// Total road the motif covers (m), where the motif is length-driven rather
    /// than count-driven.
    pub length_m: f32,
    /// The turn radius band (m).
    pub radius_m: ScalarRange,
    /// The banking band (rad).
    pub bank_rad: ScalarRange,
    /// Peak elevation wave displacement (m).
    pub elevation_amplitude_m: f32,
    /// Peak lateral wave displacement (m).
    pub lateral_amplitude_m: f32,
    /// Wavelength for both waves (m).
    pub wavelength_m: f32,
    /// Height of a crest (m).
    pub height_m: f32,
    /// Lanes the motif starts with.
    pub lanes: CountRange,
    /// Lanes the motif collapses to.
    pub narrow_lanes: u32,
}

/// The most repetitions a motif may be asked for.
///
/// A bound, not a suggestion: a motif is expanded eagerly into concrete
/// sections, so `count = 100000` would be a hundred thousand sections and a
/// compile that never finishes. The DSL rejects anything above this outright
/// (`CourseErrorCode::RepeatLimitExceeded`), which is what keeps the grammar
/// free of unbounded loops.
pub const MAX_MOTIF_COUNT: u32 = 64;

impl MotifParams {
    /// What a motif invocation that names no parameters expands with.
    pub const DEFAULT: MotifParams = MotifParams {
        count: 3,
        length_m: 600.0,
        radius_m: ScalarRange::new(140.0, 260.0),
        bank_rad: ScalarRange::new(0.06, 0.13),
        elevation_amplitude_m: 3.0,
        lateral_amplitude_m: 10.0,
        wavelength_m: 240.0,
        height_m: 7.0,
        lanes: CountRange::exact(5),
        narrow_lanes: 3,
    };

    /// Reject parameters a motif cannot be expanded with.
    pub fn validate(&self) -> CourseResult<()> {
        (self.count > 0).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidSectionLength,
                "a motif must repeat at least once".to_string(),
            )
            .in_field("count")
        })?;
        (self.count <= MAX_MOTIF_COUNT).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::RepeatLimitExceeded,
                format!(
                    "`count` is {}, above the bounded-repeat limit of {MAX_MOTIF_COUNT}",
                    self.count
                ),
            )
            .in_field("count")
        })?;
        crate::course::error::positive(
            self.length_m,
            "length_m",
            CourseErrorCode::InvalidSectionLength,
        )?;
        self.radius_m.validate("radius", true)?;
        self.bank_rad.validate("bank", false)?;
        crate::course::error::finite(self.elevation_amplitude_m, "elevation_amplitude_m")?;
        crate::course::error::finite(self.lateral_amplitude_m, "lateral_amplitude_m")?;
        crate::course::error::positive(
            self.wavelength_m,
            "wavelength_m",
            CourseErrorCode::InvalidSectionLength,
        )?;
        crate::course::error::finite(self.height_m, "height_m")?;
        super::road::validate_lane_count(self.lanes.lo, "lanes")?;
        super::road::validate_lane_count(self.lanes.hi, "lanes")?;
        super::road::validate_lane_count(self.narrow_lanes, "narrow_lanes")?;
        Ok(())
    }
}

impl Default for MotifParams {
    fn default() -> Self {
        MotifParams::DEFAULT
    }
}

/// One motif, invoked under a stable id.
#[derive(Debug, Clone, PartialEq)]
pub struct MotifInvocation {
    /// The stable id every section the motif produces is named under.
    pub id: SectionId,
    /// Which motif.
    pub kind: MotifKind,
    /// How to expand it.
    pub params: MotifParams,
    /// The environment the produced sections inherit, if the invocation names
    /// one.
    pub environment: Option<super::environment::SectionKind>,
    /// The expected player speed the produced sections inherit (m/s).
    pub expected_speed_mps: Option<f32>,
    /// Traffic for the whole span the motif produces.
    pub traffic: Option<super::traffic::TrafficZoneSpec>,
    /// Boost pickups over the whole span the motif produces, at offsets from
    /// where the motif starts.
    ///
    /// Placed against the *span*, not against the repetitions: a motif's
    /// expansion is an implementation detail (`<id>/bend0`, `<id>/link0`, …) and
    /// an author who had to name one would be writing against a shape that the
    /// motif is free to change.
    pub pickups: Vec<super::pickup::BoostPickupSpec>,
}

impl MotifInvocation {
    /// A bare invocation of `kind` under `id`, with default parameters.
    pub fn new(id: SectionId, kind: MotifKind) -> MotifInvocation {
        MotifInvocation {
            id,
            kind,
            params: MotifParams::DEFAULT,
            environment: None,
            expected_speed_mps: None,
            traffic: None,
            pickups: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_motif_round_trips_through_its_token() {
        for kind in MotifKind::ALL {
            assert_eq!(MotifKind::parse(kind.token()).unwrap(), kind);
        }
        let mut tokens: Vec<&str> = MotifKind::ALL.iter().map(|m| m.token()).collect();
        tokens.sort_unstable();
        let count = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), count);
    }

    #[test]
    fn an_unknown_motif_is_named_and_lists_the_real_ones() {
        let err = MotifKind::parse("figure_eight").unwrap_err();
        assert_eq!(err.code, CourseErrorCode::UnknownMotif);
        assert!(err.message.contains("figure_eight"), "{}", err.message);
        assert!(
            err.message.contains("high_speed_sweeps"),
            "the message lists what does exist: {}",
            err.message
        );
    }

    #[test]
    fn the_bounded_repeat_limit_is_enforced() {
        let ok = MotifParams {
            count: MAX_MOTIF_COUNT,
            ..MotifParams::DEFAULT
        };
        assert!(ok.validate().is_ok());
        let too_many = MotifParams {
            count: MAX_MOTIF_COUNT + 1,
            ..MotifParams::DEFAULT
        };
        assert_eq!(
            too_many.validate().unwrap_err().code,
            CourseErrorCode::RepeatLimitExceeded
        );
        let none = MotifParams {
            count: 0,
            ..MotifParams::DEFAULT
        };
        assert!(none.validate().is_err());
    }

    #[test]
    fn bad_motif_parameters_are_rejected() {
        let base = MotifParams::DEFAULT;
        assert!(base.validate().is_ok());
        assert_eq!(MotifParams::default(), base);
        for broken in [
            MotifParams { length_m: 0.0, ..base.clone() },
            MotifParams { radius_m: ScalarRange::new(-1.0, 20.0), ..base.clone() },
            MotifParams { bank_rad: ScalarRange::new(1.0, 0.0), ..base.clone() },
            MotifParams { elevation_amplitude_m: f32::NAN, ..base.clone() },
            MotifParams { lateral_amplitude_m: f32::NAN, ..base.clone() },
            MotifParams { wavelength_m: 0.0, ..base.clone() },
            MotifParams { height_m: f32::INFINITY, ..base.clone() },
            MotifParams { lanes: CountRange::exact(4), ..base.clone() },
            MotifParams { lanes: CountRange::new(3, 4), ..base.clone() },
            MotifParams { narrow_lanes: 2, ..base.clone() },
        ] {
            assert!(broken.validate().is_err(), "{broken:?} should not validate");
        }
    }

    #[test]
    fn a_bare_invocation_takes_the_defaults() {
        let m = MotifInvocation::new(SectionId::new("sweeps"), MotifKind::HighSpeedSweeps);
        assert_eq!(m.params, MotifParams::DEFAULT);
        assert_eq!(m.environment, None);
        assert_eq!(m.expected_speed_mps, None);
        assert_eq!(m.traffic, None);
        assert_eq!(m.id.as_str(), "sweeps");
    }
}
