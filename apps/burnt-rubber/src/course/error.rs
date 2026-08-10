//! The structured failure model for everything the course pipeline does.
//!
//! Parsing, expansion, compilation and validation all fail the same way: with a
//! [`CourseError`] carrying a **machine identity** ([`CourseErrorCode`]) plus the
//! authored thing that was wrong (section id, field name, source location) and a
//! human message.
//!
//! The shape follows [`axiom_kernel::KernelError`] deliberately: the identity of
//! an error is its code, the message exists for humans and never participates in
//! comparison, so a test can assert on *what went wrong* without pinning the
//! prose. It is app-owned rather than reusing `KernelError` directly because
//! `KernelError`'s scope/code vocabulary is a closed kernel enum and its message
//! is `&'static str`, and a course diagnostic has to name an authored identifier
//! and a line and column that only exist at runtime.
//!
//! **Nothing here panics.** Invalid authored data is a value, not a crash.

use std::fmt;

/// What went wrong, as a stable machine identity.
///
/// Every variant is a distinct authoring mistake a course source or a builder
/// can make. The list is closed on purpose: a new failure mode is a new variant,
/// not a new string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourseErrorCode {
    /// A scalar was NaN or infinite.
    InvalidFiniteScalar,
    /// A section length was zero or negative.
    InvalidSectionLength,
    /// A turn radius was zero, negative or below the course minimum.
    InvalidRadius,
    /// A lane count was zero, even, or beyond the road's reach.
    InvalidLaneCount,
    /// A lane width was zero or negative.
    InvalidLaneWidth,
    /// A road half-width was outside the course's legal band.
    InvalidRoadWidth,
    /// A headway range was reversed, negative, or ordered wrongly against its
    /// preferred value.
    InvalidHeadwayRange,
    /// A speed range was reversed or negative.
    InvalidSpeedRange,
    /// Lane occupancy weights were empty, negative, or summed to zero.
    InvalidLaneWeights,
    /// An encounter referred to a lane the road does not have.
    InvalidEncounterLane,
    /// A boost pickup was authored in a lane the road does not have where it
    /// stands.
    InvalidPickupLane,
    /// Two boost pickups occupy the same stretch of the same lane, so one of
    /// them can never be taken.
    OverlappingPickups,
    /// A boost pickup stands in a lane no route reaches — bait nobody can take.
    UnreachablePickup,
    /// An encounter's spacing and speed leave less than the reaction time it
    /// demands.
    ImpossibleReactionTime,
    /// An encounter asks for a lateral move the car cannot make in the distance
    /// available, or a clearance wider than the road.
    ImpossibleLateralClearance,
    /// Compiled geometry is not continuous in position, tangent, curvature,
    /// grade or bank.
    NonContinuousCourse,
    /// An encounter that requires a continuous route does not leave one.
    UntraversableEncounter,
    /// Two sections, two vehicles or two encounters share a stable identifier.
    DuplicateIdentifier,
    /// A motif name that has no implementation.
    UnknownMotif,
    /// A field name that no block accepts.
    UnknownField,
    /// The source is not syntactically a course.
    InvalidSyntax,
    /// A unit suffix that the grammar does not know.
    InvalidUnit,
    /// A bounded repeat asked for more iterations than the parser allows.
    RepeatLimitExceeded,
    /// The course as a whole is empty, or has no drivable length.
    EmptyCourse,
}

impl CourseErrorCode {
    /// The short stable token used in diagnostics and dumps.
    pub const fn token(self) -> &'static str {
        match self {
            CourseErrorCode::InvalidFiniteScalar => "invalid-finite-scalar",
            CourseErrorCode::InvalidSectionLength => "invalid-section-length",
            CourseErrorCode::InvalidRadius => "invalid-radius",
            CourseErrorCode::InvalidLaneCount => "invalid-lane-count",
            CourseErrorCode::InvalidLaneWidth => "invalid-lane-width",
            CourseErrorCode::InvalidRoadWidth => "invalid-road-width",
            CourseErrorCode::InvalidHeadwayRange => "invalid-headway-range",
            CourseErrorCode::InvalidSpeedRange => "invalid-speed-range",
            CourseErrorCode::InvalidLaneWeights => "invalid-lane-weights",
            CourseErrorCode::InvalidEncounterLane => "invalid-encounter-lane",
            CourseErrorCode::InvalidPickupLane => "invalid-pickup-lane",
            CourseErrorCode::OverlappingPickups => "overlapping-pickups",
            CourseErrorCode::UnreachablePickup => "unreachable-pickup",
            CourseErrorCode::ImpossibleReactionTime => "impossible-reaction-time",
            CourseErrorCode::ImpossibleLateralClearance => "impossible-lateral-clearance",
            CourseErrorCode::NonContinuousCourse => "non-continuous-course",
            CourseErrorCode::UntraversableEncounter => "untraversable-encounter",
            CourseErrorCode::DuplicateIdentifier => "duplicate-identifier",
            CourseErrorCode::UnknownMotif => "unknown-motif",
            CourseErrorCode::UnknownField => "unknown-field",
            CourseErrorCode::InvalidSyntax => "invalid-syntax",
            CourseErrorCode::InvalidUnit => "invalid-unit",
            CourseErrorCode::RepeatLimitExceeded => "repeat-limit-exceeded",
            CourseErrorCode::EmptyCourse => "empty-course",
        }
    }
}

/// Where in a source file a diagnostic points.
///
/// Line and column are both **1-based**, which is what an editor shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    /// The name the source was loaded under.
    pub source: String,
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
}

impl SourceLocation {
    /// A location in `source` at `line`:`column`.
    pub fn new(source: &str, line: u32, column: u32) -> SourceLocation {
        SourceLocation {
            source: source.to_string(),
            line,
            column,
        }
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.source, self.line, self.column)
    }
}

/// One structured failure.
#[derive(Debug, Clone, Eq)]
pub struct CourseError {
    /// The machine identity. Equality is defined on this and the authored
    /// coordinates, never on [`Self::message`].
    pub code: CourseErrorCode,
    /// The section this is about, when there is one.
    pub section: Option<String>,
    /// The field this is about, when there is one.
    pub field: Option<String>,
    /// Where in a source file, when the course came from one.
    pub at: Option<SourceLocation>,
    /// The human-readable explanation.
    pub message: String,
}

impl CourseError {
    /// A bare error with a code and a message.
    pub fn new(code: CourseErrorCode, message: impl Into<String>) -> CourseError {
        CourseError {
            code,
            section: None,
            field: None,
            at: None,
            message: message.into(),
        }
    }

    /// Attach the section this is about.
    pub fn in_section(mut self, section: impl Into<String>) -> CourseError {
        self.section = Some(section.into());
        self
    }

    /// Attach the field this is about.
    pub fn in_field(mut self, field: impl Into<String>) -> CourseError {
        self.field = Some(field.into());
        self
    }

    /// Attach a source location.
    pub fn at(mut self, at: SourceLocation) -> CourseError {
        self.at = Some(at);
        self
    }

    /// The stable one-line form used in reports and dumps — deterministic, and
    /// the shape a test asserts on.
    pub fn line(&self) -> String {
        format!("{self}")
    }
}

/// Equality is machine identity plus authored coordinates, deliberately
/// excluding the human message — the same stance [`axiom_kernel::KernelError`]
/// takes, and for the same reason: a reworded message must not change what a
/// test is asserting.
impl PartialEq for CourseError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code)
            & (self.section == other.section)
            & (self.field == other.field)
            & (self.at == other.at)
    }
}

impl fmt::Display for CourseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = self
            .at
            .as_ref()
            .map(|a| format!("{a}: "))
            .unwrap_or_default();
        let section = self
            .section
            .as_ref()
            .map(|s| format!(" [section {s}]"))
            .unwrap_or_default();
        let field = self
            .field
            .as_ref()
            .map(|s| format!(" [field {s}]"))
            .unwrap_or_default();
        write!(
            f,
            "{at}{}: {}{section}{field}",
            self.code.token(),
            self.message
        )
    }
}

impl std::error::Error for CourseError {}

/// The pipeline's result type.
pub type CourseResult<T> = Result<T, CourseError>;

/// Reject a non-finite scalar at the authoring boundary, naming the field.
pub fn finite(value: f32, field: &str) -> CourseResult<f32> {
    value.is_finite().then_some(value).ok_or_else(|| {
        CourseError::new(
            CourseErrorCode::InvalidFiniteScalar,
            format!("`{field}` is {value}, which is not a finite number"),
        )
        .in_field(field)
    })
}

/// Reject a non-positive scalar, naming the field and the code to blame.
pub fn positive(value: f32, field: &str, code: CourseErrorCode) -> CourseResult<f32> {
    let value = finite(value, field)?;
    (value > 0.0).then_some(value).ok_or_else(|| {
        CourseError::new(code, format!("`{field}` must be positive, got {value}")).in_field(field)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_ignores_the_human_message() {
        let a = CourseError::new(CourseErrorCode::InvalidRadius, "one wording");
        let b = CourseError::new(CourseErrorCode::InvalidRadius, "quite another");
        assert_eq!(a, b);
        assert_ne!(a, CourseError::new(CourseErrorCode::InvalidLaneCount, "one wording"));
    }

    #[test]
    fn equality_does_distinguish_the_authored_coordinates() {
        let base = CourseError::new(CourseErrorCode::UnknownField, "x");
        assert_ne!(base, base.clone().in_section("tunnel"));
        assert_ne!(base, base.clone().in_field("length"));
        assert_ne!(
            base,
            base.clone().at(SourceLocation::new("course.brc", 3, 9))
        );
    }

    #[test]
    fn the_display_form_carries_location_code_and_coordinates() {
        let e = CourseError::new(CourseErrorCode::InvalidSyntax, "expected `{`")
            .in_section("tunnel_squeeze")
            .in_field("length")
            .at(SourceLocation::new("burning_coast.brc", 12, 5));
        let line = e.line();
        assert!(line.contains("burning_coast.brc:12:5"), "{line}");
        assert!(line.contains("invalid-syntax"), "{line}");
        assert!(line.contains("expected `{`"), "{line}");
        assert!(line.contains("tunnel_squeeze"), "{line}");
        assert!(line.contains("length"), "{line}");
    }

    #[test]
    fn every_code_has_a_distinct_token() {
        let codes = [
            CourseErrorCode::InvalidFiniteScalar,
            CourseErrorCode::InvalidSectionLength,
            CourseErrorCode::InvalidRadius,
            CourseErrorCode::InvalidLaneCount,
            CourseErrorCode::InvalidLaneWidth,
            CourseErrorCode::InvalidRoadWidth,
            CourseErrorCode::InvalidHeadwayRange,
            CourseErrorCode::InvalidSpeedRange,
            CourseErrorCode::InvalidLaneWeights,
            CourseErrorCode::InvalidEncounterLane,
            CourseErrorCode::InvalidPickupLane,
            CourseErrorCode::OverlappingPickups,
            CourseErrorCode::UnreachablePickup,
            CourseErrorCode::ImpossibleReactionTime,
            CourseErrorCode::ImpossibleLateralClearance,
            CourseErrorCode::NonContinuousCourse,
            CourseErrorCode::UntraversableEncounter,
            CourseErrorCode::DuplicateIdentifier,
            CourseErrorCode::UnknownMotif,
            CourseErrorCode::UnknownField,
            CourseErrorCode::InvalidSyntax,
            CourseErrorCode::InvalidUnit,
            CourseErrorCode::RepeatLimitExceeded,
            CourseErrorCode::EmptyCourse,
        ];
        let mut tokens: Vec<&str> = codes.iter().map(|c| c.token()).collect();
        tokens.sort_unstable();
        let unique = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), unique, "two codes share a token");
    }

    #[test]
    fn the_scalar_guards_accept_good_values_and_name_bad_ones() {
        assert_eq!(finite(3.0, "length_m").unwrap(), 3.0);
        let bad = finite(f32::NAN, "length_m").unwrap_err();
        assert_eq!(bad.code, CourseErrorCode::InvalidFiniteScalar);
        assert_eq!(bad.field.as_deref(), Some("length_m"));

        assert_eq!(
            positive(2.0, "radius_m", CourseErrorCode::InvalidRadius).unwrap(),
            2.0
        );
        let zero = positive(0.0, "radius_m", CourseErrorCode::InvalidRadius).unwrap_err();
        assert_eq!(zero.code, CourseErrorCode::InvalidRadius);
        let nan = positive(f32::INFINITY, "radius_m", CourseErrorCode::InvalidRadius).unwrap_err();
        assert_eq!(nan.code, CourseErrorCode::InvalidFiniteScalar);
    }
}
