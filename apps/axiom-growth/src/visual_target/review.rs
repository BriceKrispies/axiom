//! The **convergence decision**: given the reigning champion scorecard, a freshly
//! rendered candidate scorecard, and the axis this iteration attacked, decide what
//! happens to the candidate — and which flaw to attack next.
//!
//! The rule the whole loop turns on:
//!
//! > A candidate can only replace the champion if it **improves the attacked axis**
//! > and **does not significantly damage any other axis**. A regression is
//! > *significant* if any non-attacked axis drops by [`SIGNIFICANT_DROP`] (2) or
//! > more points.
//!
//! That yields exactly four outcomes ([`Decision`]). A human verdict
//! ([`HumanVerdict`]) may override the machine decision and/or the next attacked
//! axis, and may accept the champion outright — but by default the machine decides,
//! deterministically, from the two scorecards alone.

use serde::{Deserialize, Serialize};

use super::axes::{Axis, Scorecard};

/// A non-attacked axis dropping by this many points (or more) is a *significant*
/// regression, and blocks the candidate from replacing the champion.
pub const SIGNIFICANT_DROP: i16 = 2;

/// What the comparator decides to do with a candidate. Exactly one per iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// The candidate improved the attacked axis with no regression anywhere. It
    /// becomes the new champion.
    KeepCandidate,
    /// The candidate improved the attacked axis and became champion, but a
    /// non-attacked axis slipped by one point (a *minor*, non-significant
    /// regression). Promoted, but the slip is flagged in the ledger.
    KeepCandidateMarkRegression,
    /// The candidate improved the attacked axis but *significantly* damaged another
    /// axis, so it may not replace the champion. Discard it and try a different
    /// bounded change on the same axis.
    RejectCandidate,
    /// The candidate failed to improve the attacked axis at all — this line of
    /// attack is a dead end. Abandon the candidate and start a fresh branch from
    /// the champion.
    StartNewCandidateBranch,
}

impl Decision {
    /// The snake_case name (ledger / CLI spelling).
    pub fn key(self) -> &'static str {
        match self {
            Decision::KeepCandidate => "keep_candidate",
            Decision::KeepCandidateMarkRegression => "keep_candidate_mark_regression",
            Decision::RejectCandidate => "reject_candidate",
            Decision::StartNewCandidateBranch => "start_new_candidate_branch",
        }
    }

    /// Whether this decision promotes the candidate to champion.
    pub fn replaces_champion(self) -> bool {
        matches!(
            self,
            Decision::KeepCandidate | Decision::KeepCandidateMarkRegression
        )
    }
}

impl std::fmt::Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

/// The per-axis change from champion → candidate on the non-attacked axes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regression {
    /// The single worst (most negative) non-attacked axis change and its size, if
    /// any axis dropped at all. `delta` is `candidate - champion` (negative = drop).
    pub worst: Option<(Axis, i16)>,
}

impl Regression {
    /// The largest drop magnitude across non-attacked axes (0 if none dropped).
    pub fn worst_drop(&self) -> i16 {
        self.worst
            .map(|(_, d)| -d)
            .filter(|&drop| drop > 0)
            .unwrap_or(0)
    }

    /// A non-attacked axis fell by `SIGNIFICANT_DROP` or more.
    pub fn is_significant(&self) -> bool {
        self.worst_drop() >= SIGNIFICANT_DROP
    }

    /// A non-attacked axis fell, but by less than `SIGNIFICANT_DROP` (a tolerable
    /// slip worth flagging).
    pub fn is_minor(&self) -> bool {
        let drop = self.worst_drop();
        drop > 0 && drop < SIGNIFICANT_DROP
    }
}

/// `candidate[attacked] - champion[attacked]`: how far the attacked axis moved.
pub fn attacked_delta(champion: &Scorecard, candidate: &Scorecard, attacked: Axis) -> i16 {
    i16::from(candidate.get(attacked)) - i16::from(champion.get(attacked))
}

/// Analyze the non-attacked axes for the worst regression from champion → candidate.
pub fn regression(champion: &Scorecard, candidate: &Scorecard, attacked: Axis) -> Regression {
    let worst = Axis::ALL
        .into_iter()
        .filter(|&a| a != attacked)
        .map(|a| (a, i16::from(candidate.get(a)) - i16::from(champion.get(a))))
        .min_by_key(|&(_, delta)| delta)
        .filter(|&(_, delta)| delta < 0);
    Regression { worst }
}

/// The deterministic machine decision, from the two scorecards and the attacked
/// axis alone. See [`Decision`] for the four outcomes.
pub fn decide(champion: &Scorecard, candidate: &Scorecard, attacked: Axis) -> Decision {
    let improved = attacked_delta(champion, candidate, attacked) > 0;
    let reg = regression(champion, candidate, attacked);
    match (improved, reg.is_significant(), reg.is_minor()) {
        (false, _, _) => Decision::StartNewCandidateBranch,
        (true, true, _) => Decision::RejectCandidate,
        (true, false, true) => Decision::KeepCandidateMarkRegression,
        (true, false, false) => Decision::KeepCandidate,
    }
}

/// A human-readable explanation of why the machine reached `decision`, for the
/// ledger's `reason` field.
pub fn reason(
    champion: &Scorecard,
    candidate: &Scorecard,
    attacked: Axis,
    decision: Decision,
) -> String {
    let delta = attacked_delta(champion, candidate, attacked);
    let before = champion.get(attacked);
    let after = candidate.get(attacked);
    let reg = regression(champion, candidate, attacked);
    let reg_note = match reg.worst {
        Some((axis, d)) => format!("worst non-attacked change {axis} {d:+}"),
        None => "no non-attacked axis dropped".to_string(),
    };
    match decision {
        Decision::KeepCandidate => format!(
            "attacked axis {attacked} {before}->{after} ({delta:+}); {reg_note}; promoted"
        ),
        Decision::KeepCandidateMarkRegression => format!(
            "attacked axis {attacked} {before}->{after} ({delta:+}); {reg_note} (minor, < {SIGNIFICANT_DROP}); promoted with regression flag"
        ),
        Decision::RejectCandidate => format!(
            "attacked axis {attacked} {before}->{after} ({delta:+}) but {reg_note} (significant, >= {SIGNIFICANT_DROP}); champion kept"
        ),
        Decision::StartNewCandidateBranch => format!(
            "attacked axis {attacked} {before}->{after} ({delta:+}) did not improve; abandon branch, restart from champion"
        ),
    }
}

/// An optional human override layered on top of the machine decision. Every field
/// is opt-in: an all-default verdict changes nothing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanVerdict {
    /// Force this decision instead of the machine's, if set.
    #[serde(default)]
    pub decision: Option<Decision>,
    /// Force the next attacked axis instead of the lowest-scoring one, if set.
    #[serde(default)]
    pub attacked_axis: Option<Axis>,
    /// Accept the current champion as complete regardless of axis scores.
    #[serde(default)]
    pub accept_champion: bool,
    /// A free-text note recorded in the ledger.
    #[serde(default)]
    pub note: String,
}

impl HumanVerdict {
    /// Load a verdict from a TOML file, or `None` if the file is absent.
    pub fn load_optional(path: &std::path::Path) -> Result<Option<HumanVerdict>, String> {
        path.exists()
            .then(|| {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| format!("cannot read verdict {}: {e}", path.display()))?;
                toml::from_str(&text).map_err(|e| format!("verdict parse error: {e}"))
            })
            .transpose()
    }
}

/// The decision after applying an optional human override, with a flag recording
/// whether the human overrode the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedDecision {
    pub machine: Decision,
    pub effective: Decision,
    pub human_overrode: bool,
}

/// Resolve the effective decision: a human `decision` override wins; otherwise the
/// machine decision stands.
pub fn resolve_decision(machine: Decision, verdict: Option<&HumanVerdict>) -> ResolvedDecision {
    match verdict.and_then(|v| v.decision) {
        Some(forced) => ResolvedDecision {
            machine,
            effective: forced,
            human_overrode: forced != machine,
        },
        None => ResolvedDecision {
            machine,
            effective: machine,
            human_overrode: false,
        },
    }
}

/// Whether the champion is complete: either every axis clears the bar, or the human
/// explicitly accepted the champion.
pub fn is_complete(champion: &Scorecard, verdict: Option<&HumanVerdict>) -> bool {
    champion.all_axes_pass() || verdict.map(|v| v.accept_champion).unwrap_or(false)
}

/// The next axis to attack, once the decision has resolved which scorecard is the
/// champion. `None` when the champion is complete. A human `attacked_axis` override
/// wins; otherwise the champion's lowest-scoring axis is the next flaw.
pub fn next_attacked_axis(champion: &Scorecard, verdict: Option<&HumanVerdict>) -> Option<Axis> {
    (!is_complete(champion, verdict)).then(|| {
        verdict
            .and_then(|v| v.attacked_axis)
            .unwrap_or_else(|| champion.lowest_axis())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A champion with a distinct low value on `attacked` so a candidate can improve
    /// it, and everything else mid.
    fn champ() -> Scorecard {
        let mut c = Scorecard::uniform(3);
        c.set(Axis::VegetationDensity, 1);
        c
    }

    #[test]
    fn clean_improvement_keeps_candidate() {
        let champion = champ();
        let mut candidate = champion;
        candidate.set(Axis::VegetationDensity, 3); // attacked axis up, nothing drops.
        let d = decide(&champion, &candidate, Axis::VegetationDensity);
        assert_eq!(d, Decision::KeepCandidate);
        assert!(d.replaces_champion());
        assert!(reason(&champion, &candidate, Axis::VegetationDensity, d).contains("promoted"));
    }

    #[test]
    fn minor_regression_is_kept_but_flagged() {
        let champion = champ();
        let mut candidate = champion;
        candidate.set(Axis::VegetationDensity, 3); // attacked up
        candidate.set(Axis::ColorPalette, 2); // a non-attacked axis down by 1 (minor)
        let d = decide(&champion, &candidate, Axis::VegetationDensity);
        assert_eq!(d, Decision::KeepCandidateMarkRegression);
        assert!(d.replaces_champion());
        let reg = regression(&champion, &candidate, Axis::VegetationDensity);
        assert!(reg.is_minor() && !reg.is_significant());
        assert_eq!(reg.worst_drop(), 1);
    }

    #[test]
    fn significant_regression_rejects_even_though_attacked_improved() {
        let champion = champ();
        let mut candidate = champion;
        candidate.set(Axis::VegetationDensity, 4); // attacked up
        candidate.set(Axis::FogAndHaze, 1); // non-attacked down by 2 (significant)
        let d = decide(&champion, &candidate, Axis::VegetationDensity);
        assert_eq!(d, Decision::RejectCandidate);
        assert!(!d.replaces_champion());
        assert!(regression(&champion, &candidate, Axis::VegetationDensity).is_significant());
    }

    #[test]
    fn no_improvement_starts_new_branch() {
        let champion = champ();
        // Attacked axis unchanged (delta 0) even though another axis went up.
        let mut candidate = champion;
        candidate.set(Axis::ColorPalette, 5);
        let d = decide(&champion, &candidate, Axis::VegetationDensity);
        assert_eq!(d, Decision::StartNewCandidateBranch);
        assert!(!d.replaces_champion());
        // A worsened attacked axis is also "no improvement".
        let mut worse = champion;
        worse.set(Axis::VegetationDensity, 0);
        assert_eq!(
            decide(&champion, &worse, Axis::VegetationDensity),
            Decision::StartNewCandidateBranch
        );
    }

    #[test]
    fn attacked_axis_drop_never_counts_as_its_own_regression() {
        // Even if the attacked axis is the biggest drop, `regression` ignores it.
        let champion = Scorecard::uniform(4);
        let mut candidate = champion;
        candidate.set(Axis::VegetationDensity, 0); // huge drop, but it's the attacked axis
        let reg = regression(&champion, &candidate, Axis::VegetationDensity);
        assert_eq!(reg.worst, None);
        // Attacked didn't improve → new branch, not reject.
        assert_eq!(
            decide(&champion, &candidate, Axis::VegetationDensity),
            Decision::StartNewCandidateBranch
        );
    }

    #[test]
    fn human_verdict_overrides_decision() {
        let machine = Decision::RejectCandidate;
        let verdict = HumanVerdict {
            decision: Some(Decision::KeepCandidate),
            ..Default::default()
        };
        let resolved = resolve_decision(machine, Some(&verdict));
        assert_eq!(resolved.effective, Decision::KeepCandidate);
        assert!(resolved.human_overrode);
        // A verdict that agrees with the machine is not counted as an override.
        let agree = HumanVerdict {
            decision: Some(machine),
            ..Default::default()
        };
        assert!(!resolve_decision(machine, Some(&agree)).human_overrode);
        // No verdict → machine stands.
        assert_eq!(resolve_decision(machine, None).effective, machine);
    }

    #[test]
    fn completion_and_next_axis() {
        // Not complete: lowest axis is the next attack.
        let champion = champ();
        assert!(!is_complete(&champion, None));
        assert_eq!(
            next_attacked_axis(&champion, None),
            Some(Axis::VegetationDensity)
        );

        // All axes pass → complete, no next axis.
        let passing = Scorecard::uniform(4);
        assert!(is_complete(&passing, None));
        assert_eq!(next_attacked_axis(&passing, None), None);

        // Human accept forces completion even on a weak champion.
        let accept = HumanVerdict {
            accept_champion: true,
            ..Default::default()
        };
        assert!(is_complete(&champion, Some(&accept)));
        assert_eq!(next_attacked_axis(&champion, Some(&accept)), None);

        // Human axis override picks the next attack when not complete.
        let force = HumanVerdict {
            attacked_axis: Some(Axis::FogAndHaze),
            ..Default::default()
        };
        assert_eq!(
            next_attacked_axis(&champion, Some(&force)),
            Some(Axis::FogAndHaze)
        );
    }

    #[test]
    fn verdict_loads_optionally() {
        let dir = std::env::temp_dir().join("axiom_vt_verdict_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("verdict.toml");
        let _ = std::fs::remove_file(&path);
        assert!(HumanVerdict::load_optional(&path).unwrap().is_none());
        std::fs::write(&path, "accept_champion = true\nnote = \"ship it\"\n").unwrap();
        let v = HumanVerdict::load_optional(&path).unwrap().unwrap();
        assert!(v.accept_champion);
        assert_eq!(v.note, "ship it");
        let _ = std::fs::remove_file(&path);
    }
}
