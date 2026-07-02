//! Golden test for the `axiom-agent`-driven soccer-penalty agent (gated behind
//! the `agent` feature, so the default workspace gates never compile it).
//!
//! Proves the agent plays a full deterministic 5-round session — every per-tick
//! decision routed through `axiom-agent` — and that two independent runs produce
//! byte-identical results.
#![cfg(feature = "soccer-penalty-agent")]

use axiom_gallery::soccer_penalty::agent::{PenaltyAgent, PenaltyShotPlan};
use axiom_gallery::soccer_penalty::penalty_result::PenaltyShotResultKind;
use axiom_gallery::soccer_penalty::penalty_session::PenaltyLoopState;
use axiom_gallery::soccer_penalty::SoccerPenaltyApp;

#[test]
fn agent_plays_a_deterministic_full_session() {
    let agent = PenaltyAgent::new(PenaltyShotPlan::scoring());
    let (session, ticks) = agent.play(SoccerPenaltyApp::new_session());

    // The session completes in five rounds.
    assert_eq!(session.loop_state, PenaltyLoopState::SessionComplete);
    assert_eq!(session.history.len(), 5);
    assert!(ticks > 0);

    // The planned scoring shot scores every round; the streak bonus climbs, so
    // the awards are 500 / 600 / 700 / 800 / 900 → 3500.
    let awards: Vec<u32> = session.history.iter().map(|r| r.award.total).collect();
    assert_eq!(awards, vec![500, 600, 700, 800, 900]);
    session
        .history
        .iter()
        .for_each(|r| assert_eq!(r.result.kind, PenaltyShotResultKind::Goal));
    assert_eq!(session.score.score, 3500);
    assert_eq!(session.best.best, 3500);
}

#[test]
fn agent_runs_are_reproducible() {
    let agent = PenaltyAgent::new(PenaltyShotPlan::scoring());
    let (a, ticks_a) = agent.play(SoccerPenaltyApp::new_session());
    let (b, ticks_b) = agent.play(SoccerPenaltyApp::new_session());
    assert_eq!(a, b, "two agent runs must produce identical sessions");
    assert_eq!(ticks_a, ticks_b);
    // The rendered HUD is identical too.
    let ha = SoccerPenaltyApp::build_session_frame(&a).hud;
    let hb = SoccerPenaltyApp::build_session_frame(&b).hud;
    assert_eq!(ha, hb);
}
