//! Soccer-penalty's native agent driver (gated behind `soccer-penalty-agent`).
//!
//! Plays a full deterministic 5-round session with [`PenaltyAgent`] (every
//! per-tick decision routed through `axiom-agent`) and prints a per-round
//! report. The game itself renders in the browser via the gallery demo.
//!
//! Run:
//!   cargo run -p axiom-gallery --features soccer-penalty-agent --bin soccer-penalty-agent

use axiom_gallery::soccer_penalty::agent::{PenaltyAgent, PenaltyShotPlan};
use axiom_gallery::soccer_penalty::penalty_session::{PenaltyLoopState, PenaltySessionState};
use axiom_gallery::soccer_penalty::SoccerPenaltyApp;

fn main() {
    let agent = PenaltyAgent::new(PenaltyShotPlan::scoring());

    println!("=== Axiom Soccer Penalty — agent session (axiom-agent driven) ===");
    let mut session = SoccerPenaltyApp::new_session();
    let mut reported = 0usize;
    let mut ticks = 0u32;
    while session.loop_state != PenaltyLoopState::SessionComplete && ticks < 4000 {
        while reported < session.history.len() {
            report_round_at(&session, reported);
            reported += 1;
        }
        let intent = agent.decide(&session);
        session = session.advance(intent);
        ticks += 1;
    }
    while reported < session.history.len() {
        report_round_at(&session, reported);
        reported += 1;
    }
    println!(
        "session complete: final score {}  best {}  ({} rounds)",
        session.score.score,
        session.best.best,
        session.history.len()
    );
}

fn report_round_at(session: &PenaltySessionState, i: usize) {
    if let Some(round) = session.history.get(i) {
        println!(
            "  round {}/5  {:?}  +{:<4} -> score {}",
            round.round_number, round.result.kind, round.award.total, round.award.score_after,
        );
    }
}
