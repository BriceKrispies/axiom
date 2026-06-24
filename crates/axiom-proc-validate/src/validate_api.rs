//! [`ProcValidateApi`] — validate and repair proc artifacts against constraints.
//!
//! Validation is a pure deterministic function of an artifact's neutral words and
//! the constraint list; repair is a pure, **bounded** transform of those words
//! that returns a new, re-validatable [`Artifact`]. No domain rules live here, and
//! repair never loops to a fixpoint or invents content.

use axiom_proc::Artifact;

use crate::constraint::{evaluate, repair_words, Constraint};
use crate::report::ValidationReport;

/// The validation facade. Stateless: a report is a pure function of an artifact's
/// words and the constraints; a repair is a pure bounded transform of them.
#[derive(Debug)]
pub struct ProcValidateApi;

impl ProcValidateApi {
    /// Validate `artifact` against `constraints`. Deterministic — the report is a
    /// pure function of the artifact's words.
    pub fn validate(artifact: &Artifact, constraints: &[Constraint]) -> ValidationReport {
        evaluate(artifact.words(), constraints)
    }

    /// Repair `artifact` toward satisfying `constraints` — a single bounded pass of
    /// word-level fixes (clamp to a max, lift off zero). Returns a new,
    /// re-validatable artifact with the same generator version. A structural
    /// constraint with no word-level fix (a minimum count) is left unsatisfied by
    /// design, since repair never invents words.
    pub fn repair(artifact: &Artifact, constraints: &[Constraint]) -> Artifact {
        Artifact::from_words(
            artifact.generator_version(),
            repair_words(artifact.words(), constraints),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(words: &[u64]) -> Artifact {
        Artifact::from_words(1, words.to_vec())
    }

    fn full() -> [Constraint; 3] {
        [
            Constraint::min_count(2),
            Constraint::max_value(10),
            Constraint::non_zero(),
        ]
    }

    #[test]
    fn validation_is_deterministic_and_pure_in_the_words() {
        let c = full();
        let r1 = ProcValidateApi::validate(&artifact(&[3, 5, 7]), &c);
        let r2 = ProcValidateApi::validate(&artifact(&[3, 5, 7]), &c);
        assert_eq!(r1, r2);
        assert_eq!(r1.to_bytes(), r2.to_bytes());
        assert!(r1.all_satisfied());
    }

    #[test]
    fn a_violating_artifact_fails_at_the_expected_constraint() {
        // 0 violates non_zero; 99 violates max_value(10); count 2 satisfies min_count(2).
        let report = ProcValidateApi::validate(&artifact(&[0, 99]), &full());
        assert!(!report.all_satisfied());
        let verdicts = report.verdicts();
        assert!(verdicts[0].1); // min_count(2) satisfied (2 words)
        assert!(!verdicts[1].1); // max_value(10) violated
        assert!(!verdicts[2].1); // non_zero violated
    }

    #[test]
    fn scoring_is_stable_and_ordered() {
        let c = [Constraint::max_value(10)];
        let low = ProcValidateApi::validate(&artifact(&[99, 99, 5]), &c).total_score();
        let high = ProcValidateApi::validate(&artifact(&[5, 5, 5]), &c).total_score();
        assert!(high > low);
        assert_eq!(high, 3);
        assert_eq!(low, 1);
    }

    #[test]
    fn repair_produces_a_revalidatable_artifact() {
        let c = [Constraint::max_value(10), Constraint::non_zero()];
        let a = artifact(&[0, 99, 4]);
        assert!(!ProcValidateApi::validate(&a, &c).all_satisfied());
        let repaired = ProcValidateApi::repair(&a, &c);
        // Clamped to <=10 then lifted off zero, in order.
        assert_eq!(repaired.words(), &[1, 10, 4]);
        assert!(ProcValidateApi::validate(&repaired, &c).all_satisfied());
        assert_eq!(repaired.generator_version(), 1);
    }

    #[test]
    fn repair_cannot_satisfy_a_structural_min_count() {
        // Repair never invents words, so a too-short artifact stays failing.
        let c = [Constraint::min_count(3), Constraint::non_zero()];
        let repaired = ProcValidateApi::repair(&artifact(&[5]), &c);
        assert_eq!(repaired.words(), &[5]); // min_count repair is a no-op; non_zero leaves 5
        assert!(!ProcValidateApi::validate(&repaired, &c).all_satisfied());
    }

    #[test]
    fn metamorphic_known_good_passes_perturbed_fails() {
        let c = [Constraint::max_value(10)];
        assert!(ProcValidateApi::validate(&artifact(&[1, 2, 3]), &c).all_satisfied());
        // Perturb one word past the bound -> fails at the max_value constraint.
        let report = ProcValidateApi::validate(&artifact(&[1, 2, 11]), &c);
        assert!(!report.all_satisfied());
        assert!(!report.verdicts()[0].1);
    }

    #[test]
    fn identical_artifacts_yield_identical_reports() {
        let c = [Constraint::min_count(1), Constraint::non_zero()];
        assert_eq!(
            ProcValidateApi::validate(&artifact(&[7, 8]), &c),
            ProcValidateApi::validate(&artifact(&[7, 8]), &c)
        );
    }

    #[test]
    fn golden_report_digest_is_stable() {
        let report = ProcValidateApi::validate(&artifact(&[3, 5, 7]), &full());
        assert_eq!(report.digest().raw(), 4_172_291_403_371_807_957);
    }

    #[test]
    fn types_are_debug() {
        let report = ProcValidateApi::validate(&artifact(&[1]), &[Constraint::min_count(1)]);
        assert!(!format!("{:?}", Constraint::non_zero()).is_empty()); // Constraint -> ConstraintKind
        assert!(!format!("{report:?}").is_empty());
        assert!(!format!("{:?}", ProcValidateApi).is_empty());
    }
}
