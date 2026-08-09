//! The gameplay HUD view model, derived from the live attempt loop: the carry
//! counter, the session line, the state caption, the play card, the four moves,
//! and the result card.
//!
//! Plus the guarantee the HUD exists to keep: it never reports whether a move
//! will *work* against a given defender. Reading the field is the game. The one
//! sanctioned exception is the charge tell, which restores information the chase
//! camera physically cannot show (closing speed, whether a man is braced) — and
//! it is asserted here as an exception rather than left to drift into a habit.

use axiom_end_zone::attempt::{AttemptLedger, AttemptPhase, AttemptStep};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::presentation::HudView;
use axiom_end_zone::showcase::ShowcaseRun;

/// Step the run until `want` holds of the attempt view, or give up.
///
/// It calls a play every tick it is offered one. The loop blocks at the line
/// until the player calls something, so a HUD test that wants to see a LIVE play
/// has to be that player.
fn run_until(
    run: &mut ShowcaseRun,
    limit: usize,
    want: impl Fn(&AttemptStep) -> bool,
) -> Option<AttemptStep> {
    for _ in 0..limit {
        run.select_concept(0);
        run.step(&[]);
        if let Some(step) = run.attempt() {
            if want(&step) {
                return Some(step);
            }
        }
    }
    None
}

fn hud_of(run: &ShowcaseRun) -> HudView {
    let step = run.attempt().expect("a live attempt");
    let ledger = run.ledger().expect("a live session");
    HudView::from_attempt(&step, &ledger, run.sim.tick)
}

#[test]
fn a_fresh_session_reads_carry_one_with_an_empty_ledger() {
    let run = ShowcaseRun::new_run(&RunConfig::new(0x00D_0001));
    let hud = HudView::from_attempt(
        &run.attempt()
            .unwrap_or_else(|| unreachable!("attempt view")),
        &AttemptLedger::new(),
        0,
    );
    assert_eq!(hud.attempt, "CARRY 001");
    assert_eq!(hud.session, "AVG 0.0   BEST 0   MOVES 0");

    // At the line the HUD shows the PLAY CARD: a blocking decision with no
    // clock, so it carries no timer and nothing is pre-selected.
    let card = hud.play_call.expect("the play card is up at the line");
    assert_eq!(card.headline, "CALL THE PLAY");
    assert_eq!(card.plays.len(), 3, "three plays to choose between");
    let keys: Vec<&str> = card.plays.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["1", "2", "3"], "keyed to the number row");
    assert!(
        card.plays.iter().all(|p| !p.routes.is_empty()),
        "each play is described by the hole it opens, not just named"
    );
    assert!(
        hud.moves.is_empty(),
        "the move row is not up before there is anybody to move"
    );
}

#[test]
fn the_move_row_appears_with_the_exchange_and_teaches_both_surfaces() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(7));
    run_until(&mut run, 600, |step| step.phase == AttemptPhase::Carrying)
        .expect("a carry begins");
    let hud = hud_of(&run);

    assert_eq!(hud.state, "RUN");
    assert_eq!(hud.moves.len(), 4, "four verbs, always the same four");
    let keys: Vec<&str> = hud.moves.iter().map(|m| m.key.as_str()).collect();
    assert_eq!(keys, ["A", "D", "S", "W"], "the desktop keys");
    assert!(
        hud.moves.iter().all(|m| !m.swipe.is_empty()),
        "and every one of them also shows the swipe that does it, so a phone and \
         a keyboard are taught the same game by one strip of UI"
    );
    let names: Vec<&str> = hud.moves.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["JUKE LEFT", "JUKE RIGHT", "SHOULDER", "LEAP"]);
}

#[test]
fn the_leap_pip_drains_through_its_cooldown_and_comes_back() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(7));
    run_until(&mut run, 600, |step| step.phase == AttemptPhase::Carrying)
        .expect("a carry begins");
    assert!(
        hud_of(&run).moves[3].ready,
        "the leap starts the carry available"
    );

    assert!(run.command(axiom_end_zone::runback::RunbackMove::Jump));
    run.step(&[]);
    let hud = hud_of(&run);
    assert!(!hud.moves[3].ready, "it is unavailable the moment it is used");
    assert!(
        hud.moves[3].cooldown > 0.5,
        "and the pip is near full at the start of the wait, got {}",
        hud.moves[3].cooldown
    );
}

/// The one thing the HUD must never do: tell the player whether a move will
/// beat the man in front of them. Only the charge is allowed a tell, and only
/// because the camera cannot show what it shows.
#[test]
fn the_hud_never_reports_whether_a_juke_or_a_leap_will_work() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(7));
    run_until(&mut run, 600, |step| step.phase == AttemptPhase::Carrying)
        .expect("a carry begins");
    let hud = hud_of(&run);
    // The juke and leap chips carry availability and nothing else. `hot` is the
    // charge tell; it must never appear on a move whose geometry the player can
    // read for themselves off the screen.
    assert!(!hud.moves[0].hot, "no tell on juke left");
    assert!(!hud.moves[1].hot, "no tell on juke right");
    assert!(!hud.moves[3].hot, "no tell on the leap");
}

#[test]
fn the_result_card_reports_the_outcome_the_yards_and_the_moves() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(7));
    let step = run_until(&mut run, 1200, |step| {
        matches!(step.phase, AttemptPhase::Result { .. })
    })
    .expect("a carry resolves");
    let hud = hud_of(&run);
    let card = hud.result.expect("a result card while one is showing");

    let record = step.last.expect("a resolved record");
    assert!(
        card.starts_with(record.outcome.label()),
        "the card leads with what happened: {card:?}"
    );
    assert!(
        card.contains("YD"),
        "and says how far it went: {card:?}"
    );
    assert_eq!(hud.state, "WHISTLE");
}

#[test]
fn the_session_line_counts_carries_and_moves_rather_than_completions() {
    let mut ledger = AttemptLedger::new();
    ledger.record(axiom_end_zone::attempt::AttemptRecord {
        index: 1,
        outcome: axiom_end_zone::attempt::AttemptOutcome::Touchdown,
        yards: 40.0,
        dodges: 1,
        broken: 2,
        hurdled: 1,
    });
    let run = ShowcaseRun::new_run(&RunConfig::new(1));
    let hud = HudView::from_attempt(
        &run.attempt().unwrap_or_else(|| unreachable!("view")),
        &ledger,
        0,
    );
    assert_eq!(
        hud.session, "AVG 40.0   BEST 40   MOVES 4",
        "a run game measures yards per carry and moves made"
    );
}
