//! Football proofs: possession sockets, deterministic release and flight,
//! deterministic spin, catch evaluation, possession ordering, and ground
//! contact.

use axiom::prelude::Vec3;
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::data::player::receiver;
use axiom_end_zone::events::{PlayEndReason, SimEvent, StampedEvent};
use axiom_end_zone::football::{carry_socket, evaluate_catch, BallState, CatchVerdict};
use axiom_end_zone::state::{SimCommand, SimState};

/// Drive a sim through the scripted showcase schedule for `ticks`.
fn scripted(sim: &mut SimState, ticks: u64) -> Vec<StampedEvent> {
    let mut all = Vec::new();
    for t in 0..ticks {
        let commands: &[SimCommand] = match t {
            0 => &[SimCommand::BeginPlay],
            80 => &[SimCommand::Snap],
            170 => &[SimCommand::ThrowNow],
            _ => &[],
        };
        all.extend_from_slice(sim.step(commands));
    }
    all
}

fn find<'a>(
    events: &'a [StampedEvent],
    pick: impl Fn(&SimEvent) -> bool,
) -> Option<&'a StampedEvent> {
    events.iter().find(|e| pick(&e.event))
}

#[test]
fn held_football_follows_its_possession_socket() {
    let mut sim = SimState::new(EndZoneConfig::default());
    scripted(&mut sim, 120);
    let carrier = sim.ball.carrier().expect("the quarterback holds the snap");
    for _ in 0..30 {
        sim.step(&[]);
        if let Some(holder) = sim.ball.carrier() {
            let player = &sim.players[holder.index()];
            let socket = carry_socket(player.pos, player.facing, player.anim);
            assert_eq!(
                sim.ball.pos, socket,
                "held ball sits exactly at the sim socket"
            );
        }
    }
    assert_eq!(sim.ball.carrier(), Some(carrier));
}

#[test]
fn the_cradled_ball_pins_its_rear_tip_to_the_forearm_crook() {
    use axiom_end_zone::football::model::cradled_ball_transform;
    use axiom_end_zone::football::state::BALL_VISUAL_SCALE;
    use axiom_math::{Quat, Transform};

    // An arbitrarily tilted forearm so the assertion is not axis-trivial.
    let forearm = Transform::new(
        Vec3::new(1.0, 2.0, -0.5),
        Quat::from_euler_xyz(0.3, -0.4, 0.2),
        Vec3::ONE,
    );
    let ball = cradled_ball_transform(&forearm);

    // The crook: the top face of the forearm box, nudged a touch off the arm.
    let up = forearm.rotation.rotate(Vec3::new(0.0, 1.0, 0.0));
    let toward_hand = forearm.rotation.rotate(Vec3::new(0.0, -1.0, 0.0));
    let crook = forearm
        .translation
        .add(up.mul_scalar(0.17))
        .add(toward_hand.mul_scalar(0.04));

    // The rear (-Y) tip is pinned to the crook — the lever point, not the hip.
    // `BALL_VISUAL_SCALE.y` is the FULL length (sphere radius 0.5), so the tip is
    // half of it from the center.
    let rear_tip = ball
        .translation
        .add(ball.rotation.rotate(Vec3::new(0.0, -BALL_VISUAL_SCALE.y * 0.5, 0.0)));
    assert!(
        rear_tip.subtract(crook).length() < 1.0e-4,
        "the ball's rear tip sits in the crook"
    );
    // And the ball lies down the forearm toward the hand.
    let axis = ball.rotation.rotate(Vec3::new(0.0, 1.0, 0.0));
    assert!(
        axis.dot(toward_hand) > 0.999,
        "the ball's long axis follows the forearm toward the hand"
    );
}

#[test]
fn release_produces_the_expected_deterministic_initial_state() {
    let run = || {
        let mut sim = SimState::new(EndZoneConfig::default());
        let events = scripted(&mut sim, 400);
        find(&events, |e| matches!(e, SimEvent::Throw { .. })).copied()
    };
    let (a, b) = (run(), run());
    let a = a.expect("the pass is thrown");
    assert_eq!(Some(a), b, "identical runs release identically (bit-exact)");
    if let SimEvent::Throw {
        release,
        velocity,
        eta_ticks,
        ..
    } = a.event
    {
        assert!(release.y > 1.5, "released above the shoulder");
        assert!(velocity.y > 0.0, "an upward arc");
        assert!(eta_ticks >= 24, "a real flight, not a teleport");
    }
}

#[test]
fn the_same_throw_inputs_produce_the_same_trajectory() {
    let sample = |seed: u64| {
        let mut sim = SimState::new(EndZoneConfig::with_seed(seed));
        let mut path = Vec::new();
        for t in 0..420u64 {
            let commands: &[SimCommand] = match t {
                0 => &[SimCommand::BeginPlay],
                80 => &[SimCommand::Snap],
                170 => &[SimCommand::ThrowNow],
                _ => &[],
            };
            sim.step(commands);
            if sim.ball.is_airborne() {
                path.push(sim.ball.pos);
            }
        }
        path
    };
    let a = sample(1);
    let b = sample(2);
    assert!(!a.is_empty());
    assert_eq!(
        a, b,
        "the seed feeds presentation only; flight is bit-identical"
    );
}

#[test]
fn flight_advances_consistently_at_fixed_ticks_and_never_teleports() {
    let mut sim = SimState::new(EndZoneConfig::default());
    // Snap at 80, throw ordered at 170, wind-up 12 → release ≈ tick 182.
    scripted(&mut sim, 186);
    assert!(
        sim.ball.is_airborne(),
        "the pass is in the air right after release"
    );
    let mut last = sim.ball.pos;
    let mut airborne_ticks = 0;
    while sim.ball.is_airborne() && airborne_ticks < 200 {
        sim.step(&[]);
        let step = sim.ball.pos.subtract(last).length();
        assert!(
            step < 0.6,
            "one tick advances less than a yard (no teleport), got {step}"
        );
        if sim.ball.is_airborne() {
            assert!(
                sim.ball.pos.z > last.z,
                "the pass travels downfield every tick"
            );
        }
        last = sim.ball.pos;
        airborne_ticks += 1;
    }
    assert!(airborne_ticks > 20, "a real flight was observed");
}

#[test]
fn spin_is_deterministic_and_only_accumulates_in_flight() {
    let run = || {
        let mut sim = SimState::new(EndZoneConfig::default());
        let mut spins = Vec::new();
        for t in 0..420u64 {
            let commands: &[SimCommand] = match t {
                0 => &[SimCommand::BeginPlay],
                80 => &[SimCommand::Snap],
                170 => &[SimCommand::ThrowNow],
                _ => &[],
            };
            sim.step(commands);
            spins.push(sim.ball.spin_angle.to_bits());
        }
        spins
    };
    let a = run();
    assert_eq!(a, run(), "spiral is bit-identical across runs");
    let distinct: std::collections::BTreeSet<u32> = a.into_iter().collect();
    assert!(
        distinct.len() > 20,
        "the spiral genuinely accumulates in flight"
    );
}

#[test]
fn catch_evaluation_succeeds_and_fails_in_known_cases() {
    let archetype = receiver();
    let ground = Vec3::new(3.0, 0.0, 10.0);
    let chest = Vec3::new(3.0, 1.45, 10.0);
    // In the volume, on time, able to act: caught.
    assert_eq!(
        evaluate_catch(chest, ground, &archetype, 100, 100, true),
        CatchVerdict::Caught
    );
    // In the volume but far outside the timing window: not caught.
    assert_eq!(
        evaluate_catch(chest, ground, &archetype, 100, 100 + 60, true),
        CatchVerdict::BadTiming
    );
    // Outside the catch volume: out of reach.
    let far = chest.add(Vec3::new(archetype.catch_radius + 0.5, 0.0, 0.0));
    assert_eq!(
        evaluate_catch(far, ground, &archetype, 100, 100, true),
        CatchVerdict::OutOfReach
    );
    // A downed player cannot catch.
    assert_eq!(
        evaluate_catch(chest, ground, &archetype, 100, 100, false),
        CatchVerdict::OutOfReach
    );
}

#[test]
fn possession_transitions_are_ordered_correctly() {
    let mut sim = SimState::new(EndZoneConfig::default());
    let events = scripted(&mut sim, 500);
    let order = |pick: &dyn Fn(&SimEvent) -> bool| {
        events
            .iter()
            .position(|e| pick(&e.event))
            .expect("event occurs")
    };
    let snap = order(&|e| matches!(e, SimEvent::Snap { .. }));
    let to_qb = order(&|e| {
        matches!(
            e,
            SimEvent::PossessionChanged {
                from: None,
                to: Some(_)
            }
        )
    });
    let throw = order(&|e| matches!(e, SimEvent::Throw { .. }));
    let released = order(&|e| {
        matches!(
            e,
            SimEvent::PossessionChanged {
                from: Some(_),
                to: None
            }
        )
    });
    let caught = order(&|e| matches!(e, SimEvent::CatchCompleted { .. }));
    assert!(snap < to_qb, "snap precedes quarterback possession");
    assert!(to_qb < throw, "possession precedes the throw");
    assert!(
        throw < released,
        "the throw event precedes its possession release"
    );
    assert!(released < caught, "release precedes the catch");
    // The catch attempt is announced no later than the completion.
    let attempt = order(&|e| matches!(e, SimEvent::CatchAttempt { .. }));
    assert!(attempt <= caught);
    // And the transfer to the receiver follows the completion.
    let transfer = events
        .iter()
        .enumerate()
        .rev()
        .find(|(_, e)| {
            matches!(
                e.event,
                SimEvent::PossessionChanged {
                    from: None,
                    to: Some(_)
                }
            )
        })
        .map(|(index, _)| index)
        .expect("the receiver takes possession");
    assert!(
        caught < transfer,
        "completion is emitted before the transfer"
    );
}

#[test]
fn ground_contact_transitions_the_ball_correctly() {
    // Data-only change: a receiver who cannot catch (zero catch volume) turns
    // the same play into an incompletion — loose ball, grounded ball, play over.
    let mut sim = SimState::new(EndZoneConfig::default());
    // Roster slot 6 of the possession (home) roster is the intended receiver.
    sim.rosters.0.players[6].archetype.catch_radius = 0.0;
    let events = scripted(&mut sim, 700);
    assert!(
        find(&events, |e| matches!(e, SimEvent::CatchCompleted { .. })).is_none(),
        "no catch with a zero catch volume"
    );
    let loose = find(&events, |e| matches!(e, SimEvent::BallLoose { .. }));
    let grounded = find(&events, |e| matches!(e, SimEvent::BallGrounded { .. }));
    assert!(loose.is_some(), "the miss goes through the loose state");
    assert!(grounded.is_some(), "the loose ball settles");
    assert!(loose.unwrap().tick <= grounded.unwrap().tick);
    let ended = find(&events, |e| {
        matches!(
            e,
            SimEvent::PlayEnded {
                reason: PlayEndReason::Incomplete
            }
        )
    });
    assert!(
        ended.is_some(),
        "an uncaught pass ends the play as incomplete"
    );
    assert!(matches!(sim.ball.state, BallState::Grounded));
}

#[test]
fn the_ball_hold_depends_on_role_and_field_position() {
    use axiom_end_zone::player::animation::{ball_hold, BallHold};
    use axiom_end_zone::player::AnimState;

    // A quarterback holding the ball behind the line is throw-ready — standing in
    // the pocket or dropping back.
    assert_eq!(
        ball_hold(true, true, false, AnimState::Idle),
        BallHold::ThrowReady
    );
    assert_eq!(
        ball_hold(true, true, false, AnimState::DropBack),
        BallHold::ThrowReady
    );
    // Once past the line he scrambles with a cradle.
    assert_eq!(
        ball_hold(true, true, true, AnimState::Sprint),
        BallHold::Cradle
    );
    // Any non-quarterback carrier cradles, wherever they are.
    assert_eq!(
        ball_hold(true, false, true, AnimState::Sprint),
        BallHold::Cradle
    );
    assert_eq!(
        ball_hold(true, false, false, AnimState::Jog),
        BallHold::Cradle
    );
    // Not carrying, or in a self-posing anim (throw / catch / down), gets no hold.
    assert_eq!(
        ball_hold(false, true, false, AnimState::Idle),
        BallHold::None
    );
    assert_eq!(
        ball_hold(true, true, false, AnimState::Throw),
        BallHold::None
    );
    assert_eq!(
        ball_hold(true, false, true, AnimState::AirborneFall),
        BallHold::None
    );
}
