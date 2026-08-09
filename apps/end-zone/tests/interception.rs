//! The defense's play on a ball in the air: a defender who is right on the ball
//! intercepts it (a turnover that ends the run for now), and one who can only
//! reach it as it arrives swats it down (a contested incompletion). The intended
//! receiver still gets first claim, so an open target completes normally.

use axiom::prelude::Vec3;
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::events::{PlayEndReason, SimEvent};
use axiom_end_zone::football::BallState;
use axiom_end_zone::identity::PlayerId;
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
        park(
            &mut sim,
            receiver,
            Vec3::new(target.x + 50.0, 0.0, target.z),
        );
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
        park(
            &mut sim,
            receiver,
            Vec3::new(target.x + 50.0, 0.0, target.z),
        );
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

// (An `an_interception_resolves_the_attempt_as_a_turnover` test lived here. The
// game layer is a run game now: no pass is thrown, so an interception cannot be
// an attempt outcome and `AttemptOutcome` no longer has a variant for one. The
// SIMULATION's interception path above is untouched and still proven — the
// football framework keeps the capability the game layer stopped using.)
