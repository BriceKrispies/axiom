//! Tests for throw targeting: the quarterback's cone, who is eligible inside
//! it, and which receiver the pass commits to. These pin the behaviour that
//! replaced the old hardcoded `throw_to` roster slot — above all, that turning
//! the quarterback changes who he throws to.

use axiom::prelude::Vec3;
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::football::targeting;
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::state::{SimCommand, SimState};

/// A sim advanced to a live drop-back, with the quarterback holding the ball.
fn dropped_back() -> SimState {
    let mut sim = SimState::new(EndZoneConfig::default());
    for t in 0..170u64 {
        let commands: &[SimCommand] = match t {
            0 => &[SimCommand::BeginPlay],
            80 => &[SimCommand::Snap],
            _ => &[],
        };
        sim.step(commands);
    }
    sim
}

fn cone(sim: &SimState) -> Vec<targeting::ThrowCandidate> {
    let qb = &sim.players[sim.quarterback.index()];
    targeting::candidates(qb, &sim.players, &sim.assignments, &sim.tuning)
}

#[test]
fn the_quarterback_sees_multiple_receivers_not_one_hardcoded_slot() {
    let sim = dropped_back();
    let picks = cone(&sim);
    assert!(
        picks.len() >= 2,
        "the cone offers a real choice of receivers, got {:?}",
        picks.iter().map(|c| c.id.0).collect::<Vec<_>>()
    );
}

#[test]
fn turning_the_quarterback_changes_who_he_throws_to() {
    let mut sim = dropped_back();
    let straight = targeting::best(&cone(&sim)).expect("someone is open facing downfield");

    // Sweep the quarterback's facing across the field and collect every
    // receiver that becomes the primary target. If aiming did nothing, this set
    // would hold exactly one id — which was the old hardcoded behaviour.
    let mut seen = Vec::new();
    for step in -6..=6 {
        sim.players[sim.quarterback.index()].facing = step as f32 * 0.18;
        if let Some(id) = targeting::best(&cone(&sim)) {
            if !seen.contains(&id) {
                seen.push(id);
            }
        }
    }
    assert!(
        seen.len() >= 2,
        "aiming the quarterback must change the target; only ever picked {seen:?}"
    );
    assert!(
        seen.contains(&straight),
        "facing straight downfield is one of the reachable targets"
    );
}

#[test]
fn the_pass_goes_to_the_receiver_nearest_the_centre_line() {
    let sim = dropped_back();
    let picks = cone(&sim);
    let best = targeting::best(&picks).expect("someone is open");
    assert_eq!(best, picks[0].id, "the pick is the head of the ordering");
    for candidate in &picks {
        assert!(
            picks[0].angle <= candidate.angle + 1.0e-6,
            "no candidate sits closer to the centre line than the pick"
        );
    }
}

#[test]
fn candidates_are_ordered_deterministically_and_stay_inside_the_cone() {
    let sim = dropped_back();
    let picks = cone(&sim);
    let again = cone(&sim);
    assert_eq!(
        picks.iter().map(|c| c.id.0).collect::<Vec<_>>(),
        again.iter().map(|c| c.id.0).collect::<Vec<_>>(),
        "the same field yields the same ordering"
    );
    for candidate in &picks {
        assert!(
            candidate.angle <= sim.tuning.throw_cone_half_angle + 1.0e-6,
            "every candidate is inside the cone half-angle"
        );
        assert!(
            candidate.distance >= sim.tuning.throw_min_range
                && candidate.distance <= sim.tuning.throw_max_range,
            "every candidate is inside the throwable range"
        );
    }
}

#[test]
fn blockers_the_snapper_and_the_quarterback_are_never_eligible() {
    let sim = dropped_back();
    // Widen the search to the whole field so only the eligibility RULE (not
    // geometry) can exclude anyone.
    let mut wide = sim.tuning;
    wide.throw_cone_half_angle = core::f32::consts::PI;
    wide.throw_min_range = 0.0;
    wide.throw_max_range = 1_000.0;
    let qb = &sim.players[sim.quarterback.index()];
    let picks = targeting::candidates(qb, &sim.players, &sim.assignments, &wide);

    assert!(
        !picks.iter().any(|c| c.id == sim.quarterback),
        "the quarterback cannot throw to himself"
    );
    // Roster slots: 0 QB, 1 snapper, 2/3 pass blockers, 4/5/6 route runners.
    // Offense ids are the possession team's block of the id space.
    let base = picks
        .first()
        .map(|c| (c.id.0 / 7) * 7)
        .expect("the offense has receivers");
    for ineligible in [0u8, 1, 2, 3] {
        assert!(
            !picks.iter().any(|c| c.id == PlayerId(base + ineligible)),
            "roster slot {ineligible} (quarterback/snapper/blocker) is not a receiver"
        );
    }
    assert!(
        picks.len() == 3,
        "exactly the three route runners are eligible, got {:?}",
        picks.iter().map(|c| c.id.0).collect::<Vec<_>>()
    );
}

#[test]
fn nobody_behind_the_quarterback_is_throwable() {
    let mut sim = dropped_back();
    // Spin him to face his own end zone: every route runner is now behind him.
    sim.players[sim.quarterback.index()].facing = core::f32::consts::PI;
    assert!(
        cone(&sim).is_empty(),
        "a quarterback facing backwards has no pass available"
    );
    assert!(targeting::best(&cone(&sim)).is_none());
}

#[test]
fn a_downed_receiver_drops_out_of_the_cone() {
    let mut sim = dropped_back();
    let target = targeting::best(&cone(&sim)).expect("someone is open");
    sim.players[target.index()].anim = axiom_end_zone::player::AnimState::GroundImpact;
    let after = cone(&sim);
    assert!(
        !after.iter().any(|c| c.id == target),
        "a receiver on the turf is not throwable"
    );
}

#[test]
fn pressing_throw_with_nobody_open_makes_no_pass_and_keeps_the_ball() {
    let mut sim = dropped_back();
    // Face backwards, then order the throw: there is no legal pass.
    sim.players[sim.quarterback.index()].facing = core::f32::consts::PI;
    let held_by = sim.possession;
    sim.step(&[SimCommand::ThrowNow]);
    for _ in 0..30 {
        sim.players[sim.quarterback.index()].facing = core::f32::consts::PI;
        sim.step(&[]);
    }
    assert_eq!(
        sim.possession, held_by,
        "the quarterback still has the ball — the throw was refused, not lost"
    );
}

#[test]
fn the_throwable_set_is_published_for_the_receiver_rings() {
    let sim = dropped_back();
    assert!(
        !sim.throwable.is_empty(),
        "the sim publishes the eligible receivers the rings are drawn from"
    );
    assert_eq!(
        sim.throwable,
        cone(&sim).iter().map(|c| c.id).collect::<Vec<_>>(),
        "the published set is exactly the cone, in the same order — what the \
         player sees cannot disagree with where the ball would go"
    );
}

#[test]
fn the_ring_geometry_tracks_the_eligible_receivers() {
    use axiom_end_zone::presentation::receiver_ring::{ring_instances, RING_SEGMENTS};
    use axiom_end_zone::presentation::snapshot::capture;

    let sim = dropped_back();
    let snapshot = capture(&sim);
    let mut rings = Vec::new();
    ring_instances(&snapshot, &mut rings);
    assert_eq!(
        rings.len(),
        snapshot.throwable.len() * RING_SEGMENTS,
        "one ring per throwable receiver"
    );
    // Each ring is centred on its receiver's feet and sits on the turf.
    for (index, id) in snapshot.throwable.iter().enumerate() {
        let feet = snapshot.player(*id).pos;
        let segments = &rings[index * RING_SEGMENTS..(index + 1) * RING_SEGMENTS];
        let centre = segments
            .iter()
            .fold(Vec3::ZERO, |acc, s| acc.add(s.transform.translation));
        let centre = centre.mul_scalar(1.0 / RING_SEGMENTS as f32);
        assert!(
            (centre.x - feet.x).abs() < 1.0e-3 && (centre.z - feet.z).abs() < 1.0e-3,
            "ring {index} is centred on its receiver"
        );
        assert!(
            segments
                .iter()
                .all(|s| s.transform.translation.y > 0.0 && s.transform.translation.y < 0.3),
            "the ring lies on the turf"
        );
    }
}

#[test]
fn the_locked_target_survives_the_windup() {
    let mut sim = dropped_back();
    let expected = targeting::best(&cone(&sim)).expect("someone is open");
    sim.step(&[SimCommand::ThrowNow]);
    // Spin the quarterback mid-wind-up: the pass must still go where he aimed
    // when the player committed to it.
    for _ in 0..40 {
        sim.players[sim.quarterback.index()].facing += 0.25;
        sim.step(&[]);
    }
    let flight = match sim.ball.state {
        axiom_end_zone::football::BallState::Airborne { flight } => Some(flight),
        _ => None,
    };
    let intended = flight
        .map(|f| f.intended)
        .or_else(|| sim.possession.filter(|id| *id != sim.quarterback))
        .expect("the pass was released");
    assert_eq!(
        intended, expected,
        "the wind-up locked the target chosen at the press"
    );
}

#[test]
fn the_current_read_is_ringed_red_and_the_rest_white() {
    use axiom_end_zone::presentation::receiver_ring::{
        ring_instances, RingKind, MAX_RINGS, RING_SEGMENTS,
    };
    use axiom_end_zone::presentation::snapshot::capture;

    let sim = dropped_back();
    let snapshot = capture(&sim);
    assert!(
        snapshot.throwable.len() >= 2,
        "this test needs a cone with a real choice in it"
    );
    let mut rings = Vec::new();
    ring_instances(&snapshot, &mut rings);

    // Exactly one ring is red, and it is the receiver the pass commits to.
    let red: Vec<_> = rings
        .iter()
        .filter(|s| s.kind == RingKind::Target)
        .collect();
    assert_eq!(
        red.len(),
        RING_SEGMENTS,
        "exactly one ring (not zero, not two) is the red target ring"
    );
    assert!(
        rings[..RING_SEGMENTS]
            .iter()
            .all(|s| s.kind == RingKind::Target),
        "the red ring is the FIRST one — the head of the cone ordering"
    );
    assert!(
        rings[RING_SEGMENTS..]
            .iter()
            .all(|s| s.kind == RingKind::Eligible),
        "every other throwable receiver stays white"
    );

    // The red ring sits on the receiver targeting would actually throw to.
    let target = targeting::best(&cone(&sim)).expect("someone is open");
    let feet = snapshot.player(target).pos;
    let centre = red
        .iter()
        .fold(Vec3::ZERO, |acc, s| acc.add(s.transform.translation))
        .mul_scalar(1.0 / RING_SEGMENTS as f32);
    assert!(
        (centre.x - feet.x).abs() < 1.0e-3 && (centre.z - feet.z).abs() < 1.0e-3,
        "the red ring is centred on the receiver the pass would go to"
    );
    assert!(rings.len() <= MAX_RINGS * RING_SEGMENTS, "the pool bound holds");
}

#[test]
fn the_red_ring_follows_the_current_read_not_a_fixed_receiver() {
    use axiom_end_zone::presentation::receiver_ring::{ring_instances, RingKind, RING_SEGMENTS};
    use axiom_end_zone::presentation::snapshot::capture;

    let sim = dropped_back();
    let mut snapshot = capture(&sim);
    assert!(snapshot.throwable.len() >= 2, "need a real choice");

    let red_centre = |snapshot: &_| {
        let mut rings = Vec::new();
        ring_instances(snapshot, &mut rings);
        rings
            .iter()
            .filter(|s| s.kind == RingKind::Target)
            .fold(Vec3::ZERO, |acc, s| acc.add(s.transform.translation))
            .mul_scalar(1.0 / RING_SEGMENTS as f32)
    };

    let first = red_centre(&snapshot);
    assert!(
        (first.x - snapshot.player(snapshot.throwable[0]).pos.x).abs() < 1.0e-3,
        "the red ring starts on the head of the cone"
    );

    // Re-order the cone as a different read would (targeting publishes the
    // current read first): the red ring must follow the HEAD, not a fixed id.
    snapshot.throwable.reverse();
    let moved = red_centre(&snapshot);
    assert!(
        (moved.x - snapshot.player(snapshot.throwable[0]).pos.x).abs() < 1.0e-3,
        "the red ring tracks whichever receiver is the current read"
    );
    assert!(
        first.subtract(moved).length() > 1.0,
        "a different read genuinely moves the red ring"
    );
}

// ----- steered quarterback: strafe, not spin -------------------------------

use axiom::prelude::Vec2;

/// Steer the controlled player with a held stick for `ticks`, returning the sim.
fn steer(mut sim: SimState, stick: Vec2, ticks: usize) -> SimState {
    for _ in 0..ticks {
        sim.user_stick = stick;
        sim.step(&[]);
    }
    sim
}

/// Absolute angle between the quarterback's facing and straight downfield.
fn yaw_off_downfield(sim: &SimState) -> f32 {
    let qb = &sim.players[sim.quarterback.index()];
    let facing = Vec3::new(qb.facing.sin(), 0.0, qb.facing.cos());
    let downfield = sim.frame.forward();
    facing.dot(downfield).clamp(-1.0, 1.0).acos()
}

#[test]
fn a_steered_quarterback_never_turns_past_his_forward_arc() {
    // Hold every direction, including straight backwards and hard sideways.
    for (x, y) in [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, -1.0),
        (1.0, -1.0),
        (-1.0, -1.0),
        (0.7, -0.7),
    ] {
        let sim = steer(dropped_back(), Vec2::new(x, y), 45);
        let off = yaw_off_downfield(&sim);
        assert!(
            off <= sim.tuning.qb_aim_max_yaw + 0.05,
            "stick ({x},{y}) turned the passer {off:.2} rad off downfield, past \
             the {:.2} rad forward arc",
            sim.tuning.qb_aim_max_yaw
        );
    }
}

#[test]
fn pushing_sideways_strafes_the_quarterback_instead_of_spinning_him() {
    let base = dropped_back();
    let start = base.players[base.quarterback.index()].pos;
    let sim = steer(base, Vec2::new(1.0, 0.0), 45);
    let qb = &sim.players[sim.quarterback.index()];

    // He genuinely travelled sideways...
    let moved = qb.pos.subtract(start);
    let lateral = moved.dot(sim.frame.right()).abs();
    assert!(
        lateral > 1.0,
        "a full sideways stick moves him laterally, got {lateral:.2} yd"
    );
    // ...while still looking downfield rather than turning to run that way.
    assert!(
        sim.frame.forward().dot(Vec3::new(qb.facing.sin(), 0.0, qb.facing.cos())) > 0.4,
        "he keeps his eyes downfield while strafing"
    );
}

#[test]
fn a_backwards_stick_backpedals_without_turning_around() {
    let sim = steer(dropped_back(), Vec2::new(0.0, -1.0), 45);
    let qb = &sim.players[sim.quarterback.index()];
    let facing = Vec3::new(qb.facing.sin(), 0.0, qb.facing.cos());
    assert!(
        facing.dot(sim.frame.forward()) > 0.9,
        "retreating keeps him square to the field, not spun around"
    );
    assert!(
        qb.vel.dot(sim.frame.forward()) < -0.5,
        "he is actually moving backwards"
    );
}

#[test]
fn the_stick_aims_the_throwing_cone_within_the_arc() {
    // Aiming must survive the strafe change: holding left vs right has to be
    // able to put the red ring on a different receiver.
    let mut reads = Vec::new();
    for x in [-1.0f32, 0.0, 1.0] {
        let sim = steer(dropped_back(), Vec2::new(x, 0.0), 40);
        if let Some(id) = targeting::best(&cone(&sim)) {
            if !reads.contains(&id) {
                reads.push(id);
            }
        }
    }
    assert!(
        reads.len() >= 2,
        "steering left/right must change the current read; only ever got {reads:?}"
    );
}

#[test]
fn a_steered_ball_carrier_still_turns_to_run_where_he_is_going() {
    // The strafe rule is the PASSER's, not everyone's: once a receiver has the
    // ball he must still turn and run downfield normally.
    let mut sim = dropped_back();
    sim.step(&[SimCommand::ThrowNow]);
    for _ in 0..240 {
        sim.step(&[]);
        let carrier = sim.possession;
        if carrier.is_some() && carrier != Some(sim.quarterback) {
            let id = carrier.expect("a receiver carries the ball");
            let sim = steer(sim, Vec2::new(1.0, 0.0), 40);
            let runner = &sim.players[id.index()];
            let facing = Vec3::new(runner.facing.sin(), 0.0, runner.facing.cos());
            assert!(
                facing.dot(sim.frame.right()) > 0.5,
                "a carrier steered sideways turns to run that way"
            );
            return;
        }
    }
}
