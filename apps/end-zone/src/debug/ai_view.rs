//! The AI + overseer debug read-out rows, split out of [`super`] so the marker
//! builder stays narrowly owned. Text only; reads the immutable snapshot.

use crate::presentation::snapshot::PresentationSnapshot;

/// The defensive overseer's read-out: its mode, adjustment, the exposed region
/// it accepted, its confidence, the last transition, and the rejected alternative.
pub(super) fn overseer_rows(snapshot: &PresentationSnapshot, rows: &mut Vec<(String, String)>) {
    let d = &snapshot.directive;
    let threat = d
        .primary_threat
        .map(|id| format!(" vs #{}", snapshot.player(id).jersey))
        .unwrap_or_default();
    rows.push((
        "overseer".to_string(),
        format!(
            "{} {}{}  conf {:.2}",
            d.mode.label(),
            d.secondary.label(),
            threat,
            d.confidence
        ),
    ));
    rows.push((
        "  tradeoff".to_string(),
        format!("exposes {} · risk {:.1}", d.exposed.label(), d.risk_tolerance),
    ));
    let (rej_mode, rej_score) = snapshot.overseer_rejected;
    rows.push((
        "  last call".to_string(),
        format!(
            "{} → {} ({}); held off {} {:.2}",
            snapshot.overseer_prev_mode.label(),
            d.mode.label(),
            snapshot.overseer_transition_reason,
            rej_mode.label(),
            rej_score
        ),
    ));
}

/// A compact summary of every defender's coordinated responsibility this tick.
pub(super) fn coverage_summary(snapshot: &PresentationSnapshot) -> String {
    let parts: Vec<String> = snapshot
        .players
        .iter()
        .filter(|p| p.responsibility != crate::ai::Responsibility::None)
        .map(|p| format!("#{}:{}", p.jersey, p.responsibility.label()))
        .collect();
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("  ")
    }
}

/// A compact summary of every live line engagement (blocker → advantage/state).
pub(super) fn line_summary(snapshot: &PresentationSnapshot) -> String {
    let parts: Vec<String> = snapshot
        .players
        .iter()
        .filter_map(|p| {
            p.engagement_state.map(|state| {
                format!("#{}:{}{:+.1}", p.jersey, state.label(), p.engagement_advantage)
            })
        })
        .collect();
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("  ")
    }
}
