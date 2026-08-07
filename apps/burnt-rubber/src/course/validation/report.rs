//! The **structured validation report** — errors, warnings and measurements,
//! in a deterministic order.
//!
//! A validator that returns a boolean tells an author nothing: "this course is
//! invalid" is not a work list. So every finding is a
//! [`CourseError`](crate::course::error::CourseError) with a code, a section and
//! a distance, and the report also carries the *measurements* the analysis made,
//! because "the tightest gap on this course is 1.9 m" is worth knowing whether
//! or not it is a failure.
//!
//! Ordering is fixed — severity, then course distance, then error code — so two
//! runs of the same compilation produce byte-identical reports and a test can
//! assert on the whole thing rather than on membership.

use crate::course::error::CourseError;
use crate::course::specification::SectionId;

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The course cannot be played as authored.
    Error,
    /// The course is playable, but is not the course that was authored.
    Warning,
}

impl Severity {
    /// The token used in dumps.
    pub const fn token(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// How serious it is.
    pub severity: Severity,
    /// Where along the course it is (m).
    pub distance_m: f32,
    /// The structured failure.
    pub error: CourseError,
}

impl Finding {
    /// An error at `distance_m`.
    pub fn error(distance_m: f32, error: CourseError) -> Finding {
        Finding {
            severity: Severity::Error,
            distance_m,
            error,
        }
    }

    /// A warning at `distance_m`.
    pub fn warning(distance_m: f32, error: CourseError) -> Finding {
        Finding {
            severity: Severity::Warning,
            distance_m,
            error,
        }
    }

    /// The stable one-line form.
    pub fn line(&self) -> String {
        format!(
            "{:>8.0}m  {:<7}  {}",
            self.distance_m,
            self.severity.token(),
            self.error
        )
    }
}

/// How a section's (or the whole course's) boost economy came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BoostStatus {
    /// No traversable route exists — the question of boost does not arise.
    Invalid,
    /// A route exists, but the intended boost cannot be sustained on it.
    Starved,
    /// Skilled play can sustain boost for the configured target.
    Acceptable,
    /// Surplus opportunities, or more than one viable route.
    Excellent,
}

impl BoostStatus {
    /// The token used in dumps.
    pub const fn token(self) -> &'static str {
        match self {
            BoostStatus::Invalid => "invalid",
            BoostStatus::Starved => "starved",
            BoostStatus::Acceptable => "acceptable",
            BoostStatus::Excellent => "excellent",
        }
    }

    /// The worse of two statuses — how a course's status is folded from its
    /// sections.
    pub fn worst(self, other: BoostStatus) -> BoostStatus {
        self.min(other)
    }
}

/// What the analysis measured for one section.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionVerdict {
    /// The section's stable name.
    pub id: SectionId,
    /// Its dense index.
    pub index: u16,
    /// Where it starts (m).
    pub start_m: f32,
    /// Where it ends (m).
    pub end_m: f32,
    /// Whether a route exists all the way through it.
    pub traversable: bool,
    /// The fewest distinct lanes that stayed reachable anywhere inside it.
    pub narrowest_corridor_lanes: u32,
    /// Compiled near-miss opportunities inside it.
    pub opportunities: u32,
    /// Boost the intended route is expected to earn here (fraction of meter).
    pub boost_earned: f32,
    /// Boost the intended route is expected to spend here.
    pub boost_spent: f32,
    /// How the economy came out.
    pub status: BoostStatus,
}

impl SectionVerdict {
    /// Earned over spent. Infinite where nothing is spent.
    pub fn ratio(&self) -> f32 {
        (self.boost_spent > 1.0e-6)
            .then(|| self.boost_earned / self.boost_spent)
            .unwrap_or(f32::INFINITY)
    }
}

/// The measurements the analysis made across the whole course.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CourseMetrics {
    /// Course length (m).
    pub length_m: f32,
    /// Compiled track samples.
    pub samples: usize,
    /// Compiled sections.
    pub sections: usize,
    /// Compiled traffic vehicles.
    pub vehicles: usize,
    /// Compiled encounters.
    pub encounters: usize,
    /// Compiled near-miss opportunity windows.
    pub near_miss_windows: usize,
    /// Cells in the traversability grid.
    pub traversal_cells: usize,
    /// Cells the grid found blocked.
    pub blocked_cells: usize,
    /// The tightest lateral gap the route ever had to take (m).
    pub tightest_corridor_m: f32,
    /// Mean vehicles per kilometre across the course.
    pub vehicles_per_km: f32,
}

impl CourseMetrics {
    /// Empty metrics.
    pub const EMPTY: CourseMetrics = CourseMetrics {
        length_m: 0.0,
        samples: 0,
        sections: 0,
        vehicles: 0,
        encounters: 0,
        near_miss_windows: 0,
        traversal_cells: 0,
        blocked_cells: 0,
        tightest_corridor_m: 0.0,
        vehicles_per_km: 0.0,
    };
}

/// The whole outcome of validating one compiled course.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    /// Findings, in a deterministic order.
    pub findings: Vec<Finding>,
    /// Per-section verdicts, in course order.
    pub sections: Vec<SectionVerdict>,
    /// The course-wide boost verdict.
    pub status: BoostStatus,
    /// What was measured.
    pub metrics: CourseMetrics,
}

impl ValidationReport {
    /// An empty report — what a course with nothing to say produces.
    pub fn empty() -> ValidationReport {
        ValidationReport {
            findings: Vec::new(),
            sections: Vec::new(),
            status: BoostStatus::Acceptable,
            metrics: CourseMetrics::EMPTY,
        }
    }

    /// Whether anything is an error.
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// The errors, in report order.
    pub fn errors(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
    }

    /// The warnings, in report order.
    pub fn warnings(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
    }

    /// Sort the findings into the report's canonical order: errors first, then
    /// by distance, then by error code, then by the rendered line.
    ///
    /// The last two keys exist so the order is **total** — two findings at the
    /// same distance must not be able to swap places between runs, or a report
    /// stops being comparable.
    pub fn sort(&mut self) {
        self.findings.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then(a.distance_m.total_cmp(&b.distance_m))
                .then(a.error.code.token().cmp(b.error.code.token()))
                .then(a.error.line().cmp(&b.error.line()))
        });
    }

    /// The deterministic text form — what a dump and a test diff on.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "status {}\nlength {:.0} m, {} samples, {} sections, {} vehicles, {} encounters, \
             {} near-miss windows\ngrid {} cells ({} blocked), tightest corridor {:.2} m, \
             {:.1} vehicles/km\n",
            self.status.token(),
            self.metrics.length_m,
            self.metrics.samples,
            self.metrics.sections,
            self.metrics.vehicles,
            self.metrics.encounters,
            self.metrics.near_miss_windows,
            self.metrics.traversal_cells,
            self.metrics.blocked_cells,
            self.metrics.tightest_corridor_m,
            self.metrics.vehicles_per_km,
        ));
        self.findings
            .iter()
            .for_each(|f| out.push_str(&format!("{}\n", f.line())));
        self.sections.iter().for_each(|s| {
            out.push_str(&format!(
                "section {:<28} {:>7.0}..{:<7.0} {:<10} corridor {} lanes, {} chances, \
                 earn {:.2} spend {:.2}\n",
                s.id,
                s.start_m,
                s.end_m,
                s.status.token(),
                s.narrowest_corridor_lanes,
                s.opportunities,
                s.boost_earned,
                s.boost_spent,
            ));
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::error::CourseErrorCode;

    fn finding(severity: Severity, distance_m: f32, code: CourseErrorCode) -> Finding {
        Finding {
            severity,
            distance_m,
            error: CourseError::new(code, "something"),
        }
    }

    #[test]
    fn the_report_orders_errors_before_warnings_and_then_by_distance() {
        let mut report = ValidationReport::empty();
        report.findings = vec![
            finding(Severity::Warning, 100.0, CourseErrorCode::InvalidRadius),
            finding(Severity::Error, 900.0, CourseErrorCode::InvalidRadius),
            finding(Severity::Error, 50.0, CourseErrorCode::InvalidLaneCount),
            finding(Severity::Warning, 20.0, CourseErrorCode::InvalidRadius),
        ];
        report.sort();
        let shape: Vec<(Severity, f32)> = report
            .findings
            .iter()
            .map(|f| (f.severity, f.distance_m))
            .collect();
        assert_eq!(
            shape,
            vec![
                (Severity::Error, 50.0),
                (Severity::Error, 900.0),
                (Severity::Warning, 20.0),
                (Severity::Warning, 100.0),
            ]
        );
        assert!(report.has_errors());
        assert_eq!(report.errors().count(), 2);
        assert_eq!(report.warnings().count(), 2);
    }

    /// Two findings at the same distance must not be able to swap places, or
    /// the report is not comparable between runs.
    #[test]
    fn the_ordering_is_total_even_at_one_distance() {
        let mut a = ValidationReport::empty();
        a.findings = vec![
            finding(Severity::Error, 500.0, CourseErrorCode::InvalidRadius),
            finding(Severity::Error, 500.0, CourseErrorCode::InvalidLaneCount),
            finding(Severity::Error, 500.0, CourseErrorCode::InvalidLaneWidth),
        ];
        let mut b = ValidationReport::empty();
        b.findings = vec![
            finding(Severity::Error, 500.0, CourseErrorCode::InvalidLaneWidth),
            finding(Severity::Error, 500.0, CourseErrorCode::InvalidRadius),
            finding(Severity::Error, 500.0, CourseErrorCode::InvalidLaneCount),
        ];
        a.sort();
        b.sort();
        assert_eq!(a.findings, b.findings);
    }

    #[test]
    fn an_empty_report_is_acceptable_and_says_nothing() {
        let report = ValidationReport::empty();
        assert!(!report.has_errors());
        assert_eq!(report.errors().count(), 0);
        assert_eq!(report.status, BoostStatus::Acceptable);
        assert_eq!(report.metrics, CourseMetrics::EMPTY);
        assert!(report.dump().contains("status acceptable"));
    }

    #[test]
    fn boost_statuses_fold_to_the_worst_one() {
        assert_eq!(
            BoostStatus::Excellent.worst(BoostStatus::Starved),
            BoostStatus::Starved
        );
        assert_eq!(
            BoostStatus::Starved.worst(BoostStatus::Invalid),
            BoostStatus::Invalid
        );
        assert_eq!(
            BoostStatus::Acceptable.worst(BoostStatus::Excellent),
            BoostStatus::Acceptable
        );
        assert!(BoostStatus::Invalid < BoostStatus::Starved);
        assert!(BoostStatus::Starved < BoostStatus::Acceptable);
        assert!(BoostStatus::Acceptable < BoostStatus::Excellent);
        for s in [
            BoostStatus::Invalid,
            BoostStatus::Starved,
            BoostStatus::Acceptable,
            BoostStatus::Excellent,
        ] {
            assert!(!s.token().is_empty());
        }
    }

    #[test]
    fn a_section_verdict_reports_its_ratio_including_the_free_case() {
        let mut verdict = SectionVerdict {
            id: SectionId::new("s"),
            index: 0,
            start_m: 0.0,
            end_m: 100.0,
            traversable: true,
            narrowest_corridor_lanes: 2,
            opportunities: 3,
            boost_earned: 0.6,
            boost_spent: 0.3,
            status: BoostStatus::Excellent,
        };
        assert!((verdict.ratio() - 2.0).abs() < 1.0e-6);
        verdict.boost_spent = 0.0;
        assert_eq!(verdict.ratio(), f32::INFINITY);
    }

    #[test]
    fn the_dump_carries_the_metrics_the_findings_and_the_sections() {
        let mut report = ValidationReport::empty();
        report.metrics = CourseMetrics {
            length_m: 9_000.0,
            samples: 4_501,
            sections: 12,
            vehicles: 110,
            encounters: 2,
            near_miss_windows: 112,
            traversal_cells: 2_250,
            blocked_cells: 300,
            tightest_corridor_m: 1.85,
            vehicles_per_km: 12.2,
            ..CourseMetrics::EMPTY
        };
        report.findings.push(finding(
            Severity::Warning,
            420.0,
            CourseErrorCode::NonContinuousCourse,
        ));
        report.sections.push(SectionVerdict {
            id: SectionId::new("opening"),
            index: 0,
            start_m: 0.0,
            end_m: 500.0,
            traversable: true,
            narrowest_corridor_lanes: 3,
            opportunities: 4,
            boost_earned: 0.4,
            boost_spent: 0.3,
            status: BoostStatus::Acceptable,
        });
        let dump = report.dump();
        assert!(dump.contains("4501 samples"), "{dump}");
        assert!(dump.contains("110 vehicles"), "{dump}");
        assert!(dump.contains("2250 cells"), "{dump}");
        assert!(dump.contains("non-continuous-course"), "{dump}");
        assert!(dump.contains("opening"), "{dump}");
        assert!(dump.contains("acceptable"), "{dump}");
        // Deterministic: the same report dumps the same text.
        assert_eq!(dump, report.dump());
    }
}
