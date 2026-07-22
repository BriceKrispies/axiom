//! Defensive-overseer proofs driven through the real simulation — situation
//! transitions, coordinated responsibilities, the emergency override, hysteresis,
//! and the readable tradeoff (spec scenarios 4, 5, 6, 10, 11, 12, 13, 14, 18, 20).

use axiom::prelude::Vec2;
use axiom_end_zone::ai::directive::ExposedRegion;
use axiom_end_zone::ai::{Responsibility, TacticalMode};
use axiom_end_zone::config::{EndZoneConfig, PLAYER_COUNT};
use axiom_end_zone::field::GOAL_LINE_Z;
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::state::{SimCommand, SimState};

/// A live sim at a dropback (quarterback holding, a few ticks in).
fn dropback() -> SimState {
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

fn defenders(sim: &SimState) -> Vec<PlayerId> {
    let offense = sim.play.possession;
    (0..PLAYER_COUNT)
        .map(|i| sim.players[i].id)
        .filter(|id| sim.players[id.index()].team != offense)
        .collect()
}

#[test]
fn a_throw_collapses_on_the_catch_point_then_a_catch_swarms() {
    let mut sim = dropback();
    for _ in 0..120 {
        sim.step(&[]);
        if !sim.throwable.is_empty() {
            break;
        }
    }
    sim.step(&[SimCommand::ThrowNow]);
    let mut saw_catch_point = false;
    let mut saw_swarm = false;
    for _ in 0..200 {
        sim.step(&[]);
        match sim.directive().mode {
            TacticalMode::CatchPointCollapse => saw_catch_point = true,
            TacticalMode::SwarmAndContain => saw_swarm = true,
            _ => {}
        }
    }
    assert!(saw_catch_point, "a thrown ball forces the catch-point collapse");
    assert!(saw_swarm, "the completed catch turns into swarm-and-contain");
}

#[test]
fn the_catch_point_collapse_assigns_ball_responsibilities() {
    let mut sim = dropback();
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
    assert_eq!(sim.directive().mode, TacticalMode::CatchPointCollapse);
    let has = |r: Responsibility| defenders(&sim).iter().any(|d| sim.responsibility(*d) == r);
    assert!(
        has(Responsibility::Intercept) || has(Responsibility::ContestCatch),
        "someone plays the ball at the catch point"
    );
    assert!(
        has(Responsibility::TackleAngle),
        "someone sets a post-catch tackle angle"
    );
}

#[test]
fn a_rollout_draws_contain_then_a_run_response_on_commitment() {
    let mut sim = dropback();
    // Roll the quarterback toward the right edge, then let him break downfield.
    sim.user_stick = Vec2::new(0.85, 0.55);
    let mut contain_at = None;
    let mut run_at = None;
    for t in 0..220u64 {
        sim.step(&[]);
        match sim.directive().mode {
            TacticalMode::ContainQb if contain_at.is_none() => contain_at = Some(t),
            TacticalMode::QbRunResponse if run_at.is_none() => run_at = Some(t),
            _ => {}
        }
    }
    let contain = contain_at.expect("a rollout draws a contain call");
    let run = run_at.expect("a committed run draws the run response");
    assert!(contain < run, "contain precedes the run response ({contain} < {run})");
}

#[test]
fn the_run_response_assigns_distinct_pursuit_roles() {
    let mut sim = dropback();
    sim.user_stick = Vec2::new(0.0, 1.0);
    for _ in 0..200 {
        sim.step(&[]);
        if sim.directive().mode == TacticalMode::QbRunResponse {
            break;
        }
    }
    assert_eq!(sim.directive().mode, TacticalMode::QbRunResponse);
    sim.step(&[]);
    let count = |r: Responsibility| {
        defenders(&sim)
            .iter()
            .filter(|d| sim.responsibility(**d) == r)
            .count()
    };
    assert_eq!(count(Responsibility::PrimaryTackler), 1, "one primary tackler");
    assert!(count(Responsibility::OutsideContain) >= 1, "an outside contain");
    assert!(count(Responsibility::Cutback) >= 1, "a cutback defender");
    assert!(count(Responsibility::DeepHelp) >= 1, "deep insurance");
}

#[test]
fn an_imminent_touchdown_overrides_ordinary_commitment() {
    let mut sim = dropback();
    // Park the quarterback (with the ball) just short of the goal, out of the
    // pocket — a touchdown is one step away.
    let qb = sim.quarterback;
    let goal_z = GOAL_LINE_Z * sim.frame.direction.sign();
    sim.players[qb.index()].pos.z = goal_z - 5.0 * sim.frame.direction.sign();
    sim.players[qb.index()].pos.x = 3.0;
    sim.user_stick = Vec2::new(1.0, 0.0); // hold him near the goal laterally
    let mut emergency = false;
    for _ in 0..8 {
        sim.step(&[]);
        if sim.directive().mode == TacticalMode::EmergencyTouchdown {
            emergency = true;
            break;
        }
    }
    assert!(emergency, "a ball at the doorstep forces emergency defense");
}

#[test]
fn the_overseer_does_not_thrash_between_modes() {
    let mut sim = dropback();
    let mut changes = 0;
    let mut mode = sim.directive().mode;
    for _ in 0..180 {
        sim.step(&[]);
        let m = sim.directive().mode;
        if m != mode {
            changes += 1;
            mode = m;
        }
    }
    assert!(changes <= 5, "commitment locking keeps calls legible ({changes} changes)");
}

#[test]
fn an_adjustment_exposes_a_readable_region() {
    // A long hold behind a stable pocket draws pressure, which gives up the
    // underneath middle — a real, inspectable tradeoff.
    let mut sim = dropback();
    let mut exposed = ExposedRegion::None;
    for _ in 0..150 {
        sim.step(&[]);
        let d = sim.directive();
        if d.mode == TacticalMode::IncreasePressure {
            exposed = d.exposed;
            break;
        }
    }
    assert_eq!(
        exposed,
        ExposedRegion::UnderneathMiddle,
        "the pressure call exposes the underneath middle"
    );
}

#[test]
fn a_bracket_holds_its_target_within_the_commitment_window() {
    let mut sim = dropback();
    // Find a tick where the overseer is bracketing.
    let mut target = None;
    for _ in 0..90 {
        sim.step(&[]);
        if sim.directive().mode == TacticalMode::BracketReceiver {
            target = sim.directive().primary_threat;
            break;
        }
    }
    if let Some(t) = target {
        // The bracket target does not flip on minor score wobble across the next
        // several evaluations.
        for _ in 0..10 {
            sim.step(&[]);
            if sim.directive().mode == TacticalMode::BracketReceiver {
                assert_eq!(
                    sim.directive().primary_threat,
                    Some(t),
                    "the bracket does not switch receivers within its window"
                );
            }
        }
    }
}
