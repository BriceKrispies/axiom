//! Defensive-overseer proofs — the tactical scoring reads football evidence, the
//! directive carries no movement command, decisions are deterministic, and
//! possession memory resets (spec scenarios 1, 2, 3, 7, 8, 15, 16, 17, 19).

use axiom::prelude::Vec2;
use axiom_end_zone::ai::field_read::{DefensiveRead, PocketState};
use axiom_end_zone::ai::overseer::PossessionMemory;
use axiom_end_zone::ai::tactics::score;
use axiom_end_zone::ai::TacticalMode;
use axiom_end_zone::config::{EndZoneConfig, PLAYER_COUNT};
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::state::{SimCommand, SimState};

/// A neutral read: a stable pocket, no clear threat.
fn neutral() -> DefensiveRead {
    DefensiveRead {
        pocket_state: PocketState::Stable,
        pressure_distance: 6.0,
        ticks_since_snap: 20,
        qb_rollout: false,
        qb_depth: 5.0,
        most_dangerous: None,
        danger_separation: 2.0,
        deep_threats: 0,
        crossing: false,
        sideline_overload: 0.0,
        touchdown_threat: false,
        first_down_threat: false,
        free_defenders: 5,
    }
}

fn mem() -> PossessionMemory {
    PossessionMemory::new()
}

#[test]
fn base_wins_when_no_threat_is_clear() {
    let r = neutral();
    let base = score(TacticalMode::Base, &r, &mem());
    for m in [
        TacticalMode::IncreasePressure,
        TacticalMode::ContainQb,
        TacticalMode::ProtectDeep,
        TacticalMode::ProtectMiddle,
        TacticalMode::ProtectOutside,
        TacticalMode::BracketReceiver,
    ] {
        assert!(base >= score(m, &r, &mem()), "base beats {m:?} when nothing is clear");
    }
}

#[test]
fn pressure_rises_when_the_quarterback_holds_a_stable_pocket() {
    let mut r = neutral();
    r.ticks_since_snap = 110;
    assert!(
        score(TacticalMode::IncreasePressure, &r, &mem()) > score(TacticalMode::Base, &r, &mem()),
        "a long hold behind a stable pocket invites pressure"
    );
}

#[test]
fn pressure_is_suppressed_when_it_would_expose_a_deep_touchdown() {
    let mut r = neutral();
    r.ticks_since_snap = 110;
    r.deep_threats = 2;
    r.touchdown_threat = true;
    assert!(
        score(TacticalMode::IncreasePressure, &r, &mem()) < score(TacticalMode::Base, &r, &mem()),
        "no blitz into an imminent deep score"
    );
    assert!(
        score(TacticalMode::ProtectDeep, &r, &mem())
            > score(TacticalMode::IncreasePressure, &r, &mem()),
        "deep protection is preferred instead"
    );
}

#[test]
fn multiple_deep_threats_call_for_deep_protection() {
    let mut r = neutral();
    r.deep_threats = 2;
    assert!(score(TacticalMode::ProtectDeep, &r, &mem()) > score(TacticalMode::Base, &r, &mem()));
}

#[test]
fn a_rollout_calls_for_contain_and_crossers_for_the_middle() {
    let mut roll = neutral();
    roll.qb_rollout = true;
    assert!(score(TacticalMode::ContainQb, &roll, &mem()) > score(TacticalMode::Base, &roll, &mem()));

    let mut cross = neutral();
    cross.crossing = true;
    assert!(
        score(TacticalMode::ProtectMiddle, &cross, &mem()) > score(TacticalMode::Base, &cross, &mem())
    );

    let mut wide = neutral();
    wide.sideline_overload = 0.9;
    assert!(
        score(TacticalMode::ProtectOutside, &wide, &mem()) > score(TacticalMode::Base, &wide, &mem())
    );
}

#[test]
fn a_dominant_receiver_invites_a_bracket_only_if_personnel_allows() {
    let mut r = neutral();
    r.most_dangerous = Some(PlayerId(6));
    r.danger_separation = 8.0;
    r.free_defenders = 5;
    assert!(
        score(TacticalMode::BracketReceiver, &r, &mem()) > score(TacticalMode::Base, &r, &mem()),
        "a clearly separated receiver warrants a bracket"
    );
    // Reassignment cost too high: not enough free defenders to double him.
    r.free_defenders = 1;
    assert!(
        score(TacticalMode::BracketReceiver, &r, &mem()) < score(TacticalMode::Base, &r, &mem()),
        "with nobody free the bracket is rejected"
    );
}

#[test]
fn the_directive_carries_no_movement_command_and_defenders_never_teleport() {
    // The overseer issues assignments/emphasis, never a position or velocity —
    // so no defender is ever displaced faster than his own legs allow.
    let mut sim = SimState::new(EndZoneConfig::default());
    sim.step(&[SimCommand::BeginPlay]);
    sim.step(&[SimCommand::Snap]);
    sim.user_stick = Vec2::new(0.3, 0.9);
    let offense = sim.play.possession;
    let mut prev: Vec<_> = sim.players.iter().map(|p| p.pos).collect();
    for _ in 0..200 {
        sim.step(&[]);
        for i in 0..PLAYER_COUNT {
            if sim.players[i].team == offense {
                prev[i] = sim.players[i].pos;
                continue;
            }
            let step = flat_len(sim.players[i].pos, prev[i]);
            let cap = sim.players[i].archetype.max_speed / 60.0 * 3.0 + 0.6;
            assert!(step <= cap, "defender {i} moved {step:.2} (> {cap:.2}) — teleport");
            prev[i] = sim.players[i].pos;
        }
    }
}

fn flat_len(a: axiom::prelude::Vec3, b: axiom::prelude::Vec3) -> f32 {
    axiom::prelude::Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

#[test]
fn identical_histories_produce_identical_directives() {
    let run = || {
        let mut sim = SimState::new(EndZoneConfig::default());
        sim.step(&[SimCommand::BeginPlay]);
        sim.step(&[SimCommand::Snap]);
        let mut modes = Vec::new();
        for t in 0..300u64 {
            let cmds: &[SimCommand] = if t == 160 { &[SimCommand::ThrowNow] } else { &[] };
            sim.step(cmds);
            let d = sim.directive();
            modes.push((d.mode, d.confidence.to_bits(), d.rush_emphasis.to_bits()));
        }
        modes
    };
    assert_eq!(run(), run(), "the overseer's directive stream replays exactly");
}

#[test]
fn possession_memory_resets_at_the_possession_boundary() {
    let mut sim = SimState::new(EndZoneConfig::default());
    sim.step(&[SimCommand::BeginPlay]);
    sim.step(&[SimCommand::Snap]);
    // Make the quarterback scramble to accumulate a tendency.
    sim.user_stick = Vec2::new(0.0, 1.0);
    for _ in 0..160 {
        sim.step(&[]);
        if sim.overseer_memory().scramble_events > 0 {
            break;
        }
    }
    assert!(
        sim.overseer_memory().scramble_events > 0,
        "the scramble was remembered this possession"
    );
    sim.note_new_possession();
    assert_eq!(
        sim.overseer_memory().scramble_events,
        0,
        "the possession boundary clears the tendency memory"
    );
}
