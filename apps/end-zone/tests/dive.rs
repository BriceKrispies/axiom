//! Diving-tackle proofs: a close, fast chaser leaves their feet at an escaping
//! carrier, a committed dive lands a tackle from extended reach, and a whiffed
//! dive lands the diver prone. Deterministic, tick-driven — no wall clock.

use axiom::prelude::Vec3;
use axiom_end_zone::ai::PlayerIntent;
use axiom_end_zone::collision_rig::CollisionRig;
use axiom_end_zone::data::player::{defender, receiver};
use axiom_end_zone::data::BehaviorTuning;
use axiom_end_zone::identity::{PlayerId, TeamId};
use axiom_end_zone::player::{contact, AnimState, PlayerSim};

const DT: f32 = 1.0 / 60.0;

/// A carrier (id 0) and a lone defender (id 1), placed `gap` yards apart along
/// `+Z`, each with the given velocity.
fn matchup(gap: f32, carrier_vel: Vec3, defender_vel: Vec3) -> [PlayerSim; 2] {
    let mut carrier = PlayerSim::at(
        PlayerId(0),
        TeamId(0),
        10,
        receiver(),
        Vec3::new(0.0, 0.0, gap),
        0.0,
    );
    carrier.vel = carrier_vel;
    carrier.set_anim(AnimState::Sprint);
    let mut chaser = PlayerSim::at(
        PlayerId(1),
        TeamId(1),
        20,
        defender(),
        Vec3::ZERO,
        0.0,
    );
    chaser.vel = defender_vel;
    chaser.set_anim(AnimState::Sprint);
    [carrier, chaser]
}

#[test]
fn a_close_fast_chaser_leaves_their_feet() {
    let tuning = BehaviorTuning::default();
    // Gap of 2.6 yd: past standing tackle range (1.3), inside the dive window
    // (1.3 * 2.4). Carrier escaping fast, chaser closing hard.
    let mut players = matchup(2.6, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 10.0));
    let intents = [
        PlayerIntent::Carry {
            point: Vec3::new(0.0, 0.0, 40.0),
        },
        PlayerIntent::Tackle {
            target: PlayerId(0),
            point: Vec3::new(0.0, 0.0, 2.6),
        },
    ];
    contact::commit_dives(&mut players, &intents, Some(PlayerId(0)), &tuning);
    assert_eq!(players[1].anim, AnimState::Dive, "the chaser dove");
    assert!(
        players[1].vertical_vel > 0.0,
        "a dive launches upward: {}",
        players[1].vertical_vel
    );
    assert!(
        players[1].vel.z > 0.0,
        "a dive drives forward at the carrier"
    );
}

#[test]
fn a_stationary_target_is_not_dived_at() {
    let tuning = BehaviorTuning::default();
    // Same geometry, but the carrier is standing still — run them down, no dive.
    let mut players = matchup(2.6, Vec3::ZERO, Vec3::new(0.0, 0.0, 10.0));
    let intents = [
        PlayerIntent::Carry {
            point: Vec3::new(0.0, 0.0, 40.0),
        },
        PlayerIntent::Tackle {
            target: PlayerId(0),
            point: Vec3::new(0.0, 0.0, 2.6),
        },
    ];
    contact::commit_dives(&mut players, &intents, Some(PlayerId(0)), &tuning);
    assert_eq!(
        players[1].anim,
        AnimState::Sprint,
        "no dive at a target you can just run down"
    );
}

#[test]
fn a_committed_dive_lands_a_tackle_on_real_body_contact() {
    let tuning = BehaviorTuning::default();
    // The diver's body overlaps the carrier's (0.9 yd < the ~1.08-yd sum of body
    // radii): the shared physics collision world reports the touch, so the dive
    // lands. The tackle is gated on that actual contact, not a distance guess.
    let mut players = matchup(0.9, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 9.5));
    players[1].set_anim(AnimState::Dive);
    // The diver's intent has already lapsed — the dive itself is the commit.
    let intents = [PlayerIntent::Hold, PlayerIntent::Recover];
    // Step the shared collision world so the diver/carrier contact is recorded,
    // exactly as the simulation does before resolving contacts.
    let mut collision = CollisionRig::new(&players);
    collision.resolve(&mut players, 0);
    let outcome =
        contact::resolve_tackle(&mut players, &intents, Some(PlayerId(0)), &tuning, &collision)
            .expect("the dive landed on real body contact");
    assert_eq!(outcome.tackler, PlayerId(1));
    assert_eq!(outcome.target, PlayerId(0));
    assert!(
        !players[0].anim.can_act(),
        "the carrier was knocked down: {:?}",
        players[0].anim
    );
    assert_eq!(
        players[1].anim,
        AnimState::GroundImpact,
        "the diver wrapped and landed prone"
    );
}

#[test]
fn a_dive_that_never_reaches_the_body_does_not_land() {
    // The regression guard for the phantom dive tackle: at 2.0 yd the diver is
    // well beyond body contact (~1.08 yd). The OLD rule landed it anyway (reach =
    // tackle_range * dive_reach ≈ 2.21). Gated on real physics contact, it misses.
    let tuning = BehaviorTuning::default();
    let mut players = matchup(2.0, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 9.5));
    players[1].set_anim(AnimState::Dive);
    let intents = [PlayerIntent::Hold, PlayerIntent::Recover];
    let mut collision = CollisionRig::new(&players);
    collision.resolve(&mut players, 0);
    assert!(
        contact::resolve_tackle(&mut players, &intents, Some(PlayerId(0)), &tuning, &collision)
            .is_none(),
        "a dive whose body never reaches the carrier registers no tackle"
    );
}

#[test]
fn a_standing_tackle_cannot_land_from_beyond_its_range() {
    let tuning = BehaviorTuning::default();
    // A standing tackle at 2.0 yd is out of range (tackle_range ≈ 1.3).
    let mut players = matchup(2.0, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 9.5));
    let intents = [
        PlayerIntent::Carry {
            point: Vec3::new(0.0, 0.0, 40.0),
        },
        PlayerIntent::Tackle {
            target: PlayerId(0),
            point: Vec3::new(0.0, 0.0, 2.0),
        },
    ];
    let collision = CollisionRig::new(&players);
    assert!(
        contact::resolve_tackle(&mut players, &intents, Some(PlayerId(0)), &tuning, &collision)
            .is_none(),
        "a standing tackle cannot land from 2.0 yd"
    );
}

#[test]
fn a_whiffed_dive_lands_the_diver_prone() {
    let tuning = BehaviorTuning::default();
    // A diver launched at empty air: advance the ballistic arc to the turf.
    let mut players = matchup(30.0, Vec3::ZERO, Vec3::ZERO);
    players[1].vel = Vec3::new(0.0, 0.0, tuning.dive_launch_forward);
    players[1].vertical_vel = tuning.dive_launch_up;
    players[1].impact_strength = tuning.dive_whiff_impact;
    players[1].set_anim(AnimState::Dive);

    let mut impacts = Vec::new();
    for _ in 0..120 {
        impacts = contact::advance_falls(&mut players, &tuning, DT);
        if players[1].anim != AnimState::Dive {
            break;
        }
    }
    assert_eq!(
        players[1].anim,
        AnimState::GroundImpact,
        "the whiffed dive hit the turf"
    );
    assert!(
        impacts.iter().any(|(id, _)| *id == PlayerId(1)),
        "the landing registered an impact (dust)"
    );
}
