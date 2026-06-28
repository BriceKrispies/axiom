//! `HostOutcome`: the validated terminal outcome (won / score / metrics).

use crate::host_metrics::HostMetrics;
use crate::score::Score;

/// The outbound half of the embed seam (SPEC-12 §5): the terminal `won` flag,
/// the final [`Score`], and named [`HostMetrics`].
///
/// This is the **one universal word** the whole reference catalogue already
/// speaks (a parent-frame "complete" report); SPEC-12 standardizes it. It is
/// minted once from deterministic final state and carried as data to the
/// platform arm, which forwards it to the host channel exactly once. Emitting it
/// is an output side-effect, never fed back into a fixed update (SPEC-12 §6), so
/// a replay reproduces the same `HostOutcome`.
#[derive(Debug, Clone, PartialEq)]
pub struct HostOutcome {
    won: bool,
    score: Score,
    metrics: HostMetrics,
}

impl HostOutcome {
    /// Mint a terminal outcome from its won flag, score, and metrics.
    pub fn new(won: bool, score: Score, metrics: HostMetrics) -> Self {
        HostOutcome {
            won,
            score,
            metrics,
        }
    }

    /// Whether the session was won.
    pub const fn won(&self) -> bool {
        self.won
    }

    /// The terminal score.
    pub const fn score(&self) -> Score {
        self.score
    }

    /// The named terminal metrics, in stable order.
    pub fn metrics(&self) -> &HostMetrics {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> HostMetrics {
        HostMetrics::new().with(String::from("score"), Score::new(50.0))
    }

    #[test]
    fn outcome_carries_won_score_and_metrics() {
        let outcome = HostOutcome::new(true, Score::new(50.0), metrics());
        assert!(outcome.won());
        assert_eq!(outcome.score(), Score::new(50.0));
        assert_eq!(outcome.metrics(), &metrics());
    }

    #[test]
    fn equal_inputs_build_equal_outcomes() {
        assert_eq!(
            HostOutcome::new(true, Score::new(50.0), metrics()),
            HostOutcome::new(true, Score::new(50.0), metrics())
        );
        assert_ne!(
            HostOutcome::new(true, Score::new(50.0), metrics()),
            HostOutcome::new(false, Score::new(50.0), metrics())
        );
    }
}
