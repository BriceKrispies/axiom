//! The decision-window attempt loop: state transitions, window triggering,
//! choice handling, stale-input rejection, reset hygiene, and the four outcomes
//! the prototype must be able to produce.
//!
//! These are the prototype's load-bearing guarantees. If the window stops
//! opening, if a declined window stops costing anything, or if a reset leaks
//! state into the next attempt, the design question can no longer be answered.

use axiom_end_zone::attempt::{
    AttemptOutcome, AttemptPhase, PlayerChoice, WindowTrigger, DECISION_TIME_SCALE, MAX_WINDOWS,
};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::showcase::ShowcaseRun;
use axiom_end_zone::state::PlayPhase;

fn run(seed: u64) -> ShowcaseRun {
    ShowcaseRun::new_run(&RunConfig::new(seed))
}

/// Step until `want` holds, returning the number of ticks it took.
fn until(
    run: &mut ShowcaseRun,
    limit: usize,
    want: impl Fn(&ShowcaseRun) -> bool,
) -> Option<usize> {
    for tick in 0..limit {
        if want(run) {
            return Some(tick);
        }
        run.step(&[]);
    }
    None
}

fn phase(run: &ShowcaseRun) -> Option<AttemptPhase> {
    run.attempt().map(|s| s.phase)
}

// --- state machine ------------------------------------------------------------

#[test]
fn an_attempt_begins_pre_snap_and_snaps_itself() {
    let mut r = run(0xA77E_0001);
    assert!(
        matches!(phase(&r), Some(AttemptPhase::PreSnap { .. })),
        "the loop opens with the offense set, got {:?}",
        phase(&r)
    );
    assert_eq!(r.sim.phase, PlayPhase::PreSnap);
    let ticks = until(&mut r, 200, |r| r.sim.phase == PlayPhase::Live)
        .expect("the ball snaps without any input");
    assert!(
        (30..=90).contains(&ticks),
        "the automatic snap lands in the pre-snap beat, took {ticks} ticks"
    );
    assert!(matches!(phase(&r), Some(AttemptPhase::Developing)));
}

#[test]
fn the_play_develops_before_any_window_opens() {
    let mut r = run(0xA77E_0002);
    until(&mut r, 200, |r| r.sim.phase == PlayPhase::Live).expect("snap");
    let developing = until(&mut r, 400, |r| {
        matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
    })
    .expect("a window opens");
    assert!(
        developing >= 60,
        "the player watches the play develop for ~1s first, got {developing} ticks"
    );
}

#[test]
fn a_decision_window_always_opens_within_the_deadline() {
    // The develop deadline is the guarantee: whatever the coverage does, the
    // player is asked at least once per attempt.
    for seed in 0..8u64 {
        let mut r = run(0xA77E_1000 + seed);
        let found = until(&mut r, 500, |r| {
            matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
        });
        assert!(found.is_some(), "seed {seed}: no decision window opened");
    }
}

#[test]
fn the_window_runs_in_slow_motion_and_full_speed_everywhere_else() {
    let mut r = run(0xA77E_0003);
    assert_eq!(r.time_scale(), 1.0, "pre-snap runs at full speed");
    until(&mut r, 500, |r| {
        matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
    })
    .expect("a window opens");
    assert_eq!(
        r.time_scale(),
        DECISION_TIME_SCALE,
        "the window dilates time"
    );
    // Slow motion is NOT a pause: the simulation still advances through it.
    let before = r.sim.tick;
    r.step(&[]);
    assert_eq!(
        r.sim.tick,
        before + 1,
        "the play keeps running in the window"
    );
}

#[test]
fn a_window_the_player_declines_closes_and_the_play_runs_on() {
    let mut r = run(0xA77E_0004);
    until(&mut r, 500, |r| {
        matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
    })
    .expect("a window opens");
    // Choose nothing; the window must close back into a live, developing play.
    let closed = until(&mut r, 120, |r| {
        matches!(phase(r), Some(AttemptPhase::Developing))
    })
    .expect("the window closes on its own");
    assert!(closed > 0, "the window stayed open for a real span");
    assert_eq!(
        r.time_scale(),
        1.0,
        "full speed returns when the window shuts"
    );
    assert_eq!(r.sim.phase, PlayPhase::Live, "the play is still live");
}

#[test]
fn later_windows_are_shorter_than_the_first() {
    let mut r = run(0xA77E_0005);
    let mut spans = Vec::new();
    for _ in 0..600 {
        if let Some(AttemptPhase::DecisionWindow {
            opened_at,
            closes_at,
            ..
        }) = phase(&r)
        {
            let span = closes_at - opened_at;
            if spans.last() != Some(&span) {
                spans.push(span);
            }
        }
        r.step(&[]);
    }
    assert!(
        spans.len() >= 2,
        "the loop re-arms a second window, got {spans:?}"
    );
    assert!(
        spans[1] < spans[0],
        "every look after the first is shorter: {spans:?}"
    );
}

#[test]
fn the_loop_stops_asking_after_its_window_budget() {
    let mut r = run(0xA77E_0006);
    let mut windows = 0u32;
    let mut open = false;
    for _ in 0..900 {
        let in_window = matches!(phase(&r), Some(AttemptPhase::DecisionWindow { .. }));
        windows += u32::from(in_window && !open);
        open = in_window;
        // A resolved attempt restarts the budget — measure only the first.
        if matches!(
            phase(&r),
            Some(AttemptPhase::Resolving | AttemptPhase::Result { .. })
        ) {
            break;
        }
        r.step(&[]);
    }
    assert!(
        windows <= MAX_WINDOWS,
        "one attempt offered {windows} windows, over the budget of {MAX_WINDOWS}"
    );
}

// --- choices ------------------------------------------------------------------

#[test]
fn a_press_outside_a_window_is_rejected_as_stale() {
    let mut r = run(0xA77E_0007);
    // Pre-snap: nothing is being asked, so nothing may be answered.
    assert!(
        !r.choose(PlayerChoice::Throw(2)),
        "a pre-snap press is stale input"
    );
    until(&mut r, 500, |r| {
        matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
    })
    .expect("a window opens");
    assert!(
        r.choose(PlayerChoice::Throw(0)),
        "the window accepts a read"
    );
    // Having committed, further presses this attempt are stale too.
    assert!(
        !r.choose(PlayerChoice::Throw(2)),
        "the attempt cannot be re-decided once committed"
    );
}

#[test]
fn each_of_the_three_reads_can_be_selected_and_throws_to_that_receiver() {
    for read in 0..3usize {
        let mut r = run(0xA77E_2000 + read as u64);
        let step = until(&mut r, 500, |r| {
            matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
        })
        .and(r.attempt())
        .expect("a window opens");
        let want = step.read.target(read);
        assert!(r.choose(PlayerChoice::Throw(read)));
        assert!(
            matches!(phase(&r), Some(AttemptPhase::DecisionWindow { .. })),
            "the choice is applied on the next tick, not instantly"
        );
        // The ball must go to the NAMED receiver, not the cone's own pick.
        let airborne = until(&mut r, 200, |r| r.sim.ball.is_airborne());
        assert!(airborne.is_some(), "read {read}: the pass went up");
        let intended = match r.sim.ball.state {
            axiom_end_zone::football::BallState::Airborne { flight } => flight.intended,
            _ => unreachable!("checked airborne"),
        };
        assert_eq!(
            intended, want,
            "read {read} must throw to the receiver it names"
        );
        assert!(matches!(
            phase(&r),
            Some(AttemptPhase::PassInFlight { read: r2 }) if r2 == read
        ));
    }
}

#[test]
fn the_throw_leads_the_receiver_rather_than_aiming_at_his_feet() {
    let mut r = run(0xA77E_0008);
    let step = until(&mut r, 500, |r| {
        matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
    })
    .and(r.attempt())
    .expect("a window opens");
    // Read 2 (the dig) is moving hard across the field when it is thrown.
    let target_id = step.read.target(1);
    assert!(r.choose(PlayerChoice::Throw(1)));
    until(&mut r, 200, |r| r.sim.ball.is_airborne()).expect("the pass went up");
    let flight = match r.sim.ball.state {
        axiom_end_zone::football::BallState::Airborne { flight } => flight,
        _ => unreachable!("checked airborne"),
    };
    let receiver = r.sim.players[target_id.index()];
    let speed = receiver.speed();
    let lead = {
        let dx = flight.target.x - receiver.pos.x;
        let dz = flight.target.z - receiver.pos.z;
        (dx * dx + dz * dz).sqrt()
    };
    // A receiver at speed must be thrown AHEAD of where he is standing.
    assert!(
        speed < 1.0 || lead > 0.5,
        "a moving receiver ({speed:.1} yd/s) was thrown a {lead:.2} yd lead"
    );
}

#[test]
fn scrambling_hands_the_quarterback_to_the_player() {
    let mut r = run(0xA77E_0009);
    until(&mut r, 500, |r| {
        matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
    })
    .expect("a window opens");
    assert!(r.choose(PlayerChoice::Scramble));
    until(&mut r, 10, |r| {
        matches!(phase(r), Some(AttemptPhase::Scrambling))
    })
    .expect("the scramble commits");
    assert_eq!(r.time_scale(), 1.0, "full speed returns for the scramble");
    assert!(
        phase(&r).map(|p| p.steerable()).unwrap_or(false),
        "the player now steers the quarterback"
    );
    // The defense must treat him as a runner IMMEDIATELY, not after a delay.
    assert_eq!(
        r.sim.ball_situation(),
        axiom_end_zone::football::BallSituation::QbScramble,
        "the defense sees a running quarterback the moment he commits"
    );
}

#[test]
fn the_player_never_steers_while_the_play_is_developing() {
    // The premise under test: the simulation owns every player until a decision
    // is made. A stick pushed during the drop-back must be dropped.
    let mut r = run(0xA77E_000A);
    until(&mut r, 200, |r| r.sim.phase == PlayPhase::Live).expect("snap");
    for _ in 0..40 {
        r.set_user_stick(axiom::prelude::Vec2::new(1.0, 1.0));
        assert_eq!(
            r.sim.user_stick,
            axiom::prelude::Vec2::ZERO,
            "a stick during {:?} must not reach the simulation",
            phase(&r)
        );
        r.step(&[]);
    }
}

// --- outcomes -----------------------------------------------------------------

#[test]
fn holding_the_ball_through_every_window_gets_the_quarterback_sacked() {
    // The cost side of the central tension. A player who never answers must be
    // punished by the rush, not quietly bailed out.
    let mut sacks = 0;
    for seed in 0..6u64 {
        let mut r = run(0xA77E_3000 + seed);
        let record = until(&mut r, 1500, |r| r.ledger().and_then(|l| l.last).is_some())
            .and_then(|_| r.ledger().and_then(|l| l.last))
            .expect("the attempt resolves without any input");
        assert!(record.declined, "no choice was ever made");
        sacks += u32::from(record.outcome == AttemptOutcome::Sacked);
    }
    assert!(
        sacks >= 4,
        "declining every window must usually end in a sack, got {sacks}/6"
    );
}

#[test]
fn ten_consecutive_attempts_reset_cleanly() {
    let mut r = run(0xA77E_000B);
    let mut seen = 0u32;
    let mut indices = Vec::new();
    for _ in 0..14_000 {
        r.step(&[]);
        let Some(record) = r.ledger().and_then(|l| l.last) else {
            continue;
        };
        if record.index > seen {
            seen = record.index;
            indices.push(record.index);
            // Every attempt is a fresh, complete attempt: it re-spots at the
            // prototype line and it offered at least one decision.
            assert!(
                record.windows >= 1,
                "attempt {} offered no window",
                record.index
            );
            assert!(
                record.yards.is_finite(),
                "attempt {} produced a finite result",
                record.index
            );
        }
        if seen >= 10 {
            break;
        }
    }
    assert_eq!(
        indices,
        (1..=10).collect::<Vec<u32>>(),
        "ten consecutive attempts, none skipped or repeated"
    );
    let ledger = r.ledger().expect("a session");
    assert_eq!(ledger.attempts, 10, "the ledger counted every attempt");
}

#[test]
fn a_reset_leaves_no_stale_target_marker_or_decision_state() {
    let mut r = run(0xA77E_000C);
    // Commit a read, let the attempt resolve, and run into the next one.
    until(&mut r, 500, |r| {
        matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
    })
    .expect("a window opens");
    r.choose(PlayerChoice::Throw(2));
    until(&mut r, 1500, |r| {
        r.ledger().map(|l| l.attempts).unwrap_or(0) >= 1
    })
    .expect("the attempt resolves");
    until(&mut r, 400, |r| {
        matches!(phase(r), Some(AttemptPhase::PreSnap { .. })) && r.sim.phase == PlayPhase::PreSnap
    })
    .expect("the next attempt lines up");

    let step = r.attempt().expect("an attempt view");
    assert_eq!(
        step.windows, 0,
        "the new attempt has offered no windows yet"
    );
    assert!(!step.phase.in_window(), "no window survived the reset");
    assert!(!step.phase.accepts_choice(), "no choice may be queued yet");
    assert!(
        r.sim.throwable.is_empty(),
        "no stale throwable receiver survived the reset"
    );
    assert_eq!(r.sim.possession, None, "the ball is dead at the spot");
    assert_eq!(
        r.sim.players.len(),
        axiom_end_zone::config::PLAYER_COUNT,
        "the reset duplicated no entities"
    );
    assert_eq!(r.time_scale(), 1.0, "no stale time dilation survived");
}

#[test]
fn a_window_headline_says_why_it_opened() {
    let mut r = run(0xA77E_000D);
    until(&mut r, 500, |r| {
        matches!(phase(r), Some(AttemptPhase::DecisionWindow { .. }))
    })
    .expect("a window opens");
    let Some(AttemptPhase::DecisionWindow { trigger, .. }) = phase(&r) else {
        unreachable!("checked above");
    };
    assert!(matches!(
        trigger,
        WindowTrigger::ReadOpen | WindowTrigger::Pressure | WindowTrigger::Deadline
    ));
    assert!(!trigger.label().is_empty());
}
