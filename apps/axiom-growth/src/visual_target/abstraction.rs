//! The **abstraction gate**. New abstractions are forbidden by default: the
//! convergence loop is meant to be boring and local, making one bounded change at a
//! time against the existing implementation. An abstraction (a new API, a new
//! generalization) may be introduced **only** when one of two things is true:
//!
//! 1. the same axis has failed to improve after at least [`MIN_FAILED_ATTEMPTS`]
//!    (3) candidate attempts, or
//! 2. the current implementation genuinely cannot express the needed visual change.
//!
//! And when it *is* introduced, it must be justified in full — every
//! [`AbstractionRecord`] names the axis it unlocks, the specific failed attempts
//! that earned it, the smallest API the next candidate needs, and proof that the
//! deterministic screenshot command still works. This module owns the gate and the
//! record shape; it cannot be constructed except through a permission the gate
//! granted.

use serde::{Deserialize, Serialize};

use super::axes::Axis;
use super::ledger::Ledger;

/// The number of failed candidate attempts on one axis that unlocks an abstraction
/// for that axis.
pub const MIN_FAILED_ATTEMPTS: usize = 3;

/// Whether — and why — an abstraction may be introduced for an axis right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbstractionPermission {
    /// The default. No abstraction may be introduced.
    Forbidden { axis: Axis, failed_attempts: usize },
    /// Unlocked because `MIN_FAILED_ATTEMPTS` bounded attempts on this axis failed.
    JustifiedByRepeatedFailure { axis: Axis, failed_attempts: Vec<u32> },
    /// Unlocked because the current implementation cannot express the change.
    JustifiedByInexpressible { axis: Axis },
}

impl AbstractionPermission {
    /// Whether an abstraction is permitted (either justification holds).
    pub fn is_permitted(&self) -> bool {
        !matches!(self, AbstractionPermission::Forbidden { .. })
    }

    /// The axis in question.
    pub fn axis(&self) -> Axis {
        match self {
            AbstractionPermission::Forbidden { axis, .. }
            | AbstractionPermission::JustifiedByRepeatedFailure { axis, .. }
            | AbstractionPermission::JustifiedByInexpressible { axis } => *axis,
        }
    }

    /// A human-readable explanation of the permission state.
    pub fn describe(&self) -> String {
        match self {
            AbstractionPermission::Forbidden { axis, failed_attempts } => format!(
                "forbidden for {axis}: only {failed_attempts}/{MIN_FAILED_ATTEMPTS} failed attempts and impl can express the change"
            ),
            AbstractionPermission::JustifiedByRepeatedFailure { axis, failed_attempts } => format!(
                "permitted for {axis}: {} failed attempts (iterations {failed_attempts:?})",
                failed_attempts.len()
            ),
            AbstractionPermission::JustifiedByInexpressible { axis } => {
                format!("permitted for {axis}: current implementation cannot express the change")
            }
        }
    }
}

/// The gate. `inexpressible` is the caller's honest claim that the current
/// implementation cannot express the needed change; repeated-failure evidence is
/// read from the ledger. Repeated-failure evidence is preferred when it exists (it
/// is concrete), otherwise the inexpressibility claim stands, otherwise forbidden.
pub fn permit(ledger: &Ledger, axis: Axis, inexpressible: bool) -> AbstractionPermission {
    let failed = ledger.failed_attempts_on(axis);
    match (failed.len() >= MIN_FAILED_ATTEMPTS, inexpressible) {
        (true, _) => AbstractionPermission::JustifiedByRepeatedFailure { axis, failed_attempts: failed },
        (false, true) => AbstractionPermission::JustifiedByInexpressible { axis },
        (false, false) => AbstractionPermission::Forbidden { axis, failed_attempts: failed.len() },
    }
}

/// A fully-justified abstraction. Can only be built via [`AbstractionRecord::new`],
/// which refuses a `Forbidden` permission and requires every justification field —
/// so an unjustified abstraction cannot even be represented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbstractionRecord {
    /// The visual axis this abstraction unlocks.
    pub axis: Axis,
    /// The specific prior failed attempts (iteration numbers) that justified it;
    /// empty only when justified purely by inexpressibility.
    pub failed_attempts: Vec<u32>,
    /// Whether the justification is that the current implementation cannot express
    /// the needed change.
    pub inexpressible: bool,
    /// The smallest API the next candidate needs (a deliberately minimal surface).
    pub smallest_api: String,
    /// The exact deterministic screenshot command that must still work.
    pub screenshot_command: String,
    /// Captured proof that the screenshot command still produces the deterministic
    /// shot (e.g. matching hashes / a PASS line).
    pub screenshot_proof: String,
}

impl AbstractionRecord {
    /// Build a justified record, or fail. Refuses a forbidden permission and any
    /// empty justification field.
    pub fn new(
        permission: &AbstractionPermission,
        smallest_api: &str,
        screenshot_command: &str,
        screenshot_proof: &str,
    ) -> Result<AbstractionRecord, String> {
        permission
            .is_permitted()
            .then_some(())
            .ok_or_else(|| format!("abstraction {}", permission.describe()))?;
        let (failed_attempts, inexpressible) = match permission {
            AbstractionPermission::JustifiedByRepeatedFailure { failed_attempts, .. } => {
                (failed_attempts.clone(), false)
            }
            AbstractionPermission::JustifiedByInexpressible { .. } => (Vec::new(), true),
            AbstractionPermission::Forbidden { .. } => (Vec::new(), false),
        };
        let record = AbstractionRecord {
            axis: permission.axis(),
            failed_attempts,
            inexpressible,
            smallest_api: smallest_api.to_string(),
            screenshot_command: screenshot_command.to_string(),
            screenshot_proof: screenshot_proof.to_string(),
        };
        record.validate()?;
        Ok(record)
    }

    /// Re-check that every required justification field is present.
    pub fn validate(&self) -> Result<(), String> {
        (!self.failed_attempts.is_empty() || self.inexpressible)
            .then_some(())
            .ok_or("abstraction record has neither failed attempts nor an inexpressibility claim")?;
        (!self.smallest_api.trim().is_empty())
            .then_some(())
            .ok_or("abstraction record must state the smallest API it needs")?;
        (!self.screenshot_command.trim().is_empty())
            .then_some(())
            .ok_or("abstraction record must state the deterministic screenshot command")?;
        (!self.screenshot_proof.trim().is_empty())
            .then_some(())
            .ok_or("abstraction record must include proof the screenshot command still works")?;
        Ok(())
    }

    /// Serialize to TOML.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("an abstraction record always serializes")
    }

    /// Parse + validate from TOML text.
    pub fn parse(toml_str: &str) -> Result<AbstractionRecord, String> {
        let record: AbstractionRecord =
            toml::from_str(toml_str).map_err(|e| format!("abstraction record parse error: {e}"))?;
        record.validate()?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ledger::LedgerEntry;
    use super::super::review::Decision;
    use super::super::axes::Scorecard;

    fn failed_entry(iteration: u32, axis: Axis) -> LedgerEntry {
        LedgerEntry {
            iteration,
            attacked_axis: axis,
            changed_files: vec![],
            champion_screenshot: "c.png".to_string(),
            candidate_screenshot: "k.png".to_string(),
            decision: Decision::RejectCandidate,
            reason: "r".to_string(),
            next_attacked_axis: Some(axis),
            abstraction_introduced: false,
            human_note: String::new(),
            scorecard_before: Scorecard::uniform(2),
            scorecard_after: Scorecard::uniform(2),
        }
    }

    fn ledger_with_failures(axis: Axis, n: u32) -> Ledger {
        let mut ledger = Ledger::new();
        (1..=n).for_each(|i| ledger.append(failed_entry(i, axis)));
        ledger
    }

    #[test]
    fn forbidden_until_three_failures() {
        let axis = Axis::VegetationClumping;
        for n in 0..3 {
            let p = permit(&ledger_with_failures(axis, n), axis, false);
            assert!(!p.is_permitted(), "should be forbidden at {n} failures");
        }
        let p = permit(&ledger_with_failures(axis, 3), axis, false);
        assert!(p.is_permitted());
        assert!(matches!(p, AbstractionPermission::JustifiedByRepeatedFailure { .. }));
    }

    #[test]
    fn inexpressible_unlocks_without_failures() {
        let axis = Axis::LightingDirectionality;
        let p = permit(&Ledger::new(), axis, true);
        assert!(p.is_permitted());
        assert!(matches!(p, AbstractionPermission::JustifiedByInexpressible { .. }));
        assert_eq!(p.axis(), axis);
    }

    #[test]
    fn repeated_failure_evidence_is_preferred_over_inexpressible() {
        let axis = Axis::FogAndHaze;
        let p = permit(&ledger_with_failures(axis, 4), axis, true);
        match p {
            AbstractionPermission::JustifiedByRepeatedFailure { failed_attempts, .. } => {
                assert_eq!(failed_attempts, vec![1, 2, 3, 4]);
            }
            other => panic!("expected repeated-failure justification, got {other:?}"),
        }
    }

    #[test]
    fn record_cannot_be_built_from_a_forbidden_permission() {
        let axis = Axis::ObjectScale;
        let p = permit(&Ledger::new(), axis, false);
        let err = AbstractionRecord::new(&p, "api", "cmd", "proof").unwrap_err();
        assert!(err.contains("forbidden"));
    }

    #[test]
    fn record_requires_every_justification_field() {
        let axis = Axis::ObjectScale;
        let p = permit(&ledger_with_failures(axis, 3), axis, false);
        assert!(AbstractionRecord::new(&p, "", "cmd", "proof").is_err());
        assert!(AbstractionRecord::new(&p, "api", "  ", "proof").is_err());
        assert!(AbstractionRecord::new(&p, "api", "cmd", "").is_err());
    }

    #[test]
    fn valid_record_round_trips_through_toml() {
        let axis = Axis::ForegroundMaterialDetail;
        let p = permit(&ledger_with_failures(axis, 3), axis, false);
        let record = AbstractionRecord::new(
            &p,
            "add `ground.detail_octaves` param",
            "cargo run --features visual-target --bin visual-target -- render manifest.toml --backend canvas2d",
            "shot_a.png and shot_b.png hashes match: f3cbcb...",
        )
        .unwrap();
        assert_eq!(record.failed_attempts, vec![1, 2, 3]);
        let back = AbstractionRecord::parse(&record.to_toml()).unwrap();
        assert_eq!(record, back);
    }
}
