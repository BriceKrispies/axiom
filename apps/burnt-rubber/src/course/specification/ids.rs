//! Stable identity for authored things.
//!
//! A [`SectionId`] is the **name the author gave a piece of road**, and it is
//! the anchor for everything derived from that piece: its seed streams, its
//! diagnostics, its traffic zone, its compiled index. That is why it is a
//! string and not an ordinal — an ordinal changes when a section is inserted
//! before it, and every seed derived from it would change with it, reshuffling
//! road the author did not touch. A name does not move.
//!
//! Motif expansion mints ids by suffixing (`coastal_sweeps/2`), so the sections
//! a motif produces are addressable, stable under a change to a *later* motif,
//! and obviously derived when read in a dump.

use crate::course::error::{CourseError, CourseErrorCode, CourseResult};

/// The stable name of an authored section.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SectionId(String);

impl SectionId {
    /// Name a section.
    pub fn new(name: impl Into<String>) -> SectionId {
        SectionId(name.into())
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A child id under this one — how motif expansion and multi-part section
    /// groups mint stable names for the pieces they produce.
    pub fn child(&self, part: impl std::fmt::Display) -> SectionId {
        SectionId(format!("{}/{part}", self.0))
    }
}

impl std::fmt::Display for SectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The stable identity of a compiled traffic vehicle.
///
/// Dense and ordered by spawn distance, so the runtime can index plans by it
/// and a replay can name the exact car that was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VehicleId(pub u32);

impl std::fmt::Display for VehicleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// The stable identity of a compiled encounter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EncounterId(pub u32);

impl std::fmt::Display for EncounterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "e{}", self.0)
    }
}

/// Reject a duplicated stable identifier, naming both the id and where the
/// clash is.
pub fn reject_duplicates(ids: &[SectionId]) -> CourseResult<()> {
    let mut seen: Vec<&SectionId> = Vec::with_capacity(ids.len());
    for id in ids {
        if seen.contains(&id) {
            return Err(CourseError::new(
                CourseErrorCode::DuplicateIdentifier,
                format!("two sections are both called `{id}`"),
            )
            .in_section(id.as_str()));
        }
        seen.push(id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_id_is_its_name_and_mints_stable_children() {
        let id = SectionId::new("coastal_sweeps");
        assert_eq!(id.as_str(), "coastal_sweeps");
        assert_eq!(id.to_string(), "coastal_sweeps");
        assert_eq!(id.child(2).as_str(), "coastal_sweeps/2");
        assert_eq!(id.child("entry").child(0).as_str(), "coastal_sweeps/entry/0");
        // Deriving a child never disturbs the parent.
        assert_eq!(id.as_str(), "coastal_sweeps");
    }

    #[test]
    fn duplicate_identifiers_are_rejected_and_named() {
        let ok = [SectionId::new("a"), SectionId::new("b")];
        assert!(reject_duplicates(&ok).is_ok());
        let clash = [
            SectionId::new("a"),
            SectionId::new("b"),
            SectionId::new("a"),
        ];
        let err = reject_duplicates(&clash).unwrap_err();
        assert_eq!(err.code, CourseErrorCode::DuplicateIdentifier);
        assert_eq!(err.section.as_deref(), Some("a"));
        assert!(reject_duplicates(&[]).is_ok());
    }

    #[test]
    fn generated_identities_print_compactly() {
        assert_eq!(VehicleId(12).to_string(), "v12");
        assert_eq!(EncounterId(3).to_string(), "e3");
        assert!(VehicleId(1) < VehicleId(2));
        assert!(EncounterId(1) < EncounterId(2));
    }
}
