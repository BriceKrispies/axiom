//! Player-vs-player collision proofs. These lock the three properties that make
//! [`CollisionRig`] a real replacement for the old positional `resolve_overlaps`
//! rather than a silent no-op:
//!
//! 1. overlapping opponents both separate AND trade closing momentum (the new
//!    rigid-body capability — the old pass only moved positions);
//! 2. a player with no one in contact is left effectively untouched (the
//!    isolation property that keeps the scripted showcase deterministic and
//!    stable — nothing nudges a free receiver off his route);
//! 3. a downed player is parked out of the world and never shoves a standing
//!    one (matching the old "both must be able to act" gate).

use axiom::prelude::Vec3;
use axiom_end_zone::collision_rig::CollisionRig;
use axiom_end_zone::data::player::{defender, receiver};
use axiom_end_zone::identity::{PlayerId, TeamId};
use axiom_end_zone::player::{AnimState, PlayerSim};

fn player(index: u8, team: u8, archetype: axiom_end_zone::data::PlayerArchetype, x: f32) -> PlayerSim {
    PlayerSim::at(
        PlayerId(index),
        TeamId(team),
        index,
        archetype,
        Vec3::new(x, 0.0, 0.0),
        0.0,
    )
}

fn gap(players: &[PlayerSim]) -> f32 {
    players[1].pos.x - players[0].pos.x
}

/// Closing speed along X (positive = approaching).
fn closing(players: &[PlayerSim]) -> f32 {
    players[0].vel.x - players[1].vel.x
}

#[test]
fn overlapping_opponents_depenetrate_and_exchange_closing_momentum() {
    let radii = receiver().body_radius + defender().body_radius;
    // Start well inside the combined radius, closing head-on along +/-X.
    let mut players = vec![
        player(0, 0, receiver(), 0.0),
        player(1, 1, defender(), radii * 0.5),
    ];
    players[0].vel = Vec3::new(4.0, 0.0, 0.0);
    players[1].vel = Vec3::new(-4.0, 0.0, 0.0);

    let mut rig = CollisionRig::new(&players);
    let start_gap = gap(&players);
    let start_closing = closing(&players);

    rig.resolve(&mut players, 0);

    assert!(rig.fault.is_none(), "no physics fault: {:?}", rig.fault);
    assert!(
        gap(&players) > start_gap,
        "overlapping bodies pushed apart: {start_gap} -> {}",
        gap(&players)
    );
    assert!(
        closing(&players) < start_closing,
        "head-on closing momentum was exchanged, not preserved as a slide: \
         {start_closing} -> {}",
        closing(&players)
    );
}

#[test]
fn an_untouched_player_is_left_effectively_unchanged() {
    // One lone runner: no contact, so the collision pass must not move or slow
    // him. (The free-flight baseline is subtracted, so only contact survives.)
    let mut players = vec![player(0, 0, receiver(), 0.0)];
    players[0].vel = Vec3::new(0.0, 0.0, 8.0);
    let mut rig = CollisionRig::new(&players);
    let before = players[0];

    rig.resolve(&mut players, 0);

    let moved = players[0].pos.subtract(before.pos).length();
    let slowed = players[0].vel.subtract(before.vel).length();
    assert!(moved < 1.0e-4, "a free runner is not displaced: {moved}");
    assert!(slowed < 1.0e-4, "a free runner is not slowed: {slowed}");
}

#[test]
fn a_downed_player_is_parked_and_never_shoves_a_standing_one() {
    // A standing player overlapping a downed one: the downed body is parked out
    // of the world, so the standing player is not pushed (the old pass skipped
    // any pair where either could not act).
    let radii = receiver().body_radius + defender().body_radius;
    let mut players = vec![
        player(0, 0, receiver(), 0.0),
        player(1, 1, defender(), radii * 0.5),
    ];
    players[1].set_anim(AnimState::GroundImpact);
    assert!(!players[1].anim.can_act(), "the second player is down");

    let mut rig = CollisionRig::new(&players);
    let before = players[0];

    rig.resolve(&mut players, 0);

    let moved = players[0].pos.subtract(before.pos).length();
    assert!(
        moved < 1.0e-4,
        "a downed body does not shove the standing player: {moved}"
    );
}

#[test]
fn in_contact_reports_touching_bodies_and_rejects_distant_ones() {
    let radii = receiver().body_radius + defender().body_radius;
    // Overlapping pair (half the combined radius apart): bodies touch.
    let mut touching = vec![
        player(0, 0, receiver(), 0.0),
        player(1, 1, defender(), radii * 0.5),
    ];
    let mut rig = CollisionRig::new(&touching);
    rig.resolve(&mut touching, 0);
    assert!(
        rig.in_contact(PlayerId(0), PlayerId(1)),
        "overlapping bodies report contact"
    );
    assert!(rig.in_contact(PlayerId(1), PlayerId(0)), "contact is symmetric");

    // Well-separated pair: no contact.
    let mut apart = vec![player(0, 0, receiver(), 0.0), player(1, 1, defender(), 6.0)];
    let mut rig_apart = CollisionRig::new(&apart);
    rig_apart.resolve(&mut apart, 0);
    assert!(
        !rig_apart.in_contact(PlayerId(0), PlayerId(1)),
        "bodies six yards apart report no contact"
    );
}
