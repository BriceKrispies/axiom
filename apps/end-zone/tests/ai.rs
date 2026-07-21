//! AI proofs: deterministic intents, deterministic route progress, route
//! mirroring across drive direction, configured reaction delay, steering
//! limits, data-driven behavior change, and stable identity ordering.

use axiom::prelude::Vec3;
use axiom_end_zone::ai::assignment::{compile_assignments, offense_player, AssignmentKind};
use axiom_end_zone::ai::{steering, PlayerIntent};
use axiom_end_zone::config::{EndZoneConfig, DT, PLAYER_COUNT};
use axiom_end_zone::data::player::receiver;
use axiom_end_zone::data::showcase_play;
use axiom_end_zone::events::{PlayEndReason, SimEvent};
use axiom_end_zone::field::{DriveDirection, OffenseFrame};
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::showcase::run_trace;
use axiom_end_zone::state::{SimCommand, SimState};

#[test]
fn same_play_seed_and_inputs_produce_identical_intents() {
    let a = run_trace(EndZoneConfig::default(), 500);
    let b = run_trace(EndZoneConfig::default(), 500);
    assert_eq!(
        a.intents, b.intents,
        "every player's intent stream replays exactly"
    );
}

#[test]
fn route_progress_is_deterministic() {
    // The primary receiver's movement targets (their compiled route) replay
    // exactly, waypoint switch ticks included.
    let receiver_id = offense_player(&showcase_play(), 6);
    let targets = |trace: &axiom_end_zone::showcase::ShowcaseTrace| -> Vec<(u32, u32, u32)> {
        trace
            .intents
            .iter()
            .filter_map(|tick| match tick[receiver_id.index()] {
                PlayerIntent::MoveToward { point, .. } => {
                    Some((point.x.to_bits(), point.y.to_bits(), point.z.to_bits()))
                }
                _ => None,
            })
            .collect()
    };
    let a = run_trace(EndZoneConfig::default(), 400);
    let b = run_trace(EndZoneConfig::default(), 400);
    let (ta, tb) = (targets(&a), targets(&b));
    assert!(!ta.is_empty(), "the receiver runs a route");
    assert_eq!(ta, tb);
}

#[test]
fn offense_relative_routes_mirror_when_drive_direction_changes() {
    let play_plus = showcase_play();
    let mut play_minus = showcase_play();
    play_minus.drive_direction = DriveDirection::MinusZ;

    let frame_plus = OffenseFrame::at_yard_line(play_plus.line_of_scrimmage, DriveDirection::PlusZ);
    let frame_minus =
        OffenseFrame::at_yard_line(play_minus.line_of_scrimmage, DriveDirection::MinusZ);
    let plus = compile_assignments(&play_plus, &frame_plus);
    let minus = compile_assignments(&play_minus, &frame_minus);

    let mut routes_checked = 0;
    for index in 0..PLAYER_COUNT {
        if plus[index].route.is_empty() {
            continue;
        }
        routes_checked += 1;
        assert_eq!(plus[index].route.len(), minus[index].route.len());
        for (p, m) in plus[index].route.iter().zip(&minus[index].route) {
            assert!(
                (p.x + m.x).abs() < 1.0e-4,
                "lateral mirrors: {p:?} vs {m:?}"
            );
            assert!(
                (p.z + m.z).abs() < 1.0e-4,
                "downfield mirrors: {p:?} vs {m:?}"
            );
        }
    }
    assert!(routes_checked >= 3, "the play carries real routes");
}

#[test]
fn defenders_obey_their_configured_reaction_delay() {
    // The free safety first emits a pursue/tackle intent on the carrier only
    // after his configured reaction delay past the catch; shortening the
    // delay through DATA makes the same code react sooner.
    let pursue_lag = |delay: u32| -> i64 {
        let mut sim = SimState::new(EndZoneConfig::default());
        let safety = 13usize;
        // Roster data drives behavior: slot 6 of the away roster is the safety.
        sim.rosters.1.players[6].archetype.reaction_delay_ticks = delay;
        let mut catch_tick: Option<u64> = None;
        let mut pursue_tick: Option<u64> = None;
        for t in 0..600u64 {
            let commands: &[SimCommand] = match t {
                0 => &[SimCommand::BeginPlay],
                80 => &[SimCommand::Snap],
                170 => &[SimCommand::ThrowNow],
                _ => &[],
            };
            let events: Vec<_> = sim.step(commands).to_vec();
            if catch_tick.is_none()
                && events
                    .iter()
                    .any(|e| matches!(e.event, SimEvent::CatchCompleted { .. }))
            {
                catch_tick = Some(t);
            }
            if catch_tick.is_some() && pursue_tick.is_none() {
                let intent = sim.intents[safety];
                let on_carrier = intent.action_target() == sim.possession;
                if on_carrier && sim.possession.is_some() {
                    pursue_tick = Some(t);
                }
            }
        }
        let caught = catch_tick.expect("catch happens") as i64;
        let pursued = pursue_tick.expect("the safety eventually pursues") as i64;
        pursued - caught
    };
    let slow = pursue_lag(24);
    let fast = pursue_lag(4);
    assert!(
        slow > fast,
        "a larger configured delay reacts later ({slow} vs {fast})"
    );
    assert!(
        slow - fast >= 12,
        "the lag difference tracks the configured delays"
    );
}

#[test]
fn steering_respects_acceleration_and_turn_rate_limits() {
    let archetype = receiver();
    // Hard 90° turn request at full speed.
    let current = Vec3::new(0.0, 0.0, archetype.max_speed);
    let desired = Vec3::new(archetype.max_speed, 0.0, 0.0);
    let next = steering::limited_velocity_update(current, desired, &archetype, DT);
    let speed_delta = (next.length() - current.length()).abs();
    assert!(
        speed_delta <= archetype.acceleration * DT + 1.0e-4,
        "speed change {speed_delta} bounded by acceleration"
    );
    let dot = (current.dot(next) / (current.length() * next.length())).clamp(-1.0, 1.0);
    let turned = dot.acos();
    assert!(
        turned <= archetype.turn_rate * DT + 1.0e-3,
        "turn {turned} bounded by turn rate"
    );
    // From rest, acceleration is likewise bounded.
    let from_rest = steering::limited_velocity_update(Vec3::ZERO, desired, &archetype, DT);
    assert!(from_rest.length() <= archetype.acceleration * DT + 1.0e-4);
}

#[test]
fn team_data_changes_behavior_without_changing_ai_code() {
    // Same code, one archetype number changed: the play resolves differently
    // (a zero catch volume turns the completion into an incompletion).
    let run = |mutate: bool| {
        let mut sim = SimState::new(EndZoneConfig::default());
        if mutate {
            sim.rosters.0.players[6].archetype.catch_radius = 0.0;
        }
        for t in 0..700u64 {
            let commands: &[SimCommand] = match t {
                0 => &[SimCommand::BeginPlay],
                80 => &[SimCommand::Snap],
                170 => &[SimCommand::ThrowNow],
                _ => &[],
            };
            sim.step(commands);
        }
        sim.end_reason
    };
    // The completed-pass outcome is `BrokeFree` since the quarterback's
    // drop-back was fixed to hold its facing downfield: he now backpedals
    // instead of running away from the play, which moves the release point and
    // so where the catch happens. The point of this test is unchanged — one
    // archetype number, flipped, changes how the play resolves.
    assert_eq!(run(false), Some(PlayEndReason::BrokeFree));
    assert_eq!(run(true), Some(PlayEndReason::Incomplete));
}

#[test]
fn stable_identity_ordering_produces_stable_resolution() {
    let trace = run_trace(EndZoneConfig::default(), 300);
    // Intents are resolved and recorded in ascending PlayerId order, and the
    // sim's player array is indexed by id at every tick.
    let mut sim = SimState::new(EndZoneConfig::default());
    for _ in 0..120 {
        sim.step(&[]);
        for (index, player) in sim.players.iter().enumerate() {
            assert_eq!(player.id, PlayerId(index as u8));
        }
        for (index, assignment) in sim.assignments.iter().enumerate() {
            // Offense ids fill 0..7 with offensive kinds in this play.
            let offensive = matches!(
                assignment.kind,
                AssignmentKind::Quarterback { .. }
                    | AssignmentKind::Snapper
                    | AssignmentKind::Route { .. }
                    | AssignmentKind::PassBlock
                    | AssignmentKind::LeadBlock
                    | AssignmentKind::BallCarry
            );
            assert_eq!(offensive, index < 7, "assignment {index} matches its side");
        }
    }
    assert_eq!(trace.intents[0].len(), PLAYER_COUNT);
}
