//! Generic, domain-free constraints over an artifact's neutral words, plus their
//! evaluation and bounded repair.
//!
//! A constraint checks a generic numeric property of the opaque `u64` words — a
//! minimum count, an upper bound, non-zeroness — never a domain rule ("rivers
//! reach the sea" is a terrain module's job). Evaluation and repair both dispatch
//! the fieldless [`ConstraintKind`] through a table (branchless). Repair is a
//! single bounded pass: each repairable constraint maps the words toward
//! satisfaction; a structural constraint with no word-level fix (a minimum count)
//! is a deliberate no-op, since repair never invents words.

use crate::report::ValidationReport;

/// What a constraint checks. Discriminants index the eval + repair tables. Domain
/// meaning never lives here.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ConstraintKind {
    /// The artifact has at least `threshold` words.
    MinCount = 0,
    /// Every word is `<= threshold`.
    MaxValue = 1,
    /// Every word is non-zero (`threshold` unused).
    NonZero = 2,
}

/// One declarative constraint: a kind plus its `threshold` parameter.
#[derive(Debug, Clone, Copy)]
pub struct Constraint {
    kind: ConstraintKind,
    threshold: u64,
}

impl Constraint {
    /// Require at least `count` words. Structural — not repairable at the word
    /// level, since repair never invents words.
    pub fn min_count(count: u64) -> Self {
        Constraint {
            kind: ConstraintKind::MinCount,
            threshold: count,
        }
    }

    /// Require every word to be at most `max`.
    pub fn max_value(max: u64) -> Self {
        Constraint {
            kind: ConstraintKind::MaxValue,
            threshold: max,
        }
    }

    /// Require every word to be non-zero.
    pub fn non_zero() -> Self {
        Constraint {
            kind: ConstraintKind::NonZero,
            threshold: 0,
        }
    }

    /// The report discriminant for this constraint's kind.
    pub(crate) fn kind_code(self) -> u32 {
        self.kind as u32
    }
}

// --- evaluation -------------------------------------------------------------

/// Evaluate one constraint against `words`, returning `(satisfied, score)`. The
/// score is the count of satisfying words (or the word count, for `MinCount`) — a
/// stable, ordered measure. Branchless table dispatch.
fn evaluate_one(constraint: &Constraint, words: &[u64]) -> (bool, u64) {
    const EVALS: [fn(&Constraint, &[u64]) -> (bool, u64); 3] =
        [eval_min_count, eval_max_value, eval_non_zero];
    EVALS[constraint.kind as usize](constraint, words)
}

fn eval_min_count(constraint: &Constraint, words: &[u64]) -> (bool, u64) {
    let count = words.len() as u64;
    (count >= constraint.threshold, count)
}

fn eval_max_value(constraint: &Constraint, words: &[u64]) -> (bool, u64) {
    let satisfying = words.iter().filter(|&&word| word <= constraint.threshold).count() as u64;
    (satisfying == words.len() as u64, satisfying)
}

fn eval_non_zero(_constraint: &Constraint, words: &[u64]) -> (bool, u64) {
    let satisfying = words.iter().filter(|&&word| word != 0).count() as u64;
    (satisfying == words.len() as u64, satisfying)
}

/// Evaluate every constraint against `words` into a [`ValidationReport`].
/// Branchless (`map`/`all`/`sum`).
pub(crate) fn evaluate(words: &[u64], constraints: &[Constraint]) -> ValidationReport {
    let verdicts: Vec<(u32, bool, u64)> = constraints
        .iter()
        .map(|constraint| {
            let (satisfied, score) = evaluate_one(constraint, words);
            (constraint.kind_code(), satisfied, score)
        })
        .collect();
    let all_satisfied = verdicts.iter().all(|&(_, satisfied, _)| satisfied);
    let total_score = verdicts.iter().map(|&(_, _, score)| score).sum();
    ValidationReport::new(verdicts, all_satisfied, total_score)
}

// --- repair -----------------------------------------------------------------

/// Apply one constraint's bounded word-level repair. Branchless table dispatch.
fn repair_one(constraint: &Constraint, words: Vec<u64>) -> Vec<u64> {
    const REPAIRS: [fn(&Constraint, Vec<u64>) -> Vec<u64>; 3] =
        [repair_min_count, repair_max_value, repair_non_zero];
    REPAIRS[constraint.kind as usize](constraint, words)
}

fn repair_min_count(_constraint: &Constraint, words: Vec<u64>) -> Vec<u64> {
    // Structural: repair never invents words, so a too-short artifact is left as
    // is (and stays failing) — a deliberate, documented limit.
    words
}

fn repair_max_value(constraint: &Constraint, words: Vec<u64>) -> Vec<u64> {
    words
        .into_iter()
        .map(|word| word.min(constraint.threshold))
        .collect()
}

fn repair_non_zero(_constraint: &Constraint, words: Vec<u64>) -> Vec<u64> {
    words.into_iter().map(|word| word.max(1)).collect()
}

/// Apply every constraint's bounded repair, in order — a single bounded pass (no
/// loop-to-fixpoint). Branchless fold.
pub(crate) fn repair_words(words: &[u64], constraints: &[Constraint]) -> Vec<u64> {
    constraints
        .iter()
        .fold(words.to_vec(), |words, constraint| repair_one(constraint, words))
}
