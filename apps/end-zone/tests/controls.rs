//! User-control proofs: the movement stick steers ONLY the offense's ball
//! holder through the same limited controller, a zero stick reproduces the
//! autonomous showcase exactly, and the contextual primary action snaps,
//! throws, and restarts.

use axiom::prelude::Vec2;
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::events::SimEvent;
use axiom_end_zone::showcase::{run_trace, DiagnosticCommand, ShowcaseRun, TRACE_THROW_TICK};
use axiom_end_zone::state::PlayPhase;

#[test]
fn a_zero_stick_reproduces_the_scripted_showcase_exactly() {
    let baseline = run_trace(EndZoneConfig::default(), 700);
    let mut run = ShowcaseRun::new(EndZoneConfig::default());
    let mut events = Vec::new();
    for tick in 0..700u64 {
        run.sim.user_stick = Vec2::ZERO;
        // The trace's one scripted input (the throw press), same tick.
        let commands: &[DiagnosticCommand] = if tick == TRACE_THROW_TICK {
            &[DiagnosticCommand::PrimaryAction]
        } else {
            &[]
        };
        let out = run.step(commands);
        events.extend(out.events);
    }
    assert_eq!(baseline.final_digest, run.sim.digest());
    assert_eq!(baseline.events, events);
}

#[test]
fn the_stick_steers_only_the_offensive_ball_holder() {
    // Hold the stick toward the offense's right from the very start: nothing
    // may move differently until the quarterback actually HOLDS the snap,
    // and then the quarterback must drift toward offense-right (world -X for
    // a +Z drive) relative to the autonomous run.
    let baseline = run_trace(EndZoneConfig::default(), 260);
    let mut run = ShowcaseRun::new(EndZoneConfig::default());
    let mut qb_held_at = None;
    for t in 0..260u64 {
        run.sim.user_stick = Vec2::new(1.0, 0.0);
        let out = run.step(&[]);
        if qb_held_at.is_none() && out.snapshot.possession == Some(out.snapshot.quarterback) {
            qb_held_at = Some(t);
        }
    }
    let qb_held_at = qb_held_at.expect("the quarterback takes the snap");
    assert!(qb_held_at > 0);
    let qb = run.sim.quarterback.index();
    let steered = run.sim.players[qb].pos;
    let auto_at_same_tick = baseline.intents.len(); // both ran 260 ticks
    assert_eq!(auto_at_same_tick, 260);
    let auto = run_trace(EndZoneConfig::default(), 260);
    let auto_qb = auto.final_digest.clone();
    let steered_digest = run.sim.digest();
    assert_ne!(auto_qb, steered_digest, "the stick changed the play");
    // Offense right for a +Z drive is world -X: the steered QB sits well to
    // the -X side of where the autonomous drop-back leaves him (x ≈ -0.42).
    assert!(
        steered.x < -3.0,
        "the quarterback was steered toward offense right, got x={}",
        steered.x
    );
}

#[test]
fn the_stick_respects_the_controller_limits() {
    // Full-stick steering must never exceed the archetype's top speed.
    let mut run = ShowcaseRun::new(EndZoneConfig::default());
    let mut max_speed = 0.0f32;
    for _ in 0..500u64 {
        run.sim.user_stick = Vec2::new(1.0, 0.0);
        let out = run.step(&[]);
        if let Some(carrier) = out.snapshot.possession {
            let view = out.snapshot.player(carrier);
            max_speed = max_speed.max(view.speed);
            let limit = run.sim.players[carrier.index()].archetype.max_speed;
            assert!(
                view.speed <= limit + 1.0e-3,
                "steered speed {} exceeds the archetype limit {}",
                view.speed,
                limit
            );
        }
    }
    assert!(max_speed > 3.0, "the stick genuinely moved the holder");
}

#[test]
fn after_the_whistle_the_carrier_stays_put_and_the_showcase_auto_resets() {
    use axiom_end_zone::showcase::RESET_DELAY;
    let mut run = ShowcaseRun::new(EndZoneConfig::default());
    let mut ended_at: Option<u64> = None;
    let mut holder_at_whistle = None;
    let mut max_drift = 0.0f32;
    let mut restarted_at: Option<u64> = None;
    for tick in 0..1200u64 {
        let commands: &[DiagnosticCommand] = if tick == TRACE_THROW_TICK {
            &[DiagnosticCommand::PrimaryAction]
        } else {
            &[]
        };
        let out = run.step(commands);
        if ended_at.is_none()
            && out
                .events
                .iter()
                .any(|e| matches!(e.event, SimEvent::PlayEnded { .. }))
        {
            ended_at = Some(tick);
            holder_at_whistle = out
                .snapshot
                .possession
                .map(|id| out.snapshot.player(id).pos);
        }
        if let (Some(_), None) = (ended_at, restarted_at) {
            if out
                .events
                .iter()
                .any(|e| matches!(e.event, SimEvent::PlayStarted { .. }))
            {
                restarted_at = Some(tick);
            } else if let (Some(holder), Some(anchor)) =
                (out.snapshot.possession, holder_at_whistle)
            {
                // The recovered carrier must NOT take off for the end zone
                // on the dead play.
                let pos = out.snapshot.player(holder).pos;
                max_drift = max_drift.max(pos.subtract(anchor).length());
            }
        }
    }
    let ended_at = ended_at.expect("the play ends");
    let restarted_at = restarted_at.expect("the showcase resets itself");
    assert!(
        max_drift < 2.5,
        "the downed carrier stayed put after the whistle (drifted {max_drift} yd)"
    );
    let pause = restarted_at - ended_at;
    assert!(
        (RESET_DELAY..RESET_DELAY + 5).contains(&pause),
        "the post-whistle beat is ~5 seconds (was {pause} ticks)"
    );
}

#[test]
fn the_primary_action_snaps_then_throws_then_restarts() {
    let mut run = ShowcaseRun::new(EndZoneConfig::default());
    // Press A immediately: the ball snaps this tick (long before the
    // scripted tick-180 snap).
    let out = run.step(&[DiagnosticCommand::PrimaryAction]);
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e.event, SimEvent::Snap { .. })),
        "primary action pre-snap snaps the ball"
    );
    // Wait for the quarterback to hold it, press A again: the throw is
    // ordered, so the release happens after the wind-up.
    let mut threw_at = None;
    let mut pressed_throw = false;
    for _ in 0..200u64 {
        let holder_is_qb = run.sim.possession == Some(run.sim.quarterback);
        let commands: &[DiagnosticCommand] = if holder_is_qb && !pressed_throw {
            pressed_throw = true;
            &[DiagnosticCommand::PrimaryAction]
        } else {
            &[]
        };
        let out = run.step(commands);
        if out
            .events
            .iter()
            .any(|e| matches!(e.event, SimEvent::Throw { .. }))
        {
            threw_at = Some(out.snapshot.tick);
            break;
        }
    }
    let threw_at = threw_at.expect("the primary action ordered the throw");
    assert!(pressed_throw);
    assert!(
        threw_at < 120,
        "user throw releases far earlier than the script"
    );
    // Run the play out, then A restarts it.
    for _ in 0..900u64 {
        if run.sim.phase == PlayPhase::Ended {
            break;
        }
        run.step(&[]);
    }
    assert_eq!(run.sim.phase, PlayPhase::Ended, "the user-driven play ends");
    run.step(&[DiagnosticCommand::PrimaryAction]);
    let out = run.step(&[]);
    // The restart re-forms the play (a start is scheduled for this tick).
    let restarted = matches!(out.snapshot.phase, PlayPhase::PreSnap)
        || out
            .events
            .iter()
            .any(|e| matches!(e.event, SimEvent::PlayStarted { .. }));
    assert!(
        restarted,
        "primary action after the whistle restarts the play"
    );
}
