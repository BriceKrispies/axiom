//! Replay verification: run the scenario twice from the same initial state and
//! prove the final state, the causal-event order, and the structural digest are
//! identical.
//!
//! sim-core's full byte-snapshot/replay seam is deferred (see sim-core's
//! deferred-features note), so this verifies determinism by deterministic re-run
//! comparison of actual state (not just success/failure).

use crate::Crucible;

/// The outcome of replay verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayCheck {
    /// Both runs produced the same structural digest.
    pub identical_digest: bool,
    /// Both runs produced the same causal-event order (and identical rows).
    pub identical_causal_order: bool,
    /// Both runs produced the same important fact/residue state.
    pub identical_state: bool,
    /// The shared digest.
    pub digest: u64,
}

impl ReplayCheck {
    /// Whether replay fully verified.
    pub fn ok(&self) -> bool {
        self.identical_digest && self.identical_causal_order && self.identical_state
    }

    /// A one-line summary.
    pub fn summary(&self) -> String {
        format!(
            "digest_match={} causal_order_match={} state_match={} -> {}",
            self.identical_digest,
            self.identical_causal_order,
            self.identical_state,
            if self.ok() { "PASS" } else { "FAIL" },
        )
    }
}

/// Run the crucible twice and compare actual deterministic state.
pub fn verify() -> ReplayCheck {
    let mut first = Crucible::new();
    first.run();
    let mut second = Crucible::new();
    second.run();

    let rows_first = first.rows();
    let rows_second = second.rows();
    let order = |rows: &[crate::CausalRow]| {
        rows.iter()
            .map(|r| (r.kind, r.code, r.tick))
            .collect::<Vec<_>>()
    };
    let identical_causal_order =
        order(&rows_first) == order(&rows_second) && rows_first == rows_second;
    let identical_state = (
        first.paw_amount(),
        first.mouth_amount(),
        first.intox_active(),
        first.groomed(),
    ) == (
        second.paw_amount(),
        second.mouth_amount(),
        second.intox_active(),
        second.groomed(),
    );

    ReplayCheck {
        identical_digest: first.digest() == second.digest(),
        identical_causal_order,
        identical_state,
        digest: first.digest(),
    }
}
