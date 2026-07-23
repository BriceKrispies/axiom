//! The defense's play on a ball in the air: a defender who is right on the ball
//! intercepts it (a turnover that ends the run for now), and one who can only
//! reach it as it arrives swats it down (a contested incompletion). The intended
//! receiver still gets first claim, so an open target completes normally.

use axiom::prelude::Vec3;
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::events::{PlayEndReason, SimEvent};
use axiom_end_zone::football::BallState;
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::showcase::{DiagnosticCommand, ShowcaseRun};
use axiom_end_zone::state::{PlayPhase, SimCommand, SimState};

/// Drive the scripted showcase schedule until the pass is in the air.
fn to_airborne(sim: &mut SimState) -> (Vec3, PlayerId) {
    for t in 0..220u64 {
        let commands: &[SimCommand] = match t {
            0 => &[SimCommand::BeginPlay],
            80 => &[SimCommand::Snap],
            170 => &[SimCommand::ThrowNow],
            _ => &[],
        };
        sim.step(commands);
        if let BallState::Airborne { flight } = sim.ball.state {
            return (flight.target, flight.intended);
        }
    }
    panic!("the scripted pass never went airborne");
}

/// Freeze a player in place (so scripted warps hold through the step) at `pos`.
fn park(sim: &mut SimState, id: PlayerId, pos: Vec3) {
    let p = &mut sim.players[id.index()];
    p.archetype.max_speed = 0.0;
    p.pos = pos;
    p.vel = Vec3::ZERO;
}

/// The lowest-id defender (opposite the intended receiver).
fn a_defender(sim: &SimState, receiver: PlayerId) -> PlayerId {
    let team = sim.players[receiver.index()].team;
    sim.players
        .iter()
        .find(|p| p.team != team)
        .map(|p| p.id)
        .expect("a defender exists")
}

#[test]
fn a_defender_on_the_ball_intercepts_and_ends_the_run() {
    let mut sim = SimState::new(EndZoneConfig::default());
    let (target, receiver) = to_airborne(&mut sim);
    let defender = a_defender(&sim, receiver);

    // The receiver is shoved out of the play; a defender tracks the ball itself
    // and sits right under it as it descends.
    let mut intercepted = false;
    for _ in 0..120 {
        if !sim.ball.is_airborne() {
            break;
        }
        park(&mut sim, receiver, Vec3::new(target.x + 50.0, 0.0, target.z));
        let ball = sim.ball.pos;
        park(&mut sim, defender, Vec3::new(ball.x, 0.0, ball.z));
        let events = sim.step(&[]).to_vec();
        if events
            .iter()
            .any(|e| matches!(e.event, SimEvent::Intercepted { .. }))
        {
            intercepted = true;
            break;
        }
    }

    assert!(intercepted, "a defender parked on the ball picks it off");
    assert_eq!(sim.phase, PlayPhase::Ended);
    assert_eq!(sim.end_reason, Some(PlayEndReason::Intercepted));
}

#[test]
fn a_defender_who_cannot_secure_it_swats_the_pass_down() {
    let mut sim = SimState::new(EndZoneConfig::default());
    let (target, receiver) = to_airborne(&mut sim);
    let defender = a_defender(&sim, receiver);

    // The defender reaches the ball's edge — in the catch volume but not clean —
    // so he knocks it down instead of picking it off.
    let mut swatted = false;
    for _ in 0..120 {
        if !sim.ball.is_airborne() {
            break;
        }
        park(&mut sim, receiver, Vec3::new(target.x + 50.0, 0.0, target.z));
        let ball = sim.ball.pos;
        park(&mut sim, defender, Vec3::new(ball.x + 0.95, 0.0, ball.z));
        let events = sim.step(&[]).to_vec();
        if events
            .iter()
            .any(|e| matches!(e.event, SimEvent::PassBrokenUp { .. }))
        {
            swatted = true;
            break;
        }
        // A swat must never be mislabeled a turnover in this scenario.
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.event, SimEvent::Intercepted { .. })),
            "an edge-of-reach play is a swat, not an interception"
        );
    }

    assert!(swatted, "a defender at the ball's edge swats it down");
    assert_ne!(
        sim.end_reason,
        Some(PlayEndReason::Intercepted),
        "a swat is an incompletion, not a turnover"
    );
}

#[test]
fn an_interception_ends_the_run() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0x1_2345));
    // Reach the huddle and call a play.
    for _ in 0..2000 {
        run.step(&[]);
        if run.huddle().is_some() {
            break;
        }
    }
    run.call_play(0);

    // Freeze the pass rush so the quarterback gets a clean pocket, and order the
    // throw once he holds a live ball.
    let qb_team = run.sim.players[run.sim.quarterback.index()].team;
    let defenders: Vec<PlayerId> = run
        .sim
        .players
        .iter()
        .filter(|p| p.team != qb_team)
        .map(|p| p.id)
        .collect();
    let mut threw = false;
    for _ in 0..800 {
        for &d in &defenders {
            let p = &mut run.sim.players[d.index()];
            p.archetype.max_speed = 0.0;
            p.vel = Vec3::ZERO;
        }
        let holds = run.sim.ball.carrier() == Some(run.sim.quarterback);
        let commands: Vec<DiagnosticCommand> = if holds {
            vec![DiagnosticCommand::PrimaryAction]
        } else {
            Vec::new()
        };
        run.step(&commands);
        if run.sim.ball.is_airborne() {
            threw = true;
            break;
        }
    }
    assert!(threw, "the quarterback threw the pass");

    // A frozen defender tracks the ball and picks it off.
    let picker = defenders[0];
    for _ in 0..120 {
        if !run.sim.ball.is_airborne() {
            break;
        }
        let ball = run.sim.ball.pos;
        let p = &mut run.sim.players[picker.index()];
        p.archetype.max_speed = 0.0;
        p.pos = Vec3::new(ball.x, 0.0, ball.z);
        p.vel = Vec3::ZERO;
        run.step(&[]);
    }
    // The drive reads the ended play on the following tick — let it resolve.
    for _ in 0..5 {
        run.step(&[]);
    }

    assert_eq!(run.sim.end_reason, Some(PlayEndReason::Intercepted));
    assert!(
        run.drive_state().map(|d| d.over).unwrap_or(false),
        "the interception ends the run"
    );
}
