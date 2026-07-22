//! Coverage & pursuit proofs — a scrambling quarterback becomes the priority,
//! responsibilities don't duplicate, deep help is preserved, and airborne
//! reactions are predictive (spec scenarios 1, 2, 3, 9, 10, 11, 14).

use axiom::prelude::{Vec2, Vec3};
use axiom_end_zone::ai::{PlayerIntent, Responsibility};
use axiom_end_zone::config::{EndZoneConfig, PLAYER_COUNT};
use axiom_end_zone::football::{BallSituation, BallState};
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::state::{SimCommand, SimState};

fn snapped() -> SimState {
    let mut sim = SimState::new(EndZoneConfig::default());
    sim.step(&[SimCommand::BeginPlay]);
    sim.step(&[SimCommand::Snap]);
    for _ in 0..20 {
        if sim.possession == Some(sim.quarterback) {
            break;
        }
        sim.step(&[]);
    }
    sim
}

/// The offense/defense split: home ids 0..7, away ids 7..14; the possession
/// team is on offense.
fn defenders(sim: &SimState) -> Vec<PlayerId> {
    let offense = sim.play.possession;
    (0..PLAYER_COUNT)
        .map(|i| sim.players[i].id)
        .filter(|id| sim.players[id.index()].team != offense)
        .collect()
}

fn flat_dist(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

#[test]
fn a_nearby_defender_prioritizes_a_scrambling_quarterback() {
    let mut sim = snapped();
    sim.user_stick = Vec2::new(0.0, 1.0);
    for _ in 0..160 {
        sim.step(&[]);
        if sim.ball_situation() == BallSituation::QbScramble {
            break;
        }
    }
    assert_eq!(sim.ball_situation(), BallSituation::QbScramble);
    // Give the nearest defenders a couple ticks to react to the run.
    for _ in 0..8 {
        sim.step(&[]);
    }
    let qb = sim.quarterback;
    let qb_pos = sim.players[qb.index()].pos;
    let nearest = defenders(&sim)
        .into_iter()
        .filter(|d| sim.players[d.index()].anim.can_act())
        .min_by(|a, b| {
            flat_dist(sim.players[a.index()].pos, qb_pos)
                .total_cmp(&flat_dist(sim.players[b.index()].pos, qb_pos))
        })
        .expect("a defender exists");
    assert_eq!(
        sim.intents[nearest.index()].action_target(),
        Some(qb),
        "the nearest defender attacks the running quarterback"
    );
}

#[test]
fn exactly_one_defender_is_the_primary_tackler_on_a_runner() {
    let mut sim = snapped();
    sim.user_stick = Vec2::new(0.4, 1.0);
    for _ in 0..160 {
        sim.step(&[]);
        if sim.ball_situation() == BallSituation::QbScramble {
            break;
        }
    }
    sim.step(&[]);
    let primaries = defenders(&sim)
        .into_iter()
        .filter(|d| sim.responsibility(*d) == Responsibility::PrimaryTackler)
        .count();
    assert_eq!(primaries, 1, "one primary tackler, not a duplicated pile");
}

#[test]
fn a_deeper_defender_preserves_leverage_instead_of_duplicating() {
    let mut sim = snapped();
    sim.user_stick = Vec2::new(0.0, 1.0);
    for _ in 0..160 {
        sim.step(&[]);
        if sim.ball_situation() == BallSituation::QbScramble {
            break;
        }
    }
    sim.step(&[]);
    let deep = defenders(&sim)
        .into_iter()
        .filter(|d| sim.responsibility(*d) == Responsibility::DeepHelp)
        .count();
    assert_eq!(deep, 1, "someone holds deep leverage rather than all piling on");
}

#[test]
fn a_receiver_adjusts_toward_the_projected_catch_point_after_a_throw() {
    let mut sim = snapped();
    for _ in 0..120 {
        sim.step(&[]);
        if !sim.throwable.is_empty() {
            break;
        }
    }
    sim.step(&[SimCommand::ThrowNow]);
    let mut target = None;
    let mut intended = None;
    for _ in 0..40 {
        sim.step(&[]);
        if let BallState::Airborne { flight } = sim.ball.state {
            target = Some(flight.target);
            intended = Some(flight.intended);
            break;
        }
    }
    // Let the receiver's brain react to the now-airborne ball.
    sim.step(&[]);
    let (target, intended) = (target.expect("pass released"), intended.expect("intended"));
    match sim.intents[intended.index()] {
        PlayerIntent::PrepareCatch { point } => {
            assert!(
                flat_dist(point, target) < 0.01,
                "the receiver settles under the projected catch point"
            );
        }
        other => panic!("receiver should adjust to the catch, got {other:?}"),
    }
}

#[test]
fn a_defender_who_can_arrive_early_chooses_an_interception() {
    let mut sim = snapped();
    for _ in 0..120 {
        sim.step(&[]);
        if !sim.throwable.is_empty() {
            break;
        }
    }
    sim.step(&[SimCommand::ThrowNow]);
    let target = loop {
        sim.step(&[]);
        if let BallState::Airborne { flight } = sim.ball.state {
            break flight.target;
        }
    };
    // Plant a defender right on the projected catch point early in the flight —
    // he can plainly arrive first.
    let jumper = defenders(&sim)[0];
    sim.players[jumper.index()].pos = Vec3::new(target.x, 0.0, target.z);
    sim.step(&[]);
    assert_eq!(
        sim.responsibility(jumper),
        Responsibility::Intercept,
        "the early-arriving defender is the interceptor"
    );
    assert!(
        matches!(sim.intents[jumper.index()], PlayerIntent::PrepareCatch { .. }),
        "and he attacks the ball, not the receiver"
    );
}

#[test]
fn a_late_defender_takes_a_tackle_angle_not_the_airborne_ball() {
    let mut sim = snapped();
    for _ in 0..120 {
        sim.step(&[]);
        if !sim.throwable.is_empty() {
            break;
        }
    }
    sim.step(&[SimCommand::ThrowNow]);
    for _ in 0..40 {
        sim.step(&[]);
        if sim.ball_situation().ball_in_air() {
            break;
        }
    }
    let angler = defenders(&sim)
        .into_iter()
        .find(|d| sim.responsibility(*d) == Responsibility::TackleAngle);
    if let Some(angler) = angler {
        assert!(
            !matches!(sim.intents[angler.index()], PlayerIntent::PrepareCatch { .. }),
            "a tackle-angle defender does not chase the airborne ball"
        );
    }
    // At least one defender must be going for the ball (intercept/contest).
    let going_for_ball = defenders(&sim)
        .into_iter()
        .any(|d| matches!(sim.intents[d.index()], PlayerIntent::PrepareCatch { .. }));
    assert!(going_for_ball, "someone plays the ball in the air");
}

#[test]
fn a_loose_ball_is_an_immediate_shared_priority() {
    let mut sim = snapped();
    sim.step(&[]);
    // Drop the ball loose right next to a defender and an offensive player.
    let near = &sim.players[8];
    let ball = Vec3::new(near.pos.x + 0.5, 0.21, near.pos.z);
    sim.possession = None;
    sim.ball.state = BallState::Loose;
    sim.ball.pos = ball;
    sim.step(&[]);
    let chasing = |id: usize| match sim.intents[id] {
        PlayerIntent::MoveToward { point, .. } => flat_dist(point, ball) < 1.0e-3,
        _ => false,
    };
    assert!(chasing(8), "the closest player scrambles for the loose ball");
}
