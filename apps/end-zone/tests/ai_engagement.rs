//! Line-engagement proofs — an offensive lineman squares and anchors, the block
//! does not oscillate, the defensive-line advantage builds and eventually sheds,
//! and a strong blocker delays the shed (spec scenarios 4, 5, 6, 7, 8).

use axiom::prelude::Vec3;
use axiom_end_zone::ai::engagement::{Engagement, EngagementState};
use axiom_end_zone::ai::PlayerIntent;
use axiom_end_zone::config::{EndZoneConfig, PLAYER_COUNT};
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::state::{PlayPhase, SimCommand, SimState};

/// A live sim `extra` ticks past the snap (the rush has reached the line).
fn engaged(extra: u32) -> SimState {
    let mut sim = SimState::new(EndZoneConfig::default());
    sim.step(&[SimCommand::BeginPlay]);
    sim.step(&[SimCommand::Snap]);
    for _ in 0..(7 + extra) {
        sim.step(&[]);
    }
    sim
}

/// The first blocker (id order) who is holding a `Block` intent and has a live
/// engagement, with his engagement.
fn first_block(sim: &SimState) -> Option<(PlayerId, Engagement)> {
    (0..PLAYER_COUNT)
        .map(|i| sim.players[i].id)
        .find(|id| matches!(sim.intents[id.index()], PlayerIntent::Block { .. }))
        .and_then(|id| sim.engagement(id).map(|e| (id, e)))
}

/// The blocker with the highest live engagement advantage (the contest that has
/// actually made contact, not a lineman still closing).
fn best_engaged(sim: &SimState) -> Option<(PlayerId, Engagement)> {
    (0..PLAYER_COUNT)
        .map(|i| sim.players[i].id)
        .filter_map(|id| sim.engagement(id).map(|e| (id, e)))
        .filter(|(_, e)| !matches!(e.state, EngagementState::Idle | EngagementState::Square))
        .max_by(|a, b| a.1.advantage.total_cmp(&b.1.advantage))
}

fn dir(from: Vec3, to: Vec3) -> Vec3 {
    Vec3::new(to.x - from.x, 0.0, to.z - from.z)
        .normalize()
        .unwrap_or(Vec3::UNIT_Z)
}

#[test]
fn an_offensive_lineman_squares_toward_the_rusher_and_protects_the_pocket() {
    let mut sim = engaged(14);
    let (blocker, engagement) = first_block(&sim).expect("a block forms");
    // Let the blocker's facing settle onto the rusher.
    for _ in 0..6 {
        sim.step(&[]);
    }
    let bpos = sim.players[blocker.index()].pos;
    let rusher = engagement.rusher;
    let rpos = sim.players[rusher.index()].pos;
    let facing = sim.players[blocker.index()].facing_dir();
    let toward = dir(bpos, rpos);
    assert!(
        facing.dot(toward) > 0.2,
        "the blocker squares his body toward the rusher"
    );
    // He walls the pocket: he is no further from the ball than the rusher is.
    let qb = sim.players[sim.quarterback.index()].pos;
    let block_gap = Vec3::new(bpos.x - qb.x, 0.0, bpos.z - qb.z).length();
    let rush_gap = Vec3::new(rpos.x - qb.x, 0.0, rpos.z - qb.z).length();
    assert!(
        block_gap <= rush_gap + 0.6,
        "the blocker keeps himself between the rusher and the quarterback"
    );
}

#[test]
fn a_block_engagement_does_not_oscillate_around_the_defender() {
    let mut sim = engaged(14);
    let (blocker, engagement) = first_block(&sim).expect("a block forms");
    let rusher = engagement.rusher;
    // Track which side of the rusher the blocker is on; a "dance" flips it often.
    let mut sign = (sim.players[blocker.index()].pos.x - sim.players[rusher.index()].pos.x).signum();
    let mut flips = 0;
    for _ in 0..40 {
        sim.step(&[]);
        if !sim.players[blocker.index()].anim.can_act() {
            break;
        }
        let s = (sim.players[blocker.index()].pos.x - sim.players[rusher.index()].pos.x).signum();
        if s != sign && s != 0.0 {
            flips += 1;
            sign = s;
        }
    }
    assert!(flips <= 2, "the blocker anchors a side rather than circling ({flips} flips)");
}

#[test]
fn the_engagement_advantage_increases_as_the_contest_continues() {
    let mut sim = engaged(20);
    let (blocker, early) = best_engaged(&sim).expect("a block makes contact");
    let rusher = early.rusher;
    // Sample a short window of the SAME contest, before any shed resets it.
    let mut late = early;
    for _ in 0..20 {
        sim.step(&[]);
        match sim.engagement(blocker) {
            Some(e) if e.rusher == rusher && e.state != EngagementState::Shed => late = e,
            _ => break,
        }
    }
    assert!(
        late.advantage > early.advantage,
        "the rush advantage builds over time ({} -> {})",
        early.advantage,
        late.advantage
    );
}

#[test]
fn a_defensive_lineman_eventually_sheds_after_winning() {
    // Never throw: the pass rush must eventually break free and get home.
    let mut sim = engaged(0);
    let mut shed = false;
    for _ in 0..300 {
        sim.step(&[]);
        let any_shed = (0..PLAYER_COUNT).any(|i| {
            sim.engagement(sim.players[i].id)
                .map(|e| e.state == EngagementState::Shed)
                .unwrap_or(false)
        });
        shed |= any_shed;
        if sim.phase == PlayPhase::Ended {
            break;
        }
    }
    assert!(shed, "the rush sheds its block and breaks free");
    assert_eq!(sim.phase, PlayPhase::Ended, "and gets home for the sack");
}

#[test]
fn a_strong_blocker_delays_the_shed() {
    // Measure how long the held quarterback survives with normal vs. dominant
    // offensive linemen; a stronger line must delay the sack.
    let sack_tick = |line_strength: f32| -> u64 {
        let mut sim = SimState::new(EndZoneConfig::default());
        for p in sim.rosters.0.players.iter_mut() {
            if p.archetype.name == "lineman" {
                p.archetype.block_strength = line_strength;
            }
        }
        sim.step(&[SimCommand::BeginPlay]);
        sim.step(&[SimCommand::Snap]);
        for t in 0..600u64 {
            sim.step(&[]);
            if sim.phase == PlayPhase::Ended {
                return t;
            }
        }
        600
    };
    let weak = sack_tick(0.2);
    let strong = sack_tick(1.0);
    assert!(
        strong > weak,
        "a stronger line delays the sack ({strong} vs {weak})"
    );
}
