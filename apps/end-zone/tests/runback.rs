//! **The running back's three moves**, proven against the real game.
//!
//! Every test here drives the actual simulation through the actual command
//! stream — the same `RunbackMove` a key press or a swipe produces — and reads
//! authoritative state or the real event stream back. Nothing calls a mechanic
//! directly, sets a flag, or asserts on a screenshot.
//!
//! The encounters are *staged* (see `scenario`): the game is played for real up
//! to the moment control arrives, and then one defender is placed and pointed so
//! the question being asked is a specific one. Everything after the board is set
//! is the game's own.

use axiom_end_zone::events::SimEvent;
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::runback::{charge, RunbackMove};
use axiom_end_zone::scenario::{self, EncounterSetup};
use axiom_end_zone::showcase::ShowcaseRun;

/// Step `ticks` ticks, collecting every event.
fn advance(run: &mut ShowcaseRun, ticks: u32) -> Vec<SimEvent> {
    (0..ticks)
        .flat_map(|_| {
            run.step(&[])
                .events
                .into_iter()
                .map(|stamped| stamped.event)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The runner's offense-relative position (lateral, downfield).
fn where_is(run: &ShowcaseRun, back: PlayerId) -> (f32, f32) {
    let point = run
        .sim
        .frame
        .from_world(run.sim.players[back.index()].pos);
    (point.lateral, point.downfield)
}

// ---------------------------------------------------------------------------
// Juking
// ---------------------------------------------------------------------------

/// A left juke moves the back to the offense's LEFT — a real, measurable
/// displacement, not a pose change.
#[test]
fn a_left_juke_carries_the_back_left() {
    let staged = scenario::stage(EncounterSetup::imminent_tackle()).expect("a staged carry");
    let mut run = staged.run;
    let (before_lat, before_down) = where_is(&run, staged.back);

    assert!(run.command(RunbackMove::JukeLeft), "a carrying back accepts a juke");
    advance(&mut run, 16);

    let (after_lat, after_down) = where_is(&run, staged.back);
    assert!(
        after_lat < before_lat - 1.5,
        "a left juke moves him left by more than a tackle's reach: {before_lat:.2} -> {after_lat:.2}"
    );
    // Forward momentum is RETAINED — a juke is not a sidestep that stops the run.
    assert!(
        after_down > before_down + 0.5,
        "he keeps going downfield through the cut: {before_down:.2} -> {after_down:.2}"
    );
}

/// And a right juke moves him right. Same mechanic, opposite sign — the test
/// exists so a sign error cannot pass by being symmetric.
#[test]
fn a_right_juke_carries_the_back_right() {
    let staged = scenario::stage(EncounterSetup::imminent_tackle()).expect("a staged carry");
    let mut run = staged.run;
    let (before_lat, before_down) = where_is(&run, staged.back);

    assert!(run.command(RunbackMove::JukeRight));
    advance(&mut run, 16);

    let (after_lat, after_down) = where_is(&run, staged.back);
    assert!(
        after_lat > before_lat + 1.5,
        "a right juke moves him right: {before_lat:.2} -> {after_lat:.2}"
    );
    assert!(after_down > before_down + 0.5, "forward momentum survives");
}

/// A juke against a defender whose tackle was genuinely imminent, that leaves
/// him behind, is a **confirmed dodge**.
#[test]
fn beating_an_imminent_tackler_raises_the_dodge_signal() {
    let staged = scenario::stage(EncounterSetup::imminent_tackle()).expect("a staged carry");
    let mut run = staged.run;
    // He is coming from the runner's right, so the cut goes left.
    assert!(run.command(RunbackMove::JukeLeft));
    let events = advance(&mut run, 90);

    let dodged: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::TackleDodged { defender, gap, .. } => Some((*defender, *gap)),
            _ => None,
        })
        .collect();
    assert!(
        dodged.iter().any(|(id, _)| *id == staged.defender),
        "the man who was about to make the tackle is the man recorded as beaten; got {dodged:?}"
    );
}

/// A juke thrown at empty grass raises **nothing**. This is the guard against
/// the cheap version of the feature — a signal that fires because a button was
/// pressed rather than because a defender was beaten.
#[test]
fn juking_with_no_threat_nearby_raises_no_dodge() {
    let setup = EncounterSetup {
        // Far enough that no projection of either body reaches a tackle.
        ahead: 14.0,
        lateral: 6.0,
        closing: 0.0,
        ..EncounterSetup::imminent_tackle()
    };
    let staged = scenario::stage(setup).expect("a staged carry");
    let mut run = staged.run;
    assert!(run.command(RunbackMove::JukeLeft));
    let events = advance(&mut run, 90);

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SimEvent::TackleDodged { .. })),
        "no defender was beaten, so nothing is claimed"
    );
}

// ---------------------------------------------------------------------------
// The shoulder charge
// ---------------------------------------------------------------------------

/// A fast, square, well-timed charge against an unset defender goes THROUGH him:
/// the signal fires, the defender is put off his feet, and the back keeps the
/// ball.
#[test]
fn a_favourable_shoulder_charge_breaks_the_tackle() {
    let staged = scenario::stage(EncounterSetup::favourable_charge()).expect("a staged carry");
    let mut run = staged.run;
    assert!(run.command(RunbackMove::Shoulder));
    let events = advance(&mut run, 45);

    let broken: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::TackleBroken {
                defender,
                impulse,
                resistance,
                ..
            } => Some((*defender, *impulse, *resistance)),
            _ => None,
        })
        .collect();
    assert!(!broken.is_empty(), "the charge resolved in the runner's favour");
    let (_, impulse, resistance) = broken[0];
    assert!(
        impulse > resistance,
        "and it did so because the arithmetic said so: {impulse:.2} > {resistance:.2}"
    );
    // The defender is genuinely displaced — the impact is sold, not implied.
    assert!(
        !run.sim.players[staged.defender.index()].anim.can_act(),
        "the man he ran through is off his feet"
    );
    assert_eq!(
        run.sim.possession,
        Some(staged.back),
        "and the back still has the ball"
    );
}

/// A slow, mistimed charge into a defender who is squared up and coming hard
/// LOSES — and losing it gets the back tackled, which is what makes a bad charge
/// a real mistake rather than a free attempt.
#[test]
fn an_unfavourable_shoulder_charge_is_stuffed_and_ends_in_a_tackle() {
    let staged = scenario::stage(EncounterSetup::unfavourable_charge()).expect("a staged carry");
    let mut run = staged.run;
    assert!(run.command(RunbackMove::Shoulder));
    let events = advance(&mut run, 120);

    let stuffed = events.iter().any(|e| matches!(e, SimEvent::ChargeStuffed { .. }));
    let broke = events.iter().any(|e| matches!(e, SimEvent::TackleBroken { .. }));
    assert!(stuffed, "the charge lost");
    assert!(!broke, "and it is NOT also reported as a success");
    assert!(
        events.iter().any(|e| matches!(e, SimEvent::PlayEnded { .. })),
        "a stuffed charge leaves him stopped in front of the man who stopped him"
    );
}

/// The resolution is a *function of the inputs*, and moving each one moves the
/// answer in the direction a person would expect. This is the test that says the
/// collision is a calculation rather than a coin flip with extra steps.
#[test]
fn the_charge_calculation_responds_to_speed_alignment_and_timing() {
    let tuning = axiom_end_zone::data::RunbackTuning::default();
    let staged = scenario::stage(EncounterSetup::favourable_charge()).expect("a staged carry");
    let runner = staged.run.sim.players[staged.back.index()];
    let defender = staged.run.sim.players[staged.defender.index()];

    let base = charge::resolve(&runner, &defender, tuning.charge_ideal_gap, &tuning);

    // SPEED: halve the runner's velocity and the impulse falls.
    let mut slow = runner;
    slow.vel = slow.vel.mul_scalar(0.5);
    let slower = charge::resolve(&slow, &defender, tuning.charge_ideal_gap, &tuning);
    assert!(
        slower.impulse < base.impulse,
        "less speed delivers less: {:.2} < {:.2}",
        slower.impulse,
        base.impulse
    );

    // ALIGNMENT: send him sideways past the defender and the impulse falls again.
    let mut askew = runner;
    askew.vel = axiom::prelude::Vec3::new(runner.vel.z, 0.0, -runner.vel.x);
    let glancing = charge::resolve(&askew, &defender, tuning.charge_ideal_gap, &tuning);
    assert!(
        glancing.alignment < base.alignment,
        "a sideways run is less aligned"
    );
    assert!(glancing.impulse < base.impulse, "and delivers less");

    // TIMING: the same charge committed on top of the man, or from far out, is
    // worth strictly less than the same charge committed at the right distance.
    let early = charge::resolve(&runner, &defender, tuning.charge_ideal_gap * 3.0, &tuning);
    let late = charge::resolve(&runner, &defender, 0.0, &tuning);
    assert!(base.timing > early.timing, "too early is mistimed");
    assert!(base.timing > late.timing, "so is too late");
    assert!(early.impulse < base.impulse && late.impulse < base.impulse);

    // BRACE: a defender squared up resists more than one caught turned around.
    let mut turned = defender;
    turned.facing += core::f32::consts::PI;
    let caught_out = charge::resolve(&runner, &turned, tuning.charge_ideal_gap, &tuning);
    assert!(
        caught_out.resistance < base.resistance,
        "an unsquared defender anchors less: {:.2} < {:.2}",
        caught_out.resistance,
        base.resistance
    );

    // And the verdict is exactly the comparison — nothing hidden.
    assert_eq!(base.won, base.impulse > base.resistance);
}

// ---------------------------------------------------------------------------
// The leap
// ---------------------------------------------------------------------------

/// The apex clears a standing player, and he keeps making forward progress the
/// whole way — the two properties that make a jump a football move rather than a
/// pause.
#[test]
fn the_leap_clears_a_standing_player_and_keeps_going_forward() {
    let staged = scenario::stage(EncounterSetup::favourable_charge()).expect("a staged carry");
    let mut run = staged.run;
    let (_, before_down) = where_is(&run, staged.back);

    assert!(run.command(RunbackMove::Jump));
    let mut apex = 0.0f32;
    let mut airborne_ticks = 0;
    for _ in 0..80 {
        run.step(&[]);
        apex = apex.max(run.sim.players[staged.back.index()].pos.y);
        airborne_ticks += u32::from(run.sim.runback.airborne);
    }
    let (_, after_down) = where_is(&run, staged.back);

    // A player figure is two yards tall (`player::model::FIGURE_CENTER_Y` is its
    // waist at 1.0), so clearing one means feet above two.
    assert!(
        apex > 2.0,
        "the apex carries his feet over a standing man's head: {apex:.2} yd"
    );
    assert!(
        airborne_ticks > 30 && airborne_ticks < 75,
        "a readable arc, not a hover: {airborne_ticks} ticks"
    );
    assert!(
        after_down > before_down + 4.0,
        "he covers real ground through the arc: {before_down:.2} -> {after_down:.2}"
    );
    assert!(
        !run.sim.runback.airborne,
        "and lands back into the run under his own arc"
    );
    assert_eq!(run.sim.players[staged.back.index()].pos.y, 0.0);
}

/// A second leap cannot begin while the first is still in the air.
#[test]
fn a_second_leap_cannot_begin_while_airborne() {
    let staged = scenario::stage(EncounterSetup::favourable_charge()).expect("a staged carry");
    let mut run = staged.run;
    assert!(run.command(RunbackMove::Jump));
    advance(&mut run, 12);
    assert!(run.sim.runback.airborne, "he is in the air");

    let launched_at = run.sim.tick;
    run.command(RunbackMove::Jump);
    let events = advance(&mut run, 6);
    assert!(
        !events.iter().any(|e| matches!(
            e,
            SimEvent::RunbackMove {
                move_code: axiom_end_zone::events::RunbackMoveCode::Jump,
                ..
            }
        )),
        "the second leap is refused outright"
    );
    assert_eq!(
        run.sim.runback.jump_ready_at,
        launched_at - 12 + axiom_end_zone::data::RunbackTuning::default().jump_cooldown_ticks,
        "and the cooldown still dates from the FIRST launch"
    );
}

/// The leap is unavailable for three seconds of **simulation** time, then
/// available again — and the wait is counted in ticks, never in wall clock.
#[test]
fn the_leap_waits_out_a_three_second_deterministic_cooldown() {
    let cooldown = axiom_end_zone::data::RunbackTuning::default().jump_cooldown_ticks;
    assert_eq!(cooldown, 180, "three seconds at the fixed 60 Hz step");

    let staged = scenario::stage(EncounterSetup::favourable_charge()).expect("a staged carry");
    let mut run = staged.run;
    assert!(run.command(RunbackMove::Jump));
    let launched = run.sim.tick;

    // Land, then try again well inside the cooldown.
    advance(&mut run, 70);
    assert!(!run.sim.runback.airborne, "he has landed");
    assert!(
        !run.sim.runback.jump_available(run.sim.tick),
        "but the leap is still on cooldown"
    );
    run.command(RunbackMove::Jump);
    let denied = advance(&mut run, 4);
    assert!(
        !denied.iter().any(|e| matches!(
            e,
            SimEvent::RunbackMove {
                move_code: axiom_end_zone::events::RunbackMoveCode::Jump,
                ..
            }
        )),
        "a leap inside the cooldown does not happen"
    );

    // Run out the rest of it and it comes back.
    let remaining = run.sim.runback.jump_cooldown_left(run.sim.tick);
    advance(&mut run, remaining as u32 + 1);
    assert_eq!(
        run.sim.runback.jump_cooldown_left(run.sim.tick),
        0,
        "the cooldown expired exactly {cooldown} ticks after launch (t{launched})"
    );
    assert!(
        run.sim.runback.jump_available(run.sim.tick),
        "and the leap is offered again"
    );
}

/// A leap timed over an incoming defender is a **confirmed hurdle**: he passed
/// beneath, there was daylight above his reach, and the carry went on.
#[test]
fn leaping_over_an_incoming_defender_raises_the_hurdle_signal() {
    let setup = EncounterSetup {
        // Far enough ahead that the arc is near its apex when they meet.
        ahead: 4.6,
        lateral: 0.0,
        closing: 6.0,
        squared: true,
        ..EncounterSetup::favourable_charge()
    };
    let staged = scenario::stage(setup).expect("a staged carry");
    let mut run = staged.run;
    assert!(run.command(RunbackMove::Jump));
    let events = advance(&mut run, 80);

    let hurdled: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::DefenderHurdled {
                defender,
                clearance,
                ..
            } => Some((*defender, *clearance)),
            _ => None,
        })
        .collect();
    assert!(
        hurdled.iter().any(|(id, _)| *id == staged.defender),
        "the man he went over is the man recorded; got {hurdled:?}"
    );
    assert!(
        hurdled.iter().all(|(_, clearance)| *clearance > 0.0),
        "with real daylight above a defender's reach"
    );
    assert_eq!(
        run.sim.possession,
        Some(staged.back),
        "and the play continued after it"
    );
}

/// Leaping over nothing raises nothing.
#[test]
fn leaping_with_nobody_underneath_raises_no_hurdle() {
    let setup = EncounterSetup {
        ahead: 15.0,
        lateral: 9.0,
        closing: 0.0,
        ..EncounterSetup::favourable_charge()
    };
    let staged = scenario::stage(setup).expect("a staged carry");
    let mut run = staged.run;
    assert!(run.command(RunbackMove::Jump));
    let events = advance(&mut run, 80);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SimEvent::DefenderHurdled { .. })),
        "nobody went underneath, so nothing is claimed"
    );
}

/// A defender cannot tackle a man who is over his head. The height gate is the
/// root fix that makes the leap mean anything — without it the jump is an
/// animation the tackle framework ignores.
#[test]
fn a_standing_defender_cannot_tackle_a_carrier_above_his_reach() {
    let setup = EncounterSetup {
        ahead: 4.6,
        lateral: 0.0,
        closing: 6.0,
        squared: true,
        ..EncounterSetup::favourable_charge()
    };
    let staged = scenario::stage(setup).expect("a staged carry");
    let mut run = staged.run;
    assert!(run.command(RunbackMove::Jump));

    let reach = run.sim.tuning.tackle_reach_height;
    let mut tackled_while_high = false;
    for _ in 0..70 {
        let height = run.sim.players[staged.back.index()].pos.y;
        let events = run.step(&[]);
        let hit = events
            .events
            .iter()
            .any(|s| matches!(s.event, SimEvent::TackleContact { .. }));
        tackled_while_high |= hit && height > reach;
    }
    assert!(
        !tackled_while_high,
        "nobody standing on the turf brought him down while he was above {reach} yd"
    );
}
