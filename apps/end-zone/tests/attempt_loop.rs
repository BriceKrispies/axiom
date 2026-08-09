//! **The attempt loop**: call a play, watch it come to you, take the exchange,
//! survive the run.
//!
//! These are the loop's load-bearing guarantees. If the play call stops waiting,
//! if the ball stops reaching the back, or if a carry stops ending, the game is
//! broken however good the moves feel — so each one is asserted against the real
//! loop driving the real simulation, never against a mock.
//!
//! (This file replaced a suite of the same name that tested the decision-window
//! prototype — reads, throws, slow motion, declining a window. That game is
//! gone; testing it would be testing nothing.)

use axiom_end_zone::attempt::{AttemptOutcome, AttemptPhase};
use axiom_end_zone::events::{PlayEndReason, SimEvent};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::runback::RunbackMove;
use axiom_end_zone::showcase::ShowcaseRun;

/// A session at `seed`, armed at the line with the play card up.
fn session(seed: u64) -> ShowcaseRun {
    ShowcaseRun::new_run(&RunConfig::new(seed))
}

/// Step until `predicate` holds, or give up after `limit` ticks.
fn run_until(
    run: &mut ShowcaseRun,
    limit: u32,
    mut predicate: impl FnMut(&ShowcaseRun) -> bool,
) -> bool {
    (0..limit).any(|_| {
        run.step(&[]);
        predicate(run)
    })
}

/// The phase this tick.
fn phase(run: &ShowcaseRun) -> AttemptPhase {
    run.attempt().expect("a session always has a view").phase
}

// ---------------------------------------------------------------------------
// The pre-snap: both ends belong to the player
// ---------------------------------------------------------------------------

/// The play call has **no clock**. An attempt never runs a play nobody chose,
/// so the offense stands at the line indefinitely — this is the one place the
/// loop is allowed to wait forever, and it must.
#[test]
fn the_play_call_waits_indefinitely_for_a_call() {
    let mut run = session(3);
    for _ in 0..600 {
        run.step(&[]);
        assert_eq!(
            phase(&run),
            AttemptPhase::PlayCall,
            "nothing snaps until a play is called"
        );
    }
}

/// Calling a play installs it and starts the shift.
#[test]
fn calling_a_play_installs_that_concept_and_shifts_the_offense() {
    let mut run = session(3);
    run.step(&[]);
    assert!(run.select_concept(2), "the card is up, so the call takes");
    assert!(
        run_until(&mut run, 120, |r| matches!(
            phase(r),
            AttemptPhase::Shifting { .. } | AttemptPhase::Mesh { .. }
        )),
        "the call moves the loop off the card"
    );
    assert_eq!(
        run.attempt().expect("view").concept,
        2,
        "and the offense is lined up in the concept that was called"
    );
}

/// The offense reaches its spots, and the ball goes because it is set — not
/// because a timer expired.
#[test]
fn the_offense_reaches_its_formation_and_then_snaps() {
    let mut run = session(5);
    run.step(&[]);
    run.select_concept(0);
    assert!(
        run_until(&mut run, 300, |r| matches!(phase(r), AttemptPhase::Mesh { .. })),
        "the shift completes and the ball is snapped"
    );
    // Every offensive player is on his spot at the snap: that fact IS the cue.
    let sim = &run.sim;
    let offense = sim.play.possession;
    let strays = sim
        .players
        .iter()
        .enumerate()
        .filter(|(_, p)| p.team == offense)
        .filter(|(index, p)| {
            let align = sim.assignments[*index].align;
            axiom::prelude::Vec3::new(p.pos.x - align.x, 0.0, p.pos.z - align.z).length() > 3.0
        })
        .count();
    assert_eq!(strays, 0, "nobody is still walking to his spot at the snap");
}

// ---------------------------------------------------------------------------
// The exchange
// ---------------------------------------------------------------------------

/// The handoff happens, and it is a real transfer with a real duration — the
/// ball is visibly between two pairs of hands before possession changes.
#[test]
fn the_snap_and_handoff_leave_the_running_back_carrying() {
    let mut run = session(7);
    run.step(&[]);
    run.select_concept(0);
    assert!(
        run_until(&mut run, 400, |r| matches!(phase(r), AttemptPhase::Exchange)),
        "the quarterback and the back meet and the exchange begins"
    );
    assert!(
        run.sim.ball.is_exchanging(),
        "the ball is in transit rather than teleported"
    );
    assert_eq!(
        run.sim.possession, None,
        "and belongs to nobody while it travels"
    );

    let back = run.sim.runback.back.expect("a run play fields a back");
    assert!(
        run_until(&mut run, 200, |r| r.sim.possession == Some(back)),
        "possession lands on the back"
    );
    assert_eq!(phase(&run), AttemptPhase::Carrying, "and control arrives with it");
    assert!(run.sim.back_is_carrying());
}

/// A move pressed before the exchange is **stale** and is dropped — never
/// banked to fire the instant control arrives.
#[test]
fn a_move_pressed_before_the_handoff_is_dropped() {
    let mut run = session(7);
    run.step(&[]);
    run.select_concept(0);
    run_until(&mut run, 400, |r| matches!(phase(r), AttemptPhase::Mesh { .. }));
    assert!(
        !run.command(RunbackMove::JukeLeft),
        "the loop refuses a move while the play is still being handed over"
    );
}

// ---------------------------------------------------------------------------
// The carry
// ---------------------------------------------------------------------------

/// Once he has it, the back runs downfield on his own — no input required.
#[test]
fn the_back_advances_toward_the_end_zone_without_being_steered() {
    let mut run = session(7);
    run.step(&[]);
    run.select_concept(0);
    assert!(run_until(&mut run, 400, |r| matches!(
        phase(r),
        AttemptPhase::Carrying
    )));
    let back = run.sim.runback.back.expect("a back");
    let start = run.sim.frame.from_world(run.sim.players[back.index()].pos).downfield;
    run_until(&mut run, 90, |_| false);
    let now = run.sim.frame.from_world(run.sim.players[back.index()].pos).downfield;
    assert!(
        now > start + 3.0,
        "he covered ground with nobody touching a control: {start:.1} -> {now:.1}"
    );
}

// ---------------------------------------------------------------------------
// The whistle
// ---------------------------------------------------------------------------

/// A carry ends, is measured, and the loop returns to the card. Whatever the
/// outcome, the session must keep cycling.
#[test]
fn a_carry_resolves_and_the_loop_returns_to_the_play_call() {
    let mut run = session(7);
    run.step(&[]);
    run.select_concept(0);
    assert!(
        run_until(&mut run, 900, |r| r.ledger().map(|l| l.attempts) == Some(2)),
        "the first carry resolved and the next attempt was built"
    );
    let ledger = run.ledger().expect("a session ledger");
    let last = ledger.last.expect("a resolved carry");
    assert!(
        matches!(
            last.outcome,
            AttemptOutcome::Tackled
                | AttemptOutcome::Touchdown
                | AttemptOutcome::OutOfBounds
                | AttemptOutcome::Botched
        ),
        "it ended in a real football outcome: {:?}",
        last.outcome
    );
    assert!(
        run_until(&mut run, 300, |r| phase(r) == AttemptPhase::PlayCall),
        "and the card comes back up"
    );
}

/// A carry can end in a **tackle** — the ordinary outcome, and the one that
/// makes every move worth making.
#[test]
fn a_carry_can_end_in_a_tackle() {
    let tackled = (0..12).any(|seed| {
        let mut run = session(seed);
        run.step(&[]);
        run.select_concept(0);
        (0..900).any(|_| {
            let out = run.step(&[]);
            out.events.iter().any(|e| {
                matches!(
                    e.event,
                    SimEvent::PlayEnded {
                        reason: PlayEndReason::Tackled
                    }
                )
            })
        })
    });
    assert!(tackled, "some carry in twelve is brought down");
}

/// And a carry can end in a **touchdown**.
#[test]
fn a_carry_can_reach_the_end_zone() {
    let scored = (0..12).any(|seed| {
        let mut run = session(seed);
        run.step(&[]);
        run.select_concept(0);
        (0..900).any(|_| {
            let out = run.step(&[]);
            out.events.iter().any(|e| {
                matches!(
                    e.event,
                    SimEvent::PlayEnded {
                        reason: PlayEndReason::BrokeFree
                    }
                )
            })
        })
    });
    assert!(scored, "some carry in twelve reaches the end zone");
}
