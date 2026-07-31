//! The gameplay HUD view model, derived from the live attempt loop: the attempt
//! counter, the session line, the state caption, the decision prompt, and the
//! result card — plus the guarantee that the prompt never leaks how open a read
//! is (the one thing the player is supposed to work out for themselves).

use axiom_end_zone::attempt::{AttemptLedger, AttemptPhase, AttemptStep};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::presentation::HudView;
use axiom_end_zone::showcase::ShowcaseRun;

/// Step the run until `want` holds of the attempt view, or give up.
///
/// It calls a play every tick it is offered one. The attempt loop blocks at the
/// line until the player calls something, so a HUD test that wants to see a
/// LIVE play has to be that player — otherwise it would sit on the play card
/// until the limit ran out.
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
    HudView::from_attempt(&step, &ledger)
}

#[test]
fn a_fresh_session_reads_attempt_one_with_an_empty_ledger() {
    let run = ShowcaseRun::new_run(&RunConfig::new(0x00D_0001));
    let hud = HudView::from_attempt(
        &run.attempt()
            .unwrap_or_else(|| unreachable!("attempt view")),
        &AttemptLedger::new(),
    );
    assert_eq!(hud.attempt, "ATTEMPT 001");
    assert_eq!(hud.session, "AVG 0.0   BEST 0   INT 0");
    // At the line the HUD shows the PLAY CARD — a separate view model from the
    // decision prompt, because it is a blocking decision with no clock.
    let card = hud.play_call.expect("the play card is up at the line");
    assert_eq!(card.headline, "CALL THE PLAY");
    assert_eq!(card.plays.len(), 3, "three plays to choose between");
    let keys: Vec<&str> = card.plays.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["1", "2", "3"], "plays are keyed like the reads");
    assert_eq!(card.plays[0].name, "TRIPLE READ");
    assert!(
        card.plays[0].routes.contains("SLANT"),
        "a play is described by the routes it runs, got {:?}",
        card.plays[0].routes
    );
    assert!(
        hud.decision.is_none(),
        "the read prompt belongs to a live ball, not to the line"
    );
    assert_eq!(hud.state, "CALL IT");
    assert!(hud.result.is_none(), "nothing has resolved yet");
}

#[test]
fn the_play_card_carries_no_clock_and_no_standing_selection() {
    // The card is the one decision nothing can run out on, and nothing is
    // pre-chosen: a highlight would claim a play is already in when the whole
    // point is that none is.
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0x00D_0009));
    for _ in 0..300 {
        run.step(&[]);
    }
    let hud = hud_of(&run);
    let card = hud.play_call.expect("the card is still up five seconds later");
    assert_eq!(card.plays.len(), 3);
    // Exhaustive destructuring pins the shape: no `remaining`, no `urgent`, no
    // `selected` may appear here without this test being rewritten on purpose.
    for play in &card.plays {
        let axiom_end_zone::presentation::PlayOption { key, name, routes } = play;
        assert!(!key.is_empty() && !name.is_empty() && !routes.is_empty());
    }
}

#[test]
fn the_decision_window_prompts_three_numbered_reads_and_a_scramble() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0x00D_0002));
    run_until(&mut run, 600, |s| s.phase.in_window()).expect("a decision window opens");
    let hud = hud_of(&run);

    let prompt = hud.decision.expect("the window prompts");
    assert_eq!(prompt.reads.len(), 3, "exactly three eligible reads");
    let keys: Vec<&str> = prompt.reads.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, ["1", "2", "3"], "reads are keyed 1/2/3 in read order");
    assert_eq!(prompt.reads[0].name, "SLANT");
    assert_eq!(prompt.reads[2].name, "POST");
    assert!(prompt.scramble.contains("SCRAMBLE"));
    assert!(
        (0.0..=1.0).contains(&prompt.remaining),
        "the timer bar is a fraction, got {}",
        prompt.remaining
    );
    assert_eq!(hud.state, "DECIDE");
}

#[test]
fn the_prompt_never_reports_how_open_a_read_is() {
    // Exhaustive destructuring pins the shape: a field that scored or ranked
    // the reads would answer the question the prototype exists to ask.
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0x00D_0003));
    run_until(&mut run, 600, |s| s.phase.in_window()).expect("a decision window opens");
    let HudView {
        attempt,
        session,
        state,
        play_call,
        decision,
        result,
    } = hud_of(&run);
    assert!(!attempt.is_empty() && !session.is_empty() && !state.is_empty());
    assert!(result.is_none());
    assert!(play_call.is_none(), "the ball is live; the card is gone");
    let prompt = decision.expect("the window prompts");
    for read in &prompt.reads {
        let axiom_end_zone::presentation::ReadPrompt { key, name } = read;
        assert!(!key.is_empty() && !name.is_empty());
    }
}

#[test]
fn the_window_timer_drains_as_the_window_runs_out() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0x00D_0004));
    run_until(&mut run, 600, |s| s.phase.in_window()).expect("a decision window opens");
    let first = hud_of(&run).decision.expect("prompt").remaining;
    run_until(&mut run, 6, |s| s.phase.in_window());
    let later = hud_of(&run).decision.expect("prompt").remaining;
    assert!(
        later < first,
        "the decision timer drains ({later} should be under {first})"
    );
}

#[test]
fn a_resolved_attempt_shows_a_result_card_with_signed_yards() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0x00D_0005));
    // Let an attempt run to its whistle without ever choosing.
    run_until(&mut run, 2000, |s| {
        matches!(s.phase, AttemptPhase::Result { .. })
    })
    .expect("an attempt resolves");
    let hud = hud_of(&run);
    let card = hud.result.expect("a result card while the whistle holds");
    assert!(card.contains("YD"), "the card reports yards, got {card:?}");
    assert_eq!(hud.state, "WHISTLE");
}

#[test]
fn the_attempt_counter_advances_across_attempts() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0x00D_0006));
    run_until(&mut run, 3000, |s| s.attempt >= 3).expect("three attempts inside the budget");
    assert_eq!(hud_of(&run).attempt, "ATTEMPT 003");
}
