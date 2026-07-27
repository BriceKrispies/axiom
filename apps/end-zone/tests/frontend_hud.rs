//! The gameplay HUD view model, derived from the live attempt loop: the attempt
//! counter, the session line, the state caption, the decision prompt, and the
//! result card — plus the guarantee that the prompt never leaks how open a read
//! is (the one thing the player is supposed to work out for themselves).

use axiom_end_zone::attempt::{AttemptLedger, AttemptPhase, AttemptStep};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::presentation::HudView;
use axiom_end_zone::showcase::ShowcaseRun;

/// Step the run until `want` holds of the attempt view, or give up.
fn run_until(
    run: &mut ShowcaseRun,
    limit: usize,
    want: impl Fn(&AttemptStep) -> bool,
) -> Option<AttemptStep> {
    for _ in 0..limit {
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
    // Pre-snap the prompt is the PLAY PICKER, not a decision window: the same
    // three chips, naming concepts instead of reads.
    let picker = hud.decision.expect("the play picker is up at the line");
    assert_eq!(picker.headline, "CALL IT");
    assert_eq!(picker.reads.len(), 3, "three concepts to choose between");
    assert!(!picker.urgent, "the line is not a timed decision");
    assert!(hud.result.is_none(), "nothing has resolved yet");
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
        decision,
        result,
    } = hud_of(&run);
    assert!(!attempt.is_empty() && !session.is_empty() && !state.is_empty());
    assert!(result.is_none());
    let prompt = decision.expect("the window prompts");
    for read in &prompt.reads {
        let axiom_end_zone::presentation::ReadPrompt { key, name, charge } = read;
        assert!(!key.is_empty() && !name.is_empty());
        // The wind-up meter is power, not openness — it says nothing about
        // whether the read is a good one.
        assert!((0.0..=1.0).contains(charge));
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
