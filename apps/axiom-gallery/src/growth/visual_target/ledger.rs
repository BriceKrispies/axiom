//! The **iteration ledger**: an append-only, TOML-serialized record of every
//! convergence iteration. One [`LedgerEntry`] is written per candidate reviewed, so
//! the whole history of "what we attacked, what changed, what we decided, and why"
//! is auditable and replayable.
//!
//! The ledger is also the *evidence base* the abstraction gate reads: whether an
//! abstraction is permitted depends on how many prior attempts on an axis failed
//! (see [`super::abstraction`]).

use serde::{Deserialize, Serialize};

use super::axes::{Axis, Scorecard};
use super::review::Decision;

/// One convergence iteration, recorded in full. The two `Scorecard` fields are
/// declared last so TOML's "values before sub-tables" rule holds for the
/// array-of-tables serialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerEntry {
    /// 1-based iteration number.
    pub iteration: u32,
    /// The single axis this iteration attacked.
    pub attacked_axis: Axis,
    /// The files the bounded candidate change touched (manifest, code, …).
    pub changed_files: Vec<String>,
    /// Path to the champion screenshot this candidate was compared against.
    pub champion_screenshot: String,
    /// Path to the candidate screenshot that was scored.
    pub candidate_screenshot: String,
    /// The decision the comparator reached (after any human override).
    pub decision: Decision,
    /// Human-readable justification for the decision.
    pub reason: String,
    /// The axis the *next* iteration will attack, or `None` if the champion is now
    /// complete.
    pub next_attacked_axis: Option<Axis>,
    /// Whether this iteration introduced an abstraction (forbidden by default; see
    /// [`super::abstraction`]).
    pub abstraction_introduced: bool,
    /// An optional free-text human note (e.g. from a `HumanVerdict`); empty if none.
    #[serde(default)]
    pub human_note: String,
    /// The champion scorecard *before* this iteration (the baseline attacked).
    pub scorecard_before: Scorecard,
    /// The candidate scorecard *after* the bounded change.
    pub scorecard_after: Scorecard,
}

/// The whole ledger — an ordered list of iterations. Serialized as a TOML
/// array-of-tables under the `iteration` key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default, rename = "iteration")]
    pub entries: Vec<LedgerEntry>,
}

impl Ledger {
    /// An empty ledger.
    pub fn new() -> Ledger {
        Ledger { entries: Vec::new() }
    }

    /// Parse a ledger from TOML text.
    pub fn parse(toml_str: &str) -> Result<Ledger, String> {
        toml::from_str(toml_str).map_err(|e| format!("ledger parse error: {e}"))
    }

    /// Load a ledger from a file, or an empty ledger if the file is absent.
    pub fn load_or_empty(path: &std::path::Path) -> Result<Ledger, String> {
        path.exists()
            .then(|| {
                std::fs::read_to_string(path)
                    .map_err(|e| format!("cannot read ledger {}: {e}", path.display()))
                    .and_then(|t| Ledger::parse(&t))
            })
            .unwrap_or_else(|| Ok(Ledger::new()))
    }

    /// Serialize the ledger to TOML text.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("a ledger always serializes")
    }

    /// Write the ledger to a file, creating parent directories.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        path.parent()
            .map(|p| std::fs::create_dir_all(p).map_err(|e| format!("create {}: {e}", p.display())))
            .transpose()?;
        std::fs::write(path, self.to_toml()).map_err(|e| format!("write {}: {e}", path.display()))
    }

    /// The iteration number the next appended entry should carry (max + 1, or 1).
    pub fn next_iteration(&self) -> u32 {
        self.entries.iter().map(|e| e.iteration).max().map_or(1, |m| m + 1)
    }

    /// Append an entry.
    pub fn append(&mut self, entry: LedgerEntry) {
        self.entries.push(entry);
    }

    /// The iteration numbers of prior *failed* attempts on `axis` — iterations that
    /// attacked it but did **not** promote the candidate. This is the evidence the
    /// abstraction gate counts.
    pub fn failed_attempts_on(&self, axis: Axis) -> Vec<u32> {
        self.entries
            .iter()
            .filter(|e| e.attacked_axis == axis && !e.decision.replaces_champion())
            .map(|e| e.iteration)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(iteration: u32, axis: Axis, decision: Decision) -> LedgerEntry {
        LedgerEntry {
            iteration,
            attacked_axis: axis,
            changed_files: vec!["manifest.candidate.toml".to_string()],
            champion_screenshot: "champion.png".to_string(),
            candidate_screenshot: "candidate.png".to_string(),
            decision,
            reason: "test".to_string(),
            next_attacked_axis: Some(Axis::FogAndHaze),
            abstraction_introduced: false,
            human_note: String::new(),
            scorecard_before: Scorecard::uniform(2),
            scorecard_after: Scorecard::uniform(3),
        }
    }

    #[test]
    fn round_trips_through_toml_with_nested_scorecards() {
        let mut ledger = Ledger::new();
        ledger.append(entry(1, Axis::VegetationDensity, Decision::KeepCandidate));
        ledger.append(entry(2, Axis::FogAndHaze, Decision::RejectCandidate));
        let text = ledger.to_toml();
        let back = Ledger::parse(&text).unwrap();
        assert_eq!(ledger, back);
        assert_eq!(back.entries[0].scorecard_after, Scorecard::uniform(3));
    }

    #[test]
    fn next_iteration_advances() {
        let mut ledger = Ledger::new();
        assert_eq!(ledger.next_iteration(), 1);
        ledger.append(entry(1, Axis::VegetationDensity, Decision::KeepCandidate));
        assert_eq!(ledger.next_iteration(), 2);
    }

    #[test]
    fn failed_attempts_count_only_non_promoting_entries_on_the_axis() {
        let mut ledger = Ledger::new();
        ledger.append(entry(1, Axis::FogAndHaze, Decision::RejectCandidate));
        ledger.append(entry(2, Axis::FogAndHaze, Decision::StartNewCandidateBranch));
        ledger.append(entry(3, Axis::FogAndHaze, Decision::KeepCandidate)); // promoted → not failed
        ledger.append(entry(4, Axis::ColorPalette, Decision::RejectCandidate)); // other axis
        assert_eq!(ledger.failed_attempts_on(Axis::FogAndHaze), vec![1, 2]);
        assert_eq!(ledger.failed_attempts_on(Axis::ColorPalette), vec![4]);
        assert!(ledger.failed_attempts_on(Axis::ObjectScale).is_empty());
    }

    #[test]
    fn load_or_empty_handles_absent_file_and_save_round_trip() {
        let dir = std::env::temp_dir().join("axiom_vt_ledger_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ledger.toml");
        let _ = std::fs::remove_file(&path);
        assert_eq!(Ledger::load_or_empty(&path).unwrap(), Ledger::new());
        let mut ledger = Ledger::new();
        ledger.append(entry(1, Axis::VegetationDensity, Decision::KeepCandidate));
        ledger.save(&path).unwrap();
        assert_eq!(Ledger::load_or_empty(&path).unwrap(), ledger);
        let _ = std::fs::remove_file(&path);
    }
}
