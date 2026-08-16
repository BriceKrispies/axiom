//! [`ProcValidateApi`] — validate and repair generated words against constraints.
//!
//! Validation is a pure deterministic function of a generation's neutral output
//! words and the constraint list; repair is a pure, **bounded** transform of those
//! words that returns a new, re-validatable word list. No domain rules live here,
//! and repair never loops to a fixpoint or invents content.
//!
//! The words are whatever a generator produced — the `Vec<u64>` output of an
//! `axiom-proc-core` run, a module's own neutral output, a golden read back from
//! disk. This layer deliberately does **not** name a container type for them: the
//! recipe stack is generic over its output type, so binding validation to one
//! concrete artifact struct would re-introduce exactly the coupling that made a
//! second recipe generation necessary.

use crate::constraint::{evaluate, repair_words, Constraint};
use crate::report::ValidationReport;

/// The validation facade. Stateless: a report is a pure function of the words and
/// the constraints; a repair is a pure bounded transform of them.
#[derive(Debug)]
pub struct ProcValidateApi;

impl ProcValidateApi {
    /// Validate `words` against `constraints`. Deterministic — the report is a
    /// pure function of the words.
    pub fn validate(words: &[u64], constraints: &[Constraint]) -> ValidationReport {
        evaluate(words, constraints)
    }

    /// Repair `words` toward satisfying `constraints` — a single bounded pass of
    /// word-level fixes (clamp to a max, lift off zero). Returns a new,
    /// re-validatable word list. A structural constraint with no word-level fix
    /// (a minimum count) is left unsatisfied by design, since repair never
    /// invents words.
    pub fn repair(words: &[u64], constraints: &[Constraint]) -> Vec<u64> {
        repair_words(words, constraints)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let r1 = ProcValidateApi::validate(&[3, 5, 7], &c);
        let r2 = ProcValidateApi::validate(&[3, 5, 7], &c);
        assert_eq!(r1, r2);
        assert_eq!(r1.to_bytes(), r2.to_bytes());
        assert!(r1.all_satisfied());
    }

    #[test]
    fn a_violating_word_list_fails_at_the_expected_constraint() {
        // 0 violates non_zero; 99 violates max_value(10); count 2 satisfies min_count(2).
        let report = ProcValidateApi::validate(&[0, 99], &full());
        assert!(!report.all_satisfied());
        let verdicts = report.verdicts();
        assert!(verdicts[0].1);
        assert!(!verdicts[1].1);
        assert!(!verdicts[2].1);
    }

    #[test]
    fn scoring_is_stable_and_ordered() {
        let c = [Constraint::max_value(10)];
        let low = ProcValidateApi::validate(&[99, 99, 5], &c).total_score();
        let high = ProcValidateApi::validate(&[5, 5, 5], &c).total_score();
        assert!(high > low);
        assert_eq!(high, 3);
        assert_eq!(low, 1);
    }

    #[test]
    fn repair_produces_a_revalidatable_word_list() {
        let c = [Constraint::max_value(10), Constraint::non_zero()];
        assert!(!ProcValidateApi::validate(&[0, 99, 4], &c).all_satisfied());
        let repaired = ProcValidateApi::repair(&[0, 99, 4], &c);
        assert_eq!(repaired, vec![1, 10, 4]);
        assert!(ProcValidateApi::validate(&repaired, &c).all_satisfied());
    }

    #[test]
    fn repair_cannot_satisfy_a_structural_min_count() {
        let c = [Constraint::min_count(3), Constraint::non_zero()];
        let repaired = ProcValidateApi::repair(&[5], &c);
        assert_eq!(repaired, vec![5]);
        assert!(!ProcValidateApi::validate(&repaired, &c).all_satisfied());
    }

    #[test]
    fn metamorphic_known_good_passes_perturbed_fails() {
        let c = [Constraint::max_value(10)];
        assert!(ProcValidateApi::validate(&[1, 2, 3], &c).all_satisfied());
        let report = ProcValidateApi::validate(&[1, 2, 11], &c);
        assert!(!report.all_satisfied());
        assert!(!report.verdicts()[0].1);
    }

    #[test]
    fn identical_word_lists_yield_identical_reports() {
        let c = [Constraint::min_count(1), Constraint::non_zero()];
        assert_eq!(
            ProcValidateApi::validate(&[7, 8], &c),
            ProcValidateApi::validate(&[7, 8], &c)
        );
    }

    #[test]
    fn golden_report_digest_is_stable() {
        // Unchanged by the P1 migration off the v1 `Artifact` container: the
        // report's bytes were always a function of the words + constraints alone.
        let report = ProcValidateApi::validate(&[3, 5, 7], &full());
        assert_eq!(report.digest().raw(), 4_172_291_403_371_807_957);
    }

    #[test]
    fn types_are_debug() {
        let report = ProcValidateApi::validate(&[1], &[Constraint::min_count(1)]);
        assert!(!format!("{:?}", Constraint::non_zero()).is_empty());
        assert!(!format!("{report:?}").is_empty());
        assert!(!format!("{:?}", ProcValidateApi).is_empty());
    }
}
