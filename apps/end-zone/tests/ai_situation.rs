//! AI foundation proofs — the derived ball situation, decision determinism, and
//! commitment locking (spec scenarios 12, 13, plus the situation machine).

use axiom::prelude::{Vec2, Vec3};
use axiom_end_zone::ai::action::{Priority, ScoredAction};
use axiom_end_zone::ai::commitment::{arbitrate, Commitment};
use axiom_end_zone::ai::PlayerIntent;
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::football::BallSituation;
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::state::{SimCommand, SimState};

/// A live sim with the quarterback holding the ball a few ticks in.
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

#[test]
fn a_fresh_play_reads_as_pre_snap() {
    let mut sim = SimState::new(EndZoneConfig::default());
    sim.step(&[SimCommand::BeginPlay]);
    assert_eq!(sim.ball_situation(), BallSituation::PreSnap);
}

#[test]
fn a_quarterback_holding_in_the_pocket_reads_as_held_by_qb() {
    let mut sim = snapped();
    sim.step(&[]);
    assert_eq!(sim.ball_situation(), BallSituation::HeldByQb);
}

#[test]
fn a_committed_scrambling_quarterback_registers_as_a_scramble() {
    let mut sim = snapped();
    // Steer the quarterback straight downfield out of the pocket.
    sim.user_stick = Vec2::new(0.0, 1.0);
    let mut scrambled = false;
    for _ in 0..160 {
        sim.step(&[]);
        if sim.ball_situation() == BallSituation::QbScramble {
            scrambled = true;
            break;
        }
    }
    assert!(scrambled, "a QB run out of the pocket becomes a scramble");
}

#[test]
fn identical_inputs_produce_identical_ai_decisions() {
    let run = || {
        let mut sim = snapped();
        sim.user_stick = Vec2::new(0.3, 0.9);
        let mut intents = Vec::new();
        for _ in 0..150 {
            sim.step(&[]);
            intents.push(sim.intents.clone());
        }
        intents
    };
    assert_eq!(run(), run(), "the AI decision stream replays exactly");
}

#[test]
fn commitment_locking_holds_an_action_through_its_window() {
    let target = PlayerId(7);
    let a = ScoredAction::new(
        PlayerIntent::Pursue { target, point: Vec3::ZERO },
        Priority::BallThreat,
        0.6,
        "a",
        10,
    );
    let mut slot: Option<Commitment> = None;
    // Commit to `a`.
    let first = arbitrate(&[a], &mut slot, 0, false);
    assert!(matches!(first, PlayerIntent::Pursue { .. }));

    // A markedly better SAME-BAND but different action appears within the lock.
    let b = ScoredAction::new(
        PlayerIntent::MoveToward { point: Vec3::ZERO, sprint: true },
        Priority::BallThreat,
        1.0,
        "b",
        10,
    );
    let a_still = ScoredAction::new(
        PlayerIntent::Pursue { target, point: Vec3::new(1.0, 0.0, 0.0) },
        Priority::BallThreat,
        0.6,
        "a",
        10,
    );
    let held = arbitrate(&[b, a_still], &mut slot, 3, false);
    assert!(
        matches!(held, PlayerIntent::Pursue { .. }),
        "within the min window the commitment holds (no thrashing)"
    );

    // After the window, the markedly better action wins.
    let switched = arbitrate(&[b, a_still], &mut slot, 20, false);
    assert!(
        matches!(switched, PlayerIntent::MoveToward { .. }),
        "past the window a meaningfully better action preempts"
    );
}

#[test]
fn a_ball_threat_emergency_preempts_a_lower_band_commitment_immediately() {
    let a = ScoredAction::new(
        PlayerIntent::MoveToward { point: Vec3::ZERO, sprint: false },
        Priority::Assignment,
        0.9,
        "assignment",
        30,
    );
    let mut slot: Option<Commitment> = None;
    arbitrate(&[a], &mut slot, 0, false);
    // A higher-band ball threat appears one tick later, well inside the window.
    let emergency = ScoredAction::new(
        PlayerIntent::Tackle { target: PlayerId(6), point: Vec3::ZERO },
        Priority::BallThreat,
        0.5,
        "threat",
        3,
    );
    let a_still = ScoredAction::new(
        PlayerIntent::MoveToward { point: Vec3::ZERO, sprint: false },
        Priority::Assignment,
        0.9,
        "assignment",
        30,
    );
    let picked = arbitrate(&[a_still, emergency], &mut slot, 1, false);
    assert!(
        matches!(picked, PlayerIntent::Tackle { .. }),
        "a football emergency overrides the commitment window"
    );
}
